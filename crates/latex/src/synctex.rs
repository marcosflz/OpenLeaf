//! SyncTeX vía el CLI `synctex.exe` (`view`/`edit`).
//!
//! Hallazgos del spike M0-S5 (ver informe; salidas reales capturadas en los tests):
//! - `synctex view -i línea:columna:[página:]archivo -o out.pdf`: la línea es **1-based**
//!   (columna 0 = no relevante) y devuelve una *lista* de registros `Page/x/y/h/v/W/H`
//!   entre `SyncTeX result begin…end`. El primero suele ser el más preciso.
//! - `synctex edit -o página:x:y:out.pdf`: `x`/`y` en **bp** (1/72"), origen arriba-izquierda.
//!   La línea devuelta es **1-based**; columna `-1` = desconocida (`Option`).
//! - El CLI **siempre sale con código 0**: si falta el `.synctex.gz` o el PDF, no emite
//!   nada. Por eso `check_available` valida el sidecar antes de invocar.
//! - `Input:` devuelve la ruta absoluta registrada en la compilación (puede contener `/./`):
//!   se normaliza antes de devolverla.
//!
//! Desviación del contrato INTERFACES §3: `SourcePoint.column` es `Option<u32>` (el CLI
//! emite `Column:-1` cuando no la conoce) y el `exe` vive en el constructor, no en cada
//! llamada. `SyncPoint` conserva la forma del contrato (x,y,w,h) tomando el *punto* x,y y
//! el tamaño W,H; los registros completos (incl. origen de caja h,v) están en `SyncRecord`.

use std::process::Command;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};

use crate::pathutil::normalize;
use crate::proc;

/// Timeout por defecto (§3: 2 s); el CLI tarda ~50 ms en el corpus.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

/// Punto en el PDF (contrato §3): bp, origen arriba-izquierda.
///
/// `x`/`y` son el punto devuelto por el CLI; `w`/`h` el tamaño de la caja.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncPoint {
    pub page: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Registro crudo de `synctex view` (punto + caja). `v` es la línea base de la caja.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncRecord {
    pub page: u32,
    pub x: f32,
    pub y: f32,
    pub h: f32,
    pub v: f32,
    pub width: f32,
    pub height: f32,
}

/// Origen en el `.tex` (para el salto PDF → editor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePoint {
    /// Ruta normalizada (absoluta si la compilación la registró absoluta).
    pub file: Utf8PathBuf,
    /// 1-based.
    pub line: u32,
    /// 1-based; `None` si el CLI no la conoce (`Column:-1`).
    pub column: Option<u32>,
}

/// Errores de SyncTeX (§3 + `Spawn` para fallos de ejecución).
#[derive(Debug, thiserror::Error)]
pub enum SynctexError {
    #[error("synctex no disponible: no existe `{0}`")]
    NotAvailable(Utf8PathBuf),
    #[error("sin coincidencia en el índice synctex")]
    NoMatch,
    #[error("synctex agotó el tiempo")]
    Timeout,
    #[error("no se pudo ejecutar synctex: {0}")]
    Spawn(String),
    #[error("salida de synctex no reconocida: {0}")]
    Parse(String),
}

/// Cliente de `synctex.exe`.
#[derive(Debug, Clone)]
pub struct Synctex {
    exe: Utf8PathBuf,
    timeout: Duration,
}

