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
//!   (el modal de permiso vive en el panel de chat).
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
    ActiveTheme as _, GlobalState, Selectable as _, Sizable as _, TitleBar, WindowExt as _,
    button::{Button, ButtonVariants as _, DropdownButton},
    command::{Command, CommandState},
    dialog::{DialogClose, DialogDescription, DialogFooter},
    dock::{DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, PanelInfo, panel_handle},
    h_flex,
    menu::AppMenuBar,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, Menu, MenuItem, ParentElement as _, Pixels, Render, SharedString, Styled as _,
    Subscription, Task, WeakEntity, Window, WindowBounds, div, point, px, size,
};
use lsx_core::config::{ConfigWarning, Settings, WindowSettings};
use lsx_core::root::PortableRoot;
use lsx_editor::document::AutosaveConfig;
use lsx_latex::{
    CompileController, CompileEvent, CompileStatus, Diagnostic, EngineInfo, LatexEngine,
    LatexSettings, TexLiveEngine, TexLivePaths, detect_main,
};

use crate::layout::{FocusMode, ShellLayout, SlotKind};
use crate::notifications;
use crate::palette::PaletteCommand;
use crate::panels::bottom::BottomPanel;
use crate::panels::chat::{ChatPanel, ChatPanelConfig, ChatPanelEvent};
use crate::panels::editor::{EditorPanel, EditorPanelConfig, EditorPanelEvent};
use crate::panels::pdf::{PdfPanel, PdfPanelEvent, SyncRect, ZoomMode};
use crate::panels::problems::{ProblemsEvent, ProblemsPanel};
use crate::panels::symbols::SymbolsPanel;
use crate::panels::tree::{TreeFsEvent, TreeOpenKind, TreePanelEvent};
use crate::panels::{Pane, PaneEvent, PaneState, TreePanel, pane_container};
use crate::project::ProjectService;
use crate::settings_ui;
use crate::status::{AiState, StatusHandle, StatusMessage, StatusModel};
use crate::{status_bar, theme, toolbar};

/// Debounce del guardado del layout (ms) tras `DockEvent::LayoutChanged`.
const LAYOUT_SAVE_DEBOUNCE: Duration = Duration::from_millis(450);
/// Debounce del guardado de la geometría de la ventana (ms).
const WINDOW_STATE_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
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
        OpenSettingsPane,
        ToggleEditorPane,
        CompileProject,
        SaveDocument,
        NewProject,
        OpenProject,
        ResetLayout,
        ShowShortcuts,
        ShowAbout,
        QuitApp
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
        KeyBinding::new("ctrl-alt-2", ToggleEditorPane, None),
        KeyBinding::new("ctrl-alt-3", TogglePdfPane, None),
        KeyBinding::new("ctrl-alt-v", TogglePdfPane, None),
        KeyBinding::new("ctrl-alt-4", ToggleChatPane, None),
        KeyBinding::new("ctrl-alt-m", ToggleFocusMode, None),
        KeyBinding::new("ctrl-alt-j", SyncForward, None),
        KeyBinding::new("ctrl-p", OpenPalette, None),
        KeyBinding::new("ctrl-shift-p", OpenPalette, None),
        KeyBinding::new("ctrl-,", OpenSettingsPane, None),
        KeyBinding::new("ctrl-s", SaveDocument, None),
        KeyBinding::new("ctrl-b", CompileProject, None),
        KeyBinding::new("f1", ShowShortcuts, None),
        KeyBinding::new("ctrl-q", QuitApp, None),
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
    /// `tex/bin/windows` efectivo (override de Ajustes o `tex/` portable).
    tex_bin: PathBuf,
    engine_info: Option<EngineInfo>,
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
    menu_bar: Entity<AppMenuBar>,
    /// Al menos un documento del editor tiene cambios sin guardar.
    editor_dirty: bool,
    /// Cierre confirmado por el usuario: `on_window_should_close` deja pasar.
    force_close: bool,
    layout_generation: u64,
    layout_dirty: bool,
    window_state_generation: u64,
    window_state_dirty: bool,
    _status_task: Task<()>,
    _compile_task: Task<()>,
    _engine_task: Task<()>,
    _dock_events: Subscription,
    _pane_events: Vec<Subscription>,
    _integration_events: Vec<Subscription>,
    _window_events: Vec<Subscription>,
}

