//! `AppRoot` — shell de producción (M1-A1..A5).
//!
//! Composición de arriba abajo:
//!
//! ```text
//! TitleBar  (controles de ventana del SO + título)
//! Toolbar   proyecto ▾ · ▶ compilar · motor … toggles de hoja · ajustes · palette
//! DockArea  h_split [árbol | editor | pdf | chat]   ← resizable, 4 hojas fijas
//! StatusBar Ln/Col · palabras · página · compilación · dot IA
//! ```
//!
//! Responsabilidades:
//! - **M1-A1**: dock real, colapso por hoja (`Panel::visible`), focus mode,
//!   atajos `Ctrl+Alt+1/3/4/V/M` y persistencia con debounce en
//!   `settings.ui.dock` (esquema §7) + restauración al arrancar.
//! - **M1-A2**: toolbar y status bar con estado por canal
//!   ([`crate::status::StatusMessage`]).
//! - **M1-A4**: notificaciones y modal de permiso
//!   ([`crate::permission::PermissionCard`], botón demo).
//! - **M1-A5**: command palette (kit `command`) y ajustes mínimos.
//!
//! Los paneles no conocen este tipo: emiten [`PaneEvent`]s y el shell decide.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use camino::{Utf8Path, Utf8PathBuf};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _, TitleBar, WindowExt as _,
    button::{Button, ButtonVariants as _, DropdownButton},
    command::{Command, CommandState},
    dialog::DialogDescription,
    dock::{DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, PanelInfo, panel_handle},
    h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, Styled as _, Subscription, Task, WeakEntity,
    Window, div, px,
};
use lsx_core::config::{ConfigWarning, Settings};
use lsx_core::root::PortableRoot;
use lsx_latex::{
    CompileController, CompileEvent, CompileStatus, Diagnostic, LatexEngine, LatexSettings,
    TexLiveEngine, detect_main,
};

use crate::layout::{FocusMode, ShellLayout, SlotKind};
use crate::notifications;
use crate::palette::PaletteCommand;
use crate::panels::bottom::BottomPanel;
use crate::panels::chat::{ChatPanel, ChatPanelConfig, ChatPanelEvent};
use crate::panels::editor::{EditorPanel, EditorPanelConfig, EditorPanelEvent};
use crate::panels::pdf::{PdfPanel, PdfPanelEvent, SyncRect};
use crate::panels::problems::{ProblemsEvent, ProblemsPanel};
use crate::panels::symbols::SymbolsPanel;
use crate::panels::tree::{TreeFsEvent, TreeOpenKind, TreePanelEvent};
use crate::panels::{Pane, PaneEvent, PaneState, TreePanel, pane_container};
use crate::permission::{PermissionCard, PermissionRequest};
use crate::project::ProjectService;
use crate::settings_ui;
use crate::status::{AiState, StatusHandle, StatusMessage, StatusModel};
use crate::{status_bar, theme, toolbar};

/// Debounce del guardado del layout (ms) tras `DockEvent::LayoutChanged`.
const LAYOUT_SAVE_DEBOUNCE: Duration = Duration::from_millis(450);
/// Quietud del cursor/scroll antes de lanzar un SyncTeX del scroll sync (ms).
const SYNC_DEBOUNCE: Duration = Duration::from_millis(250);
/// Ventana en la que se ignoran los avisos de scroll del PDF tras un scroll
/// programático (anti-bucle del scroll sync, M1-C5).
const SYNC_LOOP_GUARD: Duration = Duration::from_millis(400);
/// Espera máxima del debounce del forward: si el cursor no deja de moverse
/// (p. ej. escribiendo un párrafo), el sync se aplica igualmente.
const SYNC_MAX_WAIT: Duration = Duration::from_millis(1500);
/// Ancho de referencia cuando el dock aún no se ha medido (primer frame).
const DEFAULT_REFERENCE_WIDTH: Pixels = px(1600.);

gpui_kit::actions!(
    lsx_ui_shell,
    [
        ToggleTreePane,
        TogglePdfPane,
        ToggleChatPane,
        ToggleFocusMode,
        SyncForward,
        OpenPalette,
        OpenSettingsPane
    ]
);

/// Título de la ventana de error de arranque (la usa `app/src/main.rs`).
pub fn root_error_title() -> String {
    rust_i18n::t!("ui.app.window.root_error_title").to_string()
}

/// Pista de la ventana de error de arranque.
pub fn root_error_hint() -> String {
    rust_i18n::t!("ui.app.window.root_error_hint").to_string()
}

/// Por qué se está resolviendo un SyncTeX inverso (PDF → `.tex`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum InverseReason {
    /// Ctrl+clic sobre el visor: salta y mueve el cursor.
    #[default]
    Click,
    /// Scroll sync: desplaza el editor sin tocar el cursor.
    Scroll,
}

/// ¿Procede aplicar el scroll sync inverso (PDF → editor)?
///
/// Falso si está desactivado o si el visor se acaba de desplazar por un sync
/// hacia el PDF: sin esta guarda, el scroll programático reentraría como scroll
/// del usuario y las dos direcciones se perseguirían (M1-C5).
fn reverse_allowed(enabled: bool, last_forward: Option<Instant>, now: Instant) -> bool {
    enabled && last_forward.is_none_or(|at| now.duration_since(at) >= SYNC_LOOP_GUARD)
}

/// Registra los atajos globales del shell (una vez, en `lsx_ui::init`).
pub fn register_keybindings(cx: &mut App) {
    use gpui_kit::KeyBinding;
    cx.bind_keys([
        KeyBinding::new("ctrl-alt-1", ToggleTreePane, None),
        KeyBinding::new("ctrl-alt-3", TogglePdfPane, None),
        KeyBinding::new("ctrl-alt-v", TogglePdfPane, None),
        KeyBinding::new("ctrl-alt-4", ToggleChatPane, None),
        KeyBinding::new("ctrl-alt-m", ToggleFocusMode, None),
        KeyBinding::new("ctrl-alt-j", SyncForward, None),
        KeyBinding::new("ctrl-p", OpenPalette, None),
        KeyBinding::new("ctrl-,", OpenSettingsPane, None),
    ]);
}

/// Vista raíz de la aplicación.
pub struct AppRoot {
    portable: PortableRoot,
    settings: Settings,
    warnings: Vec<ConfigWarning>,
    project: ProjectService,
    status_model: StatusModel,
    status_handle: StatusHandle,
    layout: ShellLayout,
    active: SlotKind,
    dock: Entity<DockArea>,
    tree: Entity<TreePanel>,
    editor: Entity<EditorSlot>,
    editor_panel: Entity<EditorPanel>,
    pdf: Entity<PdfSlot>,
    pdf_panel: Entity<PdfPanel>,
    last_pdf: Option<PathBuf>,
    chat: Entity<ChatSlot>,
    problems: Entity<ProblemsPanel>,
    project_root: Option<PathBuf>,
    latex_settings: LatexSettings,
    compile: Option<Arc<CompileController>>,
    /// Scroll sync automático `.tex` ↔ PDF (`latex.syncScroll`, M1-C5).
    sync_scroll: bool,
    /// Secuencia del debounce del forward (la última señal gana).
    sync_seq: u64,
    /// Inicio de la ráfaga de movimientos de cursor pendiente de sync.
    sync_burst_start: Option<Instant>,
    /// Secuencia del debounce del inverso (la última señal gana).
    reverse_seq: u64,
    /// Último destino ya sincronizado hacia el PDF (`ruta`, línea).
    last_synced: Option<(String, u32)>,
    /// Instante del último scroll programático del PDF (guarda anti-bucle).
    last_forward_sync: Option<Instant>,
    /// Motivo del próximo [`PdfPanelEvent::InverseJump`].
    pending_inverse: InverseReason,
    focus: FocusHandle,
    palette: Entity<CommandState>,
    layout_generation: u64,
    layout_dirty: bool,
    _status_task: Task<()>,
    _compile_task: Task<()>,
    _dock_events: Subscription,
    _pane_events: Vec<Subscription>,
    _integration_events: Vec<Subscription>,
}

