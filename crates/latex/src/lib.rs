//! `lsx-latex` — motor LaTeX, parser de log, SyncTeX y cola de compilación
//! (contrato en `docs/plan/INTERFACES.md` §3).
//!
//! Piezas (todas sin gpui; produce datos puros y consume comandos):
//!
//! - [`TexLiveEngine`]: `probe`/`compile` sobre `tex/bin/windows/latexmk.exe`
//!   con timeout, cancelación por *job object* (`proc`, M1-D1), auto-install
//!   con `tlmgr` (M1-D4) y limpieza con `latexmk -c|-C` (M1-D5).
//! - [`CompileController`] (M1-D2): single-flight, coalescing y autoCompile con
//!   debounce; eventos por canal `async_channel` con generaciones.
//! - [`parse_log`]/[`parse_bib_log`] (M1-D3): diagnósticos por archivo/línea,
//!   `Underfull`, severidad `Info` y correlación estable de bloques.
//! - [`Synctex`] (M0-S5): `forward`/`forward_all`/`inverse` con el CLI.
//! - [`detect_main`] (M1-D5) y [`LatexSettings`]: heurística de `main.tex` y la
//!   sección `latex` de `config/settings.json`.
//!
//! Regla dura: sin gpui.

pub mod engine;
pub mod logparse;
pub mod mainfile;
pub(crate) mod pathutil;
pub(crate) mod proc;
pub mod queue;
pub mod root;
pub mod settings;
pub mod synctex;
pub mod tlmgr;

pub use engine::{
    CLEAN_TIMEOUT, CancelToken, CleanMode, CleanReport, CleanRequest, CompileEvent, CompileOutcome,
    CompileRequest, CompileStatus, DEFAULT_ARGS, DEFAULT_TIMEOUT, EngineError, EngineId,
    EngineInfo, LOG_TAIL_LINES, LatexEngine, MAX_AUTO_INSTALLS, PackageInstall,
    PackageInstallStage, TexLiveEngine,
};
pub use logparse::{
    Diagnostic, DiagnosticKind, DiagnosticSeverity, LogHint, MAX_LOG_BYTES, parse_bib_log,
    parse_log, parse_stdout_stream,
};
pub use mainfile::detect_main;
pub use queue::{CompileController, CompileState, QueueStatus};
pub use root::TexLivePaths;
pub use settings::{
    DEFAULT_DEBOUNCE_MS, DEFAULT_TIMEOUT_SECS, LatexSettings, MAX_DEBOUNCE_MS, TIMEOUT_SECS_RANGE,
};
pub use synctex::{SourcePoint, SyncPoint, SyncRecord, Synctex, SynctexError};
pub use tlmgr::{InstallReport, Tlmgr, TlmgrError};
