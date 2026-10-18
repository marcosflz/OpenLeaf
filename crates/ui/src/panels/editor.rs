//! Panel del editor (M1-B1..B5) — componente montable por el shell.
//!
//! Estado propio + API de comandos + callbacks; el shell (stream A) lo
//! integra en el dock y enruta los eventos de servicios:
//!
//! - **M1-B1** pestañas multi-documento (`TabBar` del kit + una entidad
//!   `EditorState` por pestaña), dirty por versión de documento
//!   (`InputEvent::Change`), cierre con confirmación y restauración de sesión
//!   (`config/editor-session.json`; pendiente migrar a `Settings` de
//!   `lsx-core` — ver `lsx_editor::session`).
//! - **M1-B2** guardado atómico vía [`FsService::write_text_atomic`] (el
//!   editor nunca escribe a mano) y conflictos del watcher con modal de 3
//!   opciones (recargar / sobrescribir / ver diff).
//! - **M1-B3** diagnósticos inline: [`EditorPanel::set_diagnostics`] mapea
//!   `lsx_latex::Diagnostic` al `DiagnosticSet` del kit.
//! - **M1-B4** buscar/reemplazar sobre la sesión de búsqueda del kit
//!   (`lsx_editor::find`): contador, anterior/siguiente, match-case,
//!   wrap-around y reemplazar uno/todos.
//! - **M1-B5** autosave opcional (off por defecto) + backups en
//!   `config/backups/` con propuesta de recuperación al abrir.
//!
//! El panel **no** arranca el watcher: [`EditorPanel::apply_fs_batch`] es la
//! puerta de entrada de los `FsEvent` (el dueño del servicio los entrega en el
//! hilo de UI; el binario de demo lo cablea con `async_channel` + `cx.spawn`).
//! Es la regla de oro de INTERFACES §8: los servicios empujan eventos y el
//! panel reduce.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogDescription, DialogFooter};
use gpui_kit::component::input::{
    Editor, EditorState, Input, InputEvent, InputState, Position, Replace as ReplaceAction,
    Search as SearchAction,
};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use lsx_core::fs::{FsEvent, FsService};
use lsx_editor::backup::{BackupEntry, BackupStore};
use lsx_editor::diff::{DiffKind, diff_lines};
use lsx_editor::document::{AutosaveConfig, DocId, DocIdGenerator, DocumentMeta, rel_key};
use lsx_editor::session::{
    SESSION_SCHEMA_VERSION, SessionCursor, SessionState, SessionStore, SessionTab,
};
use lsx_editor::{diagnostics as diag, find, latex_language};
use lsx_latex::Diagnostic;

// Comandos del panel (registrados con keybindings globales).
gpui_kit::actions!(
    lsx_editor_panel,
    [
        SaveDocument,
        ToggleFind,
        ToggleReplace,
        FindNext,
        FindPrevious,
        CloseTab
    ]
);

/// Eventos que el panel emite al shell (callbacks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorPanelEvent {
    /// Se abrió (o activó) un documento; `path` es relativo con `/`.
    Opened { path: String },
    /// Se cerró una pestaña.
    Closed { path: String },
    /// Se guardó en disco (átomo).
    Saved { path: String },
    /// Cambió el dirty del documento activo.
    DirtyChanged { dirty: bool },
    /// Mensaje corto para la status bar del shell.
    Status { message: String },
}

/// Configuración de montaje del panel.
#[derive(Debug, Clone, Copy)]
pub struct EditorPanelConfig {
    /// Autosave (off por defecto, M1-B5).
    pub autosave: AutosaveConfig,
    /// Cada cuántos segundos se escribe un backup de los documentos sucios.
    pub backup_interval_secs: u64,
    /// Restaurar pestañas/cursores guardados al montar (M1-B1).
    pub restore_session: bool,
}

impl Default for EditorPanelConfig {
    fn default() -> Self {
        Self {
            autosave: AutosaveConfig::default(),
            backup_interval_secs: 5,
            restore_session: true,
        }
    }
}

/// Un documento abierto: metadatos + la entidad `EditorState` del kit.
struct OpenDocument {
    meta: DocumentMeta,
    editor: Entity<EditorState>,
    /// Último set de diagnósticos aplicado (se re-aplica tras una recarga).
    diagnostics: Vec<Diagnostic>,
    /// Versión del último backup escrito (no se repiten backups idénticos).
    last_backup_version: Option<u64>,
    _sub: Subscription,
}

/// Panel del editor. Ver la cabecera del módulo.
pub struct EditorPanel {
    root: PathBuf,
    fs: Rc<FsService>,
    session: SessionStore,
    backups: BackupStore,
    docs: Vec<OpenDocument>,
    active: Option<usize>,
    ids: DocIdGenerator,
    find_query: Entity<InputState>,
    find_replace: Entity<InputState>,
    find_open: bool,
    find_replace_mode: bool,
    find_case_sensitive: bool,
    status: SharedString,
    autosave: AutosaveConfig,
    backup_interval_secs: u64,
    seconds: u64,
    session_dirty: bool,
    /// Recuperaciones pendientes de proponer (se abre el modal tras el frame).
    pending_recovery: Vec<(DocId, BackupEntry)>,
    focus_handle: FocusHandle,
    _subs: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl EventEmitter<EditorPanelEvent> for EditorPanel {}

impl Focusable for EditorPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EditorPanel {
    /// Monta el panel con la configuración por defecto.
    ///
    /// - `root`: raíz del proyecto (todas las rutas de documentos son
    ///   relativas a ella).
    /// - `config_dir`: `config/` de la raíz portable (sesión y backups).
    pub fn new(
        root: impl Into<PathBuf>,
        config_dir: impl Into<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_config(root, config_dir, EditorPanelConfig::default(), window, cx)
    }

