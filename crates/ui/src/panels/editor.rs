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
use gpui_kit::component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogDescription, DialogFooter};
use gpui_kit::component::input::{
    self as kit_input, Editor, EditorState, Input, InputEvent, InputState, Position,
    Replace as ReplaceAction, Search as SearchAction, TabSize,
};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use lsx_core::fs::{FsEvent, FsService};

use crate::panels::tree::{file_icon, file_icon_color};
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
        CloseTab,
        ToggleComment,
        AskAiAboutSelection,
        SaveAll
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
    /// Línea/columna del cursor del documento activo (1-based). Lo consume el
    /// shell para la status bar y para el scroll sync (M1-C5).
    CursorMoved {
        /// Ruta relativa con `/`.
        path: String,
        /// Línea 1-based.
        line: u32,
        /// Columna 1-based.
        column: u32,
    },
    /// Mensaje corto para la status bar del shell.
    Status { message: String },
    /// El usuario pulsó «Nuevo proyecto» en el estado vacío.
    NewProjectRequested,
    /// El usuario pulsó «Abrir carpeta» en el estado vacío.
    OpenProjectRequested,
    /// Bienvenida: abrir un proyecto reciente.
    OpenRecentRequested { root: PathBuf },
    /// Bienvenida: «Abrir archivo» (Ctrl+P).
    QuickOpenRequested,
    /// Bienvenida: «Configurar IA».
    ConfigureAiRequested,
    /// «Preguntar a la IA»: adjunta la selección (o la línea) al chat.
    AskAi {
        /// Ruta relativa con `/`.
        path: String,
        /// Línea 1-based del cursor.
        line: u32,
        /// Texto seleccionado (vacío si no había selección).
        selection: String,
    },
}

/// Configuración de montaje del panel.
#[derive(Debug, Clone)]
pub struct EditorPanelConfig {
    /// Autosave (off por defecto, M1-B5).
    pub autosave: AutosaveConfig,
    /// Cada cuántos segundos se escribe un backup de los documentos sucios.
    pub backup_interval_secs: u64,
    /// Restaurar pestañas/cursores guardados al montar (M1-B1).
    pub restore_session: bool,
    /// Tipografía, tabulación y ajuste de línea (`editor.*` de settings).
    pub prefs: EditorPrefs,
}

/// Preferencias visuales del editor (`editor.fontFamily/fontSize/tabSize/softWrap`).
#[derive(Debug, Clone, PartialEq)]
pub struct EditorPrefs {
    /// Familia monoespaciada; `None` = la mono del tema.
    pub font_family: Option<SharedString>,
    /// Tamaño en px lógicos.
    pub font_size: f32,
    /// Espacios por tabulación.
    pub tab_size: usize,
    /// Ajuste de línea.
    pub soft_wrap: bool,
}

impl Default for EditorPrefs {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 14.,
            tab_size: 2,
            soft_wrap: false,
        }
    }
}

impl EditorPrefs {
    fn tab(&self) -> TabSize {
        TabSize {
            tab_size: self.tab_size.clamp(1, 16),
            hard_tabs: false,
        }
    }
}

