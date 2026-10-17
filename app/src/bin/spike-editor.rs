//! Binario demo M1-B (stream B): panel del editor de `lsx-ui` con pestañas,
//! diagnósticos, watcher/conflictos, buscar/reemplazar, autosave y backups.
//!
//! ```text
//! cargo run -p latex-studio --bin spike-editor                 # ventana demo
//! cargo run -p latex-studio --bin spike-editor -- --check      # smoke sin ventana
//! cargo run -p latex-studio --bin spike-editor -- --demo-conflict
//! cargo run -p latex-studio --bin spike-editor -- --demo-recovery
//! ```
//!
//! Todo el trabajo ocurre sobre una copia en `%TEMP%\latex-studio\m1-editor`
//! (fixtures intactos). El binario cablea el watcher (`FsService::watch` +
//! `async_channel` → hilo de UI) y una toolbar de demo; el panel montable vive
//! en `lsx_ui::panels::editor` y es lo que integrará el shell (stream A).
//!
//! Demos automáticas:
//! - `--demo-conflict`: escribe en el buffer (queda sucio) y simula un editor
//!   externo; el watcher dispara el modal de 3 opciones.
//! - `--demo-recovery`: deja un backup más nuevo que `main.tex` y restaura la
//!   sesión; al abrir se propone recuperarlo (M1-B5).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(linker_messages)]

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, Theme, ThemeMode, h_flex, v_flex};
use gpui_kit::*;
use lsx_core::fs::{FsEvent, FsService, WatcherGuard};
use lsx_editor::backup::BackupStore;
use lsx_editor::document::AutosaveConfig;
use lsx_editor::latex_language;
use lsx_editor::session::{
    SESSION_SCHEMA_VERSION, SessionCursor, SessionState, SessionStore, SessionTab,
};
use lsx_latex::parse_log;
use lsx_ui::panels::editor::{EditorPanel, EditorPanelConfig, EditorPanelEvent};

/// Carpeta de trabajo en `%TEMP%`.
const WORK_DIR: &str = "m1-editor";
/// Fixture de error real (M0-S5): `\undefinedcommand` en la línea 4.
const UNDEFINED_LOG: &str = include_str!("../../../fixtures/logs/undefined-command.log");
/// Fixture del proyecto roto (línea 4 con el control sequence indefinido).
const BROKEN_TEX: &str = include_str!("../../../fixtures/projects/broken-undefined/main.tex");

const INTRO_TEX: &str = "\\section{Introducción}\n\
Este documento demuestra las pestañas del editor (M1-B1).\n\
Cada pestaña es una entidad `EditorState` del kit.\n";

const METODO_TEX: &str = "\\section{Método}\n\
El guardado es atómico (`FsService::write_text_atomic`) y el watcher\n\
propone recargar/sobrescribir/ver diff cuando el archivo cambia fuera.\n";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let check = args.iter().any(|arg| arg == "--check");
    let demo_conflict = args.iter().any(|arg| arg == "--demo-conflict");
    let demo_recovery = args.iter().any(|arg| arg == "--demo-recovery");

    let root = prepare_workspace(demo_recovery);
    if check {
        run_check(&root);
        return;
    }

    println!("spike M1-B · raíz de trabajo {}", root.display());
    println!("(los fixtures del repo nunca se escriben)");

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            let options = WindowOptions {
                window_min_size: Some(size(px(900.), px(560.))),
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(40.), px(40.)),
                    size(px(1180.), px(760.)),
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some(
                        "LaTeX Studio · M1-B editor (pestañas, watcher, diagnósticos)".into(),
                    ),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let root_for_panel = root.clone();
            gpui_kit::open_window(options, cx, move |window, cx| {
                let window_handle = window.window_handle();
                let demo = cx.new(|cx| {
                    SpikeEditor::new(root_for_panel, window_handle, demo_conflict, window, cx)
                });
                demo
            })
            .expect("no se pudo abrir la ventana del spike M1-B");
        });
}

// --- demo automática --------------------------------------------------------

/// Shell mínimo del spike: toolbar + panel del editor + watcher.
struct SpikeEditor {
    root: PathBuf,
    panel: Entity<EditorPanel>,
    watcher_alive: bool,
    log: Vec<String>,
    _window_handle: AnyWindowHandle,
    _watcher: Option<WatcherGuard>,
    _fs_task: Task<()>,
    _panel_sub: Subscription,
}