impl AppRoot {
    /// Monta el shell completo. Ver el módulo para el orden de las piezas.
    pub fn new(
        portable: PortableRoot,
        settings: Settings,
        warnings: Vec<ConfigWarning>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (dock, _skin) = DockSkin::dock_area("latex-studio-dock", Some(1), window, cx);
        let (status_handle, status_rx) = crate::status::channel();

        let project_service = ProjectService::new(&portable);
        let startup = project_service.startup_project(&settings);

        let tree = cx.new(|cx| {
            TreePanel::new(
                window,
                startup.clone(),
                status_handle.clone(),
                Some(dock.downgrade()),
                cx,
            )
        });
        // Editor real (M1-B) + panel de problemas (M1-D) apilados en la hoja.
        let project_root = startup.as_ref().map(|project| project.root.clone());
        let editor_root = project_root
            .clone()
            .unwrap_or_else(|| portable.default_workspace());
        let chat_root = editor_root.clone();
        let editor_panel = cx.new(|cx| {
            EditorPanel::with_config(
                editor_root,
                portable.config_dir(),
                EditorPanelConfig::default(),
                window,
                cx,
            )
        });
        let problems = cx.new(|cx| ProblemsPanel::new(window, cx));
        // Dock inferior del explorador (M2): PROBLEMAS · SALIDA · SÍMBOLOS.
        let symbols = cx.new(|cx| SymbolsPanel::new(window, Some(editor_panel.clone()), cx));
        let bottom = cx.new(|cx| BottomPanel::new(problems.clone(), symbols, cx));
        tree.update(cx, |tree, cx| tree.attach_bottom(bottom.clone(), cx));
        let editor = cx.new(|cx| EditorSlot::new(editor_panel.clone(), Some(dock.downgrade()), cx));
        let pdf_panel = cx.new(|cx| PdfPanel::with_cache_mb(settings.pdf.max_cache_mb, cx));
        // `synctex.exe` de la distro portable: lo usan el salto inverso
        // (Ctrl+clic) y el scroll sync inverso (M1-C5). El shell reacciona a
        // `PdfPanelEvent::InverseJump` para decidir salto vs scroll.
        pdf_panel.update(cx, |panel, _cx| {
            panel.set_synctex_exe(portable.synctex_exe());
        });
        let pdf = cx.new(|cx| PdfSlot::new(pdf_panel.clone(), Some(dock.downgrade()), cx));
        // Chat real (M1-E3..E6): el panel arranca el supervisor si `ai.autoStart`
        // (ver `panels::chat`); en modo replay (`LSX_CHAT_REPLAY`) no hay server.
        let chat_project_name = startup
            .as_ref()
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "LaTeX Studio".to_owned());
        let chat_panel = cx.new(|cx| {
            ChatPanel::new(
                window,
                cx,
                ChatPanelConfig::from_root(
                    &portable,
                    &chat_root,
                    chat_project_name,
                    &settings.ai,
                    status_handle.clone(),
                    Some(editor_panel.clone()),
                    Some(problems.clone()),
                ),
            )
        });
        let chat_events = chat_panel.clone();
        let chat = cx.new(|cx| ChatSlot::new(chat_panel, Some(dock.downgrade()), cx));

        let mut dock_layout = DockLayout::h_split();
        for kind in SlotKind::ALL {
            let handle = match kind {
                SlotKind::Tree => panel_handle(tree.clone()),
                SlotKind::Editor => panel_handle(editor.clone()),
                SlotKind::Pdf => panel_handle(pdf.clone()),
                SlotKind::Chat => panel_handle(chat.clone()),
            };
            dock_layout = dock_layout.child(DockLayout::tabs().panel_view(handle, cx), None);
        }
        dock.update(cx, |dock, cx| {
            dock.set_center(dock_layout, window, cx);
            // I-7 v1: 4 hojas fijas; el lock desactiva drag/drop de grupos y
            // conserva el redimensionado de splits.
            dock.set_locked(true, window, cx);
        });

        let dock_events = cx.subscribe(&dock, |this, _dock, event: &DockEvent, cx| {
            if matches!(event, DockEvent::LayoutChanged) {
                this.on_dock_layout_changed(cx);
            }
        });
        let pane_events = vec![
            cx.subscribe_in(
                &tree,
                window,
                |this, _panel, event: &PaneEvent, window, cx| {
                    this.on_pane_event(*event, window, cx);
                },
            ),
            cx.subscribe_in(
                &editor,
                window,
                |this, _panel, event: &PaneEvent, window, cx| {
                    this.on_pane_event(*event, window, cx);
                },
            ),
            cx.subscribe_in(
                &pdf,
                window,
                |this, _panel, event: &PaneEvent, window, cx| {
                    this.on_pane_event(*event, window, cx);
                },
            ),
            cx.subscribe_in(
                &chat,
                window,
                |this, _panel, event: &PaneEvent, window, cx| {
                    this.on_pane_event(*event, window, cx);
                },
            ),
        ];

