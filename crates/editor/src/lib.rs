//! `lsx-editor` — editor de código LaTeX sobre GPUI (tree-sitter desde el día 1).
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §6):
//! - modelo de documento (path, versión, dirty, EOL/encoding) y pestañas
//! - registro de lenguaje `tree-sitter-latex` + queries de resaltado
//! - mapeo de `Diagnostic` (lsx-latex) → marcadores inline
//! - buscar/reemplazar (M1.5); LSP texlab (M4)
//!
//! Depende de gpui-kit; la lógica de buffer/parseo debe poder testearse sin UI.
//!
//! Estado: M0-S2 (spike). El spike deja dos módulos:
//! - [`latex_language`]: registro de `tree-sitter-latex` + query propia en el
//!   `LanguageRegistry` del kit (I-1: sí hay registro de gramáticas custom).
//! - [`spike`]: vista demo con el `Editor` del kit, gutter, dirty y guardado
//!   sobre una copia temporal del fixture `fixtures/projects/cv/main.tex`.

pub mod latex_language;
pub mod spike;
