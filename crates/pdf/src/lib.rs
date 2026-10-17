//! `lsx-pdf` — documento PDF (hayro), render a RGBA, caché y coordenadas.
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §4):
//! - `PdfDocument` (abrir con hayro, page_count, page_size_pt, render → `PageBitmap`)
//! - `PdfCache` LRU por bytes, re-render progresivo, invalidación por generación
//! - `coords` (bp ↔ px, origen arriba-izquierda, UNA sola implementación)
//! - `PdfRenderer` (documento + caché, apto para `background_spawn`)
//!
//! Regla dura: sin gpui (los bitmaps se entregan a `lsx-ui`, que los sube a textura).
//!
//! Estado M1-C2: `PdfDocument`, `PageBitmap`, `coords`, `PdfCache` y
//! `PdfRenderer` implementados y medidos contra `fixtures/pdf/{cv,multipage}.pdf`.
//! La recarga con `generation++` y el render progresivo los orquesta el visor
//! (`lsx-ui::panels::pdf`).
//!
//! Nota de plataforma UI (hallazgo del spike, para `lsx-ui`): el `RenderImage`
//! de GPUI 0.3.9 espera bytes **BGRA** (`image::Frame` sobre `RgbaImage`), y su
//! `scale_factor` es privado: el bitmap debe subirse convirtiendo RGBA→BGRA y
//! pintando con tamaño lógico `px(ancho / dpr)`.

mod document;

mod cache;
pub mod coords;
mod renderer;

pub use cache::{CacheKey, CacheStats, DEFAULT_MAX_CACHE_MB, PdfCache, scale_bucket};
pub use coords::BP_PER_INCH;
pub use document::{PageBitmap, PdfDocument, PdfError};
pub use renderer::PdfRenderer;