        let status_task = cx.spawn(async move |this, cx| {
            while let Ok(message) = status_rx.recv().await {
                let updated = this.update(cx, |root, cx| {
                    if root.status_model.apply(&message) {
                        cx.notify();
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        });

        // --- Compilación (integración M1): motor real + cola con debounce -----
        let latex_settings = bridge_latex_settings(&settings.latex);
        let sync_scroll = settings.latex.sync_scroll;
        let startup_main: Option<Utf8PathBuf> = project_root
            .as_deref()
            .and_then(Utf8Path::from_path)
            .and_then(|root| {
                detect_main(root, latex_settings.main_file.as_deref(), None)
                    .ok()
                    .flatten()
            });
        let compile: Option<Arc<CompileController>> = TexLiveEngine::detect().ok().map(|engine| {
            let engine: Arc<dyn LatexEngine> = Arc::new(engine);
            let controller = CompileController::with_settings(engine, &latex_settings);
            if let (Some(root), Some(main)) = (
                project_root.as_deref().and_then(Utf8Path::from_path),
                startup_main.as_ref(),
            ) {
                controller.set_request(latex_settings.request(root, main.clone()));
            }
            Arc::new(controller)
        });
        let compile_task = if let Some(controller) = &compile {
            let rx = controller.subscribe();
            cx.spawn(async move |this, cx| {
                while let Ok(event) = rx.recv().await {
                    if this
                        .update(cx, |root, cx| root.on_compile_event(event, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
        } else {
            cx.spawn(async move |_this, _cx| {})
        };

        let integration_events = vec![
            cx.subscribe_in(
                &tree,
                window,
                |this, _tree, event: &TreeFsEvent, window, cx| {
                    let TreeFsEvent::Batch(batch) = event;
                    let batch = batch.clone();
                    this.editor_panel.update(cx, |panel, cx| {
                        panel.apply_fs_batch(batch, window, cx);
                    });
                    if let Some(controller) = &this.compile {
                        controller.notify_change();
                    }
                },
            ),
            cx.subscribe_in(
                &pdf_panel,
                window,
                |this, _panel, event: &PdfPanelEvent, window, cx| {
                    this.on_pdf_event(event, window, cx);
                },
            ),
            cx.subscribe_in(
                &chat_events,
                window,
                |this, _panel, event: &ChatPanelEvent, _window, cx| {
                    this.on_chat_event(event, cx);
                },
            ),
            cx.subscribe_in(
                &editor_panel,
                window,
                |this, _panel, event: &EditorPanelEvent, window, cx| {
                    this.on_editor_event(event, window, cx);
                },
            ),
            cx.subscribe_in(
                &problems,
                window,
                |this, _panel, event: &ProblemsEvent, window, cx| {
                    let ProblemsEvent::OpenSource { file, line, .. } = event;
                    let rel = file.as_std_path().to_path_buf();
                    this.editor_panel.update(cx, |panel, cx| {
                        panel.open_file(rel, Some(*line), window, cx);
                    });
                },
            ),
            cx.subscribe_in(
                &tree,
                window,
                |this, _tree, event: &TreePanelEvent, window, cx| match event {
                    TreePanelEvent::OpenFile { rel, kind } => {
                        let abs = this
                            .project_root
                            .clone()
                            .map(|root| root.join(rel))
                            .unwrap_or_else(|| rel.clone());
                        match kind {
                            TreeOpenKind::Tex => {
                                this.editor_panel.update(cx, |panel, cx| {
                                    panel.open_path(abs, None, window, cx);
                                });
                            }
                            TreeOpenKind::Pdf => {
                                this.last_pdf = Some(abs.clone());
                                this.pdf_panel.update(cx, |panel, cx| {
                                    panel.set_document(abs, 1, cx);
                                });
                            }
                        }
                    }
                    TreePanelEvent::ProjectCreated { project } => {
                        let root = project.root.clone();
                        this.open_project(root, window, cx);
                    }
                },
            ),
        ];

        let focus = cx.focus_handle();
        let palette = cx.new(|cx| CommandState::new(window, cx));

        let mut warnings = warnings;

        // Motor LaTeX: solo comprobación de presencia (el probe real llega en M1-D1).
        let latexmk = portable.latexmk();
        let engine_available = latexmk.is_file();
        let status_model = StatusModel {
            engine_label: if engine_available {
                rust_i18n::t!("ui.app.engine.ready").to_string()
            } else {
                rust_i18n::t!("ui.app.engine.missing").to_string()
            },
            engine_available,
            project: startup.as_ref().map(|project| project.name.clone()),
            ai: AiState::Stopped,
            ..StatusModel::default()
        };

        // Layout persistido; si no parsea, default + aviso.
        let layout = match ShellLayout::from_value(&settings.ui.dock) {
            Ok(layout) => layout,
            Err(err) => {
                warnings.push(ConfigWarning::new(
                    "ui.dock",
                    format!("layout ilegible ({err}); se usa el layout por defecto"),
                ));
                ShellLayout::four_columns()
            }
        };

        let mut root = Self {
            portable,
            settings,
            warnings,
            project: project_service,
            status_model,
            status_handle,
            layout,
            active: SlotKind::Editor,
            dock,
            tree,
            editor,
            editor_panel,
            pdf,
            pdf_panel,
            last_pdf: None,
            chat,
            problems,
            project_root,
            latex_settings,
            compile,
            sync_scroll,
            sync_seq: 0,
            sync_burst_start: None,
            reverse_seq: 0,
            last_synced: None,
            last_forward_sync: None,
            pending_inverse: InverseReason::default(),
            focus,
            palette,
            layout_generation: 0,
            layout_dirty: false,
            _status_task: status_task,
            _compile_task: compile_task,
            _dock_events: dock_events,
            _pane_events: pane_events,
            _integration_events: integration_events,
        };
        let layout = root.layout.clone();
        root.apply_layout(&layout, window, cx);

        // Preload del proyecto: si hay un main.tex detectado (con PDF hermano),
        // se abre en el editor y su PDF se carga en el visor.
        if let Some(main) = &startup_main {
            let pdf_sibling = main.with_extension("pdf");
            if pdf_sibling.is_file() {
                let pdf_path = pdf_sibling.clone().into_std_path_buf();
                root.last_pdf = Some(pdf_path.clone());
                root.pdf_panel.update(cx, |panel, cx| {
                    panel.set_document(pdf_path, 1, cx);
                });
            }
            let rel = root
                .project_root
                .as_deref()
                .and_then(Utf8Path::from_path)
                .and_then(|project| main.strip_prefix(project).ok())
                .map(|rel| rel.to_string());
            if let Some(rel) = rel {
                root.editor_panel.update(cx, |panel, cx| {
                    panel.open_file(rel, None, window, cx);
                });
            }
        }

        // El foco arranca en la raíz para que los atajos globales siempre
        // encuentren un `on_action` vivo.
        let handle = root.focus.clone();
        window.defer(cx, move |window, cx| handle.focus(window, cx));
        root
    }

    /// Estado del layout (tests / QA).
    pub fn layout(&self) -> &ShellLayout {
        &self.layout
    }

    /// Panel activo según el dock.
    pub fn active(&self) -> SlotKind {
        self.active
    }

    /// Ajustes vivos.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Avisos de configuración pendientes de mostrar.
    pub fn warnings(&self) -> &[ConfigWarning] {
        &self.warnings
    }

    /// Entidad del dock (tests de integración).
    pub fn dock(&self) -> &Entity<DockArea> {
        &self.dock
    }

    /// Handle de estado que pueden usar otros paneles/servicios.
    pub fn status_handle(&self) -> &StatusHandle {
        &self.status_handle
    }

    // --- paneles -----------------------------------------------------------

    /// Panel de una hoja como valor homogéneo para el shell.
    fn panel(&self, kind: SlotKind) -> AnyPanel {
        match kind {
            SlotKind::Tree => AnyPanel::Tree(self.tree.clone()),
            SlotKind::Editor => AnyPanel::Editor(self.editor.clone()),
            SlotKind::Pdf => AnyPanel::Pdf(self.pdf.clone()),
            SlotKind::Chat => AnyPanel::Chat(self.chat.clone()),
        }
    }

    fn on_pane_event(&mut self, event: PaneEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            PaneEvent::Activated(kind) => {
                if self.active != kind {
                    self.active = kind;
                    cx.notify();
                }
            }
            PaneEvent::VisibilityChanged { kind, visible } => {
                if let Some(slot) = self.layout.slot_mut(kind) {
                    slot.collapsed = !visible;
                }
                self.schedule_layout_save(cx);
                cx.notify();
            }
            PaneEvent::OpenProjectRequested => self.open_project_dialog(window, cx),
        }
    }

    // --- layout (M1-A1) -----------------------------------------------------

    /// Aplica un layout: visibilidad por hoja + anchos del split central.
    fn apply_layout(&mut self, layout: &ShellLayout, window: &mut Window, cx: &mut Context<Self>) {
        self.layout = layout.clone();
        for kind in SlotKind::ALL {
            let visible = !self.layout.is_collapsed(kind);
            self.panel(kind).set_visible(visible, cx);
        }

        let measured = self.dock.read(cx).bounds().size.width;
        let reference = if measured > px(0.) {
            measured
        } else {
            DEFAULT_REFERENCE_WIDTH
        };
        let sizes: Vec<Pixels> = layout
            .sizes_px(f32::from(reference))
            .into_iter()
            .map(px)
            .collect();
        if let Some(node) = self
            .dock
            .read(cx)
            .layout(DockPlacement::Center)
            .map(|tree| tree.root().id())
        {
            self.dock
                .update(cx, |dock, cx| dock.set_split_sizes(node, sizes, window, cx));
        }
        self.dock.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Alterna una hoja (Ctrl+Alt+1/3/4/V, toolbar o palette).
    pub fn toggle_slot(&mut self, kind: SlotKind, window: &mut Window, cx: &mut Context<Self>) {
        let mut layout = self.layout.clone();
        if layout.focus.is_some() {
            exit_focus(&mut layout);
        }
        if let Some(slot) = layout.slot_mut(kind) {
            slot.collapsed = !slot.collapsed;
        }
        self.apply_layout(&layout, window, cx);
        self.schedule_layout_save(cx);
    }

    /// Maximiza el panel activo o restaura los paneles (Ctrl+Alt+M).
    pub fn toggle_focus_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut layout = self.layout.clone();
        if layout.focus.is_some() {
            exit_focus(&mut layout);
        } else {
            let active = if self.layout.is_collapsed(self.active) {
                SlotKind::ALL
                    .into_iter()
                    .find(|kind| !self.layout.is_collapsed(*kind))
                    .unwrap_or(self.active)
            } else {
                self.active
            };
            enter_focus(&mut layout, active);
        }
        self.apply_layout(&layout, window, cx);
        self.schedule_layout_save(cx);
    }

    /// Sincroniza las fracciones con el dock real; `true` si cambiaron.
    fn sync_from_dock(&mut self, cx: &App) -> bool {
        let dump = self.dock.read(cx).dump(cx);
        let sizes: Vec<f32> = match &dump.center.info {
            PanelInfo::Stack { sizes, .. } => sizes.iter().map(|size| f32::from(*size)).collect(),
            _ => Vec::new(),
        };
        let total: f32 = sizes.iter().sum();
        let observed: Vec<(SlotKind, f32, bool)> = dump
            .center
            .children
            .iter()
            .enumerate()
            .filter_map(|(index, group)| {
                let leaf = group.children.first()?;
                let kind = SlotKind::from_panel_name(&leaf.panel_name)?;
                let fraction = if total > 0.0 {
                    sizes.get(index).copied().unwrap_or(0.0) / total
                } else {
                    0.0
                };
                Some((kind, fraction, self.layout.is_collapsed(kind)))
            })
            .collect();
        let observed_layout = ShellLayout::from_observed(&observed);

        let mut changed = false;
        for kind in SlotKind::ALL {
            let (Some(current), Some(new)) = (self.layout.slot(kind), observed_layout.slot(kind))
            else {
                continue;
            };
            if (current.width_fraction - new.width_fraction).abs() > 0.0005 {
                changed = true;
                if let Some(slot) = self.layout.slot_mut(kind) {
                    slot.width_fraction = new.width_fraction;
                }
            }
        }
        changed
    }

    fn on_dock_layout_changed(&mut self, cx: &mut Context<Self>) {
        if self.sync_from_dock(cx) {
            self.schedule_layout_save(cx);
        }
    }

    /// Programa un guardado con debounce (la última generación gana).
    fn schedule_layout_save(&mut self, cx: &mut Context<Self>) {
        self.layout_dirty = true;
        self.layout_generation = self.layout_generation.wrapping_add(1);
        let generation = self.layout_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LAYOUT_SAVE_DEBOUNCE).await;
            this.update(cx, |root, cx| {
                if root.layout_dirty && root.layout_generation == generation {
                    root.persist_layout(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Escribe `ui.dock` en `config/settings.json` (atómico).
    fn persist_layout(&mut self, cx: &mut Context<Self>) {
        self.layout_dirty = false;
        self.sync_from_dock(cx);
        self.settings.ui.dock = self.layout.to_value();
        match self.settings.save(&self.portable) {
            Ok(()) => tracing::debug!(target: "lsx::ui", "layout guardado en settings.json"),
            Err(err) => {
                tracing::warn!(target: "lsx::ui", error = %err, "no se pudo guardar el layout");
            }
        }
    }

    // --- proyecto -----------------------------------------------------------

    /// Diálogo "abrir proyecto": candidatos de `workspace/` + recientes.
    pub fn open_project_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let candidates = self.project.candidates(&self.settings);
        let current = self
            .tree
            .read(cx)
            .project()
            .map(|project| project.root.clone());
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let mut body = v_flex().gap_1();
            if candidates.is_empty() {
                body = body.child(
                    div()
                        .py_4()
                        .text_sm()
                        .text_center()
                        .child(rust_i18n::t!("ui.app.project.empty").to_string()),
                );
            }
            for info in &candidates {
                let weak = weak.clone();
                let path = info.root.clone();
                let is_current = current.as_deref() == Some(path.as_path());
                let mut row =
                    Button::new(SharedString::from(format!("project-{}", path.display())))
                        .ghost()
                        .small()
                        .icon(IconName::FolderClosed)
                        .label(format!("{}   {}", info.name, path.display()))
                        .selected(is_current);
                row = row.on_click(move |_, window, cx| {
                    let path = path.clone();
                    let _ = weak.update(cx, |root, cx| {
                        root.open_project(path, window, cx);
                    });
                    window.close_dialog(cx);
                });
                body = body.child(row);
            }
            dialog
                .title(rust_i18n::t!("ui.app.project.dialog_title").to_string())
                .width(px(560.))
                .child(
                    DialogDescription::new()
                        .child(rust_i18n::t!("ui.app.project.dialog_hint").to_string()),
                )
                .child(body)
        });
    }

    /// Abre el wizard «Nuevo proyecto» (M1-F4).
    pub fn open_new_project_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        let parent = self.project.workspace().to_path_buf();
        crate::new_project::open_new_project_dialog(
            window,
            cx,
            crate::new_project::NewProjectDialogConfig {
                default_parent: parent,
            },
            move |project, window, cx| {
                let path = project.root.clone();
                let _ = weak.update(cx, |root, cx| {
                    root.open_project(path, window, cx);
                });
            },
        );
    }

    /// Abre un proyecto y lo recuerda en `settings.project`.
    pub fn open_project(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.project.open(&path) {
            Ok(info) => {
                self.project
                    .remember(&mut self.settings, &self.portable, &info.root);
                let _ = self.settings.save(&self.portable);
                self.tree
                    .update(cx, |tree, cx| tree.set_project(info.clone(), cx));
                self.status_model.project = Some(info.name.clone());
                notifications::info(
                    rust_i18n::t!("ui.app.tree.welcome", name = info.name).to_string(),
                    window,
                    cx,
                );
            }
            Err(err) => notifications::error(err.to_string(), window, cx),
        }
        cx.notify();
    }

    // --- acciones de shell ---------------------------------------------------

    /// Compilar el proyecto (botón ▶, paleta o Ctrl+Alt+B).
    pub fn request_compile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(controller) = self.compile.clone() {
            if controller.submit_stored().is_some() {
                return;
            }
            // Último recurso: el .tex activo como documento principal.
            if let (Some(root), Some(path)) = (
                self.project_root.clone(),
                self.editor_panel.read(cx).active_path(),
            ) && path.ends_with(".tex")
                && let Some(root) = Utf8Path::from_path(&root)
            {
                controller.set_request(self.latex_settings.request(root, path.as_str()));
                if controller.submit_stored().is_some() {
                    return;
                }
            }
        }
        notifications::info(
            rust_i18n::t!("ui.app.toolbar.compile_unavailable").to_string(),
            window,
            cx,
        );
        cx.notify();
    }

    /// Reduce un evento de la cola de compilación (integración M1-D2).
    fn on_compile_event(&mut self, event: CompileEvent, cx: &mut Context<Self>) {
        use crate::status::{CompileState, StatusMessage};

        match event {
            CompileEvent::Queued { .. } => {
                self.status_handle
                    .send(StatusMessage::Compile(CompileState::Queued));
            }
            CompileEvent::Started { .. } => {
                self.status_handle
                    .send(StatusMessage::Compile(CompileState::Running { hint: None }));
            }
            CompileEvent::Progress { line, .. } => {
                self.status_handle
                    .send(StatusMessage::Compile(CompileState::Running {
                        hint: Some(line),
                    }));
            }
            CompileEvent::PackageInstall { package, stage, .. } => {
                let hint = match stage {
                    lsx_latex::engine::PackageInstallStage::Started => {
                        rust_i18n::t!("ui.app.toolbar.compile_installing", package = package)
                            .to_string()
                    }
                    _ => rust_i18n::t!("ui.app.toolbar.compile_installed", package = package)
                        .to_string(),
                };
                self.status_handle
                    .send(StatusMessage::Compile(CompileState::Running {
                        hint: Some(hint),
                    }));
            }
            CompileEvent::Finished {
                generation,
                outcome,
            } => {
                match outcome.status {
                    CompileStatus::Success => {
                        self.status_handle
                            .send(StatusMessage::Compile(CompileState::Success {
                                duration_ms: outcome.duration.as_millis() as u64,
                            }));
                    }
                    CompileStatus::Cancelled => {
                        self.status_handle
                            .send(StatusMessage::Compile(CompileState::Idle));
                    }
                    CompileStatus::Failed | CompileStatus::Timeout => {
                        self.status_handle
                            .send(StatusMessage::Compile(CompileState::Failed {
                                message: rust_i18n::t!("ui.app.toolbar.compile_failed_short")
                                    .to_string(),
                            }));
                    }
                }
                self.problems.update(cx, |panel, cx| {
                    panel.apply_outcome(generation, &outcome, cx);
                });
                if let Some(pdf_path) = &outcome.pdf {
                    let pdf_path = pdf_path.as_std_path().to_path_buf();
                    self.last_pdf = Some(pdf_path.clone());
                    self.pdf_panel.update(cx, |panel, cx| {
                        panel.set_document(pdf_path, generation, cx);
                    });
                }
                let mut by_file: BTreeMap<camino::Utf8PathBuf, Vec<Diagnostic>> = BTreeMap::new();
                for diag in &outcome.diagnostics {
                    if let Some(file) = &diag.file {
                        by_file.entry(file.clone()).or_default().push(diag.clone());
                    }
                }
                self.editor_panel.update(cx, |panel, cx| {
                    for (file, diags) in &by_file {
                        panel.set_diagnostics(file.as_std_path(), diags, cx);
                    }
                });
                cx.notify();
            }
            CompileEvent::Cancelled { .. } => {
                self.status_handle
                    .send(StatusMessage::Compile(CompileState::Idle));
            }
        }
    }

    /// Eventos del panel de editor (apertura de documento → posible main.tex).
    fn on_editor_event(
        &mut self,
        event: &EditorPanelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorPanelEvent::Opened { path } => {
                if path.ends_with(".tex")
                    && let (Some(controller), Some(root)) = (&self.compile, &self.project_root)
                    && let Some(root) = Utf8Path::from_path(root)
                {
                    controller.set_request(self.latex_settings.request(root, path.as_str()));
                }
            }
            EditorPanelEvent::CursorMoved { line, column, .. } => {
                // Status bar + scroll sync `.tex` → PDF (M1-C5).
                self.status_handle.send(StatusMessage::Cursor {
                    line: *line,
                    column: *column,
                });
                self.schedule_sync_forward(cx);
            }
            _ => {}
        }
        cx.notify();
    }

    /// Eventos del panel de chat (persistencia de ajustes que toca el usuario).
    fn on_chat_event(&mut self, event: &ChatPanelEvent, cx: &mut Context<Self>) {
        match event {
            ChatPanelEvent::PermissionPresetChanged(preset) => {
                self.settings.ai.permission_preset = preset.clone();
                match self.settings.save(&self.portable) {
                    Ok(()) => tracing::debug!(
                        target: "lsx::ui",
                        preset = %preset,
                        "ai.permissionPreset guardado"
                    ),
                    Err(err) => tracing::warn!(
                        target: "lsx::ui",
                        error = %err,
                        "no se pudo guardar ai.permissionPreset"
                    ),
                }
                cx.notify();
            }
        }
    }

    /// Eventos del visor PDF: página visible (status bar), scroll del usuario
    /// (scroll sync inverso) y saltos inverse.
    fn on_pdf_event(&mut self, event: &PdfPanelEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            PdfPanelEvent::PageChanged { page, count } => {
                self.status_handle.send(StatusMessage::Page {
                    current: *page,
                    total: *count,
                });
            }
            PdfPanelEvent::ViewportScrolled => self.schedule_reverse_sync(window, cx),
            PdfPanelEvent::InverseJump(jump) => match std::mem::take(&mut self.pending_inverse) {
                InverseReason::Click => {
                    self.editor_panel.update(cx, |editor, cx| {
                        editor.open_path(&jump.file, Some(jump.line), window, cx);
                    });
                }
                // Scroll sync: desplaza sin mover el cursor ni robar el foco.
                InverseReason::Scroll => {
                    self.editor_panel.update(cx, |editor, cx| {
                        editor.scroll_to_source(&jump.file, jump.line, window, cx);
                    });
                }
            },
            PdfPanelEvent::DocumentReady { .. }
            | PdfPanelEvent::DocumentFailed { .. }
            | PdfPanelEvent::ZoomChanged { .. } => {}
        }
    }

    // --- scroll sync `.tex` ↔ PDF (M1-C5) ------------------------------------

    /// Alterna el scroll sync y lo persiste en `latex.syncScroll`.
    fn toggle_sync_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_scroll = !self.sync_scroll;
        self.settings.latex.sync_scroll = self.sync_scroll;
        if self.sync_scroll {
            // Al reactivar, fuerza un sync aunque el cursor no se haya movido.
            self.last_synced = None;
        }
        let message = if self.sync_scroll {
            rust_i18n::t!("ui.app.toolbar.sync_scroll_on")
        } else {
            rust_i18n::t!("ui.app.toolbar.sync_scroll_off")
        };
        notifications::info(message.to_string(), window, cx);
        if let Err(err) = self.settings.save(&self.portable) {
            tracing::warn!(target: "lsx::ui", error = %err, "no se pudo guardar latex.syncScroll");
        }
        cx.notify();
    }

    /// Programa el forward del scroll sync tras un movimiento de cursor
    /// (debounce: el cursor se mueve a ráfagas).
    ///
    /// Si el usuario no deja de moverse (escribiendo), el sync se aplica
    /// igualmente pasado [`SYNC_MAX_WAIT`] en vez de quedarse esperando.
    fn schedule_sync_forward(&mut self, cx: &mut Context<Self>) {
        if !self.sync_scroll {
            return;
        }
        let burst_start = *self.sync_burst_start.get_or_insert_with(Instant::now);
        self.sync_seq = self.sync_seq.wrapping_add(1);
        let generation = self.sync_seq;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SYNC_DEBOUNCE).await;
            this.update(cx, |root, cx| {
                if root.sync_seq == generation || burst_start.elapsed() >= SYNC_MAX_WAIT {
                    root.sync_burst_start = None;
                    root.apply_sync_forward(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Forward del scroll sync: cursor → posición en el PDF, sin resaltado.
    ///
    /// El CLI corre en el executor de fondo (~50 ms) y si no hay cambio de
    /// destino (`ruta:línea`) no se vuelve a invocar.
    fn apply_sync_forward(&mut self, cx: &mut Context<Self>) {
        if !self.sync_scroll {
            return;
        }
        let Some(pdf_path) = self.last_pdf.clone() else {
            return;
        };
        let Some(rel) = self.editor_panel.read(cx).active_path() else {
            return;
        };
        let Some((line, _column)) = self.editor_panel.read(cx).active_cursor(cx) else {
            return;
        };
        let Some(root) = self.project_root.clone() else {
            return;
        };
        if self.last_synced.as_ref() == Some(&(rel.clone(), line)) {
            return;
        }
        self.last_synced = Some((rel.clone(), line));
        let src = root.join(&rel);
        let (Some(pdf), Some(src)) = (
            Utf8Path::from_path(&pdf_path).map(|path| path.to_path_buf()),
            Utf8Path::from_path(&src).map(|path| path.to_path_buf()),
        ) else {
            return;
        };
        let Ok(exe) = Utf8PathBuf::from_path_buf(self.portable.synctex_exe()) else {
            return;
        };
        let synctex = lsx_latex::Synctex::new(exe);
        let key = (rel.clone(), line);
        cx.spawn(async move |this, cx| {
            let records = cx
                .background_spawn(async move { synctex.forward_all(&pdf, &src, line, 0) })
                .await;
            let Ok(records) = records else {
                // Sin sidecar (o CLI caído): permite reintentar al volver aquí.
                this.update(cx, |root, _cx| root.last_synced = None).ok();
                return;
            };
            let Some(first) = records.first().copied() else {
                this.update(cx, |root, _cx| root.last_synced = None).ok();
                return;
            };
            this.update(cx, |root, cx| {
                // El cursor pudo moverse mientras corría el CLI: solo se aplica
                // si este destino sigue siendo el último pedido.
                if root.last_synced.as_ref() == Some(&key) {
                    let rect = SyncRect::from_record(&first);
                    root.last_forward_sync = Some(Instant::now());
                    root.pdf_panel
                        .update(cx, |panel, cx| panel.scroll_to_sync_rect(rect, cx));
                }
            })
            .ok();
        })
        .detach();
    }

    /// Programa el inverso del scroll sync tras un desplazamiento del visor.
    fn schedule_reverse_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !reverse_allowed(self.sync_scroll, self.last_forward_sync, Instant::now()) {
            return;
        }
        self.reverse_seq = self.reverse_seq.wrapping_add(1);
        let generation = self.reverse_seq;
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SYNC_DEBOUNCE).await;
            this.update_in(cx, |root, window, cx| {
                if root.reverse_seq == generation {
                    root.apply_reverse_sync(window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Inverso del scroll sync: borde superior del visor → línea del `.tex`
    /// (el `InverseJump` resultante se aplica como scroll, no como salto).
    fn apply_reverse_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.sync_scroll {
            return;
        }
        let Some((page0, x_bp, y_bp)) = self.pdf_panel.read(cx).source_point_at_top() else {
            return;
        };
        self.pending_inverse = InverseReason::Scroll;
        self.pdf_panel.update(cx, |panel, cx| {
            panel.inverse_at(page0, x_bp, y_bp, window, cx);
        });
    }

    /// SyncTeX forward (Ctrl+Alt+J): cursor → rectángulo en el PDF.
    fn sync_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pdf_path) = self.last_pdf.clone() else {
            notifications::info(
                rust_i18n::t!("ui.app.toolbar.sync_no_pdf").to_string(),
                window,
                cx,
            );
            return;
        };
        let (rel, cursor) = (
            self.editor_panel.read(cx).active_path(),
            self.editor_panel.read(cx).active_cursor(cx),
        );
        let (Some(rel), Some((line, _column))) = (rel, cursor) else {
            notifications::info(
                rust_i18n::t!("ui.app.toolbar.sync_no_editor").to_string(),
                window,
                cx,
            );
            return;
        };
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let src = root.join(&rel);
        let (Some(pdf_u8), Some(src_u8)) =
            (Utf8Path::from_path(&pdf_path), Utf8Path::from_path(&src))
        else {
            return;
        };
        let Ok(exe) = Utf8PathBuf::from_path_buf(self.portable.synctex_exe()) else {
            return;
        };
        let synctex = lsx_latex::Synctex::new(exe);
        match synctex.forward_all(pdf_u8, src_u8, line, 0) {
            Ok(records) if !records.is_empty() => {
                let rect = SyncRect::from_record(&records[0]);
                let page = rect.page;
                self.pdf_panel.update(cx, |panel, cx| {
                    panel.goto_page(page, cx);
                    panel.flash_sync_rect(rect, cx);
                });
            }
            Ok(_) => notifications::info(
                rust_i18n::t!("ui.app.toolbar.sync_no_match").to_string(),
                window,
                cx,
            ),
            Err(err) => notifications::error(format!("SyncTeX: {err}"), window, cx),
        }
    }

    /// Abre la paleta de comandos (Ctrl+P).
    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let palette = self.palette.clone();
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let weak = weak.clone();
            dialog
                .title(rust_i18n::t!("ui.app.palette.title").to_string())
                .width(px(620.))
                .child(
                    Command::new(&palette)
                        .items(PaletteCommand::items())
                        .max_h(px(380.))
                        .placeholder(rust_i18n::t!("ui.app.palette.placeholder").to_string())
                        .on_confirm(move |index, window, cx| {
                            let Some(command) = PaletteCommand::from_index(index.row) else {
                                return;
                            };
                            let _ = weak.update(cx, |root, cx| {
                                root.run_palette_command(command, window, cx);
                            });
                        }),
                )
        });
        // El foco del buscador se aplica en el siguiente tick: en el momento de
        // construir el diálogo su `Command` aún no está montado y el foco se
        // perdería (bug detectado en la captura de QA de M1-A).
        let palette_for_focus = self.palette.clone();
        window.defer(cx, move |window, cx| {
            palette_for_focus.update(cx, |state, cx| state.focus(window, cx));
        });
    }

    /// Ejecuta un comando de la paleta.
    pub fn run_palette_command(
        &mut self,
        command: PaletteCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            PaletteCommand::Compile => self.request_compile(window, cx),
            PaletteCommand::ToggleTree => self.toggle_slot(SlotKind::Tree, window, cx),
            PaletteCommand::ToggleEditor => self.toggle_slot(SlotKind::Editor, window, cx),
            PaletteCommand::TogglePdf => self.toggle_slot(SlotKind::Pdf, window, cx),
            PaletteCommand::ToggleChat => self.toggle_slot(SlotKind::Chat, window, cx),
            PaletteCommand::ToggleFocus => self.toggle_focus_mode(window, cx),
            PaletteCommand::OpenSettings => self.open_settings(window, cx),
            PaletteCommand::ReloadTree => {
                self.tree.update(cx, |tree, cx| tree.refresh(cx));
                cx.notify();
            }
            PaletteCommand::OpenProject => self.open_project_dialog(window, cx),
            PaletteCommand::NewProject => self.open_new_project_dialog(window, cx),
            PaletteCommand::PermissionDemo => self.demo_permission(window, cx),
        }
    }

    /// Abre los ajustes (Ctrl+,).
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let weak = cx.weak_entity();
        settings_ui::open_dialog(window, cx, settings, weak);
    }

    /// Aplica ajustes guardados desde el diálogo (validados de nuevo aquí).
    pub fn apply_settings(
        &mut self,
        mut settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.warnings = settings.validate();
        self.settings = settings;
        // El diálogo de ajustes también puede cambiar `latex.syncScroll`.
        self.sync_scroll = self.settings.latex.sync_scroll;
        theme::apply(&self.settings, cx);
        match self.settings.save(&self.portable) {
            Ok(()) => notifications::success(
                rust_i18n::t!("ui.app.settings.saved").to_string(),
                window,
                cx,
            ),
            Err(err) => notifications::error(
                rust_i18n::t!("ui.app.settings.save_error", error = err).to_string(),
                window,
                cx,
            ),
        }
        cx.notify();
    }

    /// Muestra el modal de permiso de la IA con datos de demo (M1-A4).
    pub fn demo_permission(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = PermissionCard::show(window, cx, PermissionRequest::demo());
        cx.spawn(async move |this, cx| {
            if let Ok(decision) = receiver.recv().await {
                let message =
                    rust_i18n::t!("ui.app.permission.result", decision = decision.label())
                        .to_string();
                let _ = this.update_in(cx, |_root, window, cx| {
                    notifications::info(message, window, cx);
                });
            }
        })
        .detach();
    }

    // --- render ---------------------------------------------------------------

    fn render_title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project = self
            .status_model
            .project
            .clone()
            .unwrap_or_else(|| rust_i18n::t!("ui.app.status.no_project").to_string());
        TitleBar::new().child(
            h_flex()
                .flex_1()
                .items_center()
                .px_2()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .child(rust_i18n::t!("ui.app.title").to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(project),
                ),
        )
    }

    fn render_toolbar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project_name = self
            .status_model
            .project
            .clone()
            .unwrap_or_else(|| rust_i18n::t!("ui.app.status.no_project").to_string());
        let weak = cx.weak_entity();
        let recents = self.project.candidates(&self.settings);
        let current_root = self.tree.read(cx).project().map(|p| p.root.clone());

        let project_button = Button::new("toolbar-project")
            .icon(IconName::FolderOpen)
            .label(project_name)
            .small()
            .ghost()
            .tooltip(rust_i18n::t!("ui.app.toolbar.open_folder").to_string())
            .on_click(cx.listener(|this, _, window, cx| this.open_project_dialog(window, cx)));

        let project_menu = DropdownButton::new("toolbar-project-menu")
            .button(project_button)
            .dropdown_menu(move |menu, window, cx| {
                let callbacks = toolbar::ProjectMenuCallbacks::new(
                    {
                        let weak = weak.clone();
                        move |window, cx| {
                            let _ =
                                weak.update(cx, |root, cx| root.open_project_dialog(window, cx));
                        }
                    },
                    {
                        let weak = weak.clone();
                        move |window, cx| {
                            let _ = weak
                                .update(cx, |root, cx| root.open_new_project_dialog(window, cx));
                        }
                    },
                    {
                        let weak = weak.clone();
                        move |path, window, cx| {
                            let _ = weak.update(cx, |root, cx| root.open_project(path, window, cx));
                        }
                    },
                );
                toolbar::project_menu(
                    window,
                    cx,
                    menu,
                    &recents,
                    current_root.as_deref(),
                    callbacks,
                )
            });

        let mut toggles = h_flex().items_center().gap_1();
        for kind in SlotKind::ALL {
            let collapsed = self.layout.is_collapsed(kind);
            toggles = toggles.child(toolbar::slot_toggle(
                cx,
                kind,
                collapsed,
                cx.listener(move |this, _, window, cx| this.toggle_slot(kind, window, cx)),
            ));
        }

        h_flex()
            .flex_none()
            .h(px(toolbar::HEIGHT))
            .px_2()
            .gap_2()
            .items_center()
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(project_menu)
            .child(toolbar::separator(cx))
            .child(toolbar::compile_button(
                rust_i18n::t!("ui.app.toolbar.compile").to_string(),
                self.status_model.compile.is_busy(),
                cx.listener(|this, _, window, cx| this.request_compile(window, cx)),
            ))
            .child(toolbar::engine_indicator(
                cx,
                &self.status_model.engine_label.clone(),
                self.status_model.engine_available,
            ))
            .child(div().flex_1())
            .child(toggles)
            .child(toolbar::separator(cx))
            .child(toolbar::icon_button(
                cx,
                "toolbar-sync-scroll",
                IconName::ArrowLeftRight,
                if self.sync_scroll {
                    rust_i18n::t!("ui.app.toolbar.sync_scroll_on").to_string()
                } else {
                    rust_i18n::t!("ui.app.toolbar.sync_scroll_off").to_string()
                },
                self.sync_scroll,
                cx.listener(|this, _, window, cx| this.toggle_sync_scroll(window, cx)),
            ))
            .child(toolbar::separator(cx))
            .child(toolbar::icon_button(
                cx,
                "toolbar-focus",
                IconName::Maximize,
                if self.layout.focus.is_some() {
                    rust_i18n::t!("ui.app.toolbar.focus_exit").to_string()
                } else {
                    rust_i18n::t!("ui.app.toolbar.focus_enter").to_string()
                },
                self.layout.focus.is_some(),
                cx.listener(|this, _, window, cx| this.toggle_focus_mode(window, cx)),
            ))
            .child(toolbar::separator(cx))
            .child(toolbar::icon_button(
                cx,
                "toolbar-settings",
                IconName::Settings,
                rust_i18n::t!("ui.app.toolbar.settings").to_string(),
                false,
                cx.listener(|this, _, window, cx| this.open_settings(window, cx)),
            ))
            .child(toolbar::icon_button(
                cx,
                "toolbar-palette",
                IconName::Search,
                rust_i18n::t!("ui.app.toolbar.palette").to_string(),
                false,
                cx.listener(|this, _, window, cx| this.open_palette(window, cx)),
            ))
            .child(toolbar::icon_button(
                cx,
                "toolbar-permission-demo",
                IconName::ShieldCheck,
                rust_i18n::t!("ui.app.toolbar.permission_demo").to_string(),
                false,
                cx.listener(|this, _, window, cx| this.demo_permission(window, cx)),
            ))
    }
}

