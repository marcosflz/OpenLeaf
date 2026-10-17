//! `lsx-latex` — motor LaTeX, parser de log y SyncTeX (contrato en `docs/plan/INTERFACES.md` §3).
//!
//! Estado: spike M0-S5 implementado — `TexLiveEngine::{probe, compile}` sobre
//! `tex/bin/windows/latexmk.exe`, `parse_log`/`parse_stdout_stream` con el corpus
//! `fixtures/logs/`, `Synctex` (`synctex.exe`) y auto-install vía `tlmgr`.
//! Pendiente para M1: `CompileQueue` (single-flight/coalescing, D2), job objects +
//! `CREATE_NO_WINDOW` (D1) y correlación de diagnósticos (D3).
//!
//! Regla dura: sin gpui; produce datos puros y consume comandos.

pub mod engine;
pub mod logparse;
pub(crate) mod pathutil;
pub(crate) mod proc;
pub mod root;
pub mod synctex;
pub mod tlmgr;

pub use engine::{
    CancelToken, CompileEvent, CompileOutcome, CompileRequest, CompileStatus, EngineError,
    EngineId, EngineInfo, LatexEngine, PackageInstall, TexLiveEngine,
};
pub use logparse::{
    Diagnostic, DiagnosticKind, DiagnosticSeverity, LogHint, parse_log, parse_stdout_stream,
};
pub use root::TexLivePaths;
pub use synctex::{SourcePoint, SyncPoint, SyncRecord, Synctex, SynctexError};
pub use tlmgr::{InstallReport, Tlmgr, TlmgrError};
