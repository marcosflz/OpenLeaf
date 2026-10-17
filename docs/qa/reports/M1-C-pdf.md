# M1 Stream C — PDF/SyncTeX (implementación completa; cancelado antes del reporte)

- Fecha: 2026-10-17 · Sesión: `ses_eb5b77d73ffeHE0xX4Xi1k4qgJ` (**cancelada** tras implementar; el orchestrator cerró integración y reporte)
- **ESTADO: ✅ implementado y verificado por evidencia** (20 tests verdes, clippy limpio)

## Entregable (commiteado en `9b6d4fa`)

- `crates/pdf/src/{cache,renderer}.rs` + `lib.rs` — caché LRU por bytes con render progresivo (baja escala → alta en idle), invalidación por `generation`, fingerprint de archivo (mtime+tamaño) para no recargar si no cambió.
- `crates/pdf/tests/multipage.rs` — **fixture nueva** `fixtures/pdf/multipage.pdf` (30 páginas, generada con latexmk; fuente en `multipage-src/`).
- `crates/ui/src/panels/pdf.rs` — `PdfPanel` completo: visor virtualizado, toolbar (zoom fit-width/fit-page/±, página N/M, abrir externo), **C3** `set_document(path, generation)` con ancla de scroll + flash "actualizado", **C4** `flash_sync_rect(SyncRect)`/`goto_page`/`on_inverse_jump(InverseJump)`; medición de frames incorporada.
- `app/src/bin/spike-pdf.rs` — demo actualizada (multipage, zoom, medición).

## Evidencia

```
cargo test -p lsx-pdf  →  10 unit + 6 cv + 4 multipage = 20 passed
cargo clippy -p lsx-pdf --all-targets -- -D warnings  →  limpio
```

## Integración en el shell (orchestrator, commits `d6e70ca`/`a6b593f`)

- **PdfSlot** adapter (patrón `PaneState`, igual que EditorSlot) montado en la hoja PDF.
- **Recarga al compilar**: `Finished` → `set_document(outcome.pdf, generation)` (conserva página/offset, flash).
- **SyncTeX forward** `Ctrl+Alt+J`: cursor del editor → `Synctex::forward_all` → `goto_page` + `flash_sync_rect`. API nueva `EditorPanel::active_cursor()`.
- **SyncTeX inverse**: `on_inverse_jump` → abre `archivo:línea` en el editor.
- **Preload al arrancar**: `main.tex` detectado se abre en el editor y su PDF hermano se carga en el visor.
- Evidencia: `shots/m1-pdf.png` (main.tex resaltado + CV renderizado, pág 1/2, zoom 50%).

## Pendientes

- Probar visualmente el rect de forward/inverse en vivo (parte del E2E G1, paso 5/6).
- `PdfPanel` no implementa el contrato `Pane` de A directamente (lo aporta `PdfSlot`); si M2 mueve `PaneState` al panel, unificar.
