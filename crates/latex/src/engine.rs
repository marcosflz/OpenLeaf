//! Motor LaTeX (`LatexEngine`) con backend TeX Live portátil sobre `latexmk`.
//!
//! Spike M0-S5 del contrato `docs/plan/INTERFACES.md` §3. Añadidos al contrato
//! (documentados en el informe del spike):
//! - `CompileRequest.out_dir` (opcional): salidas a otro directorio con `-outdir`.
//! - `CompileRequest.auto_install` (spike de M1-D4): bucle `tlmgr` tope 3.
//! - `CompileOutcome.log` y `CompileOutcome.exit_code`.
//!
//! `probe()` verifica `latexmk -v` (versión 4.88 en el corpus) y la presencia de
//! `synctex.exe`/`tlmgr.bat`. `compile()` corre `latexmk` con
//! `-pdf -synctex=1 -interaction=nonstopmode -file-line-error -outdir=<dir>`, con
//! timeout y cancelación (kill de árbol vía taskkill; job objects llegan en M1-D1).

use std::collections::HashSet;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use camino::{Utf8Path, Utf8PathBuf};

use crate::logparse::{Diagnostic, DiagnosticKind, parse_log, parse_stdout_stream};
use crate::pathutil::normalize;
use crate::proc;
use crate::root::TexLivePaths;
use crate::tlmgr::Tlmgr;

/// Args por defecto de `latexmk` (§5.6 / INTERFACES §3).
pub const DEFAULT_ARGS: [&str; 4] = [
    "-pdf",
    "-synctex=1",
    "-interaction=nonstopmode",
    "-file-line-error",
];
/// Timeout por defecto (`latex.timeoutSecs` = 600).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
/// Tope de auto-instalaciones por compilación (M1-D4).
pub const MAX_AUTO_INSTALLS: usize = 3;
/// Líneas conservadas en `CompileOutcome::log_tail` (~200, §3).
pub const LOG_TAIL_LINES: usize = 200;

/// Backend del motor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineId {
    TexLive,
    Tectonic,
}

/// Resultado de `probe()` (versión, ruta, disponible).
#[derive(Debug, Clone)]
pub struct EngineInfo {
    pub id: EngineId,
    pub available: bool,
    pub version: Option<String>,
    pub latexmk: Utf8PathBuf,
    pub texbin: Utf8PathBuf,
    pub synctex: Option<Utf8PathBuf>,
    pub tlmgr: Option<Utf8PathBuf>,
}

/// Petición de compilación (§3 + `out_dir`/`auto_install`).
#[derive(Debug, Clone)]
pub struct CompileRequest {
    /// Directorio del proyecto (cwd de `latexmk`).
    pub project: Utf8PathBuf,
    /// Archivo principal (absoluto; si es relativo se resuelve contra `project`).
    pub main: Utf8PathBuf,
    /// Directorio de salida (`-outdir`). `None` → junto a las fuentes.
    pub out_dir: Option<Utf8PathBuf>,
    /// Args extra; vacío → `DEFAULT_ARGS`.
    pub args: Vec<String>,
    pub timeout: Duration,
    /// Compilación limpia: `latexmk -C` antes del build.
    pub clean: bool,
    /// Auto-install de paquetes con `tlmgr` (spike M1-D4).
    pub auto_install: bool,
}

impl CompileRequest {
    /// Petición con defaults (`DEFAULT_ARGS`, timeout 600 s, sin clean/auto-install).
    pub fn new(project: impl Into<Utf8PathBuf>, main: impl Into<Utf8PathBuf>) -> Self {
        Self {
            project: project.into(),
            main: main.into(),
            out_dir: None,
            args: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
            clean: false,
            auto_install: false,
        }
    }
}

/// Estado final de una compilación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileStatus {
    Success,
    Failed,
    Cancelled,
    Timeout,
}

/// Resultado completo de una compilación (§3 + `log`/`exit_code`).
#[derive(Debug, Clone)]
pub struct CompileOutcome {
    pub status: CompileStatus,
    pub duration: Duration,
    pub pdf: Option<Utf8PathBuf>,
    /// Ruta del `.log` de `latexmk`/`pdflatex` (queda en `out_dir`).
    pub log: Option<Utf8PathBuf>,
    /// Últimas ~200 líneas del log.
    pub log_tail: String,
    pub diagnostics: Vec<Diagnostic>,
    /// Intentos de auto-install (spike M1-D4).
    pub package_installs: Vec<PackageInstall>,
    /// Código de salida de `latexmk` (`Some(12)` en errores LaTeX reales).
    pub exit_code: Option<i32>,
}

/// Intento de instalación de un paquete vía tlmgr.
#[derive(Debug, Clone)]
pub struct PackageInstall {
    pub package: String,
    pub success: bool,
    pub message: Option<String>,
}