impl Synctex {
    pub fn new(exe: impl Into<Utf8PathBuf>) -> Self {
        Self {
            exe: exe.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn exe(&self) -> &Utf8Path {
        &self.exe
    }

    /// Forward (editor → PDF): primer registro para `line:col`.
    ///
    /// `src` se pasa como aparezca en el log de TeX (relativo al cwd de compilación)
    /// o absoluto; absoluto es lo robusto si el proceso no corre en el proyecto.
    pub fn forward(
        &self,
        pdf: &Utf8Path,
        src: &Utf8Path,
        line: u32,
        col: u32,
    ) -> Result<SyncPoint, SynctexError> {
        let records = self.forward_all(pdf, src, line, col)?;
        let first = records.first().ok_or(SynctexError::NoMatch)?;
        Ok(SyncPoint {
            page: first.page,
            x: first.x,
            y: first.y,
            w: first.width,
            h: first.height,
        })
    }

    /// Forward con todos los registros (el CLI devuelve varios por línea).
    pub fn forward_all(
        &self,
        pdf: &Utf8Path,
        src: &Utf8Path,
        line: u32,
        col: u32,
    ) -> Result<Vec<SyncRecord>, SynctexError> {
        self.check_available(pdf)?;
        let mut cmd = Command::new(self.exe.as_std_path());
        cmd.arg("view")
            .arg("-i")
            .arg(format!("{line}:{col}:{src}"))
            .arg("-o")
            .arg(pdf.as_str());
        let out = self.exec(&mut cmd)?;
        let records = parse_view(&out.stdout);
        if records.is_empty() {
            return Err(SynctexError::NoMatch);
        }
        Ok(records)
    }

    /// Inverse (PDF → editor): origen para el punto `(x, y)` en bp.
    pub fn inverse(
        &self,
        pdf: &Utf8Path,
        page: u32,
        x: f32,
        y: f32,
    ) -> Result<SourcePoint, SynctexError> {
        self.check_available(pdf)?;
        let mut cmd = Command::new(self.exe.as_std_path());
        cmd.arg("edit")
            .arg("-o")
            .arg(format!("{page}:{x}:{y}:{pdf}"));
        let out = self.exec(&mut cmd)?;
        parse_edit(&out.stdout).ok_or(SynctexError::NoMatch)
    }

    fn check_available(&self, pdf: &Utf8Path) -> Result<(), SynctexError> {
        if !self.exe.is_file() {
            return Err(SynctexError::NotAvailable(self.exe.clone()));
        }
        if !pdf.is_file() {
            return Err(SynctexError::NotAvailable(pdf.to_owned()));
        }
        if sidecar_synctex(pdf).is_none() {
            return Err(SynctexError::NotAvailable(pdf.to_owned()));
        }
        Ok(())
    }

    fn exec(&self, cmd: &mut Command) -> Result<proc::ProcOutput, SynctexError> {
        let out = proc::run(cmd, self.timeout, None, None)
            .map_err(|e| SynctexError::Spawn(e.to_string()))?;
        if out.timed_out {
            return Err(SynctexError::Timeout);
        }
        Ok(out)
    }
}

/// `.synctex.gz` (o `.synctex`) junto al PDF; el `-d` del CLI no se usa en v1.
fn sidecar_synctex(pdf: &Utf8Path) -> Option<Utf8PathBuf> {
    let gz = pdf.with_extension("synctex.gz");
    if gz.is_file() {
        return Some(gz);
    }
    let raw = pdf.with_extension("synctex");
    raw.is_file().then_some(raw)
}

/// Parsea la salida de `synctex view` (registros `Page/x/y/h/v/W/H`).
fn parse_view(stdout: &str) -> Vec<SyncRecord> {
    let mut records: Vec<SyncRecord> = Vec::new();
    let mut current: Option<SyncRecord> = None;
    for line in stdout.lines() {
        if let Some(value) = line.strip_prefix("Page:") {
            if let Some(record) = current.take() {
                records.push(record);
            }
            if let Ok(page) = value.trim().parse::<u32>() {
                current = Some(SyncRecord {
                    page,
                    x: 0.0,
                    y: 0.0,
                    h: 0.0,
                    v: 0.0,
                    width: 0.0,
                    height: 0.0,
                });
            }
            continue;
        }
        let Some(record) = current.as_mut() else {
            continue;
        };
        if let Some(value) = line.strip_prefix("x:") {
            set_f32(&mut record.x, value);
        } else if let Some(value) = line.strip_prefix("y:") {
            set_f32(&mut record.y, value);
        } else if let Some(value) = line.strip_prefix("h:") {
            set_f32(&mut record.h, value);
        } else if let Some(value) = line.strip_prefix("v:") {
            set_f32(&mut record.v, value);
        } else if let Some(value) = line.strip_prefix("W:") {
            set_f32(&mut record.width, value);
        } else if let Some(value) = line.strip_prefix("H:") {
            set_f32(&mut record.height, value);
        }
    }
    if let Some(record) = current {
        records.push(record);
    }
    records
}

fn set_f32(slot: &mut f32, value: &str) {
    if let Ok(parsed) = value.trim().parse() {
        *slot = parsed;
    }
}

/// Parsea la salida de `synctex edit` (`Input:`, `Line:`, `Column:`).
fn parse_edit(stdout: &str) -> Option<SourcePoint> {
    let mut input: Option<Utf8PathBuf> = None;
    let mut line: Option<u32> = None;
    let mut column: Option<u32> = None;
    for raw in stdout.lines() {
        if let Some(value) = raw.strip_prefix("Input:") {
            let value = value.trim();
            if !value.is_empty() {
                input = Some(normalize(Utf8Path::new(value)));
            }
        } else if let Some(value) = raw.strip_prefix("Line:") {
            line = value.trim().parse().ok();
        } else if let Some(value) = raw.strip_prefix("Column:") {
            column = value
                .trim()
                .parse::<i64>()
                .ok()
                .and_then(|c| u32::try_from(c).ok())
                .filter(|c| *c > 0);
        }
    }
    Some(SourcePoint {
        file: input?,
        line: line?,
        column,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Salida real de `synctex view -i 78:0:main.tex -o main.pdf` (recortada a 2 registros).
    const VIEW_REAL: &str = "\
This is SyncTeX command line utility, version 1.5
SyncTeX result begin
Output:C:\\tmp\\out\\main.pdf
Page:1
x:46.799553
y:311.051819
h:46.799553
v:314.638397
W:523.800598
H:11.955168
before:
offset:-1
middle:
after:
Output:C:\\tmp\\out\\main.pdf
Page:1
x:21.892921
y:311.051819
h:21.892921
v:326.593567
W:554.107056
H:23.910336
before:
offset:-1
middle:
after:
SyncTeX result end
";

    /// Salida real de `synctex edit -o 1:46.799553:311.051819:main.pdf`.
    const EDIT_REAL: &str = "\
This is SyncTeX command line utility, version 1.5
SyncTeX result begin
Output:C:\\tmp\\out\\main.pdf
Input:C:/Users/marco/AppData/Local/Temp/lsx-s5/probe-cv/src/./main.tex
Line:79
Column:-1
Offset:0
Context:
SyncTeX result end
";

    #[test]
    fn parsea_view_real() {
        let records = parse_view(VIEW_REAL);
        assert_eq!(records.len(), 2);
        let r = records[0];
        assert_eq!(r.page, 1);
        assert!((r.x - 46.799_55).abs() < 0.001);
        assert!((r.y - 311.051_82).abs() < 0.001);
        assert!((r.width - 523.800_6).abs() < 0.001);
        assert!((r.height - 11.955_17).abs() < 0.001);
        assert!((r.v - 314.638_4).abs() < 0.001);
    }

    #[test]
    fn parsea_edit_real_normalizando_ruta() {
        let source = parse_edit(EDIT_REAL).expect("source point");
        assert_eq!(source.line, 79);
        assert_eq!(source.column, None);
        assert!(source.file.as_str().ends_with("main.tex"));
        assert!(!source.file.as_str().contains("/./"));
    }

    #[test]
    fn view_vacio_es_nomatch() {
        // Caso `No tag for otro.tex`: solo banner, sin registros.
        let stdout = "This is SyncTeX command line utility, version 1.5\nSyncTeX Warning: No tag for otro.tex\n";
        assert!(parse_view(stdout).is_empty());
    }
}