impl Default for EditorPanelConfig {
    fn default() -> Self {
        Self {
            autosave: AutosaveConfig::default(),
            backup_interval_secs: 5,
            restore_session: true,
            prefs: EditorPrefs::default(),
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
    /// Observador del `notify` del editor: detecta cambios de cursor (scroll
    /// sync / status bar, M1-C5).
    _cursor_obs: Subscription,
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
    prefs: EditorPrefs,
    backup_interval_secs: u64,
    seconds: u64,
    session_dirty: bool,
    /// Última señal de cursor emitida (`ruta`, línea, columna) para no repetir
    /// `CursorMoved` en cada `notify` (el cursor parpadea y notifica a menudo).
    cursor_sig: Option<(String, u32, u32)>,
    /// Sección LaTeX que contiene el cursor (breadcrumb), recalculada al
    /// cambiar de línea.
    breadcrumb_section: Option<String>,
    /// Proyectos recientes (nombre, raíz) para la bienvenida.
    recent: Vec<(String, PathBuf)>,
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

        let find_query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(rust_i18n::t!("ui.app.text.buscar_2").to_string())
        });
        let find_replace = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(rust_i18n::t!("ui.app.text.reemplazar_por").to_string())
        });

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
            KeyBinding::new("ctrl-/", ToggleComment, None),
            KeyBinding::new("ctrl-alt-s", SaveAll, None),
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
            prefs: config.prefs.clone(),
            backup_interval_secs: config.backup_interval_secs.max(1),
            seconds: 0,
            session_dirty: false,
            cursor_sig: None,
            breadcrumb_section: None,
            recent: Vec::new(),
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
        let language = language_for(&rel);
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .line_number(true)
                .soft_wrap(self.prefs.soft_wrap)
                .tab_size(self.prefs.tab())
                .default_value(content.clone())
        });
        let doc_id = self.ids.next_id();
        let sub = cx.subscribe(&editor, move |this, _editor, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_document_edited(doc_id, cx);
            }
        });
        let meta = DocumentMeta::new(doc_id, &rel, text_file.eol, text_file.had_bom);
        // El cursor del editor notifica a menudo (parpadeo, selección): el
        // observador filtra por `(ruta, línea)` antes de emitir.
        let cursor_obs = cx.observe(&editor, |this, _editor, cx| this.on_editor_notify(cx));
        self.docs.push(OpenDocument {
            meta,
            editor,
            diagnostics: Vec::new(),
            last_backup_version: None,
            _sub: sub,
            _cursor_obs: cursor_obs,
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

    /// Proyectos recientes que lista la pantalla de bienvenida.
    pub fn set_recent_projects(&mut self, recent: Vec<(String, PathBuf)>, cx: &mut Context<Self>) {
        self.recent = recent;
        cx.notify();
    }

    /// Comenta o descomenta (`% `) las líneas que toca la selección (Ctrl+/).
    pub fn toggle_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        self.docs[ix].editor.update(cx, |state, cx| {
            let text = state.value().to_string();
            let range = state.selected_range();
            let (start, end) = line_span(&text, range.start, range.end);
            let toggled = toggle_comment_lines(&text[start..end]);
            state.set_selected_range(start..end, cx);
            state.replace(toggled, window, cx);
        });
    }

    /// «Preguntar a la IA»: emite la selección (o la línea del cursor) al shell.
    pub fn ask_ai_about_selection(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        let state = self.docs[ix].editor.read(cx);
        let selection = state.selected_value().to_string();
        let line = state.cursor_position().line + 1;
        let path = rel_key(&self.docs[ix].meta.rel);
        cx.emit(EditorPanelEvent::AskAi {
            path,
            line,
            selection,
        });
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

    /// Aplica tipografía, tabulación y ajuste de línea a todas las pestañas.
    pub fn set_preferences(
        &mut self,
        prefs: EditorPrefs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prefs == prefs {
            return;
        }
        for doc in &self.docs {
            doc.editor.update(cx, |editor, cx| {
                editor.set_soft_wrap(prefs.soft_wrap, window, cx);
                editor.set_tab_size(prefs.tab(), cx);
            });
        }
        self.prefs = prefs;
        cx.notify();
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

    /// Desplaza el documento que contiene `line` **sin mover el cursor ni
    /// robar el foco**: es el scroll sync inverso (PDF → editor, M1-C5).
    ///
    /// Si `file` está fuera de la raíz o no está abierto, no hace nada (el
    /// scroll sync nunca abre pestañas por su cuenta). Cambia de pestaña si el
    /// origen pertenece a otro documento ya abierto.
    pub fn scroll_to_source(
        &mut self,
        file: &Path,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rel = file
            .strip_prefix(&self.root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| file.to_path_buf());
        let Some(ix) = self.index_by_rel(&rel) else {
            return;
        };
        if self.active != Some(ix) {
            self.set_active_index(ix, false, window, cx);
        }
        self.scroll_to_line(line, cx);
    }

    /// Desplaza el documento activo para dejar `line` (1-based) en la parte
    /// superior del viewport **sin mover el cursor**: es el scroll sync
    /// inverso (PDF → editor, M1-C5), que no debe interrumpir lo que escribes.
    pub fn scroll_to_line(&mut self, line: u32, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        let line = line.max(1);
        self.docs[ix].editor.update(cx, |state, cx| {
            let line_height = state.line_height().unwrap_or(px(18.));
            let y = -f32::from(line_height) * (line - 1) as f32;
            state.set_scroll_offset(point(px(0.), px(y)), cx);
        });
    }

    /// Observador del editor: emite [`EditorPanelEvent::CursorMoved`] cuando el
    /// cursor del documento activo cambia de línea/columna (o de archivo). El
    /// editor notifica también por parpadeo del cursor, así que se filtra por
    /// señal.
    fn on_editor_notify(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some((line, column))) = (self.active_path(), self.active_cursor(cx))
        else {
            return;
        };
        let sig = (path.clone(), line, column);
        if self.cursor_sig.as_ref() == Some(&sig) {
            return;
        }
        let line_changed = self
            .cursor_sig
            .as_ref()
            .is_none_or(|(old_path, old_line, _)| *old_path != path || *old_line != line);
        self.cursor_sig = Some(sig);
        if line_changed {
            self.breadcrumb_section = self.active.and_then(|ix| {
                let text = self.docs[ix].editor.read(cx).value();
                section_at(&text, line)
            });
            cx.notify();
        }
        cx.emit(EditorPanelEvent::CursorMoved { path, line, column });
    }

    /// Texto del documento activo (tests/QA: comprobar inserciones).
    pub fn active_text(&self, cx: &App) -> Option<String> {
        let ix = self.active?;
        Some(self.docs[ix].editor.read(cx).text().to_string())
    }

    /// `true` si el documento activo tiene cambios sin guardar.
    pub fn active_is_dirty(&self) -> bool {
        self.active
            .and_then(|ix| self.docs.get(ix))
            .is_some_and(|doc| doc.meta.is_dirty())
    }

    /// Número de documentos abiertos con cambios sin guardar.
    pub fn dirty_count(&self) -> usize {
        self.docs.iter().filter(|doc| doc.meta.is_dirty()).count()
    }

    /// Guarda todos los documentos sucios. `false` si alguno falló (conflicto,
    /// E/S); el llamador decide si puede cerrar la ventana.
    pub fn save_all(&mut self, cx: &mut Context<Self>) -> bool {
        let dirty: Vec<usize> = self
            .docs
            .iter()
            .enumerate()
            .filter(|(_, doc)| doc.meta.is_dirty())
            .map(|(ix, _)| ix)
            .collect();
        for ix in dirty {
            if !self.save_doc(ix, cx) {
                return false;
            }
        }
        true
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
                .child(
                    DialogDescription::new()
                        .child(rust_i18n::t!("ui.app.text.external_conflict").to_string()),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("conflict-diff")
                                .label(rust_i18n::t!("ui.app.text.ver_diff").to_string())
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
                                .label(rust_i18n::t!("ui.app.text.recargar").to_string())
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
                                .label(rust_i18n::t!("ui.app.text.sobrescribir").to_string())
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
                                .label(rust_i18n::t!("ui.app.text.cerrar").to_string())
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("diff-reload")
                                .label(rust_i18n::t!("ui.app.text.recargar").to_string())
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
                            Button::new("diff-overwrite")
                                .label(rust_i18n::t!("ui.app.text.sobrescribir").to_string())
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
                .title(rust_i18n::t!("ui.app.text.cambios_sin_guardar_encontrados").to_string())
                .width(px(560.))
                .child(
                    DialogDescription::new()
                        .child(rust_i18n::t!("ui.app.text.recovery_body").to_string()),
                )
                .child(v_flex().gap_1().children(rows))
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("recovery-discard")
                                .label(rust_i18n::t!("ui.app.text.descartar").to_string())
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
                                .label(rust_i18n::t!("ui.app.text.recuperar").to_string())
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
                    DialogDescription::new().child(
                        rust_i18n::t!("ui.app.text.quieres_guardar_antes_de_cerrar_la_pestana")
                            .to_string(),
                    ),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("close-cancel")
                                .label(rust_i18n::t!("ui.app.text.cancelar").to_string())
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("close-discard")
                                .label(rust_i18n::t!("ui.app.text.descartar").to_string())
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
                                .label(rust_i18n::t!("ui.app.text.guardar_y_cerrar").to_string())
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
                let icon = file_icon(&doc.meta.display_name());
                let icon_color = file_icon_color(icon, cx);
                // Pestaña activa al estilo VS Code: barra de acento arriba.
                // En taffy el `absolute` se ancla al padre directo: el prefijo ocupa
                // todo el alto de la pestaña (32 px en el kit) y la barra se estira de
                // sobra; el `overflow_hidden` de la pestaña la recorta a su ancho.
                let prefix = div()
                    .relative()
                    .h(px(TAB_HEIGHT))
                    .flex()
                    .items_center()
                    .child(Icon::new(icon).size(px(14.)).text_color(icon_color))
                    .when(ix == active, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top_0()
                                .left(px(-48.))
                                .w(px(800.))
                                .h(px(2.))
                                .bg(cx.theme().primary),
                        )
                    });
                Tab::new()
                    .prefix(prefix)
                    .label(label)
                    .aria_label(doc.meta.display_path())
                    .on_click(cx.listener(move |this, _, window, cx| this.activate(ix, window, cx)))
                    .suffix(
                        Button::new(SharedString::from(format!("tab-close-{}", doc.meta.id.0)))
                            .icon(IconName::X)
                            .xsmall()
                            .ghost()
                            .tooltip(rust_i18n::t!("ui.app.text.cerrar_pestana_ctrl_w").to_string())
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

    /// Breadcrumb `carpeta › archivo › sección` bajo las pestañas (VS Code).
    fn render_breadcrumb(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let doc = self.active.and_then(|ix| self.docs.get(ix))?;
        let path = rel_key(&doc.meta.rel);
        let mut items: Vec<BreadcrumbItem> = path
            .split('/')
            .map(|part| BreadcrumbItem::new(part.to_owned()))
            .collect();
        if let Some(section) = &self.breadcrumb_section {
            items.push(BreadcrumbItem::new(section.clone()));
        }
        Some(
            h_flex()
                .flex_none()
                .h(px(22.))
                .px_3()
                .items_center()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(Breadcrumb::new().children(items))
                .into_any_element(),
        )
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
                            .tooltip(rust_i18n::t!("ui.app.text.anterior_shift_f3").to_string())
                            .on_click(cx.listener(|this, _, _window, cx| this.find_previous(cx))),
                    )
                    .child(
                        Button::new("find-next")
                            .icon(IconName::ChevronDown)
                            .small()
                            .ghost()
                            .tooltip(rust_i18n::t!("ui.app.text.siguiente_f3").to_string())
                            .on_click(cx.listener(|this, _, _window, cx| this.find_next(cx))),
                    )
                    .child(
                        Button::new("find-case")
                            .icon(IconName::CaseSensitive)
                            .small()
                            .when(!case_active, |button| button.ghost())
                            .tooltip(
                                rust_i18n::t!("ui.app.text.distinguir_mayusculas_y_minusculas")
                                    .to_string(),
                            )
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
                            .tooltip(
                                rust_i18n::t!("ui.app.text.mostrar_reemplazo_ctrl_h").to_string(),
                            )
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
                            .tooltip(rust_i18n::t!("ui.app.text.cerrar_la_busqueda").to_string())
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
                                .label(rust_i18n::t!("ui.app.text.reemplazar").to_string())
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.find_replace_current(window, cx)
                                })),
                        )
                        .child(
                            Button::new("find-replace-all")
                                .label(rust_i18n::t!("ui.app.text.reemplazar_todo").to_string())
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.find_replace_all(window, cx)
                                })),
                        ),
                );
        }

        bar.into_any_element()
    }

    /// Pantalla de bienvenida (sin documentos abiertos), al estilo de VS Code:
    /// acciones para empezar, proyectos recientes y atajos clave.
    fn render_empty(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let link = cx.theme().primary;
        let section_title = |text: &str| {
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .pb_1()
                .child(text.to_owned())
        };
        let action = |id: &'static str, icon: IconName, label: &str, event: EditorPanelEvent| {
            h_flex()
                .id(id)
                .gap_2()
                .items_center()
                .py(px(3.))
                .text_size(px(13.))
                .text_color(link)
                .cursor_pointer()
                .hover(|style| style.underline())
                .child(Icon::new(icon).size(px(15.)))
                .child(label.to_owned())
                .on_click(cx.listener(move |_this, _, _window, cx| cx.emit(event.clone())))
        };

        let start = v_flex()
            .gap(px(2.))
            .child(section_title(&rust_i18n::t!("ui.app.text.empezar")))
            .child(action(
                "welcome-new-project",
                IconName::FolderPlus,
                &rust_i18n::t!("ui.app.text.nuevo_proyecto"),
                EditorPanelEvent::NewProjectRequested,
            ))
            .child(action(
                "welcome-open-folder",
                IconName::FolderOpen,
                &rust_i18n::t!("ui.app.text.abrir_carpeta_2"),
                EditorPanelEvent::OpenProjectRequested,
            ))
            .child(action(
                "welcome-quick-open",
                IconName::Search,
                &rust_i18n::t!("ui.app.text.abrir_archivo"),
                EditorPanelEvent::QuickOpenRequested,
            ))
            .child(action(
                "welcome-configure-ai",
                IconName::Bot,
                &rust_i18n::t!("ui.app.text.configurar_el_asistente_ia"),
                EditorPanelEvent::ConfigureAiRequested,
            ));

        let mut recent = v_flex()
            .gap(px(2.))
            .child(section_title(&rust_i18n::t!("ui.app.text.recientes")));
        if self.recent.is_empty() {
            recent =
                recent.child(div().text_size(px(13.)).text_color(muted).child(
                    rust_i18n::t!("ui.app.text.aun_no_hay_proyectos_recientes").to_string(),
                ));
        }
        for (ix, (name, root)) in self.recent.iter().take(6).enumerate() {
            let event = EditorPanelEvent::OpenRecentRequested { root: root.clone() };
            recent = recent.child(
                h_flex()
                    .id(("welcome-recent", ix))
                    .gap_2()
                    .items_baseline()
                    .py(px(3.))
                    .text_size(px(13.))
                    .cursor_pointer()
                    .child(
                        div()
                            .text_color(link)
                            .hover(|style| style.underline())
                            .child(name.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child(root.display().to_string()),
                    )
                    .on_click(cx.listener(move |_this, _, _window, cx| cx.emit(event.clone()))),
            );
        }

        let hint = |keys: &str, label: &str| {
            h_flex()
                .gap_2()
                .justify_between()
                .w(px(300.))
                .text_size(px(12.))
                .child(div().text_color(muted).child(label.to_owned()))
                .child(
                    div()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(keys.to_owned()),
                )
        };

        v_flex()
            .id("editor-welcome")
            .size_full()
            .overflow_y_scroll()
            .items_center()
            .justify_center()
            .gap_6()
            .p_6()
            .text_color(cx.theme().foreground)
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(26.))
                            .font_weight(FontWeight::LIGHT)
                            .child("LaTeX Studio"),
                    )
                    .child(
                        div().text_size(px(13.)).text_color(muted).child(
                            rust_i18n::t!(
                                "ui.app.text.escribe_compila_y_corrige_latex_con_ayuda_de_la"
                            )
                            .to_string(),
                        ),
                    ),
            )
            .child(
                h_flex()
                    .gap_12()
                    .items_start()
                    .child(start)
                    .child(div().max_w(px(420.)).child(recent)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(hint(
                        "Ctrl+P",
                        &rust_i18n::t!("ui.app.text.abrir_archivo_2"),
                    ))
                    .child(hint(
                        "Ctrl+Shift+P",
                        &rust_i18n::t!("ui.app.text.paleta_de_comandos"),
                    ))
                    .child(hint("Ctrl+B", &rust_i18n::t!("ui.app.text.compilar")))
                    .child(hint("F1", &rust_i18n::t!("ui.app.text.todos_los_atajos"))),
            )
            .into_any_element()
    }
}