/// Hoja PDF del shell: visor real (M1-C) con el chrome del contrato `Pane`.
struct PdfSlot {
    state: PaneState,
    pdf: Entity<PdfPanel>,
}

impl PdfSlot {
    fn new(
        pdf: Entity<PdfPanel>,
        dock_area: Option<WeakEntity<DockArea>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            state: PaneState::new(SlotKind::Pdf, dock_area, cx),
            pdf,
        }
    }

    fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.state.set_visible(visible, cx);
    }
}

impl Pane for PdfSlot {
    fn pane_state(&self) -> &PaneState {
        &self.state
    }

    fn pane_state_mut(&mut self) -> &mut PaneState {
        &mut self.state
    }
}

impl gpui_kit::component::dock::BasePanel for PdfSlot {
    fn panel_name(&self) -> &'static str {
        self.state.kind().panel_name()
    }

    fn visible(&self, cx: &App) -> bool {
        self.pane_visible(cx)
    }

    fn closable(&self, cx: &App) -> bool {
        self.pane_closable(cx)
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.state.set_active(active, cx);
    }
}

impl gpui_kit::component::dock::Panel for PdfSlot {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.pane_title().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.pane_title()
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        Some(vec![self.collapse_button(cx)])
    }
}

impl gpui_kit::EventEmitter<gpui_kit::component::dock::PanelEvent> for PdfSlot {}

