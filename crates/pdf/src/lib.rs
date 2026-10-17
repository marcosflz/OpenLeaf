//! `lsx-pdf` — documento PDF (hayro), render a RGBA, caché y coordenadas.
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §4):
//! - `PdfDocument` (abrir con hayro, page_count, page_size_pt, render → `PageBitmap`)
//! - `PdfCache` LRU por bytes, re-render progresivo, invalidación por generación
//! - `coords` (bp ↔ px, origen arriba-izquierda, UNA sola implementación)
//! - traducción de `SyncPoint` (SyncTeX) a rectángulo en pantalla
//!
//! Regla dura: sin gpui (los bitmaps se entregan a `lsx-ui`, que los sube a textura).
//!
//! Estado M0-S3 (spike): `PdfDocument`, `PageBitmap` y `coords` implementados y
//! medidos contra `fixtures/pdf/cv.pdf`. `PdfCache` (M1-C2) seguirá el diseño
//! LRU por bytes con clave `(generation, page, scale_bucket)`.
//!
//! Nota de plataforma UI (hallazgo del spike, para `lsx-ui`): el `RenderImage`
//! de GPUI 0.3.9 espera bytes **BGRA** (`image::Frame` sobre `RgbaImage`), y su
//! `scale_factor` es privado: el bitmap debe subirse convirtiendo RGBA→BGRA y
//! pintando con tamaño lógico `px(ancho / dpr)`.

mod document;

pub mod coords;

pub use coords::BP_PER_INCH;
pub use document::{PageBitmap, PdfDocument, PdfError};