/// Alto de la pestaña del kit (`TabVariant::Tab`, tamaño medio).
const TAB_HEIGHT: f32 = 32.;

/// Menú contextual del editor (nativo del SO). Las acciones viajan por el
/// árbol de foco: las de edición las atiende el editor del kit y las de
/// SyncTeX/IA el shell.
fn editor_context_menu(menu: NativeMenu) -> NativeMenu {
    menu.menu(
        rust_i18n::t!("ui.app.text.cortar").to_string(),
        Box::new(kit_input::Cut),
    )
    .menu(
        rust_i18n::t!("ui.app.text.copiar").to_string(),
        Box::new(kit_input::Copy),
    )
    .menu(
        rust_i18n::t!("ui.app.text.pegar").to_string(),
        Box::new(kit_input::Paste),
    )
    .menu(
        rust_i18n::t!("ui.app.text.seleccionar_todo").to_string(),
        Box::new(kit_input::SelectAll),
    )
    .separator()
    .menu(
        rust_i18n::t!("ui.app.text.comentar_o_descomentar_lineas").to_string(),
        Box::new(ToggleComment),
    )
    .separator()
    .menu(
        rust_i18n::t!("ui.app.text.ir_a_esta_linea_en_el_pdf").to_string(),
        Box::new(crate::app::SyncForward),
    )
    .menu(
        rust_i18n::t!("ui.app.text.preguntar_a_la_ia_sobre_la_seleccion").to_string(),
        Box::new(AskAiAboutSelection),
    )
    .menu(
        rust_i18n::t!("ui.app.text.arreglar_errores_con_ia").to_string(),
        Box::new(crate::app::FixWithAi),
    )
}