    /// Monta el panel con configuración explícita.
    pub fn with_config(
        root: impl Into<PathBuf>,
        config_dir: impl Into<PathBuf>,
        config: EditorPanelConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        latex_language::register_latex_language();
        let root = root.into();
        let root = std::path::absolute(&root).unwrap_or(root);
        let config_dir = config_dir.into();
        let fs = Rc::new(FsService::new(&root));

        let find_query = cx.new(|cx| InputState::new(window, cx).placeholder("Buscar…"));
        let find_replace = cx.new(|cx| InputState::new(window, cx).placeholder("Reemplazar por…"));

        let mut subs = Vec::new();
        subs.push(cx.subscribe_in(
            &find_query,
            window,
            |this, state, event, _window, cx| match event {
                InputEvent::Change => {
                    let _ = state;
                    this.on_find_query_changed(cx);
                }
                InputEvent::PressEnter { shift, .. } => {
                    if *shift {
                        this.find_previous(cx);
                    } else {
                        this.find_next(cx);
                    }
                }
                _ => {}
            },
        ));
        subs.push(
            cx.subscribe_in(&find_replace, window, |this, _state, event, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.find_replace_mode = true;
                    cx.notify();
                }
            }),
        );

        cx.bind_keys([
            KeyBinding::new("ctrl-s", SaveDocument, None),
            KeyBinding::new("ctrl-f", ToggleFind, None),
            KeyBinding::new("ctrl-h", ToggleReplace, None),
            KeyBinding::new("f3", FindNext, None),
            KeyBinding::new("shift-f3", FindPrevious, None),
            KeyBinding::new("ctrl-w", CloseTab, None),
        ]);

        let mut this = Self {
            root,
            fs,
            session: SessionStore::new(&config_dir),
            backups: BackupStore::new(config_dir.join("backups")),
            docs: Vec::new(),
            active: None,
            ids: DocIdGenerator::default(),
            find_query,
            find_replace,
            find_open: false,
            find_replace_mode: false,
            find_case_sensitive: false,
            status: "listo".into(),
            autosave: config.autosave,
            backup_interval_secs: config.backup_interval_secs.max(1),
            seconds: 0,
            session_dirty: false,
            pending_recovery: Vec::new(),
            focus_handle: cx.focus_handle(),
            _subs: subs,
            _tasks: Vec::new(),
        };