impl gpui_kit::EventEmitter<PaneEvent> for PdfSlot {}

impl gpui_kit::Focusable for PdfSlot {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.state.focus_handle().clone()
    }
}

impl Render for PdfSlot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        pane_container(SlotKind::Pdf, cx)
            .flex()
            .flex_col()
            .track_focus(self.state.focus_handle())
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.activate_by_user(window, cx)),
            )
            .child(self.pdf.clone())
    }
}

/// Hoja Chat del shell: panel de chat real (M1-E) con el chrome del contrato
/// `Pane` (mismo patrón que `PdfSlot`).
struct ChatSlot {
    state: PaneState,
    chat: Entity<ChatPanel>,
}

impl ChatSlot {
    fn new(
        chat: Entity<ChatPanel>,
        dock_area: Option<WeakEntity<DockArea>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            state: PaneState::new(SlotKind::Chat, dock_area, cx),
            chat,
        }
    }

    fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.state.set_visible(visible, cx);
    }
}

impl Pane for ChatSlot {
    fn pane_state(&self) -> &PaneState {
        &self.state
    }

    fn pane_state_mut(&mut self) -> &mut PaneState {
        &mut self.state
    }
}

impl gpui_kit::component::dock::BasePanel for ChatSlot {
    fn panel_name(&self) -> &'static str {
        self.state.kind().panel_name()
    }

    fn visible(&self, cx: &App) -> bool {
        self.pane_visible(cx)
    }

    fn closable(&self, cx: &App) -> bool {
        self.pane_closable(cx)
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.state.set_active(active, cx);
    }
}

