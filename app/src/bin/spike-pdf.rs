#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Demo/evidencia M1-C — el panel PDF de producción (`lsx_ui::panels::pdf`)
//! montado en una ventana propia, con disparadores para los criterios del gate:
//!
//! ```text
//! cargo run --release -p latex-studio --bin spike-pdf            # multipage (30 págs)
//! cargo run --release -p latex-studio --bin spike-pdf -- bench   # mide scroll (fps)
//! cargo run --release -p latex-studio --bin spike-pdf -- sync    # cv + SyncTeX forward
//! cargo run --release -p latex-studio --bin spike-pdf -- multipage --page 12
//! cargo run --release -p latex-studio --bin spike-pdf -- multipage --page 12 --recompile-after 6
//! ```
//!
//! Botones: recompilar simulado (mtime nuevo → recarga C3 conservando scroll),
//! `set_document` sin cambios (no recarga), medir scroll, cargar `cv.pdf`
//! (copia temporal con sidecar synctex) y forward SyncTeX a `main.tex:78`.
//!
//! Las líneas de evidencia se escriben además en
//! `%TEMP%\lsx-c1-demo\evidence.txt` (en release el bin no tiene consola por
//! `windows_subsystem = "windows"`).
//!
//! Todo el estado del visor vive en [`PdfPanel`]; este bin solo lo conduce.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;
use lsx_latex::TexLivePaths;
use lsx_ui::panels::pdf::{InverseJump, PdfPanel, PdfPanelEvent, ZoomMode};

/// Fixture multipágina (30 páginas, inmutable).
const MULTIPAGE_PDF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/pdf/multipage.pdf");
/// Fixture CV (1 página) + sidecar SyncTeX (fixtures inmutables).
const CV_PDF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/pdf/cv.pdf");
const CV_SYNCTEX: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../fixtures/synctex/cv.synctex.gz"
);
/// Fuente grabada en el sidecar de la fixture (`Input:1:`).
const CV_TEX: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../fixtures/projects/cv/main.tex"
);
/// `config/settings.json` (estado de usuario; puede no existir).
const SETTINGS_JSON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../config/settings.json");

/// Modo de arranque para la evidencia automatizada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Multipage,
    Bench,
    Sync,
}

/// Escribe una línea de evidencia a stdout y al archivo temporal de la demo.
fn evidence(line: &str) {
    println!("{line}");
    let dir = std::env::temp_dir().join("lsx-c1-demo");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("evidence.txt"))
    {
        let _ = writeln!(file, "{line}");
    }
}

struct SpikeC1 {
    panel: Entity<PdfPanel>,
    mode: Mode,
    focus: FocusHandle,
    /// Copia temporal del fixture (simula el PDF de `out/` que reescribe latexmk).
    demo_path: PathBuf,
    /// Generación del "compile queue" simulado.
    generation: u64,
    /// Ruta de la copia cv con sidecar synctex.
    cv_path: PathBuf,
    /// Página a la que saltar tras la primera carga.
    auto_page: Option<u32>,
    /// Segundos tras la primera carga para recompilar solo (evidencia C3);
    /// `None` = no recompilar.
    recompile_after: Option<f64>,
    status: String,
    _events: Subscription,
}