/// Rango de bytes `[inicio de la primera línea, fin de la última línea]` que
/// cubre la selección `start..end` (el salto final no se incluye).
fn line_span(text: &str, start: usize, end: usize) -> (usize, usize) {
    let start = start.min(text.len());
    let mut end = end.min(text.len()).max(start);
    // Una selección que acaba justo al principio de una línea no la incluye.
    if end > start && text[..end].ends_with('\n') {
        end -= 1;
    }
    let line_start = text[..start].rfind('\n').map_or(0, |ix| ix + 1);
    let line_end = text[end..].find('\n').map_or(text.len(), |ix| end + ix);
    let line_end = if line_end > line_start && text[..line_end].ends_with('\r') {
        line_end - 1
    } else {
        line_end
    };
    (line_start, line_end.max(line_start))
}

/// Comenta (`% `) o descomenta un bloque de líneas. Si todas las líneas con
/// contenido ya están comentadas, las descomenta; si no, comenta todas.
fn toggle_comment_lines(block: &str) -> String {
    let lines: Vec<&str> = block.split('\n').collect();
    let has_content = |line: &&&str| !line.trim().is_empty();
    let all_commented = lines
        .iter()
        .filter(has_content)
        .all(|line| line.trim_start().starts_with('%'));
    let indent = lines
        .iter()
        .filter(has_content)
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                (*line).to_owned()
            } else if all_commented {
                let lead = line.len() - line.trim_start().len();
                let rest = &line[lead + 1..];
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                format!("{}{rest}", &line[..lead])
            } else {
                format!("{}% {}", &line[..indent], &line[indent..])
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Título de la sección (`\chapter`, `\section`, `\subsection`…) que contiene
/// la línea `line` (1-based), para el breadcrumb.
fn section_at(text: &str, line: u32) -> Option<String> {
    const COMMANDS: [&str; 5] = [
        "\\chapter",
        "\\section",
        "\\subsection",
        "\\subsubsection",
        "\\paragraph",
    ];
    let mut found = None;
    for raw in text.split('\n').take(line as usize) {
        let line = raw.trim_start();
        if line.starts_with('%') {
            continue;
        }
        for command in COMMANDS {
            let Some(rest) = line.strip_prefix(command) else {
                continue;
            };
            let rest = rest.strip_prefix('*').unwrap_or(rest);
            let Some(rest) = rest.strip_prefix('{') else {
                continue;
            };
            let title: String = rest.chars().take_while(|c| *c != '}').collect();
            if !title.trim().is_empty() {
                found = Some(title.trim().to_owned());
            }
            break;
        }
    }
    found
}

/// Lenguaje del editor según la extensión: LaTeX para la familia TeX
/// (`.tex`, `.sty`, `.cls`…) y texto plano para el resto (`.bib`, `.txt`…).
fn language_for(rel: &Path) -> &'static str {
    let ext = rel
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "tex" | "ltx" | "sty" | "cls" | "dtx" | "ins" | "def" | "cfg" | "clo" | "bbx" | "cbx"
        | "lbx" => latex_language::LATEX_LANGUAGE_NAME,
        "json" => "json",
        _ => "text",
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
        let breadcrumb = self.render_breadcrumb(cx);
        let body = match self.active.and_then(|ix| self.docs.get(ix)) {
            Some(doc) => div()
                .id("editor-body")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_hidden()
                .text_size(px(self.prefs.font_size))
                .when_some(self.prefs.font_family.clone(), |this, family| {
                    this.font_family(family)
                })
                .child(
                    Editor::new(&doc.editor)
                        .bordered(false)
                        .h(relative(1.0))
                        .context_menu(|menu, _window, _cx| editor_context_menu(menu)),
                )
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
            .on_action(cx.listener(|this, _: &ToggleComment, window, cx| {
                this.toggle_comment(window, cx);
            }))
            .on_action(cx.listener(|this, _: &AskAiAboutSelection, _window, cx| {
                this.ask_ai_about_selection(cx);
            }))
            .on_action(cx.listener(|this, _: &SaveAll, _window, cx| {
                this.save_all(cx);
            }))
            .on_action(cx.listener(|this, _: &SearchAction, window, cx| {
                this.toggle_find(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ReplaceAction, window, cx| {
                this.toggle_find(true, window, cx);
            }))
            .when_some(tabs, |this, tabs| this.child(tabs))
            .when_some(breadcrumb, |this, crumbs| this.child(crumbs))
            .when_some(find_bar, |this, find| this.child(find))
            .child(body)
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
        ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window, px,
    };

    use super::{EditorPanel, language_for, line_span, section_at, toggle_comment_lines};
    use crate::layout::SlotKind;
    use crate::panels::pane::{PaneState, pane_container};

    #[test]
    fn toggle_comment_comments_and_uncomments_blocks() {
        // Como VS Code: el `%` va en la sangría mínima del bloque.
        assert_eq!(toggle_comment_lines("a\n  b"), "% a\n%   b");
        assert_eq!(toggle_comment_lines("  x\n  y"), "  % x\n  % y");
        assert_eq!(toggle_comment_lines("% a\n\n  % b"), "a\n\n  b");
        // Mezcla: comenta todo.
        assert_eq!(toggle_comment_lines("% a\nb"), "% % a\n% b");
    }

    #[test]
    fn line_span_covers_whole_lines() {
        let text = "uno\ndos\ntres";
        assert_eq!(line_span(text, 5, 5), (4, 7));
        assert_eq!(line_span(text, 1, 6), (0, 7));
        // Selección que acaba al inicio de la línea siguiente.
        assert_eq!(line_span(text, 0, 4), (0, 3));
        assert_eq!(line_span("a\r\nb", 0, 0), (0, 1));
    }

    #[test]
    fn section_at_finds_the_enclosing_heading() {
        let text = "\\documentclass{x}\n\\section{Intro}\ntexto\n% \\section{No}\n\\subsection*{Datos}\nmás";
        assert_eq!(section_at(text, 1), None);
        assert_eq!(section_at(text, 3).as_deref(), Some("Intro"));
        assert_eq!(section_at(text, 4).as_deref(), Some("Intro"));
        assert_eq!(section_at(text, 6).as_deref(), Some("Datos"));
    }

    #[test]
    fn language_follows_the_extension() {
        assert_eq!(language_for(Path::new("main.tex")), "latex");
        assert_eq!(language_for(Path::new("estilo/Mio.STY")), "latex");
        assert_eq!(language_for(Path::new("refs.bib")), "text");
        assert_eq!(language_for(Path::new("notas.txt")), "text");
    }

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

    /// Proyecto temporal con un `main.tex` mínimo (una carpeta por test: dos
    /// tests del mismo binario corren en paralelo).
    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lsx-ui-editor-{tag}-{}", std::process::id()));
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
        let root = temp_project("foco");
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

    #[test]
    fn dirty_count_y_save_all_cierran_el_flujo_de_ventana() {
        let root = temp_project("dirty-all");
        with_editor(&root, &mut TestAppContext::single(), |cx, editor| {
            let doc = cx.read(|cx| editor.read(cx).docs[0].editor.clone());
            let input_id = ("input", doc.entity_id());
            cx.update(|window, cx| window.click(input_id, cx));
            cx.update(|window, cx| window.input("cambio", cx));
            assert_eq!(cx.read(|cx| editor.read(cx).dirty_count()), 1);
            let saved = cx.update(|_, cx| editor.update(cx, |panel, cx| panel.save_all(cx)));
            assert!(saved, "save_all debe guardar el documento sucio");
            assert_eq!(cx.read(|cx| editor.read(cx).dirty_count()), 0);
        });
    }

    /// El scroll sync inverso (PDF → editor, M1-C5) desplaza el documento sin
    /// mover el cursor.
    #[test]
    fn scroll_to_line_desplaza_sin_mover_el_cursor() {
        let root = temp_project("scroll");
        // Documento con líneas suficientes para poder desplazarse.
        let body: String = (1..=200).map(|n| format!("% línea {n}\n")).collect();
        std::fs::write(root.join("main.tex"), body).expect("main.tex largo");
        with_editor(&root, &mut TestAppContext::single(), |cx, editor| {
            let doc = cx.read(|cx| editor.read(cx).docs[0].editor.clone());
            let before = cx.read(|cx| doc.read(cx).cursor_position());
            cx.update(|window, cx| {
                let _ = window;
                editor.update(cx, |panel, cx| panel.scroll_to_line(120, cx));
            });
            cx.update(|window, cx| window.render_frame(cx));
            cx.update(|window, cx| window.render_frame(cx));
            let (offset, before_after) = cx.read(|cx| {
                let state = doc.read(cx);
                (state.scroll_offset().y, state.cursor_position())
            });
            assert!(
                offset < px(-100.),
                "el documento debe quedar desplazado; offset: {offset:?}"
            );
            assert_eq!(before, before_after, "el scroll sync no mueve el cursor");
        });
    }
}