impl gpui_kit::component::dock::Panel for ChatSlot {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.pane_title().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.pane_title()
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        Some(vec![self.collapse_button(cx)])
    }
}

impl gpui_kit::EventEmitter<gpui_kit::component::dock::PanelEvent> for ChatSlot {}

impl gpui_kit::EventEmitter<PaneEvent> for ChatSlot {}

impl gpui_kit::Focusable for ChatSlot {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.state.focus_handle().clone()
    }
}

impl Render for ChatSlot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        pane_container(SlotKind::Chat, cx)
            .flex()
            .flex_col()
            .track_focus(self.state.focus_handle())
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.activate_by_user(window, cx)),
            )
            .child(self.chat.clone())
    }
}

/// Hoja Editor del shell: editor real (M1-B). Los problemas viven en el dock
/// inferior del explorador (M2).
struct EditorSlot {
    state: PaneState,
    editor: Entity<EditorPanel>,
}

impl EditorSlot {
    fn new(
        editor: Entity<EditorPanel>,
        dock_area: Option<WeakEntity<DockArea>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            state: PaneState::new(SlotKind::Editor, dock_area, cx),
            editor,
        }
    }

    fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.state.set_visible(visible, cx);
    }
}

impl Pane for EditorSlot {
    fn pane_state(&self) -> &PaneState {
        &self.state
    }