impl SpikeC1 {
    fn new(
        mode: Mode,
        auto_page: Option<u32>,
        recompile_after: Option<f64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (cache_mb, zoom_mode) = read_pdf_settings();
        evidence(&format!(
            "[m1-c] settings: pdf.maxCacheMB={cache_mb} zoomMode={} (defaults si no existe)",
            zoom_mode.as_setting()
        ));

        let panel = cx.new(|cx| PdfPanel::with_cache_mb(cache_mb, cx));
        let synctex = TexLivePaths::detect().ok().map(|paths| paths.synctex);
        match &synctex {
            Some(exe) => evidence(&format!("[m1-c] synctex.exe: {exe}")),
            None => evidence("[m1-c] AVISO: synctex.exe no encontrado; SyncTeX deshabilitado"),
        }

        let demo_dir = std::env::temp_dir().join("lsx-c1-demo");
        std::fs::create_dir_all(&demo_dir).expect("temp de la demo");
        let demo_path = demo_dir.join("multipage.pdf");
        std::fs::copy(MULTIPAGE_PDF, &demo_path).expect("copiar multipage.pdf a temp");
        let cv_path = demo_dir.join("cv.pdf");
        std::fs::copy(CV_PDF, &cv_path).expect("copiar cv.pdf a temp");
        std::fs::copy(CV_SYNCTEX, demo_dir.join("cv.synctex.gz"))
            .expect("copiar cv.synctex.gz a temp");

        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        let weak = cx.entity().downgrade();
        panel.update(cx, |panel, cx| {
            if let Some(exe) = synctex {
                panel.set_synctex_exe(exe);
            }
            panel.set_zoom_mode(zoom_mode, cx);
            panel.set_document(demo_path.clone(), 1, cx);
            panel.on_inverse_jump(
                move |jump: InverseJump, _window: &mut Window, cx: &mut App| {
                    evidence(&format!(
                        "[m1-c] inverse → {}:{} (col {:?})",
                        jump.file.display(),
                        jump.line,
                        jump.column
                    ));
                    weak.update(cx, |this, cx| {
                        this.status = format!(
                            "inverse → {}:{}",
                            jump.file
                                .file_name()
                                .map(|name| name.to_string_lossy().to_string())
                                .unwrap_or_default(),
                            jump.line
                        );
                        cx.notify();
                    })
                    .ok();
                },
                cx,
            );
        });

        let events = cx.subscribe(&panel, |this, _panel, event: &PdfPanelEvent, cx| {
            let line = match event {
                PdfPanelEvent::DocumentReady {
                    path,
                    generation,
                    page_count,
                } => format!(
                    "listo: {} · {} págs · gen {}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    page_count,
                    generation
                ),
                PdfPanelEvent::DocumentFailed { message, .. } => {
                    format!("falló la carga: {message}")
                }
                PdfPanelEvent::PageChanged { page, count } => format!("pág {page}/{count}"),
                PdfPanelEvent::ViewportScrolled => "scroll".to_owned(),
                PdfPanelEvent::ZoomChanged { zoom, mode } => {
                    format!("zoom {:.0}% ({})", zoom * 100.0, mode.as_setting())
                }
                PdfPanelEvent::InverseJump(jump) => {
                    format!("inverse → {}:{}", jump.file.display(), jump.line)
                }
            };
            if matches!(event, PdfPanelEvent::DocumentReady { .. })
                && let Some(page) = this.auto_page.take()
            {
                this.panel.update(cx, |panel, cx| panel.goto_page(page, cx));
            }
            if matches!(event, PdfPanelEvent::DocumentReady { .. })
                && let Some(seconds) = this.recompile_after.take()
            {
                let handle = cx.entity();
                cx.spawn(async move |_this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_secs_f64(seconds))
                        .await;
                    handle.update(cx, |this, cx| this.recompile(cx));
                })
                .detach();
            }
            this.status = line;
            cx.notify();
        });

        Self {
            panel,
            mode,
            focus,
            demo_path,
            generation: 1,
            cv_path,
            auto_page,
            recompile_after,
            status: "listo".to_string(),
            _events: events,
        }
    }

    /// Simula una recompilación: reescribe la copia, fuerza mtime nuevo (en
    /// Windows `fs::copy` preserva el mtime del origen) y sube la generación;
    /// el panel debe recargar conservando página/offset.
    fn recompile(&mut self, cx: &mut Context<Self>) {
        std::fs::copy(MULTIPAGE_PDF, &self.demo_path).expect("reescribir copia");
        if let Ok(file) = std::fs::OpenOptions::new()
            .write(true)
            .open(&self.demo_path)
        {
            let _ = file.set_modified(SystemTime::now());
        }
        self.generation += 1;
        let generation = self.generation;
        let path = self.demo_path.clone();
        self.panel
            .update(cx, |panel, cx| panel.set_document(path, generation, cx));
        self.status = format!("recompilado → generation {generation}");
        evidence(&format!(
            "[m1-c] recompilado: generation {generation} (mtime nuevo)"
        ));
        cx.notify();
    }

    /// `set_document` con el mismo archivo: no debe recargar (mtime+len igual).
    fn set_document_sin_cambios(&mut self, cx: &mut Context<Self>) {
        let generation = self.generation;
        let path = self.demo_path.clone();
        self.panel.update(cx, |panel, cx| {
            panel.set_document(path.clone(), generation, cx)
        });
        self.status = "set_document sin cambios (no recarga)".to_string();
        evidence(&format!(
            "[m1-c] set_document sin cambios: gen {generation} (esperado: no recarga)"
        ));
        cx.notify();
    }

    /// Carga `cv.pdf` (copia con sidecar synctex) para probar C4.
    fn load_cv(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let path = self.cv_path.clone();
        self.panel
            .update(cx, |panel, cx| panel.set_document(path, generation, cx));
        self.status = "cv.pdf cargado (copia con synctex)".to_string();
        cx.notify();
    }

    /// Forward SyncTeX a una línea conocida del fixture `cv`.
    fn forward_sync(&mut self, cx: &mut Context<Self>) {
        let src = Path::new(CV_TEX);
        self.panel
            .update(cx, |panel, cx| panel.sync_forward(src, 78, 0, cx));
        self.status = "SyncTeX forward main.tex:78".to_string();
        cx.notify();
    }

    /// Inverse SyncTeX sobre la caja conocida del fixture (46.8, 311.05) bp.
    fn inverse_demo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel.update(cx, |panel, cx| {
            // Equivale a un Ctrl+clic sobre la línea resaltada por forward.
            panel.inverse_at(0, 46.799, 311.052, window, cx);
        });
        self.status = "SyncTeX inverse (46.8, 311.05) bp".to_string();
        cx.notify();
    }

    /// Mide fluidez de scroll (20 páginas en 1.4 s) con el contador de frames.
    fn bench(&mut self, cx: &mut Context<Self>) {
        self.panel.update(cx, |panel, cx| {
            panel.bench_scroll(20, Duration::from_millis(1400), cx)
        });
        self.status = "midiendo scroll…".to_string();
        // En release no hay consola: volcar el resumen a evidence.txt.
        cx.spawn(async move |this, cx| {
            for _ in 0..120 {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let report = this
                    .read_with(cx, |this, cx| {
                        this.panel.read(cx).bench_report().map(str::to_string)
                    })
                    .ok()
                    .flatten();
                if let Some(report) = report {
                    evidence(&format!("[m1-c] {report}"));
                    this.update(cx, |this, cx| {
                        this.status = report;
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            }
            evidence("[m1-c] bench: sin resumen tras 12 s");
        })
        .detach();
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "f5" => self.recompile(cx),
            "f6" => self.set_document_sin_cambios(cx),
            "f7" => self.bench(cx),
            "f8" => self.load_cv(cx),
            "f9" => self.forward_sync(cx),
            "f10" => self.inverse_demo(window, cx),
            _ => {}
        }
    }
}

impl Render for SpikeC1 {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let bar = h_flex()
            .flex_shrink_0()
            .h(px(38.0))
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_sm()
                    .child("LaTeX Studio — M1-C · visor PDF (panel de producción)"),
            )
            .child(div().flex_1())
            .child(
                Button::new("c-recompile")
                    .outline()
                    .label("Recompilar (F5)")
                    .on_click(cx.listener(|this, _, _, cx| this.recompile(cx))),
            )
            .child(
                Button::new("c-nochange")
                    .outline()
                    .label("Sin cambios (F6)")
                    .on_click(cx.listener(|this, _, _, cx| this.set_document_sin_cambios(cx))),
            )
            .child(
                Button::new("c-bench")
                    .outline()
                    .label("Medir scroll (F7)")
                    .on_click(cx.listener(|this, _, _, cx| this.bench(cx))),
            )
            .child(
                Button::new("c-cv")
                    .outline()
                    .label("CV + synctex (F8)")
                    .on_click(cx.listener(|this, _, _, cx| this.load_cv(cx))),
            )
            .child(
                Button::new("c-forward")
                    .outline()
                    .label("Forward 78 (F9)")
                    .on_click(cx.listener(|this, _, _, cx| this.forward_sync(cx))),
            )
            .child(
                Button::new("c-inverse")
                    .outline()
                    .label("Inverse (F10)")
                    .on_click(cx.listener(|this, _, window, cx| this.inverse_demo(window, cx))),
            );

        let status = h_flex()
            .flex_shrink_0()
            .h(px(26.0))
            .px_3()
            .gap_3()
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(format!("modo {:?}", self.mode))
            .child(div().flex_1())
            .child(self.status.clone());

        v_flex()
            .relative()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    this.on_key(event, window, cx)
                }),
            )
            .child(bar)
            .child(div().flex_1().min_h(px(0.0)).child(self.panel.clone()))
            .child(status)
    }
}

