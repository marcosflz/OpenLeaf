# Spike M0-S3 — Visor PDF (hayro + GPUI)

- Fecha: 2026-10-17 · Sesión: `ses_eb6356f5cffeI5008v7WLzSdMd`
- **ESTADO: OK** (demo funcional + medidas + tests verdes en su scope)

## Entregable

- `crates/pdf/src/{lib,document,coords}.rs` — `PdfDocument::{open,page_count,page_size_pt,render,render_with_cache}`, `PageBitmap`, `PdfError`, `BP_PER_INCH`
- `crates/pdf/tests/cv.rs` — 6 tests de integración contra `fixtures/pdf/cv.pdf`
- `app/src/bin/spike-pdf.rs` — ventana "LaTeX Studio — spike S3" con zoom ± (teclado), scroll, botón "Medir 5×"
- **Desviación**: `app/Cargo.toml` (+`lsx-pdf`, +`image = "0.25"`) — necesaria para el bin. Reconciliar en la consolidación con los otros spikes que editaron el mismo archivo.

## Demo

```
cargo run -p latex-studio --bin spike-pdf
```
Smoke OK (ventana viva 8 s, 0 huérfanos). Salida real: `cv.pdf abierto: 1 página(s); página 1 = 612×792 bp` · `render pág 1 escala 2.00 → 1224×1584 px en 180.1 ms` (debug; dpr de este equipo = 2.0).

## Respuestas de interfaz

**I-2 — CONFIRMADO: visor propio con `RenderImage`; NO adoptar `gpui-pdf`** (GPL-3.0 + modelo ajeno; nuestro pipeline es medible y suficiente). Hallazgos duros:
1. `RenderImage` espera bytes **BGRA** en Windows (hayro entrega RGBA premultiplicado → swap en `lsx-ui`).
2. `RenderImage::scale_factor` es privado → pintar con tamaño lógico explícito `.w(px(w_bmp/dpr))`.
3. Zoom re-renderiza la textura: viable con generación + `background_spawn`; M1 añade buckets de escala + LRU por bytes + virtualización. `RenderCache` no puede vivir dentro de `PdfDocument` (invariancia `'a` por `RefCell<Font<'a>>`).

**Tiempos** (página 1 CV, release): p50 ≈ **4.3 / 5.6 / 7.9 ms** a escala 1.0 / 1.5 / 2.0 (cache reusado: 2.2 / 4.3 / 6.6). Debug ≈ 91 / 131 / 180 ms. Presupuesto `<120 ms @100 %` cumplido con ~27× de margen.

**Unidades**: bp = 1/72", origen arriba-izquierda; `scale = zoom × dpr`; ejemplo verificado en test: A4, zoom 1.5, dpr 2 → (100,200) bp = (300,600) px (±0.01). CV = Letter 612×792 bp.

## API descubierta (→ INTERFACES §12)

```rust
Pdf::new(data) -> Result<Pdf, LoadPdfError>          // LoadPdfError sin Display/Error
pdf.pages() -> &Pages<'_>                             // len(), get(i); Deref a [Page]
Page::render_dimensions() -> (f32, f32)               // rotación aplicada
hayro::render(&Page, &RenderCache, &InterpreterSettings, &RenderSettings, &PixmapSettings) -> vello_cpu::Pixmap
PixmapSettings { x_scale, y_scale, bg_color }         // WHITE = papel
Pixmap::data_as_u8_slice() -> &[u8]                   // RGBA8 premultiplicado (u16 w, u16 h)
RenderImage::new(impl Into<SmallVec<[image::Frame; 1]>>)  // Frame::new(RgbaImage) — BGRA en Windows
img(Arc<RenderImage>).w(px(w/dpr)).h(px(h/dpr))
```

## Riesgos / TODO

1. `app/Cargo.toml` compartido entre spikes → reconciliar.
2. Decidir cache del visor (per-call vs sesión con `render_with_cache`) en M1-C2; definir straight vs premult si M1 permite fondos del tema.
3. Falta fixture PDF con `/Rotate` real para cerrar el punto de rotación.
4. Leak intencional de `RenderCache` en el spike; envolver tras trait en M1.
5. Sin test UI automatizado del bin (zoom/scroll verificados por código+medidas); migrar test al panel `lsx-ui` en M1.
6. `nextest` no instalado en la máquina → gates con `cargo test` (nota para P0/CI local).