impl AppRoot {
    /// Monta el shell completo. Ver el módulo para el orden de las piezas.
    pub fn new(
        portable: PortableRoot,
        settings: Settings,
        mut warnings: Vec<ConfigWarning>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (dock, _skin) = DockSkin::dock_area("latex-studio-dock", Some(1), window, cx);
        let (status_handle, status_rx) = crate::status::channel();

        // Primer arranque de una instalación (M3-3): el instalador ligero no crea
        // `workspace/` ni `config/`, y sin workspace el `cwd` de `opencode serve`
        // no existe (el chat se quedaría sin agente).
        if let Err(error) = portable.ensure_layout() {
            warnings.push(ConfigWarning::new(
                "root.layout",
                format!("no se pudo preparar workspace/ y config/: {error}"),
            ));
        }

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
                EditorPanelConfig {
                    autosave: AutosaveConfig {
                        enabled: settings.editor.autosave.enabled,
                        interval_secs: u64::from(settings.editor.autosave.interval_secs),
                    },
                    ..EditorPanelConfig::default()
                },
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
        let tex_bin = resolve_tex_bin(&settings, &portable);
        let pdf_panel = cx.new(|cx| PdfPanel::with_cache_mb(settings.pdf.max_cache_mb, cx));
        // `synctex.exe` de la distro portable: lo usan el salto inverso
        // (Ctrl+clic) y el scroll sync inverso (M1-C5). El shell reacciona a
        // `PdfPanelEvent::InverseJump` para decidir salto vs scroll.
        pdf_panel.update(cx, |panel, cx| {
            panel.set_synctex_exe(tex_bin.join("synctex.exe"));
            let mode = ZoomMode::from_setting(&settings.pdf.zoom_mode);
            panel.set_zoom_mode(mode, cx);
            if mode == ZoomMode::Manual {
                panel.set_zoom(settings.pdf.zoom, cx);
            }
        });
        let pdf = cx.new(|cx| PdfSlot::new(pdf_panel.clone(), Some(dock.downgrade()), cx));
        // Chat real (M1-E3..E6): el panel arranca el supervisor si `ai.autoStart`
        // (ver `panels::chat`); en modo replay (`LSX_CHAT_REPLAY`) no hay server.
        let chat_project_name = startup
            .as_ref()
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "LaTeX Studio".to_owned());
        let mut chat_config = ChatPanelConfig::from_root(
            &portable,
            &chat_root,
            chat_project_name,
            &settings.ai,
            status_handle.clone(),
            Some(editor_panel.clone()),
            Some(problems.clone()),
        );
        if let Some(path) = &settings.advanced.opencode_exe_override {
            chat_config.exe = resolve_opencode_exe(path);
        }
        let chat_panel = cx.new(|cx| ChatPanel::new(window, cx, chat_config));
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
        let tex_bin_utf8 = Utf8PathBuf::from_path_buf(tex_bin.clone()).ok();
        let startup_main: Option<Utf8PathBuf> = project_root
            .as_deref()
            .and_then(Utf8Path::from_path)
            .and_then(|root| {
                detect_main(root, latex_settings.main_file.as_deref(), None)
                    .ok()
                    .flatten()
            });
        let compile: Option<Arc<CompileController>> = tex_bin_utf8
            .filter(|bin| bin.join("latexmk.exe").is_file())
            .map(|bin| {
                let engine: Arc<dyn LatexEngine> =
                    Arc::new(TexLiveEngine::new(TexLivePaths::from_bin(bin)));
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
                |this, _panel, event: &ChatPanelEvent, window, cx| {
                    this.on_chat_event(event, window, cx);
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

        // Motor LaTeX: presencia inmediata + probe real asíncrono (`probe_engine`).
        let engine_available = tex_bin.join("latexmk.exe").is_file();
        let status_model = StatusModel {
            engine_label: if engine_available {
                "TeX Live (comprobando…)".to_owned()
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

        // Barra de menús de escritorio (Archivo/Ver/Herramientas/Ayuda). Los
        // items despachan acciones que el `render` de la raíz escucha.
        let menus = app_menus(&layout);
        GlobalState::global_mut(cx).set_app_menus(menus.into_iter().map(Menu::owned).collect());
        let menu_bar = AppMenuBar::new(cx);

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
            tex_bin,
            engine_info: None,
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
            menu_bar,
            editor_dirty: false,
            force_close: false,
            layout_generation: 0,
            layout_dirty: false,
            window_state_generation: 0,
            window_state_dirty: false,
            _status_task: status_task,
            _compile_task: compile_task,
            _engine_task: cx.spawn(async move |_this, _cx| {}),
            _dock_events: dock_events,
            _pane_events: pane_events,
            _integration_events: integration_events,
            _window_events: Vec::new(),
        };
        let layout = root.layout.clone();
        root.apply_layout(&layout, window, cx);
        root.sync_app_menus(cx);

        // Persistencia de geometría + cierre seguro con documentos sucios.
        root._window_events
            .push(cx.observe_window_bounds(window, |root, window, cx| {
                root.on_window_bounds_changed(window, cx);
            }));
        // `ui.theme = system`: si el SO cambia de claro a oscuro con la app
        // abierta, reaplicamos el tema (incluido el acento) sin reiniciar.
        root._window_events
            .push(cx.observe_window_appearance(window, |root, _window, cx| {
                if root.settings.ui.theme.eq_ignore_ascii_case("system") {
                    theme::apply(&root.settings, cx);
                }
            }));
        // Hook de QA/visual: abre Ajustes al arrancar sin depender de clics
        // sintéticos. No se usa en producción salvo que se exporte la variable.
        if let Some(open) = std::env::var_os("LSX_QA_OPEN_SETTINGS") {
            let ai = open.to_string_lossy() == "ai";
            let weak = cx.weak_entity();
            window.defer(cx, move |window, cx| {
                let _ = weak.update(cx, |root, cx| {
                    if ai {
                        root.open_ai_settings(window, cx);
                    } else {
                        root.open_settings(window, cx);
                    }
                });
            });
        }
        if std::env::var_os("LSX_QA_OPEN_ENGINE").is_some() {
            let weak = cx.weak_entity();
            window.defer(cx, move |window, cx| {
                let _ = weak.update(cx, |root, cx| root.open_engine_help(window, cx));
            });
        }

        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let force = weak
                .read_with(cx, |root, _| root.force_close)
                .unwrap_or(true);
            if force {
                return true;
            }
            let dirty = weak
                .read_with(cx, |root, cx| root.editor_panel.read(cx).dirty_count())
                .unwrap_or(0);
            if dirty == 0 {
                return true;
            }
            let _ = weak.update(cx, |root, cx| {
                root.persist_window_state(cx);
                root.open_close_dialog(dirty, window, cx);
            });
            false
        });

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
        root.probe_engine(cx);
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
        self.sync_app_menus(cx);
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

    // --- ventana ------------------------------------------------------------

    /// Reconstruye los menús con el estado actual (checks de paneles) y pide a
    /// la barra que se recargue.
    fn sync_app_menus(&self, cx: &mut Context<Self>) {
        let menus = app_menus(&self.layout);
        GlobalState::global_mut(cx).set_app_menus(menus.into_iter().map(Menu::owned).collect());
        self.menu_bar.update(cx, |bar, cx| bar.reload(cx));
    }

    fn on_window_bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_fullscreen() {
            return;
        }
        let maximized = window.is_maximized();
        let bounds = window.window_bounds().get_bounds();
        let width = bounds.size.width.as_f32();
        let height = bounds.size.height.as_f32();
        if !width.is_finite() || !height.is_finite() || width < 640.0 || height < 480.0 {
            return;
        }
        self.settings.ui.window = WindowSettings {
            x: Some(bounds.origin.x.as_f32()),
            y: Some(bounds.origin.y.as_f32()),
            width,
            height,
            maximized,
            extra: Default::default(),
        };
        self.schedule_window_state_save(cx);
    }

    fn schedule_window_state_save(&mut self, cx: &mut Context<Self>) {
        self.window_state_dirty = true;
        self.window_state_generation = self.window_state_generation.wrapping_add(1);
        let generation = self.window_state_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(WINDOW_STATE_SAVE_DEBOUNCE)
                .await;
            this.update(cx, |root, cx| {
                if root.window_state_dirty && root.window_state_generation == generation {
                    root.persist_window_state(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn persist_window_state(&mut self, cx: &mut Context<Self>) {
        self.window_state_dirty = false;
        if let Err(err) = self.settings.save(&self.portable) {
            tracing::warn!(target: "lsx::ui", error = %err, "no se pudo guardar la ventana");
        }
        let _ = cx;
    }

    /// Diálogo de cierre con documentos sin guardar.
    fn open_close_dialog(&mut self, dirty: usize, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let weak_discard = weak.clone();
            let weak_save = weak.clone();
            dialog
                .title("Cerrar LaTeX Studio")
                .width(px(540.))
                .child(DialogDescription::new().child(format!(
                    "Hay {dirty} documento(s) con cambios sin guardar. \
                     Si cierras sin guardar, esos cambios se pierden."
                )))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().child(
                                Button::new("close-window-cancel")
                                    .label("Cancelar")
                                    .outline(),
                            ),
                        )
                        .child(
                            Button::new("close-window-discard")
                                .label("Descartar y cerrar")
                                .danger()
                                .on_click(move |_, window, cx| {
                                    let _ = weak_discard.update(cx, |root, cx| {
                                        root.force_close = true;
                                        root.persist_window_state(cx);
                                        window.close_dialog(cx);
                                        window.remove_window();
                                    });
                                }),
                        )
                        .child(
                            Button::new("close-window-save")
                                .label("Guardar y cerrar")
                                .primary()
                                .on_click(move |_, window, cx| {
                                    let saved = weak_save
                                        .update(cx, |root, cx| {
                                            let saved = root
                                                .editor_panel
                                                .update(cx, |panel, cx| panel.save_all(cx));
                                            if saved {
                                                root.force_close = true;
                                                root.persist_window_state(cx);
                                            }
                                            saved
                                        })
                                        .unwrap_or(false);
                                    if saved {
                                        window.close_dialog(cx);
                                        window.remove_window();
                                    } else {
                                        notifications::error(
                                            "No se pudieron guardar todos los documentos.",
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                        ),
                )
        });
    }

    /// Restablece el layout de cuatro columnas y lo persiste.
    fn reset_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let layout = ShellLayout::four_columns();
        self.apply_layout(&layout, window, cx);
        self.sync_app_menus(cx);
        self.schedule_layout_save(cx);
        notifications::success("Layout restablecido.", window, cx);
    }

    fn open_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.open_dialog(cx, |dialog, _window, _cx| {
            let rows = [
                ("Compilar", "Ctrl+B / botón Compilar"),
                ("Guardar", "Ctrl+S"),
                ("Buscar", "Ctrl+F"),
                ("Reemplazar", "Ctrl+H"),
                ("Paleta de comandos", "Ctrl+P / Ctrl+Shift+P"),
                ("Ajustes", "Ctrl+,"),
                ("Mostrar/ocultar paneles", "Ctrl+Alt+1..4"),
                ("Modo focus", "Ctrl+Alt+M"),
                ("SyncTeX forward", "Ctrl+Alt+J"),
                ("Ayuda", "F1"),
            ];
            dialog
                .title("Atajos de teclado")
                .width(px(560.))
                .child(v_flex().gap_1().children(rows.map(|(action, keys)| {
                    h_flex()
                        .justify_between()
                        .gap_4()
                        .child(div().text_sm().child(action))
                        .child(
                            div()
                                .text_xs()
                                .text_color(_cx.theme().muted_foreground)
                                .child(keys),
                        )
                })))
        });
    }

    /// Comprueba el motor real en background (versión, synctex, tlmgr).
    fn probe_engine(&mut self, cx: &mut Context<Self>) {
        let Ok(bin) = Utf8PathBuf::from_path_buf(self.tex_bin.clone()) else {
            self.status_model.engine_available = false;
            self.status_model.engine_label = "TeX Live no disponible (ruta no UTF-8)".to_owned();
            cx.notify();
            return;
        };
        self.status_model.engine_available = bin.join("latexmk.exe").is_file();
        self.status_model.engine_label = "TeX Live (comprobando…)".to_owned();
        cx.notify();
        self._engine_task = cx.spawn(async move |this, cx| {
            let info = cx
                .background_spawn(async move {
                    TexLiveEngine::new(TexLivePaths::from_bin(bin)).probe().ok()
                })
                .await;
            this.update(cx, |root, cx| {
                root.engine_info = info;
                match &root.engine_info {
                    Some(info) if info.available => {
                        let version = info.version.as_deref().unwrap_or("?");
                        let mut label = format!("TeX Live {version}");
                        if info.synctex.is_none() {
                            label.push_str(" · sin SyncTeX");
                        }
                        root.status_model.engine_available = true;
                        root.status_model.engine_label = label;
                    }
                    Some(_) => {
                        root.status_model.engine_available = false;
                        root.status_model.engine_label = "TeX Live con error".to_owned();
                    }
                    None => {
                        root.status_model.engine_available = false;
                        root.status_model.engine_label = "TeX Live no encontrado".to_owned();
                    }
                }
                cx.notify();
            })
            .ok();
        });
    }

    /// Explica el estado del motor y ofrece reintentar/abrir Ajustes.
    fn open_engine_help(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        let info = self.engine_info.clone();
        let tex_bin = self.tex_bin.clone();
        let available = self.status_model.engine_available;
        let engine_label = self.status_model.engine_label.clone();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let sync = info
                .as_ref()
                .and_then(|info| info.synctex.as_ref())
                .is_some()
                || tex_bin.join("synctex.exe").is_file();
            let biber = tex_bin.join("biber.exe").is_file();
            let detail = if available {
                format!(
                    "Motor disponible: {engine_label}.\nSyncTeX: {} · biber: {}.\n\n\
                     LaTeX Studio lo usa desde:\n{}",
                    if sync { "sí" } else { "no" },
                    if biber { "sí" } else { "no" },
                    tex_bin.display()
                )
            } else {
                format!(
                    "No se encontró un motor LaTeX utilizable.\n\n\
                     Vuelve a ejecutar el instalador y elige «Descargar TinyTeX-1», o copia un \
                     TeX Live con latexmk/synctex/biber en:\n{}\n\n\
                     También puedes usar Ajustes → Compilación para revisar la ruta.",
                    tex_bin.display()
                )
            };
            let weak_retry = weak.clone();
            let weak_settings = weak.clone();
            let folder = tex_bin.clone();
            dialog
                .title("Motor LaTeX")
                .width(px(620.))
                .child(DialogDescription::new().child(detail))
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("engine-open-folder")
                                .label("Abrir carpeta")
                                .outline()
                                .on_click(move |_, window, cx| {
                                    if let Err(error) = crate::ai_setup::open_folder(&folder) {
                                        notifications::error(error.to_string(), window, cx);
                                    }
                                }),
                        )
                        .child(
                            Button::new("engine-settings")
                                .label("Abrir Ajustes")
                                .outline()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak_settings.update(cx, |root, cx| {
                                        root.open_compilation_settings(window, cx);
                                    });
                                }),
                        )
                        .child(
                            Button::new("engine-retry")
                                .label("Reintentar")
                                .primary()
                                .on_click(move |_, window, cx| {
                                    let _ = weak_retry.update(cx, |root, cx| {
                                        root.probe_engine(cx);
                                    });
                                    window.close_dialog(cx);
                                    notifications::info("Comprobando el motor LaTeX…", window, cx);
                                }),
                        ),
                )
        });
    }

    fn open_compilation_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let weak = cx.weak_entity();
        settings_ui::open_compilation_dialog(window, cx, settings, weak);
    }

    fn open_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.open_dialog(cx, |dialog, _window, _cx| {
            dialog
                .title("Acerca de LaTeX Studio")
                .width(px(520.))
                .child(DialogDescription::new().child(format!(
                    "LaTeX Studio {} — IDE LaTeX nativo y portable.\n\
                     Editor, PDF con SyncTeX y chat IA sobre opencode.\n\n\
                     Repositorio: github.com/marcosflz/OpenLeaf",
                    env!("CARGO_PKG_VERSION")
                )))
        });
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
        window: &mut Window,
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
            EditorPanelEvent::DirtyChanged { dirty } => {
                self.editor_dirty = *dirty;
            }
            EditorPanelEvent::NewProjectRequested => self.open_new_project_dialog(window, cx),
            EditorPanelEvent::OpenProjectRequested => self.open_project_dialog(window, cx),
            _ => {}
        }
        cx.notify();
    }

    /// Eventos del panel de chat (persistencia de ajustes que toca el usuario).
    fn on_chat_event(
        &mut self,
        event: &ChatPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            ChatPanelEvent::SelectionChanged { model, agent } => {
                if model.is_some() {
                    self.settings.ai.model = model.clone();
                }
                self.settings.ai.agent = agent.clone();
                if let Err(err) = self.settings.save(&self.portable) {
                    tracing::warn!(
                        target: "lsx::ui",
                        error = %err,
                        "no se pudo guardar ai.model/ai.agent"
                    );
                }
            }
            ChatPanelEvent::OpenAiSettings => self.open_ai_settings(window, cx),
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
        let Ok(exe) = Utf8PathBuf::from_path_buf(self.tex_bin.join("synctex.exe")) else {
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
        let Ok(exe) = Utf8PathBuf::from_path_buf(self.tex_bin.join("synctex.exe")) else {
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
        }
    }

    /// Abre los ajustes (Ctrl+,).
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let weak = cx.weak_entity();
        settings_ui::open_dialog(window, cx, settings, weak);
    }

    /// Abre Ajustes en la sección IA (CTA del chat).
    fn open_ai_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let weak = cx.weak_entity();
        settings_ui::open_ai_dialog(window, cx, settings, weak);
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
        self.latex_settings = bridge_latex_settings(&self.settings.latex);
        if let Some(controller) = &self.compile {
            controller.apply_settings(&self.latex_settings);
        }
        theme::apply(&self.settings, cx);

        // Autosave y zoom del PDF se aplican en caliente.
        let autosave = AutosaveConfig {
            enabled: self.settings.editor.autosave.enabled,
            interval_secs: u64::from(self.settings.editor.autosave.interval_secs),
        };
        self.editor_panel
            .update(cx, |panel, cx| panel.set_autosave(autosave, cx));
        let zoom_mode = ZoomMode::from_setting(&self.settings.pdf.zoom_mode);
        self.pdf_panel.update(cx, |panel, cx| {
            panel.set_zoom_mode(zoom_mode, cx);
            if zoom_mode == ZoomMode::Manual {
                panel.set_zoom(self.settings.pdf.zoom, cx);
            }
        });

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

    // --- render ---------------------------------------------------------------

    fn render_title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project = self
            .status_model
            .project
            .clone()
            .unwrap_or_else(|| rust_i18n::t!("ui.app.status.no_project").to_string());
        let project = if self.editor_dirty {
            format!("• {project}")
        } else {
            project
        };
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
                cx.listener(|this, _, window, cx| this.open_engine_help(window, cx)),
            ))
            .child(toolbar::separator(cx))
            .child(toggles)
            .child(div().flex_1())
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
        Some(vec![
            Button::new("slot-pdf-external")
                .icon(IconName::ExternalLink)
                .xsmall()
                .tab_stop(false)
                .tooltip("Abrir en visor externo")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.pdf.update(cx, |panel, _| panel.open_external());
                })),
        ])
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
        _cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        None
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
        _cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        None
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