        if config.restore_session {
            this.restore_session(window, cx);
        }
        this.focus_active(window, cx);
        this.start_timers(cx);
        this
    }

    // --- API de comandos ---------------------------------------------------

    /// Abre `rel` (relativo a la raíz) o la activa si ya estaba abierta;
    /// `line` (1-based) mueve el cursor.
    pub fn open_file(
        &mut self,
        rel: impl AsRef<Path>,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let rel = rel.as_ref().to_path_buf();
        if let Some(ix) = self.index_by_rel(&rel) {
            self.activate(ix, window, cx);
            if let Some(line) = line {
                self.goto_line(ix, line, window, cx);
            }
            return true;
        }

        let text_file = match self.fs.read_text(&rel) {
            Ok(file) => file,
            Err(err) => {
                self.set_status(format!("No se pudo abrir «{}»: {err}", rel_key(&rel)), cx);
                return false;
            }
        };

        let content = text_file.text;
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(latex_language::LATEX_LANGUAGE_NAME)
                .line_number(true)
                .default_value(content.clone())
        });
        let doc_id = self.ids.next_id();
        let sub = cx.subscribe(&editor, move |this, _editor, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_document_edited(doc_id, cx);
            }
        });
        let meta = DocumentMeta::new(doc_id, &rel, text_file.eol, text_file.had_bom);
        self.docs.push(OpenDocument {
            meta,
            editor,
            diagnostics: Vec::new(),
            last_backup_version: None,
            _sub: sub,
        });
        let ix = self.docs.len() - 1;
        self.active = Some(ix);
        self.session_dirty = true;
        if let Some(line) = line {
            self.goto_line(ix, line, window, cx);
        }
        self.focus_active(window, cx);
        self.apply_find_to_active(cx);
        cx.emit(EditorPanelEvent::Opened {
            path: rel_key(&self.docs[ix].meta.rel),
        });
        self.queue_recovery_for(ix);
        cx.notify();
        true
    }

    /// Abre una ruta absoluta si cae dentro de la raíz del proyecto.
    pub fn open_path(
        &mut self,
        path: impl AsRef<Path>,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let path = path.as_ref();
        match path.strip_prefix(&self.root) {
            Ok(rel) => self.open_file(rel, line, window, cx),
            Err(_) => {
                self.set_status(
                    format!("«{}» queda fuera del proyecto abierto", path.display()),
                    cx,
                );
                false
            }
        }
    }

    /// Guarda el documento activo. `false` si no había ninguno o falló.
    pub fn save_active(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.active {
            Some(ix) => self.save_doc(ix, cx),
            None => false,
        }
    }

    /// Guarda un documento abierto por ruta relativa.
    pub fn save_file(&mut self, rel: impl AsRef<Path>, cx: &mut Context<Self>) -> bool {
        match self.index_by_rel(rel.as_ref()) {
            Some(ix) => self.save_doc(ix, cx),
            None => false,
        }
    }

    /// Inserta `text` en el cursor del documento activo.
    ///
    /// Pasa por el mismo camino de edición que el teclado (`insert` del kit):
    /// la historia de deshacer y la versión de documento quedan consistentes.
    /// Lo usará el panel de chat (M1-E4) para aplicar ediciones del agente.
    pub fn insert_active(
        &mut self,
        text: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(ix) = self.active else {
            return false;
        };
        let text = text.into();
        self.docs[ix].editor.update(cx, |state, cx| {
            state.insert(text, window, cx);
        });
        true
    }

    /// Cierra la pestaña activa (con confirmación si está sucia).
    pub fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.active {
            self.request_close_ix(ix, window, cx);
        }
    }

    /// Enfoca el editor activo.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_active(window, cx);
    }

    /// Enruta un lote de eventos del watcher (M1-B2).
    pub fn apply_fs_batch(
        &mut self,
        batch: Vec<FsEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for event in batch {
            match event {
                FsEvent::Created(rel) | FsEvent::Modified(rel) => {
                    self.on_external_write(rel, window, cx)
                }
                FsEvent::Removed(rel) => {
                    if self.index_by_rel(&rel).is_some() {
                        self.set_status(
                            format!(
                                "«{}» se borró en disco (la pestaña sigue abierta; guarda para recrearlo)",
                                rel_key(&rel)
                            ),
                            cx,
                        );
                    }
                }
                FsEvent::Renamed(from, to) => {
                    if let Some(ix) = self.index_by_rel(&from) {
                        self.docs[ix].meta.rel = to.clone();
                        self.set_status(
                            format!("«{}» se renombró a «{}»", rel_key(&from), rel_key(&to)),
                            cx,
                        );
                    }
                }
            }
        }
    }

    /// Aplica diagnósticos de `lsx-latex` al documento `rel` (M1-B3).
    /// Devuelve cuántos marcadores quedaron inline.
    pub fn set_diagnostics(
        &mut self,
        rel: impl AsRef<Path>,
        diags: &[Diagnostic],
        cx: &mut Context<Self>,
    ) -> usize {
        let Some(ix) = self.index_by_rel(rel.as_ref()) else {
            return 0;
        };
        let diags = diags.to_vec();
        let file_name = self.docs[ix]
            .meta
            .rel
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let count = self.docs[ix].editor.update(cx, |state, cx| {
            let count = diag::apply_diagnostics_for(state, file_name.as_deref(), &diags);
            cx.notify();
            count
        });
        self.docs[ix].diagnostics = diags;
        cx.notify();
        count
    }

    /// Configura el autosave (M1-B5; el intervalo viene de settings).
    pub fn set_autosave(&mut self, config: AutosaveConfig, cx: &mut Context<Self>) {
        self.autosave = config;
        self.set_status(
            if config.enabled {
                format!("autosave cada {} s", config.interval_secs.max(1))
            } else {
                "autosave desactivado".into()
            },
            cx,
        );
    }

    /// Estado de autosave actual.
    pub fn autosave(&self) -> AutosaveConfig {
        self.autosave
    }

    /// Escribe backups de recuperación de los documentos sucios ahora mismo.
    pub fn backup_dirty_now(&mut self, cx: &mut Context<Self>) -> usize {
        self.flush_backups(cx)
    }

    // --- consultas ----------------------------------------------------------

    /// Ruta relativa (con `/`) del documento activo.
    /// Cursor del documento activo (línea/columna **1-based**) para SyncTeX.
    pub fn active_cursor(&self, cx: &App) -> Option<(u32, u32)> {
        let ix = self.active?;
        let position = self.docs[ix].editor.read(cx).cursor_position();
        Some((position.line + 1, position.character + 1))
    }

    pub fn active_path(&self) -> Option<String> {
        self.active
            .and_then(|ix| self.docs.get(ix))
            .map(|doc| rel_key(&doc.meta.rel))
    }

    /// `true` si el documento activo tiene cambios sin guardar.
    pub fn active_is_dirty(&self) -> bool {
        self.active
            .and_then(|ix| self.docs.get(ix))
            .is_some_and(|doc| doc.meta.is_dirty())
    }

    /// Cuántas pestañas hay abiertas.
    pub fn tab_count(&self) -> usize {
        self.docs.len()
    }

    /// Raíz del proyecto.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Fuerza el guardado de la sesión (para el cierre ordenado del shell).
    pub fn persist_session(&mut self, cx: &mut Context<Self>) {
        self.session_dirty = true;
        self.flush_session(cx);
    }

    // --- internos: sesión ---------------------------------------------------

    /// Restaura pestañas y cursores del JSON de sesión (M1-B1).
    fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.session.load();
        if state.tabs.is_empty() {
            return;
        }
        let wanted_active = state.effective_active();
        let mut restored = 0usize;
        for tab in &state.tabs {
            let rel = PathBuf::from(&tab.path);
            if self.open_file(&rel, Some(tab.cursor.line + 1), window, cx) {
                restored += 1;
            }
        }
        if restored == 0 {
            return;
        }
        self.set_active_index(
            wanted_active.min(self.docs.len().saturating_sub(1)),
            false,
            window,
            cx,
        );
        self.set_status(format!("sesión restaurada: {restored} pestaña(s)"), cx);
        self.collect_recovery_candidates();
    }

    fn session_state(&self, cx: &App) -> SessionState {
        let tabs = self
            .docs
            .iter()
            .map(|doc| {
                let position = doc.editor.read(cx).cursor_position();
                SessionTab {
                    path: rel_key(&doc.meta.rel),
                    cursor: SessionCursor {
                        line: position.line,
                        character: position.character,
                    },
                }
            })
            .collect();
        SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            tabs,
            active: self.active.unwrap_or(0),
        }
    }

    fn flush_session(&mut self, cx: &mut Context<Self>) {
        if !self.session_dirty {
            return;
        }
        self.session_dirty = false;
        let state = self.session_state(cx);
        if let Err(err) = self.session.save(&state) {
            tracing::warn!(target: "lsx::editor::panel", error = %err, "no se pudo guardar la sesión");
        }
    }

    // --- internos: timer ----------------------------------------------------

    fn start_timers(&mut self, cx: &mut Context<Self>) {
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |panel, cx| panel.on_tick(cx)).is_err() {
                    break;
                }
            }
        });
        self._tasks.push(tick);
    }

    /// Un segundo de vida del panel: backups, autosave y sesión.
    fn on_tick(&mut self, cx: &mut Context<Self>) {
        self.seconds = self.seconds.wrapping_add(1);
        let backup_due =
            self.backup_interval_secs > 0 && self.seconds.is_multiple_of(self.backup_interval_secs);
        if backup_due {
            self.flush_backups(cx);
        }
        if self.autosave.enabled {
            let interval = self.autosave.interval_secs.max(1);
            if self.seconds.is_multiple_of(interval) {
                self.flush_autosave(cx);
            }
        }
        self.flush_session(cx);
    }

    /// Backups de los documentos sucios cuyo contenido cambió desde el último.
    fn flush_backups(&mut self, cx: &mut Context<Self>) -> usize {
        let mut written = 0usize;
        for doc in &mut self.docs {
            if !doc.meta.is_dirty() {
                continue;
            }
            if doc.last_backup_version == Some(doc.meta.version) {
                continue;
            }
            let text = doc.editor.read(cx).text().to_string();
            match self.backups.write(&doc.meta.rel, &text) {
                Ok(_) => {
                    doc.last_backup_version = Some(doc.meta.version);
                    written += 1;
                }
                Err(err) => {
                    tracing::warn!(target: "lsx::editor::panel", error = %err, "no se pudo escribir el backup");
                }
            }
        }
        if written > 0 {
            self.set_status(format!("{written} backup(s) de recuperación escritos"), cx);
        }
        written
    }

    fn flush_autosave(&mut self, cx: &mut Context<Self>) {
        let dirty: Vec<usize> = self
            .docs
            .iter()
            .enumerate()
            .filter(|(_, doc)| doc.meta.is_dirty())
            .map(|(ix, _)| ix)
            .collect();
        for ix in dirty {
            self.save_doc(ix, cx);
        }
    }

    // --- internos: watcher / conflictos ------------------------------------

    fn on_external_write(&mut self, rel: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.index_by_rel(&rel) else {
            return;
        };
        let current = self.docs[ix].editor.read(cx).text().to_string();
        let Ok(disk) = self.fs.read_text(&rel) else {
            return;
        };
        if disk.text == current {
            // Nuestro propio guardado atómico (o un cambio externo idéntico).
            if self.docs[ix].meta.is_dirty() {
                self.docs[ix].meta.mark_saved();
                cx.emit(EditorPanelEvent::DirtyChanged { dirty: false });
            }
            cx.notify();
            return;
        }
        if !self.docs[ix].meta.is_dirty() {
            let message = format!("«{}» recargado desde disco", rel_key(&rel));
            self.reload_from_disk(ix, disk.text, window, cx);
            self.set_status(message, cx);
        } else {
            let doc_id = self.docs[ix].meta.id;
            self.show_conflict(doc_id, rel, disk.text, window, cx);
        }
    }

    /// Recarga silenciosa (el buffer no tenía cambios locales).
    fn reload_from_disk(
        &mut self,
        ix: usize,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let diags = self.docs[ix].diagnostics.clone();
        let file_name = self.docs[ix]
            .meta
            .rel
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let editor = self.docs[ix].editor.clone();
        editor.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            if !diags.is_empty() {
                diag::apply_diagnostics_for(state, file_name.as_deref(), &diags);
            }
            cx.notify();
        });
        self.docs[ix].meta.mark_saved();
        self.docs[ix].last_backup_version = None;
        let _ = self.backups.remove_for(&self.docs[ix].meta.rel);
        cx.emit(EditorPanelEvent::DirtyChanged { dirty: false });
        cx.notify();
    }

    /// Modal de conflicto: recargar / sobrescribir / ver diff (M1-B2).
    fn show_conflict(
        &mut self,
        doc_id: DocId,
        rel: PathBuf,
        disk_text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.weak_entity();
        let name = self
            .index_by_id(doc_id)
            .map(|ix| self.docs[ix].meta.display_name())
            .unwrap_or_else(|| rel_key(&rel));

        let rel_diff = rel.clone();
        let disk_diff = disk_text.clone();
        let rel_reload = rel.clone();
        let disk_reload = disk_text.clone();
        let rel_overwrite = rel.clone();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let weak_diff = weak.clone();
            let weak_reload = weak.clone();
            let weak_overwrite = weak.clone();
            let rel_diff = rel_diff.clone();
            let disk_diff = disk_diff.clone();
            let rel_reload = rel_reload.clone();
            let disk_reload = disk_reload.clone();
            let rel_overwrite = rel_overwrite.clone();
            let name = name.clone();
            dialog
                .title(format!("«{name}» cambió en disco"))
                .width(px(520.))
                .child(DialogDescription::new().child(
                    "El archivo tiene cambios fuera del editor y tu pestaña también está sin guardar. ¿Qué quieres hacer?",
                ))
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("conflict-diff")
                                .label("Ver diff")
                                .outline()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak_diff.update(cx, |panel, cx| {
                                        panel.show_conflict_diff(
                                            doc_id,
                                            rel_diff.clone(),
                                            disk_diff.clone(),
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        )
                        .child(
                            Button::new("conflict-reload")
                                .label("Recargar")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak_reload.update(cx, |panel, cx| {
                                        panel.reload_doc(
                                            doc_id,
                                            disk_reload.clone(),
                                            rel_reload.clone(),
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        )
                        .child(
                            Button::new("conflict-overwrite")
                                .label("Sobrescribir")
                                .primary()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak_overwrite.update(cx, |panel, cx| {
                                        panel.overwrite_doc(doc_id, rel_overwrite.clone(), cx);
                                    });
                                }),
                        ),
                )
        });
    }

    /// Segunda modal: diff por líneas buffer ↔ disco.
    fn show_conflict_diff(
        &mut self,
        doc_id: DocId,
        rel: PathBuf,
        disk_text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.index_by_id(doc_id) else {
            return;
        };
        let buffer_text = self.docs[ix].editor.read(cx).text().to_string();
        let (lines, stats) = diff_lines(&buffer_text, &disk_text);
        let name = self.docs[ix].meta.display_name();

        let weak = cx.weak_entity();
        let rel_reload = rel.clone();
        let disk_reload = disk_text.clone();
        let rel_overwrite = rel.clone();
        let subtitle = format!("Rojo = tu buffer · Verde = disco · {}", stats.summary());
        window.open_dialog(cx, move |dialog, _window, cx| {
            let weak_reload = weak.clone();
            let weak_overwrite = weak.clone();
            let rel_reload = rel_reload.clone();
            let disk_reload = disk_reload.clone();
            let rel_overwrite = rel_overwrite.clone();
            // Las filas se reconstruyen en cada rebuild del diálogo (los
            // `AnyElement` no se pueden clonar).
            let rows: Vec<AnyElement> = lines
                .iter()
                .map(|line| {
                    let (prefix, color) = match line.kind {
                        DiffKind::Removed => ("- ", cx.theme().danger),
                        DiffKind::Added => ("+ ", cx.theme().success),
                        DiffKind::Same => ("  ", cx.theme().muted_foreground),
                    };
                    h_flex()
                        .gap_1()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .child(div().text_color(color).child(prefix.to_string()))
                        .child(
                            div()
                                .text_color(cx.theme().foreground)
                                .child(line.text.clone()),
                        )
                        .into_any_element()
                })
                .collect();
            dialog
                .title(format!("Diff de «{name}»"))
                .width(px(760.))
                .child(DialogDescription::new().child(subtitle.clone()))
                .child(
                    div()
                        .id("conflict-diff-body")
                        .max_h(px(420.))
                        .overflow_y_scroll()
                        .children(rows),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("diff-close")
                                .label("Cerrar")
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("diff-reload").label("Recargar").on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = weak_reload.update(cx, |panel, cx| {
                                    panel.reload_doc(
                                        doc_id,
                                        disk_reload.clone(),
                                        rel_reload.clone(),
                                        window,
                                        cx,
                                    );
                                });
                            },
                        ))
                        .child(
                            Button::new("diff-overwrite")
                                .label("Sobrescribir")
                                .primary()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak_overwrite.update(cx, |panel, cx| {
                                        panel.overwrite_doc(doc_id, rel_overwrite.clone(), cx);
                                    });
                                }),
                        ),
                )
        });
    }

    /// Acción "Recargar" del conflicto.
    fn reload_doc(
        &mut self,
        doc_id: DocId,
        disk_text: String,
        rel: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.index_by_id(doc_id) else {
            return;
        };
        self.reload_from_disk(ix, disk_text, window, cx);
        self.set_status(
            format!("«{}» recargado (se descartaron tus cambios)", rel_key(&rel)),
            cx,
        );
    }

    /// Acción "Sobrescribir" del conflicto.
    fn overwrite_doc(&mut self, doc_id: DocId, rel: PathBuf, cx: &mut Context<Self>) {
        let Some(ix) = self.index_by_id(doc_id) else {
            return;
        };
        if self.save_doc(ix, cx) {
            self.set_status(format!("«{}» sobrescrito con tu buffer", rel_key(&rel)), cx);
        }
    }

    // --- internos: recuperación (M1-B5) ------------------------------------

    /// Busca backups más nuevos que el disco y los encola para proponer.
    fn collect_recovery_candidates(&mut self) {
        let candidates: Vec<(DocId, BackupEntry)> = self
            .docs
            .iter()
            .filter_map(|doc| {
                let abs = self.root.join(&doc.meta.rel);
                let disk_mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok()?;
                self.backups
                    .recovery_candidate(&doc.meta.rel, disk_mtime)
                    .map(|entry| (doc.meta.id, entry))
            })
            .collect();
        for (id, entry) in candidates {
            self.pending_recovery
                .retain(|(existing, _)| *existing != id);
            self.pending_recovery.push((id, entry));
        }
    }

    fn queue_recovery_for(&mut self, ix: usize) {
        let doc = &self.docs[ix];
        let abs = self.root.join(&doc.meta.rel);
        let Ok(disk_mtime) = std::fs::metadata(&abs).and_then(|m| m.modified()) else {
            return;
        };
        if let Some(entry) = self.backups.recovery_candidate(&doc.meta.rel, disk_mtime) {
            self.pending_recovery
                .retain(|(existing, _)| *existing != doc.meta.id);
            self.pending_recovery.push((doc.meta.id, entry));
        }
    }

    /// Abre el modal de recuperación (llamado desde `render` tras el frame).
    fn open_recovery_dialog(
        &mut self,
        candidates: Vec<(DocId, BackupEntry)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.weak_entity();
        let mut lines: Vec<String> = candidates
            .iter()
            .map(|(id, entry)| {
                let age = std::time::SystemTime::now()
                    .duration_since(entry.created)
                    .map(|age| {
                        if age.as_secs() >= 60 {
                            format!("hace {} min", age.as_secs() / 60)
                        } else {
                            format!("hace {} s", age.as_secs())
                        }
                    })
                    .unwrap_or_else(|_| "ahora".into());
                match self.index_by_id(*id) {
                    Some(ix) => format!("· {}  ({age})", self.docs[ix].meta.display_path()),
                    None => format!("· {}  ({age})", entry.source),
                }
            })
            .collect();
        lines.sort();
        let names = lines;

        let ok_candidates = candidates.clone();
        let discard_candidates = candidates;
        window.open_dialog(cx, move |dialog, _window, cx| {
            let weak_ok = weak.clone();
            let weak_discard = weak.clone();
            let ok_candidates = ok_candidates.clone();
            let discard_candidates = discard_candidates.clone();
            let rows: Vec<AnyElement> = names
                .iter()
                .map(|line| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(line.clone())
                        .into_any_element()
                })
                .collect();
            dialog
                .title("Cambios sin guardar encontrados")
                .width(px(560.))
                .child(DialogDescription::new().child(
                    "La aplicación se cerró (o murió) con cambios sin guardar. ¿Quieres recuperar la copia más reciente?",
                ))
                .child(v_flex().gap_1().children(rows))
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("recovery-discard")
                                .label("Descartar")
                                .outline()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak_discard.update(cx, |panel, cx| {
                                        panel.discard_backups(&discard_candidates, cx);
                                    });
                                }),
                        )
                        .child(
                            Button::new("recovery-accept")
                                .label("Recuperar")
                                .primary()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak_ok.update(cx, |panel, cx| {
                                        panel.recover_backups(&ok_candidates, window, cx);
                                    });
                                }),
                        ),
                )
        });
    }

    /// Aplica los backups: el buffer queda sucio con el contenido recuperado.
    fn recover_backups(
        &mut self,
        candidates: &[(DocId, BackupEntry)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut recovered = 0usize;
        for (id, entry) in candidates {
            let Some(ix) = self.index_by_id(*id) else {
                continue;
            };
            let Ok(text) = self.backups.read(entry) else {
                continue;
            };
            self.docs[ix].editor.update(cx, |state, cx| {
                state.set_value(text, window, cx);
                cx.notify();
            });
            self.docs[ix].meta.notify_edit();
            self.docs[ix].last_backup_version = None;
            recovered += 1;
        }
        if recovered > 0 {
            self.session_dirty = true;
            cx.emit(EditorPanelEvent::DirtyChanged { dirty: true });
            self.set_status(
                format!("{recovered} documento(s) recuperados de backup (sin guardar)"),
                cx,
            );
        }
    }

    fn discard_backups(&mut self, candidates: &[(DocId, BackupEntry)], cx: &mut Context<Self>) {
        let mut removed = 0usize;
        for (id, _) in candidates {
            if let Some(ix) = self.index_by_id(*id) {
                let rel = self.docs[ix].meta.rel.clone();
                removed += self.backups.remove_for(&rel).unwrap_or(0);
                self.docs[ix].last_backup_version = None;
            }
        }
        self.set_status(format!("{removed} backup(s) descartados"), cx);
    }

    // --- internos: guardado / cierre ---------------------------------------

    fn save_doc(&mut self, ix: usize, cx: &mut Context<Self>) -> bool {
        let text = self.docs[ix].editor.read(cx).text().to_string();
        let rel = self.docs[ix].meta.rel.clone();
        match self.fs.write_text_atomic(&rel, &text) {
            Ok(()) => {
                self.docs[ix].meta.mark_saved();
                self.docs[ix].last_backup_version = None;
                let _ = self.backups.remove_for(&rel);
                self.session_dirty = true;
                cx.emit(EditorPanelEvent::DirtyChanged { dirty: false });
                cx.emit(EditorPanelEvent::Saved {
                    path: rel_key(&rel),
                });
                self.set_status(format!("guardado «{}»", rel_key(&rel)), cx);
                true
            }
            Err(err) => {
                tracing::warn!(target: "lsx::editor::panel", error = %err, "fallo al guardar");
                self.set_status(format!("No se pudo guardar «{}»: {err}", rel_key(&rel)), cx);
                false
            }
        }
    }

    fn request_close_ix(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.docs[ix].meta.is_dirty() {
            self.remove_doc(ix, cx);
            return;
        }
        let doc_id = self.docs[ix].meta.id;
        let name = self.docs[ix].meta.display_name();
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let weak_save = weak.clone();
            let weak_discard = weak.clone();
            dialog
                .title(format!("«{name}» tiene cambios sin guardar"))
                .width(px(500.))
                .child(
                    DialogDescription::new().child("¿Quieres guardar antes de cerrar la pestaña?"),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("close-cancel")
                                .label("Cancelar")
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("close-discard")
                                .label("Descartar")
                                .danger()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak_discard.update(cx, |panel, cx| {
                                        if let Some(ix) = panel.index_by_id(doc_id) {
                                            panel.remove_doc(ix, cx);
                                        }
                                    });
                                }),
                        )
                        .child(
                            Button::new("close-save")
                                .label("Guardar y cerrar")
                                .primary()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak_save.update(cx, |panel, cx| {
                                        if let Some(ix) = panel.index_by_id(doc_id)
                                            && panel.save_doc(ix, cx)
                                        {
                                            panel.remove_doc(ix, cx);
                                        }
                                    });
                                }),
                        ),
                )
        });
    }

    fn remove_doc(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.docs.len() {
            return;
        }
        let doc = self.docs.remove(ix);
        let path = rel_key(&doc.meta.rel);
        self.session_dirty = true;
        self.active = if self.docs.is_empty() {
            None
        } else {
            Some(ix.min(self.docs.len() - 1))
        };
        cx.emit(EditorPanelEvent::Closed { path });
        cx.emit(EditorPanelEvent::DirtyChanged {
            dirty: self.active_is_dirty(),
        });
        self.apply_find_to_active(cx);
        cx.notify();
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.set_active_index(ix, true, window, cx);
    }

    fn set_active_index(
        &mut self,
        ix: usize,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ix >= self.docs.len() {
            return;
        }
        let changed = self.active != Some(ix);
        self.active = Some(ix);
        if focus {
            self.focus_active(window, cx);
        }
        if changed {
            self.session_dirty = true;
            self.apply_find_to_active(cx);
            cx.emit(EditorPanelEvent::DirtyChanged {
                dirty: self.docs[ix].meta.is_dirty(),
            });
        }
        cx.notify();
    }

    fn focus_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.active {
            let handle = self.docs[ix].editor.focus_handle(cx);
            handle.focus(window, cx);
        } else {
            self.focus_handle.focus(window, cx);
        }
    }

    fn goto_line(&mut self, ix: usize, line: u32, window: &mut Window, cx: &mut Context<Self>) {
        self.docs[ix].editor.update(cx, |state, cx| {
            state.set_cursor_position(Position::new(line.saturating_sub(1), 0), window, cx);
        });
    }

    fn on_document_edited(&mut self, doc_id: DocId, cx: &mut Context<Self>) {
        let Some(ix) = self.index_by_id(doc_id) else {
            return;
        };
        let was_dirty = self.docs[ix].meta.is_dirty();
        self.docs[ix].meta.notify_edit();
        self.session_dirty = true;
        let dirty = self.docs[ix].meta.is_dirty();
        if dirty || dirty != was_dirty {
            cx.emit(EditorPanelEvent::DirtyChanged { dirty });
        }
        cx.notify();
    }

    // --- búsqueda (API pública para el shell) ------------------------------

    /// Abre la barra de búsqueda (o la de reemplazo) y enfoca el campo.
    pub fn toggle_find(&mut self, replace_mode: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = true;
        if replace_mode {
            self.find_replace_mode = true;
        }
        // Prefill con la selección del editor activo (una sola línea).
        if let Some(ix) = self.active
            && let Some(selected) = find::selected_text(self.docs[ix].editor.read(cx))
            && !selected.is_empty()
            && !selected.contains('\n')
        {
            let query = self.find_query.clone();
            query.update(cx, |state, cx| state.set_value(selected, window, cx));
        }
        let handle = self.find_query.focus_handle(cx);
        handle.focus(window, cx);
        self.apply_find_to_active(cx);
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = false;
        if let Some(ix) = self.active {
            self.docs[ix].editor.update(cx, find::close);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    fn on_find_query_changed(&mut self, cx: &mut Context<Self>) {
        self.apply_find_to_active(cx);
        cx.notify();
    }

    fn apply_find_to_active(&mut self, cx: &mut Context<Self>) {
        if !self.find_open {
            return;
        }
        let Some(ix) = self.active else {
            return;
        };
        let query = self.find_query.read(cx).value().to_string();
        let case_sensitive = self.find_case_sensitive;
        self.docs[ix].editor.update(cx, |state, cx| {
            find::set_query(state, &query, case_sensitive, cx);
            cx.notify();
        });
    }

    pub fn find_next(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        self.docs[ix].editor.update(cx, |state, cx| {
            if let Some(range) = find::next(state, cx) {
                state.set_selected_range(range, cx);
            }
            cx.notify();
        });
        cx.notify();
    }

    pub fn find_previous(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        self.docs[ix].editor.update(cx, |state, cx| {
            if let Some(range) = find::previous(state, cx) {
                state.set_selected_range(range, cx);
            }
            cx.notify();
        });
        cx.notify();
    }

    fn find_replace_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        let replacement = self.find_replace.read(cx).value().to_string();
        let replaced = self.docs[ix].editor.update(cx, |state, cx| {
            let replaced = find::replace_current(state, &replacement, window, cx);
            if let Some(range) = find::current_range(state) {
                state.set_selected_range(range, cx);
            }
            cx.notify();
            replaced
        });
        if replaced {
            self.set_status("coincidencia reemplazada", cx);
        }
        cx.notify();
    }

    fn find_replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        let replacement = self.find_replace.read(cx).value().to_string();
        let count = self.docs[ix].editor.update(cx, |state, cx| {
            let count = find::replace_all(state, &replacement, window, cx);
            cx.notify();
            count
        });
        if count > 0 {
            self.set_status(format!("{count} coincidencia(s) reemplazadas"), cx);
        }
        cx.notify();
    }

    fn find_label(&self, cx: &App) -> String {
        self.active
            .and_then(|ix| self.docs.get(ix))
            .map(|doc| find::match_label(doc.editor.read(cx)))
            .unwrap_or_else(|| "0/0".into())
    }

    // --- internos: utilidades ----------------------------------------------

    fn index_by_rel(&self, rel: &Path) -> Option<usize> {
        let key = rel_key(rel);
        self.docs
            .iter()
            .position(|doc| rel_key(&doc.meta.rel) == key)
    }

    fn index_by_id(&self, id: DocId) -> Option<usize> {
        self.docs.iter().position(|doc| doc.meta.id == id)
    }

    fn set_status(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = message.into();
        cx.emit(EditorPanelEvent::Status {
            message: self.status.to_string(),
        });
        cx.notify();
    }

    // --- render -------------------------------------------------------------

    fn render_tabs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.active.unwrap_or(usize::MAX);
        let tabs: Vec<Tab> = self
            .docs
            .iter()
            .enumerate()
            .map(|(ix, doc)| {
                let name = doc.meta.display_name();
                let label = if doc.meta.is_dirty() {
                    format!("● {name}")
                } else {
                    name
                };
                let close_weak = cx.weak_entity();
                let close_id = doc.meta.id;
                Tab::new()
                    .label(label)
                    .aria_label(doc.meta.display_path())
                    .on_click(cx.listener(move |this, _, window, cx| this.activate(ix, window, cx)))
                    .suffix(
                        Button::new(SharedString::from(format!("tab-close-{}", doc.meta.id.0)))
                            .icon(IconName::X)
                            .xsmall()
                            .ghost()
                            .tooltip("Cerrar pestaña (Ctrl+W)")
                            .on_click(move |_, window, cx| {
                                let _ = close_weak.update(cx, |panel, cx| {
                                    if let Some(ix) = panel.index_by_id(close_id) {
                                        panel.request_close_ix(ix, window, cx);
                                    }
                                });
                            }),
                    )
            })
            .collect();

        TabBar::new("editor-tabs")
            .selected_index(active)
            .menu(true)
            .max_width(px(220.))
            .children(tabs)
            .into_any_element()
    }

    fn render_find_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let label = self.find_label(cx);
        let case_active = self.find_case_sensitive;
        let replace_mode = self.find_replace_mode;

        let mut bar = v_flex()
            .flex_none()
            .px_2()
            .py_1()
            .gap_1()
            .bg(cx.theme().secondary)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .w(px(240.))
                            .child(Input::new(&self.find_query).small().cleanable(true)),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .child(
                        Button::new("find-prev")
                            .icon(IconName::ChevronUp)
                            .small()
                            .ghost()
                            .tooltip("Anterior (Shift+F3)")
                            .on_click(cx.listener(|this, _, _window, cx| this.find_previous(cx))),
                    )
                    .child(
                        Button::new("find-next")
                            .icon(IconName::ChevronDown)
                            .small()
                            .ghost()
                            .tooltip("Siguiente (F3)")
                            .on_click(cx.listener(|this, _, _window, cx| this.find_next(cx))),
                    )
                    .child(
                        Button::new("find-case")
                            .icon(IconName::CaseSensitive)
                            .small()
                            .when(!case_active, |button| button.ghost())
                            .tooltip("Distinguir mayúsculas y minúsculas")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.find_case_sensitive = !this.find_case_sensitive;
                                this.apply_find_to_active(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("find-toggle-replace")
                            .icon(IconName::Replace)
                            .small()
                            .when(!replace_mode, |button| button.ghost())
                            .tooltip("Mostrar reemplazo (Ctrl+H)")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.find_replace_mode = !this.find_replace_mode;
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("find-close")
                            .icon(IconName::X)
                            .small()
                            .ghost()
                            .tooltip("Cerrar la búsqueda")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_find(window, cx)),
                            ),
                    ),
            );

        if replace_mode {
            bar =
                bar.child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(
                            div()
                                .w(px(240.))
                                .child(Input::new(&self.find_replace).small().cleanable(true)),
                        )
                        .child(
                            Button::new("find-replace-one")
                                .label("Reemplazar")
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.find_replace_current(window, cx)
                                })),
                        )
                        .child(
                            Button::new("find-replace-all")
                                .label("Reemplazar todo")
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.find_replace_all(window, cx)
                                })),
                        ),
                );
        }

        bar.into_any_element()
    }

    fn render_empty(&self, cx: &App) -> AnyElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .text_color(cx.theme().muted_foreground)
            .child(Icon::new(IconName::FileText).size(px(28.)))
            .child(div().text_sm().child("Ningún documento abierto"))
            .child(
                div()
                    .text_xs()
                    .child("Usa EditorPanel::open_file / open_path para abrir una pestaña"),
            )
            .into_any_element()
    }

    fn render_status(&self, cx: &App) -> AnyElement {
        let (path, position, dirty) = match self.active.and_then(|ix| self.docs.get(ix)) {
            Some(doc) => {
                let position = doc.editor.read(cx).cursor_position();
                (doc.meta.display_path(), position, doc.meta.is_dirty())
            }
            None => ("—".into(), Position::new(0, 0), false),
        };
        h_flex()
            .flex_none()
            .h(px(22.))
            .px_2()
            .gap_3()
            .items_center()
            .text_size(px(12.))
            .border_t_1()
            .border_color(cx.theme().border)
            .text_color(cx.theme().muted_foreground)
            .child(div().flex_1().min_w_0().truncate().child(path))
            .child(div().child(format!(
                "Ln {}, Col {}",
                position.line + 1,
                position.character + 1
            )))
            .child(
                div()
                    .when(dirty, |this| this.text_color(cx.theme().yellow))
                    .child(if dirty {
                        "● sin guardar"
                    } else {
                        "sin cambios"
                    }),
            )
            .child(div().max_w(px(420.)).truncate().child(self.status.clone()))
            .into_any_element()
    }
}

impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Recuperación pendiente (M1-B5): se propone tras el primer frame para
        // que el diálogo nunca se abra a mitad de la construcción del panel.
        if !self.pending_recovery.is_empty() {
            let candidates = std::mem::take(&mut self.pending_recovery);
            let weak = cx.weak_entity();
            window.defer(cx, move |window, cx| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |panel, cx| {
                        panel.open_recovery_dialog(candidates.clone(), window, cx)
                    });
                }
            });
        }

        let tabs = if self.docs.is_empty() {
            None
        } else {
            Some(self.render_tabs(cx))
        };
        let find_bar = self.find_open.then(|| self.render_find_bar(cx));
        let body = match self.active.and_then(|ix| self.docs.get(ix)) {
            Some(doc) => div()
                .id("editor-body")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_hidden()
                .child(Editor::new(&doc.editor).bordered(false).h(relative(1.0)))
                .into_any_element(),
            None => self.render_empty(cx),
        };

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &SaveDocument, window, cx| {
                this.save_active(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleFind, window, cx| {
                this.toggle_find(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleReplace, window, cx| {
                this.toggle_find(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FindNext, _window, cx| this.find_next(cx)))
            .on_action(cx.listener(|this, _: &FindPrevious, _window, cx| this.find_previous(cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.close_active(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SearchAction, window, cx| {
                this.toggle_find(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ReplaceAction, window, cx| {
                this.toggle_find(true, window, cx);
            }))
            .when_some(tabs, |this, tabs| this.child(tabs))
            .when_some(find_bar, |this, find| this.child(find))
            .child(body)
            .child(self.render_status(cx))
    }
}

#[cfg(test)]
mod tests {
    // Nota (gpui-kit 0.7.3): con `test-support` el glob `gpui_kit::*` trae la
    // macro `test` de GPUI y tapa la de Rust; aquí se importa explícito.
    use std::path::{Path, PathBuf};

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, MouseButton,
        ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window,
    };

    use super::EditorPanel;
    use crate::layout::SlotKind;
    use crate::panels::pane::{PaneState, pane_container};

    /// Chrome mínimo de una hoja del shell (igual que `EditorSlot`, M1-A2).
    struct Host {
        pane: PaneState,
        editor: Entity<EditorPanel>,
    }

    impl Render for Host {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            pane_container(SlotKind::Editor, cx)
                .size_full()
                .track_focus(self.pane.focus_handle())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.pane.focus_if_outside(window, cx);
                    }),
                )
                .child(self.editor.clone())
        }
    }

    /// Proyecto temporal con un `main.tex` mínimo.
    fn temp_project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lsx-ui-editor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("directorio de test");
        std::fs::write(dir.join("main.tex"), "\\documentclass{article}\n").expect("main.tex");
        dir
    }

    fn with_editor<R>(
        root: &Path,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut VisualTestContext, Entity<EditorPanel>) -> R,
    ) -> R {
        cx.update(gpui_kit::init);
        let mut panel = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let editor = cx.new(|cx| EditorPanel::new(root, root.join("config"), window, cx));
            editor.update(cx, |panel, cx| {
                assert!(
                    panel.open_file("main.tex", None, window, cx),
                    "abre main.tex"
                );
            });
            panel = Some(editor.clone());
            Host {
                pane: PaneState::new(SlotKind::Editor, None, cx),
                editor,
            }
        });
        let editor = panel.unwrap();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        f(cx, editor)
    }

    /// Regresión (QA 2026-10-18): el clic dentro del editor debe dejar el foco
    /// en el documento; si el chrome de la hoja se lo roba, no se puede
    /// escribir.
    #[test]
    fn clic_en_el_editor_permite_escribir() {
        let root = temp_project();
        with_editor(&root, &mut TestAppContext::single(), |cx, editor| {
            let doc = cx.read(|cx| editor.read(cx).docs[0].editor.clone());
            let input_id = ("input", doc.entity_id());
            cx.update(|window, cx| window.click(input_id, cx));
            cx.update(|window, cx| window.input("hola", cx));
            let text = cx.read(|cx| doc.read(cx).text().to_string());
            assert!(
                text.contains("hola"),
                "el editor debe recibir lo tecleado; texto: {text:?}"
            );
        });
    }
}
