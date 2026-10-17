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
//! Estado: P0 — stub. Se implementa en M0-S2 (spike) y M1 (stream B).