/// `tex/bin/windows` efectivo: override de `advanced.texBinOverride` o el
/// `tex/` portable. Acepta tanto el directorio `bin` como el `latexmk.exe`.
fn resolve_tex_bin(settings: &Settings, portable: &PortableRoot) -> PathBuf {
    if let Some(raw) = &settings.advanced.tex_bin_override {
        let path = PathBuf::from(raw);
        if path.join("latexmk.exe").is_file() {
            return path;
        }
        if path.is_file()
            && path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("latexmk.exe"))
            && let Some(parent) = path.parent()
        {
            return parent.to_path_buf();
        }
        tracing::warn!(
            target: "lsx::ui",
            path = %path.display(),
            "advanced.texBinOverride no contiene latexmk.exe; se usa tex/ portable"
        );
    }
    portable.tex_dir().join("bin").join("windows")
}

fn resolve_opencode_exe(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_dir() {
        path.join("opencode.exe")
    } else {
        path
    }
}

/// Calcula los bounds iniciales de la ventana a partir de `ui.window`.
///
/// Si la posición guardada ya no cae en ningún monitor visible (por ejemplo,
/// se desconectó la pantalla externa), la ventana se centra. `maximized`
/// conserva el tamaño de restaurar.
pub fn initial_window_bounds(state: &WindowSettings, cx: &App) -> WindowBounds {
    let width = state.width.clamp(640.0, 16_384.0);
    let height = state.height.clamp(480.0, 16_384.0);
    let size = size(px(width), px(height));
    let restore = match (state.x, state.y) {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => {
            Bounds::new(point(px(x), px(y)), size)
        }
        _ => Bounds::centered(None, size, cx),
    };
    let on_screen = cx
        .displays()
        .iter()
        .any(|display| display.visible_bounds().contains(&restore.center()));
    let restore = if on_screen {
        restore
    } else {
        Bounds::centered(None, restore.size, cx)
    };
    if state.maximized {
        WindowBounds::Maximized(restore)
    } else {
        WindowBounds::Windowed(restore)
    }
}