/// Eventos de compilación (§3). En M1 el `CompileQueue` es quien emite
/// `Queued`/`Started`/`Finished`; el spike los emite desde el engine.
#[derive(Debug, Clone)]
pub enum CompileEvent {
    Queued,
    Started {
        generation: u64,
    },
    Progress {
        line: String,
    },
    Finished {
        generation: u64,
        outcome: CompileOutcome,
    },
    Cancelled {
        generation: u64,
    },
}

/// Token de cancelación compartido (`cancel(generation)` en M1-D2).
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Errores del motor.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("no se encontró TeX Live portable (rutas probadas: {tried:?})")]
    TexLiveNotFound { tried: Vec<Utf8PathBuf> },
    #[error("no se pudo ejecutar `{program}`: {source}")]
    Spawn {
        program: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("el proyecto no existe o no es un directorio: {0}")]
    ProjectNotFound(Utf8PathBuf),
}

/// Trait de motor (§3).
pub trait LatexEngine: Send + Sync {
    fn id(&self) -> EngineId;
    fn probe(&self) -> Result<EngineInfo, EngineError>;
    fn compile(
        &self,
        req: &CompileRequest,
        on_event: &(dyn Fn(CompileEvent) + Send + Sync),
        cancel: &CancelToken,
    ) -> Result<CompileOutcome, EngineError>;
}

/// Backend TeX Live portátil (`tex/bin/windows`).
#[derive(Debug, Clone)]
pub struct TexLiveEngine {
    paths: TexLivePaths,
}

impl TexLiveEngine {
    pub fn new(paths: TexLivePaths) -> Self {
        Self { paths }
    }

    /// Detecta el TeX Live portable (ver `TexLivePaths::detect`).
    pub fn detect() -> Result<Self, EngineError> {
        Ok(Self::new(TexLivePaths::detect()?))
    }

    pub fn paths(&self) -> &TexLivePaths {
        &self.paths
    }
}

impl LatexEngine for TexLiveEngine {
    fn id(&self) -> EngineId {
        EngineId::TexLive
    }

    fn probe(&self) -> Result<EngineInfo, EngineError> {
        let mut cmd = Command::new(self.paths.latexmk.as_std_path());
        cmd.arg("-v");
        let out = proc::run(&mut cmd, Duration::from_secs(15), None, None).map_err(|source| {
            EngineError::Spawn {
                program: self.paths.latexmk.clone(),
                source,
            }
        })?;
        let combined = out.combined();
        let version = parse_latexmk_version(&combined);
        Ok(EngineInfo {
            id: EngineId::TexLive,
            available: version.is_some(),
            version,
            latexmk: self.paths.latexmk.clone(),
            texbin: self.paths.bin.clone(),
            synctex: self
                .paths
                .synctex
                .is_file()
                .then(|| self.paths.synctex.clone()),
            tlmgr: self.paths.tlmgr.is_file().then(|| self.paths.tlmgr.clone()),
        })
    }

    fn compile(
        &self,
        req: &CompileRequest,
        on_event: &(dyn Fn(CompileEvent) + Send + Sync),
        cancel: &CancelToken,
    ) -> Result<CompileOutcome, EngineError> {
        let generation = 0u64;
        on_event(CompileEvent::Started { generation });
        let outcome = self.compile_with_autoinstall(req, on_event, cancel)?;
        if outcome.status == CompileStatus::Cancelled {
            on_event(CompileEvent::Cancelled { generation });
        } else {
            on_event(CompileEvent::Finished {
                generation,
                outcome: outcome.clone(),
            });
        }
        Ok(outcome)
    }
}

