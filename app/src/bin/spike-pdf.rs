#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Spike M0-S3 — visor PDF mínimo sobre `lsx-pdf` (hayro 0.8) + GPUI.
//!
//! - Abre `fixtures/pdf/cv.pdf` (solo lectura) y muestra la página 1 con
//!   `img()` sobre un `RenderImage`.
//! - Zoom −/+ (5 niveles): cada cambio re-renderiza la textura en
//!   `background_spawn`; el bitmap anterior sigue visible hasta que llega el
//!   nuevo (sin parpadeo). Teclado: `+`/`=` y `-`/`_`.
//! - Scroll vertical con `.overflow_y_scrollbar()` del kit.
//! - Botón «Medir 5×»: media de 5 renders a la escala actual (también a stdout).
//!
//! Hallazgo de formato (documentado para INTERFACES §4): hayro entrega RGBA
//! premultiplicado sobre fondo blanco opaco; `RenderImage` de GPUI 0.3.9 en
//! Windows espera bytes **BGRA**. La conversión vive en `bitmap_to_render_image`.

use std::sync::Arc;
use std::time::Instant;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::*;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use image::{Frame, RgbaImage};
use lsx_pdf::{PageBitmap, PdfDocument};

/// Fixture del spike (inmutable: solo se lee).
const CV_PDF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/pdf/cv.pdf");

/// Niveles de zoom del spike, en tanto por uno.
const ZOOM_LEVELS: [f32; 5] = [0.5, 0.75, 1.0, 1.5, 2.0];
/// Índice por defecto: 100 %.
const ZOOM_DEFAULT: usize = 2;

struct SpikePdf {
    doc: Arc<PdfDocument>,
    focus: FocusHandle,
    page: u32,
    zoom_index: usize,
    dpr: f32,
    /// Descarta renders obsoletos (zoom pulsado varias veces seguidas).
    generation: u64,
    image: Option<Arc<RenderImage>>,
    /// Tamaño lógico (px de UI) con el que se pinta el bitmap.
    image_size: (f32, f32),
    render_ms: Option<f64>,
    bench_text: Option<String>,
}