/// Menús de la barra superior. El estado de los checks se recalcula con
/// [`AppRoot::sync_app_menus`] cuando cambia el layout.
fn app_menus(layout: &ShellLayout) -> Vec<Menu> {
    let checked = |kind: SlotKind| !layout.is_collapsed(kind);
    vec![
        Menu::new("Archivo").items([
            MenuItem::action("Nuevo proyecto…", NewProject),
            MenuItem::action("Abrir carpeta…", OpenProject),
            MenuItem::separator(),
            MenuItem::action("Guardar", SaveDocument),
            MenuItem::separator(),
            MenuItem::action("Salir", QuitApp),
        ]),
        Menu::new("Ver").items([
            MenuItem::action("Explorador", ToggleTreePane).checked(checked(SlotKind::Tree)),
            MenuItem::action("Editor", ToggleEditorPane).checked(checked(SlotKind::Editor)),
            MenuItem::action("PDF", TogglePdfPane).checked(checked(SlotKind::Pdf)),
            MenuItem::action("Chat IA", ToggleChatPane).checked(checked(SlotKind::Chat)),
            MenuItem::separator(),
            MenuItem::action("Modo focus", ToggleFocusMode).checked(layout.focus.is_some()),
            MenuItem::action("Restablecer layout", ResetLayout),
        ]),
        Menu::new("Herramientas").items([
            MenuItem::action("Compilar", CompileProject),
            MenuItem::action("Ir a la línea del PDF (SyncTeX)", SyncForward),
            MenuItem::separator(),
            MenuItem::action("Ajustes…", OpenSettingsPane),
            MenuItem::action("Paleta de comandos…", OpenPalette),
        ]),
        Menu::new("Ayuda").items([
            MenuItem::action("Atajos de teclado", ShowShortcuts),
            MenuItem::action("Acerca de LaTeX Studio", ShowAbout),
        ]),
    ]
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
        let project = self
            .status_model
            .project
            .clone()
            .unwrap_or_else(|| "LaTeX Studio".to_owned());
        let title = if self.editor_dirty {
            format!("• {project} — LaTeX Studio")
        } else {
            format!("{project} — LaTeX Studio")
        };
        window.set_window_title(&title);
        window.set_window_edited(self.editor_dirty);

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
            .on_action(cx.listener(|this, _: &ToggleEditorPane, window, cx| {
                this.toggle_slot(SlotKind::Editor, window, cx)
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
            .on_action(
                cx.listener(|this, _: &CompileProject, window, cx| {
                    this.request_compile(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &SaveDocument, window, cx| {
                this.editor_panel
                    .update(cx, |panel, cx| panel.save_active(window, cx));
            }))
            .on_action(cx.listener(|this, _: &NewProject, window, cx| {
                this.open_new_project_dialog(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &OpenProject, window, cx| {
                    this.open_project_dialog(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &ResetLayout, window, cx| this.reset_layout(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShowShortcuts, window, cx| this.open_shortcuts(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ShowAbout, window, cx| this.open_about(window, cx)))
            .on_action(cx.listener(|_this, _: &QuitApp, window, _cx| {
                window.remove_window();
            }))
            .child(self.render_title_bar(window, cx))
            .child(
                div()
                    .flex_none()
                    .h(px(28.))
                    .px_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().title_bar)
                    .child(self.menu_bar.clone()),
            )
            .child(self.render_toolbar(window, cx))
            .child(div().flex_1().min_h_0().child(self.dock.clone()))
            .child(status_bar::render(
                &self.status_model,
                cx,
                cx.listener(|this, _, window, cx| this.open_engine_help(window, cx)),
                cx.listener(|this, _, window, cx| this.open_ai_settings(window, cx)),
            ))
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
    fn resolve_tex_bin_respeta_el_override_y_cae_al_portable() {
        let root = std::env::temp_dir().join(format!("lsx-ui-texbin-{}", std::process::id()));
        let portable_bin = root.join("tex").join("bin").join("windows");
        let override_bin = root.join("otro-tex").join("bin").join("windows");
        std::fs::create_dir_all(&portable_bin).unwrap();
        std::fs::create_dir_all(&override_bin).unwrap();
        std::fs::write(portable_bin.join("latexmk.exe"), b"x").unwrap();
        std::fs::write(override_bin.join("latexmk.exe"), b"x").unwrap();

        let portable = PortableRoot::from_dir(&root);
        let mut settings = Settings::default();
        assert_eq!(resolve_tex_bin(&settings, &portable), portable_bin);

        settings.advanced.tex_bin_override = Some(override_bin.display().to_string());
        assert_eq!(resolve_tex_bin(&settings, &portable), override_bin);

        settings.advanced.tex_bin_override =
            Some(override_bin.join("latexmk.exe").display().to_string());
        assert_eq!(resolve_tex_bin(&settings, &portable), override_bin);

        settings.advanced.tex_bin_override = Some(root.join("no-existe").display().to_string());
        assert_eq!(resolve_tex_bin(&settings, &portable), portable_bin);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_opencode_exe_acepta_fichero_o_carpeta() {
        assert_eq!(
            resolve_opencode_exe("C:/tools/opencode.exe"),
            PathBuf::from("C:/tools/opencode.exe")
        );
        assert_eq!(
            resolve_opencode_exe("."),
            PathBuf::from(".").join("opencode.exe")
        );
    }

    #[test]
    fn la_ventana_restaura_maximizado_y_centra_si_la_posicion_no_es_finita() {
        let cx = gpui_kit::TestAppContext::single();
        cx.update(|cx| {
            let maximized = WindowSettings {
                x: Some(100.0),
                y: Some(80.0),
                width: 1200.0,
                height: 800.0,
                maximized: true,
                extra: Default::default(),
            };
            assert!(matches!(
                initial_window_bounds(&maximized, cx),
                WindowBounds::Maximized(_)
            ));

            let offscreen = WindowSettings {
                x: Some(f32::NAN),
                y: None,
                maximized: false,
                ..maximized
            };
            assert!(matches!(
                initial_window_bounds(&offscreen, cx),
                WindowBounds::Windowed(_)
            ));
        });
    }

    #[test]
    fn los_menus_cubren_archivo_ver_herramientas_y_ayuda() {
        let menus = app_menus(&ShellLayout::four_columns());
        let names: Vec<String> = menus.iter().map(|menu| menu.name.to_string()).collect();
        assert_eq!(names, ["Archivo", "Ver", "Herramientas", "Ayuda"]);
        assert!(menus.iter().all(|menu| !menu.items.is_empty()));
    }

    #[test]
    fn el_menu_ver_refleja_los_paneles_colapsados() {
        let mut layout = ShellLayout::four_columns();
        layout.slot_mut(SlotKind::Chat).unwrap().collapsed = true;
        let menus = app_menus(&layout);
        let view = menus
            .iter()
            .find(|menu| menu.name == "Ver")
            .expect("menú Ver");
        let chat = view
            .items
            .iter()
            .find(|item| matches!(item, MenuItem::Action { name, .. } if name == "Chat IA"))
            .expect("item Chat IA");
        assert!(!chat.is_checked());
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