impl SpikeEditor {
    fn new(
        root: PathBuf,
        window_handle: AnyWindowHandle,
        demo_conflict: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config_dir = root.join("config");
        let config = EditorPanelConfig {
            autosave: AutosaveConfig::default(),
            backup_interval_secs: 5,
            restore_session: true,
        };
        let panel = cx.new(|cx| EditorPanel::with_config(&root, &config_dir, config, window, cx));

        // Sin sesión previa: abre las pestañas demo. Con sesión, restaura las
        // que había (M1-B1). Después se inyectan siempre los diagnósticos
        // reales del fixture sobre `main.tex` (parse_log → set_diagnostics).
        if panel.read(cx).tab_count() == 0 {
            let _ = panel.update(cx, |panel, cx| {
                panel.open_file("main.tex", None, window, cx);
                panel.open_file(Path::new("capitulos").join("intro.tex"), None, window, cx);
                panel.open_file(Path::new("secciones").join("metodo.tex"), None, window, cx);
                // Deja `main.tex` activa.
                panel.open_file("main.tex", Some(1), window, cx);
            });
        }
        let _ = panel.update(cx, |panel, cx| {
            let diags = parse_log(UNDEFINED_LOG);
            panel.set_diagnostics("main.tex", &diags, cx);
        });

        let panel_sub = cx.subscribe(&panel, |shell, _panel, event: &EditorPanelEvent, cx| {
            let line = match event {
                EditorPanelEvent::Opened { path } => format!("abierto  {path}"),
                EditorPanelEvent::Closed { path } => format!("cerrado  {path}"),
                EditorPanelEvent::Saved { path } => format!("guardado {path}"),
                EditorPanelEvent::DirtyChanged { dirty } => {
                    format!("dirty    {dirty}")
                }
                EditorPanelEvent::Status { message } => format!("status   {message}"),
            };
            shell.push_log(line, cx);
        });

        // Watcher: hilo de debounce → async_channel → hilo de UI.
        let fs = FsService::new(&root);
        let (tx, rx) = async_channel::unbounded::<Vec<FsEvent>>();
        let watcher = match fs.watch(move |batch| {
            let _ = tx.send_blocking(batch);
        }) {
            Ok(guard) => Some(guard),
            Err(err) => {
                eprintln!("watcher no disponible: {err}");
                None
            }
        };
        let watcher_alive = watcher.is_some();

        let panel_for_task = panel.clone();
        let fs_task = cx.spawn(async move |this, cx| {
            while let Ok(batch) = rx.recv().await {
                let Some(handle) = cx.update(|app| app.active_window()) else {
                    break;
                };
                let ok = cx
                    .update_window(handle, |_, window, app| {
                        let _ = panel_for_task.update(app, |panel, cx| {
                            panel.apply_fs_batch(batch.clone(), window, cx)
                        });
                        let _ = this.update(app, |shell, cx| {
                            for event in &batch {
                                let line = match event {
                                    FsEvent::Created(path) => format!("fs +  {}", path.display()),
                                    FsEvent::Modified(path) => format!("fs ~  {}", path.display()),
                                    FsEvent::Removed(path) => format!("fs -  {}", path.display()),
                                    FsEvent::Renamed(from, to) => {
                                        format!("fs >  {} => {}", from.display(), to.display())
                                    }
                                };
                                shell.push_log(line, cx);
                            }
                        });
                    })
                    .is_ok();
                if !ok {
                    break;
                }
            }
        });

        if demo_conflict {
            let panel = panel.clone();
            let root = root.clone();
            window.defer(cx, move |window, cx| {
                // 1) el buffer queda sucio; 2) otro editor escribe el archivo.
                let _ = panel.update(cx, |panel, cx| {
                    panel.insert_active(
                        "\n% EDITADO EN EL BUFFER (demo conflicto externo)\n",
                        window,
                        cx,
                    );
                });
                let path = root.join("main.tex");
                let original = std::fs::read_to_string(&path).unwrap_or_default();
                let _ = std::fs::write(
                    &path,
                    format!("{original}% escrito por OTRO editor (fuera del buffer)\n"),
                );
            });
        }

        let mut this = Self {
            root,
            panel,
            watcher_alive,
            log: Vec::new(),
            _window_handle: window_handle,
            _watcher: watcher,
            _fs_task: fs_task,
            _panel_sub: panel_sub,
        };
        this.push_log("demo lista: pestañas + watcher + diagnósticos", cx);
        this
    }