impl SpikePdf {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let bytes =
            std::fs::read(CV_PDF).unwrap_or_else(|e| panic!("no se pudo leer {CV_PDF}: {e}"));
        let doc = PdfDocument::open(bytes).expect("cv.pdf no es un PDF válido");
        let (page_w, page_h) = doc.page_size_pt(0).expect("página 1 medible en cv.pdf");
        println!(
            "[spike-s3] cv.pdf abierto: {} página(s); página 1 = {page_w}×{page_h} bp",
            doc.page_count(),
        );

        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        let mut this = Self {
            doc: Arc::new(doc),
            focus,
            page: 0,
            zoom_index: ZOOM_DEFAULT,
            dpr: window.scale_factor(),
            generation: 0,
            image: None,
            image_size: (0., 0.),
            render_ms: None,
            bench_text: None,
        };
        this.queue_render(window, cx);
        this
    }

    /// Escala de render: `zoom × dpr` (px de bitmap por bp de PDF).
    fn scale(&self) -> f32 {
        ZOOM_LEVELS[self.zoom_index] * self.dpr
    }

    fn zoom_by(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let last = ZOOM_LEVELS.len() as i32 - 1;
        let next = (self.zoom_index as i32 + delta).clamp(0, last) as usize;
        if next != self.zoom_index {
            self.zoom_index = next;
            self.queue_render(window, cx);
        }
    }

    /// Re-renderiza la página actual en un hilo de background y sube el bitmap.
    fn queue_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let doc = self.doc.clone();
        let page = self.page;
        let scale = self.scale();
        let dpr = self.dpr;

        cx.spawn_in(window, async move |this, cx| {
            let (bitmap, elapsed) = cx
                .background_spawn(async move {
                    let t0 = Instant::now();
                    let bitmap = doc.render(page, scale)?;
                    Ok::<_, lsx_pdf::PdfError>((bitmap, t0.elapsed()))
                })
                .await
                .expect("render de cv.pdf");

            println!(
                "[spike-s3] render pág {} escala {scale:.2} → {}×{} px en {:.1} ms",
                page + 1,
                bitmap.width,
                bitmap.height,
                elapsed.as_secs_f64() * 1000.0
            );

            let image = Arc::new(bitmap_to_render_image(&bitmap));
            let size = (bitmap.width as f32 / dpr, bitmap.height as f32 / dpr);
            this.update_in(cx, move |this, _, cx| {
                if this.generation == generation {
                    this.image = Some(image);
                    this.image_size = size;
                    this.render_ms = Some(elapsed.as_secs_f64() * 1000.0);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Mide 5 renders consecutivos a la escala actual (evidencia del spike).
    fn bench_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc.clone();
        let page = self.page;
        let scale = self.scale();
        self.bench_text = Some("midiendo 5 renders…".to_string());
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let times = cx
                .background_spawn(async move {
                    let mut times = Vec::with_capacity(5);
                    for _ in 0..5 {
                        let t0 = Instant::now();
                        doc.render(page, scale)?;
                        times.push(t0.elapsed().as_secs_f64() * 1000.0);
                    }
                    Ok::<_, lsx_pdf::PdfError>(times)
                })
                .await
                .expect("bench de cv.pdf");

            let mean = times.iter().sum::<f64>() / times.len() as f64;
            let min = times.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = times.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            println!(
                "[spike-s3] bench escala {scale:.2}: media {mean:.1} ms · min {min:.1} · max {max:.1} · runs {times:?}"
            );
            let text = format!("medir 5×: media {mean:.0} ms (min {min:.0} / max {max:.0})");
            this.update_in(cx, move |this, _, cx| {
                this.bench_text = Some(text);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for SpikePdf {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let zoom = ZOOM_LEVELS[self.zoom_index];
        let page_count = self.doc.page_count();

        let toolbar = h_flex()
            .flex_shrink_0()
            .h(px(44.))
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_sm()
                    .child("LaTeX Studio — spike S3 · visor PDF (hayro 0.8)"),
            )
            .child(div().flex_1())
            .child(Button::new("zoom-out").label("−").on_click(
                cx.listener(|this, _: &ClickEvent, window, cx| this.zoom_by(-1, window, cx)),
            ))
            .child(
                div()
                    .w(px(56.))
                    .text_sm()
                    .text_center()
                    .child(format!("{:.0}%", zoom * 100.)),
            )
            .child(Button::new("zoom-in").label("+").on_click(
                cx.listener(|this, _: &ClickEvent, window, cx| this.zoom_by(1, window, cx)),
            ))
            .child(
                Button::new("zoom-100")
                    .outline()
                    .label("100%")
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        if this.zoom_index != ZOOM_DEFAULT {
                            this.zoom_index = ZOOM_DEFAULT;
                            this.queue_render(window, cx);
                        }
                    })),
            )
            .child(
                Button::new("bench")
                    .outline()
                    .label("Medir 5×")
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.bench_current(window, cx);
                    })),
            );

        let page_view: AnyElement = match (&self.image, self.image_size) {
            (Some(image), (w, h)) => div()
                .shadow_lg()
                .bg(white())
                .child(img(image.clone()).w(px(w)).h(px(h)))
                .into_any_element(),
            _ => div()
                .text_color(theme.muted_foreground)
                .child("Renderizando PDF…")
                .into_any_element(),
        };

        let scroll = div()
            .id("spike-pdf-scroll")
            .flex_1()
            .w_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "+" | "=" => this.zoom_by(1, window, cx),
                    "-" | "_" => this.zoom_by(-1, window, cx),
                    _ => {}
                }
            }))
            .overflow_y_scrollbar()
            .child(
                div()
                    .w_full()
                    .py_8()
                    .flex()
                    .justify_center()
                    .child(page_view),
            );

        let mut status_right = String::new();
        if let Some(ms) = self.render_ms {
            status_right.push_str(&format!("render {ms:.0} ms"));
        }
        if let Some(bench) = &self.bench_text {
            if !status_right.is_empty() {
                status_right.push_str(" · ");
            }
            status_right.push_str(bench);
        }

        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(toolbar)
            .child(scroll)
            .child(
                h_flex()
                    .flex_shrink_0()
                    .h(px(28.))
                    .px_3()
                    .gap_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("Página {} / {page_count}", self.page + 1))
                    .child(format!(
                        "zoom {:.0}% · dpr {:.2} · {:.2} px/bp",
                        zoom * 100.,
                        self.dpr,
                        self.scale()
                    ))
                    .child(div().flex_1())
                    .child(status_right),
            )
    }
}

/// `PageBitmap` (RGBA premultiplicado de hayro) → `RenderImage` (BGRA) de GPUI.
///
/// Con el fondo blanco opaco del render, `alpha == 255` y premultiplicado ≡
/// straight: basta el intercambio de canales R↔B. El resto es defensivo por si
/// algún día se renderiza con transparencia.
fn bitmap_to_render_image(bitmap: &PageBitmap) -> RenderImage {
    let mut data = bitmap.rgba.clone();
    for px in data.as_chunks_mut::<4>().0.iter_mut() {
        px.swap(0, 2); // RGBA → BGRA
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = (((*c as u32) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    let buffer = RgbaImage::from_raw(bitmap.width, bitmap.height, data)
        .expect("el bitmap debe tener width × height × 4 bytes");
    RenderImage::new(vec![Frame::new(buffer)])
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("LaTeX Studio — spike S3".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(80.), px(60.)),
                    size: size(px(1080.), px(820.)),
                })),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| SpikePdf::new(window, cx))
            })
            .expect("no se pudo abrir la ventana del spike");
        });
}
