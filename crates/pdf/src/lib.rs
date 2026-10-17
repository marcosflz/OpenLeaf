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
//! Estado: P0 — stub. Se implementa en M0-S3 (spike) y M1 (stream C).