    fn push_log(&mut self, line: impl Into<String>, cx: &mut Context<Self>) {
        self.log.push(line.into());
        if self.log.len() > 200 {
            self.log.remove(0);
        }
        cx.notify();
    }

    /// "Editar fuera": escribe el archivo activo desde otro proceso lógico.
    fn simulate_external_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rel) = self.panel.read(cx).active_path() else {
            return;
        };
        let path = self.root.join(&rel);
        let original = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::write(
            &path,
            format!("{original}% línea añadida por un editor externo\n"),
        );
        let _ = window;
        self.push_log(format!("simulada edición externa en {rel}"), cx);
    }

    fn inject_diagnostics(&mut self, cx: &mut Context<Self>) {
        let diags = parse_log(UNDEFINED_LOG);
        let applied = self.panel.update(cx, |panel, cx| {
            panel.set_diagnostics("main.tex", &diags, cx)
        });
        self.push_log(
            format!("{applied} diagnóstico(s) inyectados en main.tex"),
            cx,
        );
    }

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let autosave_on = self.panel.read(cx).autosave().enabled;
        let dirty = self.panel.read(cx).active_is_dirty();
        let watcher_icon = if self.watcher_alive {
            IconName::CircleCheck
        } else {
            IconName::TriangleAlert
        };
        let watcher_color = if self.watcher_alive {
            cx.theme().success
        } else {
            cx.theme().danger
        };
        h_flex()
            .flex_none()
            .h(px(34.))
            .px_2()
            .gap_2()
            .items_center()
            .bg(cx.theme().secondary)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("M1-B · editor"),
            )
            .child(
                Button::new("demo-save")
                    .label("Guardar (Ctrl+S)")
                    .small()
                    .primary()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.panel.update(cx, |panel, cx| {
                            panel.save_active(window, cx);
                        });
                    })),
            )
            .child(
                Button::new("demo-find")
                    .label("Buscar (Ctrl+F)")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.panel
                            .update(cx, |panel, cx| panel.toggle_find(false, window, cx));
                    })),
            )
            .child(
                Button::new("demo-find-next")
                    .label("Siguiente (F3)")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.panel.update(cx, |panel, cx| panel.find_next(cx));
                    })),
            )
            .child(
                Button::new("demo-diags")
                    .label("Inyectar diagnósticos")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _window, cx| this.inject_diagnostics(cx))),
            )
            .child(
                Button::new("demo-external")
                    .label("Editar fuera")
                    .small()
                    .ghost()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.simulate_external_edit(window, cx)),
                    ),
            )
            .child(
                Button::new("demo-autosave")
                    .label(if autosave_on {
                        "Autosave: ON"
                    } else {
                        "Autosave: OFF"
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _window, cx| {
                        let next = !this.panel.read(cx).autosave().enabled;
                        this.panel.update(cx, |panel, cx| {
                            panel.set_autosave(
                                AutosaveConfig {
                                    enabled: next,
                                    interval_secs: 5,
                                },
                                cx,
                            );
                        });
                    })),
            )
            .child(
                Button::new("demo-backup")
                    .label("Backup ahora")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _window, cx| {
                        let n = this
                            .panel
                            .update(cx, |panel, cx| panel.backup_dirty_now(cx));
                        this.push_log(format!("{n} backup(s) escritos"), cx);
                    })),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(if dirty {
                        cx.theme().yellow
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(if dirty {
                        "buffer sucio"
                    } else {
                        "buffer limpio"
                    }),
            )
            .child(
                Icon::new(watcher_icon)
                    .size(px(14.))
                    .text_color(watcher_color),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.watcher_alive {
                        "watcher"
                    } else {
                        "sin watcher"
                    }),
            )
            .into_any_element()
    }

    fn render_log(&self, cx: &App) -> AnyElement {
        let mono = cx.theme().mono_font_family.clone();
        let rows: Vec<AnyElement> = self
            .log
            .iter()
            .rev()
            .take(5)
            .map(|line| {
                div()
                    .font_family(mono.clone())
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(line.clone())
                    .into_any_element()
            })
            .collect();
        v_flex()
            .flex_none()
            .px_2()
            .py_1()
            .gap(px(1.))
            .border_t_1()
            .border_color(cx.theme().border)
            .children(rows)
            .into_any_element()
    }
}

