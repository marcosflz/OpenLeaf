# Spike M0-S2 — Editor del kit + tree-sitter LaTeX

- Fecha: 2026-10-17 · Sesión: `ses_eb6356f6dffeNoGtdmTfauw4CU`
- **ESTADO: OK** (demo funcional + `--check` headless + 4 tests verdes)

## Entregable

- `crates/editor/src/latex_language.rs` (**nuevo**) — registro de lenguaje + query embebida + helpers + 4 tests
- `crates/editor/src/spike.rs` (**nuevo**) — vista demo (Editor del kit, gutter, dirty, Ctrl+S) + modo `--check` headless
- `crates/editor/src/bin/spike-editor.rs` y `app/src/bin/spike-editor.rs` — bins de la demo
- `assets/tree-sitter/latex/highlights.scm` (**nuevo**) — query propia (la del plan da 404)
- Desviaciones: `crates/editor/Cargo.toml` (feature `tree-sitter-languages`; pin `tree-sitter = "0.26.13"`; alias `tree-sitter-latex = { package = "codebook-tree-sitter-latex", version = "0.6.1" }`), `app/Cargo.toml` (+`lsx-editor`) → reconciliar en consolidación

## Demo

```
cargo run -p latex-studio --bin spike-editor            # ventana (copia en %TEMP%, fixture intacta)
cargo run -p latex-studio --bin spike-editor -- --check # headless
#  → parse ok: true · nodos section: 5 · spans estilo: 270 (141 con color)
```
Smoke OK (alive 8 s, 0 huérfanos). Fixture verificada intacta (hash idéntico).

## Respuestas de interfaz

**I-1 — SÍ hay registro de lenguajes custom y rangos de diagnóstico; NO hay modelo de archivo/pestañas.**
- Registro: `LanguageRegistry::singleton().register(name, &GrammarConfig::new(...))` + `register_parser_factory` para gramáticas dinámicas. Resaltado funcionando (141 spans con color en el CV).
- `EditorState` = **un documento** sin path/dirty/save/tabs. **M1-B1**: pestañas = `tab::TabBar` del kit + N `Entity<EditorState>` gestionados por la app; dirty vía `InputEvent::Change`.
- Diagnósticos inline: `state.diagnostics_mut()` + `DiagnosticSet` (estilos por severidad + popover); `state.lsp_mut()` expone completion/hover/definition/code-actions. Fallback de spans propios **innecesario**.

**I-3 — Buffer del componente** (`Rope` re-export de `component::input`, ropey 2.0.0-beta.1 interno). ropey directo solo para procesamiento fuera del editor (p. ej. LogParser), nunca como buffer de edición paralelo.

**Mapeo capturas → tema**: `SyntaxColors::style(nombre)` sobre 41 `HIGHLIGHT_NAMES`; fallback al prefijo; captura desconocida → default (puede tapar colores padre). El tema Default Dark NO define color para `label/operator/punctuation/variable` → TODO M1-A3.

## Hallazgo crítico: `tree-sitter-latex 0.1.0` (crates.io) está ROTO

- El tarball **no incluye `src/scanner.c`** pero el `parser.c` referencia `tree_sitter_latex_external_scanner_*` → **LNK2001** al enlazar (build.rs lo compila solo `if exists`). El commit fuente ya no existe en GitHub.
- **Remedio aplicado (ratificado por orchestrator)**: `codebook-tree-sitter-latex = "0.6.1"` — republish del **mismo upstream** (`latex-lsp/tree-sitter-latex`, autor original) con scanner, `LANGUAGE: LanguageFn`, compat tree-sitter 0.26.
- **Versiones**: gpui-component 0.7.3 usa `tree-sitter 0.26.13`; el pin del workspace 0.27 es semver-incompatible (0.x) → una instancia única con 0.26.13 (verificado `cargo tree`). **Acción consolidación**: `[workspace.dependencies]`: `tree-sitter = "0.26"`, `codebook-tree-sitter-latex = "0.6.1"` (alias como `tree-sitter-latex`).

## API descubierta (→ INTERFACES §12)

```rust
// registro/parseo sin UI
gpui_kit::component::highlighter::{GrammarConfig, LanguageRegistry, HighlightTheme, SyntaxHighlighter};
LanguageRegistry::singleton().register("latex", &GrammarConfig::new(
    "latex", codebook_tree_sitter_latex::LANGUAGE.into(), vec![], QUERY, "", ""));
// editor
EditorState::new(window, cx).language("latex").line_number(true).default_value(text)
st.set_highlighter("latex", cx); Editor::new(&ed).bordered(false).h(relative(1.0)).flex_1();
st.text() -> &Rope · st.cursor_position() · st.set_selected_range(r, cx) · st.diagnostics_mut()
cx.subscribe(&ed, |.., ev: &InputEvent, ..|)   // Change/Focus/Blur/PressEnter
// highlighter headless (tests/CLI)
SyntaxHighlighter::new(lang).update(None, &Rope, None)  // → bool
    .styles(&range, &*HighlightTheme::default_dark()) -> Vec<(Range<usize>, HighlightStyle)>
```

## Riesgos / TODO

1. `\titleformat{\section}` genera un nodo `section` espurio → el outline de M1 debe filtrar declaraciones.
2. Tema: faltan colores `label/operator/punctuation/variable` (M1-A3).
3. Deuda del spike: save escribe directo (M1: `EditorIntent::Save` + FsService); dirty compara O(n) por tecla (M1: versión de documento).
4. Interacción real tecla→Ctrl+S: se graba para la revisión G0.