    fn pane_state_mut(&mut self) -> &mut PaneState {
        &mut self.state
    }
}

impl gpui_kit::component::dock::BasePanel for EditorSlot {
    fn panel_name(&self) -> &'static str {
        self.state.kind().panel_name()
    }

    fn visible(&self, cx: &App) -> bool {
        self.pane_visible(cx)
    }

    fn closable(&self, cx: &App) -> bool {
        self.pane_closable(cx)
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.state.set_active(active, cx);
    }
}

impl gpui_kit::component::dock::Panel for EditorSlot {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.pane_title().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.pane_title()
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        Some(vec![self.collapse_button(cx)])
    }
}

impl gpui_kit::EventEmitter<gpui_kit::component::dock::PanelEvent> for EditorSlot {}

impl gpui_kit::EventEmitter<PaneEvent> for EditorSlot {}

impl gpui_kit::Focusable for EditorSlot {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.state.focus_handle().clone()
    }
}

impl Render for EditorSlot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Los problemas viven en el dock inferior del explorador (M2).
        pane_container(SlotKind::Editor, cx)
            .flex()
            .flex_col()
            .track_focus(self.state.focus_handle())
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.activate_by_user(window, cx)),
            )
            .child(div().flex_1().min_h_0().child(self.editor.clone()))
    }
}

