---
name: lsx-editor-ts
description: Usar cuando se implemente o modifique el editor (stream B, crate lsx-editor) — registro de tree-sitter-latex y queries highlights.scm, mapeo de capturas a estilos del tema, marcadores de diagnóstico desde el LogParser, buscar/reemplazar y la regla de guardado atómico vía FsService. Recoge el estado seed de docs/plan/INTERFACES.md §6 y los hallazgos pendientes del spike M0-S2 (decisiones I-1 e I-3).
---

# Editor tree-sitter (lsx-editor)

Estado: contratos seed en `docs/plan/INTERFACES.md` §6. Las APIs del componente editor del kit se descubren en el spike M0-S2 → los huecos van marcados como TODOs abajo.

## Contrato (INTERFACES §6)

- `Document { id, path, version, dirty, eol, encoding }`; `EditorState { docs, active, cursors, scroll, find }`.
- `LanguageRegistry` registra `tree-sitter-latex` + queries; `LATEX_HIGHLIGHT_QUERY` con `include_str!("../../../assets/tree-sitter/latex/highlights.scm")`.
- `map_diagnostics(&Document, &[Diagnostic]) -> Vec<InlineMarker>` con `{line, col, len, severity, message}`; fuente = `lsx-latex::parse_log` (M1-B3).
- El editor NUNCA escribe en disco: emite `EditorIntent::Save`; `lsx-core::FsService::write_text_atomic` ejecuta el guardado; la UI escucha `FsEvent`.
- Conflicto externo: dirty + cambio en disco → modal de 3 opciones (recargar, sobrescribir, ver diff) (M1-B2).

## Registro del lenguaje (pasos)

1. Dependencias: `tree-sitter 0.27` + `tree-sitter-latex` (MIT; versión `0.1+` o git pin si el registro lo exige — PLAN §5.1).
2. Vendorizar `assets/tree-sitter/latex/highlights.scm` con su aviso de licencia — `TODO(M0-S2): verificar` que la carpeta `assets/` aún no existe en el repo y crearla en el spike.
3. Registrar la grammar en el registry del editor del kit siguiendo `examples/editor` del repo gpui-kit.
4. Mapear capturas → tokens de tema del kit (sin colores hardcodeados; consultar Design Guides).
5. `TODO(M0-S2): verificar` (I-1) si el editor del kit expone modelo de archivo + rangos de diagnóstico + registry de lenguajes, y (I-3) si se usa su buffer o ropey; volcar aquí las APIs reales con snippets.

## Diagnósticos

- Entrada: `lsx-latex::parse_log` → `Diagnostic { severity, kind, file, line, column, message, raw_block }`.
- Se refrescan por generación de compilación; click en problemas → cursor en la línea (M1-D3, panel ampliado M2-9).
- Squiggle inline si la API lo permite; fallback gutter + tooltip + panel de problemas (PLAN §5.8; decisión en M0-S2).

## Buscar/reemplazar (M1-B4)

- Barra en el editor con contador, toggles case-sensitive/regex (regex off por defecto) y wrap-around.
- Criterio de aceptación: los 20 casos del guion QA pasan; se testea en unit + TestAppContext.

## Reglas de documento

- Encoding UTF-8 sin BOM (BOM preservado); otros encodings → aviso y edición deshabilitada en v1.
- EOL LF/CRLF detectado y preservado por documento.
- Indentación por espacios (2 por defecto; settings `editor.tabSize`).

## Tests

- Unit: `map_diagnostics` (rangos por línea/columna), find/replace; snapshots `insta` si aporta (ver skill `lsx-quality-gates`).
- UI: teclas simuladas → buffer/cursor con `TestAppContext` (skill `lsx-gpui-patterns`).
- Integración de flujo: guardar → `FsEvent` → recompilar (cubierto por M1-B2/B3).

## Enlaces útiles

- `docs/plan/INTERFACES.md` §6 (contrato) y §9 (convenciones); `docs/plan/PLAN-MAESTRO.md` §5.8 (editor).
- Spikes: `docs/plan/TAREAS.md` §3, M0-S2 (preguntas I-1/I-3; fallback highlighter propio).
