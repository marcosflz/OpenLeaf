//! `lsx-editor` — editor de código LaTeX sobre GPUI (tree-sitter desde el día 1).
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §6):
//! - modelo de documento (path, versión, dirty, EOL/encoding) y pestañas
//! - registro de lenguaje `tree-sitter-latex` + queries de resaltado
//! - mapeo de `Diagnostic` (lsx-latex) → marcadores inline
//! - buscar/reemplazar (M1-B4); LSP texlab (M4)
//!
//! Depende de gpui-kit; la lógica de buffer/parseo debe poder testearse sin UI.
//!
//! Módulos M1 (stream B):
//! - [`document`]: `DocumentMeta` con versión + dirty O(1) (M1-B1)
//! - [`session`]: restauración de pestañas/cursores en `config/` (M1-B1)
//! - [`backup`]: backups de recuperación en `config/backups/` (M1-B5)
//! - [`diagnostics`]: `lsx_latex::Diagnostic` → `DiagnosticSet` del kit (M1-B3)
//! - [`find`]: buscar/reemplazar sobre la sesión de búsqueda del kit (M1-B4)
//! - [`diff`]: diff por líneas para el conflicto externo (M1-B2)
//! - [`latex_language`]: registro de tree-sitter-latex (M0-S2)
//! - [`spike`]: demo del spike M0-S2 (se conserva como referencia)
//!
//! El panel montable por el shell vive en `lsx-ui::panels::editor`
//! (stream B de M1), que compone estos módulos con `FsService` + `notify`.

pub mod backup;
pub mod diagnostics;
pub mod diff;
pub mod document;
pub mod find;
pub mod latex_language;
pub mod session;
pub mod spike;

pub use document::{AutosaveConfig, DocId, DocumentMeta};
pub use latex_language::{LATEX_HIGHLIGHT_QUERY, LATEX_LANGUAGE_NAME, register_latex_language};