impl TexLiveEngine {
    fn compile_with_autoinstall(
        &self,
        req: &CompileRequest,
        on_event: &(dyn Fn(CompileEvent) + Send + Sync),
        cancel: &CancelToken,
    ) -> Result<CompileOutcome, EngineError> {
        let project = resolve_project(&req.project)?;
        let main_abs = if req.main.is_absolute() {
            req.main.clone()
        } else {
            project.join(&req.main)
        };
        let out_dir = req.out_dir.clone().unwrap_or_else(|| project.clone());
        let tlmgr = Tlmgr::new(self.paths.tlmgr.clone());

        let mut installs: Vec<PackageInstall> = Vec::new();
        let mut ctx = CompileCtx {
            project: &project,
            main_abs: &main_abs,
            out_dir: &out_dir,
            force: false,
        };
        let mut outcome = self.compile_once(req, &ctx, on_event, cancel)?;
        let mut attempted: HashSet<String> = HashSet::new();
        while req.auto_install
            && outcome.status == CompileStatus::Failed
            && installs.len() < MAX_AUTO_INSTALLS
        {
            let Some(package) = outcome.diagnostics.iter().find_map(|d| match &d.kind {
                DiagnosticKind::MissingPackage { package } if !attempted.contains(package) => {
                    Some(package.clone())
                }
                _ => None,
            }) else {
                break;
            };
            attempted.insert(package.clone());
            on_event(CompileEvent::Progress {
                line: format!("tlmgr: instalando paquete `{package}`…"),
            });
            let entry = match tlmgr.install(&package) {
                Ok(report) => PackageInstall {
                    package: package.clone(),
                    success: true,
                    message: report.forced.then(|| {
                        "instalado con --force (tlmgr desactualizado frente al repositorio)"
                            .to_string()
                    }),
                },
                Err(error) => PackageInstall {
                    package: package.clone(),
                    success: false,
                    message: Some(error.to_string()),
                },
            };
            let repaired = entry.success;
            installs.push(entry);
            if !repaired {
                break;
            }
            on_event(CompileEvent::Progress {
                line: "Recompilando tras instalar…".to_string(),
            });
            // Sin `-g`, latexmk reutiliza el fallo cacheado en `.fdb_latexmk` y no
            // reintenta pdflatex aunque el paquete ya esté disponible (ver informe S5).
            ctx.force = true;
            outcome = self.compile_once(req, &ctx, on_event, cancel)?;
        }
        outcome.package_installs = installs;
        Ok(outcome)
    }

    fn compile_once(
        &self,
        req: &CompileRequest,
        ctx: &CompileCtx<'_>,
        on_event: &(dyn Fn(CompileEvent) + Send + Sync),
        cancel: &CancelToken,
    ) -> Result<CompileOutcome, EngineError> {
        let project = ctx.project;
        let main_abs = ctx.main_abs;
        let out_dir = ctx.out_dir;
        if cancel.is_cancelled() {
            return Ok(CompileOutcome {
                status: CompileStatus::Cancelled,
                duration: Duration::ZERO,
                pdf: None,
                log: None,
                log_tail: String::new(),
                diagnostics: Vec::new(),
                package_installs: Vec::new(),
                exit_code: None,
            });
        }
        std::fs::create_dir_all(out_dir)?;
        let stem = main_abs.file_stem().unwrap_or("main");
        let main_arg = main_arg(project, main_abs);

        if req.clean {
            let mut clean = self.command(project, &clean_args(out_dir, &main_arg));
            let _ = proc::run(&mut clean, Duration::from_secs(60), None, None);
        }

        let mut cmd = self.command(project, &build_args(req, out_dir, &main_arg, ctx.force));
        tracing::debug!(
            target: "lsx::latex",
            program = %self.paths.latexmk,
            "ejecutando latexmk"
        );
        let started = Instant::now();
        let on_line = |line: &str| {
            if parse_stdout_stream(line).is_some() {
                on_event(CompileEvent::Progress {
                    line: line.to_string(),
                });
            }
        };
        let out =
            proc::run(&mut cmd, req.timeout, Some(cancel), Some(&on_line)).map_err(|source| {
                EngineError::Spawn {
                    program: self.paths.latexmk.clone(),
                    source,
                }
            })?;
        let duration = started.elapsed();

        let log_path = out_dir.join(format!("{stem}.log"));
        let log_text = std::fs::read(&log_path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let mut diagnostics = log_text.as_deref().map(parse_log).unwrap_or_default();
        for diag in &mut diagnostics {
            if let Some(file) = diag.file.take() {
                diag.file = Some(resolve_diag_file(project, &file));
            }
        }
        let log_tail = log_text
            .as_deref()
            .map(|text| tail_lines(text, LOG_TAIL_LINES))
            .unwrap_or_default();
        let pdf_candidate = out_dir.join(format!("{stem}.pdf"));
        let pdf = pdf_candidate.is_file().then_some(pdf_candidate);
        let status = if out.cancelled {
            CompileStatus::Cancelled
        } else if out.timed_out {
            CompileStatus::Timeout
        } else if out.code == Some(0) {
            CompileStatus::Success
        } else {
            CompileStatus::Failed
        };
        Ok(CompileOutcome {
            status,
            duration,
            pdf,
            log: log_text.is_some().then_some(log_path),
            log_tail,
            diagnostics,
            package_installs: Vec::new(),
            exit_code: out.code,
        })
    }

    fn command(&self, project: &Utf8Path, args: &[String]) -> Command {
        let mut cmd = Command::new(self.paths.latexmk.as_std_path());
        cmd.args(args);
        cmd.current_dir(project.as_std_path());
        cmd
    }
}

/// Contexto resuelto de una compilación (evita parámetros de más en `compile_once`).
struct CompileCtx<'a> {
    project: &'a Utf8Path,
    main_abs: &'a Utf8Path,
    out_dir: &'a Utf8Path,
    /// Añade `-g` a `latexmk` (obligatorio al recomponer tras auto-install).
    force: bool,
}