/// Lee `pdf.maxCacheMB` y `pdf.zoomMode` de `config/settings.json` si existe.
fn read_pdf_settings() -> (u32, ZoomMode) {
    let default = (256_u32, ZoomMode::FitWidth);
    let Ok(text) = std::fs::read_to_string(SETTINGS_JSON) else {
        return default;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return default;
    };
    let pdf = &json["pdf"];
    let mb = pdf["maxCacheMB"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(default.0);
    let mode = pdf["zoomMode"]
        .as_str()
        .map(ZoomMode::from_setting)
        .unwrap_or(default.1);
    (mb, mode)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.first().map(String::as_str) {
        Some("bench") => Mode::Bench,
        Some("sync") => Mode::Sync,
        _ => Mode::Multipage,
    };
    let auto_page = args
        .iter()
        .position(|arg| arg == "--page")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse::<u32>().ok());
    let auto_recompile = args.iter().any(|arg| arg == "--recompile");
    let recompile_after = if auto_recompile {
        Some(0.4_f64)
    } else {
        args.iter()
            .position(|arg| arg == "--recompile-after")
            .and_then(|index| args.get(index + 1))
            .and_then(|value| value.parse::<f64>().ok())
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("LaTeX Studio — M1-C visor PDF".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(60.), px(40.)),
                    size: size(px(1180.), px(900.)),
                })),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, move |window, cx| {
                let view = cx.new(|cx| SpikeC1::new(mode, auto_page, recompile_after, window, cx));
                // Arranque automatizado para la evidencia por captura/tiempos.
                match mode {
                    Mode::Bench => {
                        let handle = view.clone();
                        cx.spawn(async move |cx| {
                            cx.background_executor()
                                .timer(Duration::from_millis(1000))
                                .await;
                            handle.update(cx, |this, cx| this.bench(cx));
                        })
                        .detach();
                    }
                    Mode::Sync => {
                        let handle = view.clone();
                        let panel = view.read(cx).panel.clone();
                        let window_handle = window.window_handle();
                        cx.spawn(async move |cx| {
                            // 1) Cargar cv.pdf (copia con sidecar synctex).
                            cx.background_executor()
                                .timer(Duration::from_millis(500))
                                .await;
                            handle.update(cx, |this, cx| this.load_cv(cx));
                            // 2) Esperar a que el documento cargado sea cv.pdf.
                            let mut ready = false;
                            for _ in 0..40 {
                                cx.background_executor()
                                    .timer(Duration::from_millis(100))
                                    .await;
                                ready = handle.update(cx, |this, cx| {
                                    this.panel
                                        .read(cx)
                                        .document_path()
                                        .is_some_and(|path| path.ends_with("cv.pdf"))
                                });
                                if ready {
                                    break;
                                }
                            }
                            if !ready {
                                return;
                            }
                            // 3) Forward 78 → resalta la caja.
                            handle.update(cx, |this, cx| this.forward_sync(cx));
                            // 4) Inverse (equivale a Ctrl+clic) sobre la caja.
                            cx.background_executor()
                                .timer(Duration::from_millis(3500))
                                .await;
                            cx.update_window(window_handle, move |_view, window, cx| {
                                panel.update(cx, |panel, cx| {
                                    panel.inverse_at(0, 46.799, 311.052, window, cx)
                                });
                            })
                            .ok();
                        })
                        .detach();
                    }
                    Mode::Multipage => {}
                }
                view
            })
            .expect("no se pudo abrir la ventana de la demo M1-C");
        });
}