impl Render for SpikeEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toolbar = self.render_toolbar(cx);
        let log = self.render_log(cx);
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(toolbar)
            .child(div().flex_1().min_h_0().w_full().child(self.panel.clone()))
            .child(log)
    }
}

impl Focusable for SpikeEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.panel.read(cx).focus_handle(cx)
    }
}

// --- workspace de trabajo ---------------------------------------------------

/// Prepara `%TEMP%\latex-studio\m1-editor` con la copia de trabajo.
fn prepare_workspace(demo_recovery: bool) -> PathBuf {
    let root = std::env::temp_dir().join("latex-studio").join(WORK_DIR);
    std::fs::create_dir_all(&root).expect("crear raíz de trabajo");
    std::fs::create_dir_all(root.join("capitulos")).expect("crear capitulos");
    std::fs::create_dir_all(root.join("secciones")).expect("crear secciones");
    std::fs::create_dir_all(root.join("config")).expect("crear config");

    if !root.join("main.tex").is_file() {
        std::fs::write(root.join("main.tex"), BROKEN_TEX).expect("crear main.tex");
    }
    if !root.join("capitulos").join("intro.tex").is_file() {
        std::fs::write(root.join("capitulos").join("intro.tex"), INTRO_TEX)
            .expect("crear intro.tex");
    }
    if !root.join("secciones").join("metodo.tex").is_file() {
        std::fs::write(root.join("secciones").join("metodo.tex"), METODO_TEX)
            .expect("crear metodo.tex");
    }

    let session = SessionStore::new(root.join("config"));
    if !session.path().is_file() || demo_recovery {
        let state = SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            tabs: vec![
                SessionTab {
                    path: "main.tex".into(),
                    cursor: SessionCursor {
                        line: 3,
                        character: 0,
                    },
                },
                SessionTab {
                    path: "capitulos/intro.tex".into(),
                    cursor: SessionCursor::default(),
                },
                SessionTab {
                    path: "secciones/metodo.tex".into(),
                    cursor: SessionCursor::default(),
                },
            ],
            active: 0,
        };
        let _ = session.save(&state);
    }

    if demo_recovery {
        // Backup más nuevo que el archivo en disco → el panel lo propondrá.
        let backups = BackupStore::new(root.join("config").join("backups"));
        let _ = backups.write(
            Path::new("main.tex"),
            "\\documentclass{article}\n\\begin{document}\n\
             Hola \\textbf{contenido recuperado todavía sin guardar}.\n\\end{document}\n",
        );
    }
    root
}

// --- smoke sin ventana ------------------------------------------------------

fn run_check(root: &Path) {
    latex_language::register_latex_language();
    println!("spike M1-B · check sin ventana");
    println!("  raíz            : {}", root.display());

    // M1-B3: parse_log de fixtures → marcadores con posición.
    let diags = parse_log(UNDEFINED_LOG);
    let text = std::fs::read_to_string(root.join("main.tex")).unwrap_or_default();
    let markers = lsx_editor::diagnostics::map_diagnostics(&text, &diags);
    println!(
        "  diagnósticos    : {} del fixture undefined-command",
        diags.len()
    );
    for marker in &markers {
        println!(
            "    · Ln {} Col {}-{}: {}",
            marker.range.start.line + 1,
            marker.range.start.character,
            marker.range.end.character,
            marker.message
        );
    }

    // M1-B5: ida y vuelta de backup.
    let backups = BackupStore::new(root.join("config").join("backups"));
    if let Ok(path) = backups.write(Path::new("check.tex"), "contenido de prueba") {
        let ok = backups
            .latest_for(Path::new("check.tex"))
            .and_then(|entry| backups.read(&entry).ok())
            .is_some_and(|content| content == "contenido de prueba");
        println!("  backup round-trip: {ok} ({})", path.display());
    }
    // M1-B1: ida y vuelta de sesión.
    let store = SessionStore::new(root.join("config"));
    let loaded = store.load();
    println!(
        "  sesión           : {} pestaña(s) persistidas",
        loaded.tabs.len()
    );
    println!("  pestañas demo    : main.tex, capitulos/intro.tex, secciones/metodo.tex");
}