/// Args efectivos: los de la petición o `DEFAULT_ARGS`, + `-g` si `force`,
/// + `-outdir` + archivo.
fn build_args(
    req: &CompileRequest,
    out_dir: &Utf8Path,
    main_arg: &str,
    force: bool,
) -> Vec<String> {
    let mut args: Vec<String> = if req.args.is_empty() {
        DEFAULT_ARGS.iter().map(|s| (*s).to_string()).collect()
    } else {
        req.args.clone()
    };
    if force {
        args.push("-g".to_string());
    }
    args.push(format!("-outdir={out_dir}"));
    args.push(main_arg.to_string());
    args
}

/// `latexmk -C` (limpia auxiliares y PDF) con el mismo `-outdir`.
fn clean_args(out_dir: &Utf8Path, main_arg: &str) -> Vec<String> {
    vec![
        "-C".to_string(),
        format!("-outdir={out_dir}"),
        main_arg.to_string(),
    ]
}

/// Nombre de `main` relativo al cwd de latexmk (o absoluto si está fuera).
fn main_arg(project: &Utf8Path, main_abs: &Utf8Path) -> String {
    match main_abs.strip_prefix(project) {
        Ok(rel) => rel.as_str().to_string(),
        Err(_) => main_abs.as_str().to_string(),
    }
}

fn resolve_project(project: &Utf8Path) -> Result<Utf8PathBuf, EngineError> {
    let abs = if project.is_absolute() {
        project.to_owned()
    } else {
        let cwd = Utf8PathBuf::from_path_buf(std::env::current_dir()?).map_err(|p| {
            EngineError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("ruta no UTF-8: {}", p.display()),
            ))
        })?;
        cwd.join(project)
    };
    if !abs.is_dir() {
        return Err(EngineError::ProjectNotFound(abs));
    }
    Ok(abs)
}

/// Resuelve la ruta de un diagnóstico: relativas → contra el cwd de compilación.
fn resolve_diag_file(project: &Utf8Path, file: &Utf8Path) -> Utf8PathBuf {
    if file.is_absolute() {
        return normalize(file);
    }
    let joined = project.join(file);
    if joined.exists() {
        normalize(&joined)
    } else {
        file.to_owned()
    }
}

/// `Latexmk, John Collins, 9 March 2026. Version 4.88` → `4.88`.
fn parse_latexmk_version(output: &str) -> Option<String> {
    let idx = output.find("Version ")?;
    let rest = &output[idx + "Version ".len()..];
    let version: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
        .collect();
    (!version.is_empty()).then_some(version)
}

fn tail_lines(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_de_latexmk() {
        let out = "Latexmk, John Collins, 9 March 2026. Version 4.88\n";
        assert_eq!(parse_latexmk_version(out).as_deref(), Some("4.88"));
        assert_eq!(parse_latexmk_version("sin version aqui"), None);
    }

    #[test]
    fn args_por_defecto_con_outdir() {
        let req = CompileRequest::new("C:/proj", "C:/proj/main.tex");
        let args = build_args(&req, Utf8Path::new("C:/out"), "main.tex", false);
        assert_eq!(
            args,
            vec![
                "-pdf",
                "-synctex=1",
                "-interaction=nonstopmode",
                "-file-line-error",
                "-outdir=C:/out",
                "main.tex",
            ]
        );
    }

    #[test]
    fn force_anade_g_para_el_rerun() {
        let req = CompileRequest::new("C:/proj", "C:/proj/main.tex");
        let args = build_args(&req, Utf8Path::new("C:/out"), "main.tex", true);
        assert!(args.contains(&"-g".to_string()), "{args:?}");
        let force_pos = args.iter().position(|a| a == "-g").unwrap();
        let outdir_pos = args.iter().position(|a| a.starts_with("-outdir=")).unwrap();
        assert!(force_pos < outdir_pos, "{args:?}");
    }

    #[test]
    fn main_fuera_del_proyecto_va_absoluto() {
        let arg = main_arg(Utf8Path::new("C:/proj"), Utf8Path::new("C:/otro/main.tex"));
        assert_eq!(arg, "C:/otro/main.tex");
        let rel = main_arg(
            Utf8Path::new("C:/proj"),
            Utf8Path::new("C:/proj/sub/main.tex"),
        );
        assert_eq!(rel, "sub/main.tex");
    }

    #[test]
    fn cancel_token_es_compartido() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        token.cancel();
        assert!(clone.is_cancelled());
    }

    #[test]
    fn tail_recorta() {
        let text = "a\nb\nc\nd\n";
        assert_eq!(tail_lines(text, 2), "c\nd");
    }
}
