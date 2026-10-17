//! Envoltorio de `tlmgr` (auto-install de paquetes, M1-D4).
//!
//! Hallazgos del spike M0-S5 (ver informe):
//! - `tlmgr.bat` **siempre devuelve código de salida 0**, incluso en errores:
//!   el éxito se decide parseando la salida (`An error has occurred`, `Terminating`,
//!   `not present in repository`, …).
//! - El TeX Live portable del repo está desactualizado frente a su repositorio
//!   (`tlnet`): `tlmgr install` muere con «tlmgr itself needs to be updated» hasta
//!   que se hace `tlmgr update --self`. Este módulo reintenta con `--force`, que
//!   continúa sin tocar la infraestructura (decisión para M1-D4; alternativa M3:
//!   fijar el repositorio a un snapshot coincidente).

use std::process::Command;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};

use crate::proc;

/// Timeout de comandos informativos (`info`, `--version`).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// Timeout de `install`/`remove` (descarga + mktexlsr).
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(600);

/// Resultado de un `install` correcto.
#[derive(Debug, Clone)]
pub struct InstallReport {
    pub package: String,
    pub output: String,
    /// `true` si hubo que reintentar con `--force` por el guard de tlmgr.
    pub forced: bool,
}

/// Errores de tlmgr.
#[derive(Debug, thiserror::Error)]
pub enum TlmgrError {
    #[error("no se encontró tlmgr en `{0}`")]
    NotFound(Utf8PathBuf),
    #[error("no se pudo ejecutar tlmgr: {0}")]
    Spawn(String),
    #[error("tlmgr falló para `{package}` (ver salida)")]
    Failed { package: String, output: String },
}

/// Cliente de `tex/bin/windows/tlmgr.bat`.
#[derive(Debug, Clone)]
pub struct Tlmgr {
    exe: Utf8PathBuf,
}

impl Tlmgr {
    pub fn new(exe: impl Into<Utf8PathBuf>) -> Self {
        Self { exe: exe.into() }
    }

    pub fn exe(&self) -> &Utf8Path {
        &self.exe
    }

    /// Versión de tlmgr (cabecera de `--version`).
    pub fn version(&self) -> Result<String, TlmgrError> {
        let out = self.run(&["--version"], DEFAULT_TIMEOUT)?;
        Ok(out
            .combined()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(3)
            .collect::<Vec<_>>()
            .join(" | "))
    }

    /// ¿Está instalado `package`? (parsea `installed: Yes|No` de `tlmgr info`).
    pub fn is_installed(&self, package: &str) -> Result<bool, TlmgrError> {
        let out = self.run(&["info", "--only-installed", package], DEFAULT_TIMEOUT)?;
        for line in out.combined().lines() {
            if let Some(value) = line.trim().strip_prefix("installed:") {
                return Ok(value.trim().eq_ignore_ascii_case("yes"));
            }
        }
        Ok(false)
    }

    /// Instala `package`. Si tlmgr se declara desactualizado, reintenta con `--force`.
    pub fn install(&self, package: &str) -> Result<InstallReport, TlmgrError> {
        let out = self.run(&["install", package], INSTALL_TIMEOUT)?;
        let mut combined = out.combined();
        let mut forced = false;
        if combined.contains("tlmgr itself needs to be updated")
            && !combined.contains("Continuing due to --force")
        {
            forced = true;
            let out = self.run(&["install", "--force", package], INSTALL_TIMEOUT)?;
            combined = out.combined();
        }
        if looks_failed(&combined) {
            return Err(TlmgrError::Failed {
                package: package.to_string(),
                output: tail(&combined, 40),
            });
        }
        Ok(InstallReport {
            package: package.to_string(),
            output: combined,
            forced,
        })
    }

    /// Desinstala `package` (usado por tests/maintenance; reversible con `install`).
    pub fn remove(&self, package: &str) -> Result<(), TlmgrError> {
        let out = self.run(&["remove", package], INSTALL_TIMEOUT)?;
        let combined = out.combined();
        if looks_failed(&combined) {
            return Err(TlmgrError::Failed {
                package: package.to_string(),
                output: tail(&combined, 40),
            });
        }
        Ok(())
    }

    fn run(&self, args: &[&str], timeout: Duration) -> Result<proc::ProcOutput, TlmgrError> {
        if !self.exe.is_file() {
            return Err(TlmgrError::NotFound(self.exe.clone()));
        }
        let mut cmd = Command::new(self.exe.as_std_path());
        cmd.args(args);
        proc::run(&mut cmd, timeout, None, None).map_err(|e| TlmgrError::Spawn(e.to_string()))
    }
}

fn looks_failed(combined: &str) -> bool {
    [
        "Terminating; please see warning above",
        "An error has occurred",
        "returned an error",
        "not present in repository",
        "cannot find package",
    ]
    .iter()
    .any(|marker| combined.contains(marker))
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}
