---
name: lsx-pdf-hayro
description: Usar cuando se trabaje en el visor PDF (stream C, crate lsx-pdf) — apertura y render de páginas con hayro 0.8, conversión de coordenadas bp/px con origen arriba-izquierda, subida del bitmap a RenderImage de GPUI, caché LRU por generación, zoom y scroll virtualizado, medición con criterion y comparativa cargo xtask pdf-golden. Incluye los límites de hayro y el fallback pdfium tras el trait PdfRender.
---

# PDF en lsx-pdf (hayro 0.8)

Contrato: `docs/plan/INTERFACES.md` §4. Flujo: `docs/plan/PLAN-MAESTRO.md` §5.7. Pin: `hayro = "0.8"` (MIT/Apache, pure Rust; features default `embed-fonts`, `embed-cmaps`, `simd`).

## Hayro (apertura y render)

- `PdfDocument::open(bytes)` en `spawn_blocking` (plan: `hayro::Pdf::new(bytes)`); guardar `page_count` y tamaños pt.
- Render: `render(page, scale)` → `PageBitmap { width, height, scale, rgba }`; `scale = zoom × dpr` de ventana.
- `page_size_pt(page)` en bp (PDF points).
- `TODO(M0-S3): verificar` los nombres/parámetros exactos de la API hayro 0.8 (tipos de página, matriz de transformación, recursos, rotación) y volcarlos aquí tras el spike.

## Coordenadas (UNA sola implementación)

- `lsx-pdf::coords`: `pdf_pt_to_px` / `px_to_pdf_pt`; `BP_PER_INCH = 72.0`; origen arriba-izquierda SIEMPRE (INTERFACES §4 y §9).
- Ejemplo numérico: A4 595×842 bp; zoom 1.5 y dpr 2.0 → `scale = 3.0`; punto (100, 200) bp → (300, 600) px; el inverso debe volver exacto.
- Tests: round-trip ±0.01 px; caso con `page_rotation` documentado y cubierto.
- SyncTeX entrega/consume bp con el mismo origen (ver skill `lsx-latex-engine`).

## Subida a GPUI (RenderImage)

- Patrón: render en background (`spawn_blocking`) → `PageBitmap` → subir a textura → pintar; el PDF viejo sigue visible hasta que la nueva generación esté lista (sin parpadeo, PLAN §4.1 panel 3).
- `TODO(M0-S3): verificar` la API exacta de `RenderImage` en el snapshot gpui-pre 0.3.9 (constructor, formato del buffer y premultiplicado) con el spike; no asumir.

## Caché y zoom

- LRU por bytes (default 256 MB; settings `pdf.maxCacheMB`); clave `(generation, page, scale_bucket)`; prioriza página visible y vecinas; re-render progresivo 1× inmediato → 2×/3× en idle (PLAN §4.1 / §5.4).
- Invalidación por `doc_generation` al recompilar; conservar página y offset de scroll (M1-C3).
- Zoom modes fit-width / fit-page / 50–400 % (settings `pdf.zoomMode`, `pdf.zoom`).

## Medición y golden

- M0-S3 mide render p50/p95 con criterion; presupuesto: página típica al 100 % < 120 ms (PLAN §6.3).
- `cargo run -p xtask -- pdf-golden` compara PNGs contra referencias — `TODO(M0): verificar` en el stub actual solo imprime `TODO(M0-S3)`: la tarea no está implementada y `fixtures/pdf/` aún no existe (se crea en M0).
- Bench formal (`cargo xtask bench` según SWARM §5): no existe en el stub de `xtask` → `TODO(M0): verificar`/coordinar con el orchestrator (dueño de `xtask/`); mientras, medir con criterion local y anotarlo en el informe del spike.
- Publicar resultados en `docs/perf.md` en cada gate (se crea en el primer gate).

## Límites y fallback

- hayro no expone selección/búsqueda de texto → queda para M4 (M4-3).
- `gpui-pdf` / `ratex-gpui` son GPL-3.0: SOLO lectura de patrones, nunca dependencia ni copia de código (decisión D7; PLAN §5.7).
- Default del plan (I-2): render propio `lsx-pdf` + `RenderImage`.
- Fallback aprobado si el rendimiento no cumple en G0-S3: `pdfium-render` (DLL nativa, BSD) detrás del trait `PdfRender`.

## Tests

- Unit: coords (round-trip, rotación), caché LRU (evicción, prioridades), page_count/page_size sobre `fixtures/pdf/*.pdf` (cuando existan).
- Golden: `xtask pdf-golden` con tolerancia perceptual simple + no-blank + page count (PLAN §6.1).