/// Convierte la sección `latex` de `settings.json` (lsx-core) al tipo del motor.
fn bridge_latex_settings(section: &lsx_core::config::LatexSettings) -> LatexSettings {
    serde_json::to_value(section)
        .ok()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

/// Panel homogéneo para el shell (el dock los mantiene como entidades propias).
enum AnyPanel {
    Tree(Entity<TreePanel>),
    Editor(Entity<EditorSlot>),
    Pdf(Entity<PdfSlot>),
    Chat(Entity<ChatSlot>),
}

impl AnyPanel {
    fn set_visible(&self, visible: bool, cx: &mut App) {
        match self {
            AnyPanel::Tree(panel) => {
                panel.update(cx, |panel, cx| panel.set_visible(visible, cx));
            }
            AnyPanel::Editor(panel) => {
                panel.update(cx, |panel, cx| panel.set_visible(visible, cx));
            }
            AnyPanel::Pdf(panel) => {
                panel.update(cx, |panel, cx| panel.set_visible(visible, cx));
            }
            AnyPanel::Chat(panel) => {
                panel.update(cx, |panel, cx| panel.set_visible(visible, cx));
            }
        }
    }
}

/// Entra en focus mode sobre `active`: oculta el resto y guarda sus anchos.
///
/// Devuelve `true` si entró (siempre, por ahora: incluso sin hojas que ocultar
/// el modo queda registrado y Ctrl+Alt+M lo deshace).
pub fn enter_focus(layout: &mut ShellLayout, active: SlotKind) -> bool {
    if layout.focus.is_some() {
        return false;
    }
    let mut hidden = Vec::new();
    let mut fractions = Vec::new();
    for kind in SlotKind::ALL {
        if kind == active {
            continue;
        }
        let Some(slot) = layout.slot(kind) else {
            continue;
        };
        if slot.collapsed {
            continue;
        }
        hidden.push(slot.id.clone());
        fractions.push(slot.width_fraction);
    }
    for id in &hidden {
        if let Some(kind) = SlotKind::from_id(id)
            && let Some(slot) = layout.slot_mut(kind)
        {
            slot.collapsed = true;
        }
    }
    layout.focus = Some(FocusMode {
        hidden,
        previous_fractions: fractions,
    });
    true
}

/// Sale del focus mode: restaura visibilidad y fracciones previas.
pub fn exit_focus(layout: &mut ShellLayout) -> bool {
    let Some(focus) = layout.focus.take() else {
        return false;
    };
    for (id, fraction) in focus.hidden.iter().zip(focus.previous_fractions.iter()) {
        let Some(kind) = SlotKind::from_id(id) else {
            continue;
        };
        let Some(slot) = layout.slot_mut(kind) else {
            continue;
        };
        slot.collapsed = false;
        if fraction.is_finite() && *fraction > 0.0 {
            slot.width_fraction = *fraction;
        }
    }
    true
}

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .id("app-root")
            .track_focus(&self.focus)
            .key_context("LxStudio")
            .on_action(cx.listener(|this, _: &ToggleTreePane, window, cx| {
                this.toggle_slot(SlotKind::Tree, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SyncForward, window, cx| this.sync_forward(window, cx)),
            )
            .on_action(cx.listener(|this, _: &TogglePdfPane, window, cx| {
                this.toggle_slot(SlotKind::Pdf, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleChatPane, window, cx| {
                this.toggle_slot(SlotKind::Chat, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleFocusMode, window, cx| {
                this.toggle_focus_mode(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenSettingsPane, window, cx| {
                    this.open_settings(window, cx)
                }),
            )
            .child(self.render_title_bar(window, cx))
            .child(self.render_toolbar(window, cx))
            .child(div().flex_1().min_h_0().child(self.dock.clone()))
            .child(status_bar::render(&self.status_model, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collapsed(layout: &ShellLayout) -> Vec<SlotKind> {
        SlotKind::ALL
            .into_iter()
            .filter(|kind| layout.is_collapsed(*kind))
            .collect()
    }

    #[test]
    fn el_inverso_del_scroll_sync_se_bloquea_tras_un_scroll_programatico() {
        let now = Instant::now();
        // Desactivado: nunca.
        assert!(!reverse_allowed(false, None, now));
        // Activado y sin forward reciente: sí.
        assert!(reverse_allowed(true, None, now));
        // Activado pero el visor acaba de desplazarse por el sync: se ignora
        // (si no, las dos direcciones se perseguirían).
        assert!(!reverse_allowed(true, Some(now), now));
        assert!(!reverse_allowed(
            true,
            Some(now),
            now + SYNC_LOOP_GUARD - Duration::from_millis(1)
        ));
        // Pasada la ventana de guarda vuelve a permitirse.
        assert!(reverse_allowed(
            true,
            Some(now),
            now + SYNC_LOOP_GUARD + Duration::from_millis(1)
        ));
    }

    #[test]
    fn focus_mode_hides_the_other_visible_panels_and_restores_them() {
        let mut layout = ShellLayout::four_columns();
        layout.slot_mut(SlotKind::Chat).unwrap().collapsed = true;
        layout.slot_mut(SlotKind::Tree).unwrap().width_fraction = 0.33;

        assert!(enter_focus(&mut layout, SlotKind::Editor));
        assert_eq!(
            collapsed(&layout),
            [SlotKind::Tree, SlotKind::Pdf, SlotKind::Chat]
        );
        let focus = layout.focus.as_ref().expect("focus registrado");
        assert_eq!(focus.hidden, ["tree", "pdf"]);
        assert_eq!(focus.previous_fractions.len(), 2);

        assert!(exit_focus(&mut layout));
        assert_eq!(collapsed(&layout), [SlotKind::Chat]);
        assert!((layout.slot(SlotKind::Tree).unwrap().width_fraction - 0.33).abs() < 1e-6);
        assert!(layout.focus.is_none());
    }

    #[test]
    fn focus_mode_is_a_no_op_when_already_active() {
        let mut layout = ShellLayout::four_columns();
        assert!(enter_focus(&mut layout, SlotKind::Pdf));
        assert!(!enter_focus(&mut layout, SlotKind::Tree));
        assert_eq!(
            layout.focus.as_ref().unwrap().hidden,
            ["tree", "editor", "chat"]
        );
        assert!(exit_focus(&mut layout));
        assert!(!exit_focus(&mut layout));
    }

    #[test]
    fn focus_mode_keeps_panels_collapsed_before_the_maximization() {
        let mut layout = ShellLayout::four_columns();
        layout.slot_mut(SlotKind::Tree).unwrap().collapsed = true;
        layout.slot_mut(SlotKind::Chat).unwrap().collapsed = true;
        enter_focus(&mut layout, SlotKind::Editor);
        assert_eq!(layout.focus.as_ref().unwrap().hidden, ["pdf"]);
        exit_focus(&mut layout);
        // El árbol ya estaba colapsado antes: sigue colapsado.
        assert_eq!(collapsed(&layout), [SlotKind::Tree, SlotKind::Chat]);
    }

    #[test]
    fn the_warning_list_starts_with_the_dock_parse_issue() {
        let warning = ConfigWarning::new("ui.dock", "layout ilegible");
        assert_eq!(warning.key, "ui.dock");
    }
}
