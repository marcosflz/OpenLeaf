//! Localización del TeX Live portátil del repo (`tex/bin/windows`).
//!
//! Sustituto *spike* de `lsx-core::PortableRoot` (INTERFACES §2, aún stub). Orden de
//! búsqueda: `LSX_TEX_BIN` → `LATEX_STUDIO_ROOT/tex/bin/windows` → ancestros de
//! `CARGO_MANIFEST_DIR` (dev) → ancestros del ejecutable (distro portable).
//! M1 lo reemplaza por el helper de `lsx-core`.

use camino::{Utf8Path, Utf8PathBuf};

use crate::engine::EngineError;

/// Ruta relativa del binario de TeX Live dentro de la raíz portable.
const TEX_BIN_REL: &str = "tex/bin/windows";

/// Rutas relevantes de la instalación TeX Live.
#[derive(Debug, Clone)]
pub struct TexLivePaths {
    /// Raíz portable (la que contiene `tex/`, `agent/`, `workspace/`).
    pub root: Utf8PathBuf,
    /// `tex/bin/windows`.
    pub bin: Utf8PathBuf,
    /// `tex/bin/windows/latexmk.exe`.
    pub latexmk: Utf8PathBuf,
    /// `tex/bin/windows/synctex.exe`.
    pub synctex: Utf8PathBuf,
    /// `tex/bin/windows/tlmgr.bat`.
    pub tlmgr: Utf8PathBuf,
}

impl TexLivePaths {
    /// Construye el juego de rutas a partir del directorio `bin`.
    pub fn from_bin(bin: impl Into<Utf8PathBuf>) -> Self {
        let bin = bin.into();
        let root = bin
            .ancestors()
            .nth(3)
            .map(Utf8Path::to_owned)
            .unwrap_or_else(|| bin.clone());
        Self {
            root,
            latexmk: bin.join("latexmk.exe"),
            synctex: bin.join("synctex.exe"),
            tlmgr: bin.join("tlmgr.bat"),
            bin,
        }
    }

    /// Auto-detección (ver doc del módulo). Error con las rutas probadas si falla.
    pub fn detect() -> Result<Self, EngineError> {
        let mut tried: Vec<Utf8PathBuf> = Vec::new();

        if let Some(bin) = env_dir("LSX_TEX_BIN") {
            if has_latexmk(&bin) {
                return Ok(Self::from_bin(bin));
            }
            tried.push(bin);
        }
        if let Some(root) = env_dir("LATEX_STUDIO_ROOT") {
            let bin = root.join(TEX_BIN_REL);
            if has_latexmk(&bin) {
                return Ok(Self::from_bin(bin));
            }
            tried.push(bin);
        }
        if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR")
            && let Some(bin) = search_up(Utf8Path::new(manifest))
        {
            return Ok(Self::from_bin(bin));
        }
        if let Ok(exe) = std::env::current_exe()
            && let Ok(exe) = Utf8PathBuf::from_path_buf(exe)
        {
            let start = exe.parent().unwrap_or(Utf8Path::new("."));
            if let Some(bin) = search_up(start) {
                return Ok(Self::from_bin(bin));
            }
        }
        Err(EngineError::TexLiveNotFound { tried })
    }
}

fn has_latexmk(bin: &Utf8Path) -> bool {
    bin.join("latexmk.exe").is_file()
}

fn env_dir(var: &str) -> Option<Utf8PathBuf> {
    let value = std::env::var_os(var)?;
    Utf8PathBuf::from_path_buf(value.into()).ok()
}

/// Busca `<ancestro>/tex/bin/windows` subiendo hasta 8 niveles.
fn search_up(start: &Utf8Path) -> Option<Utf8PathBuf> {
    start
        .ancestors()
        .take(8)
        .map(|ancestor| ancestor.join(TEX_BIN_REL))
        .find(|bin| has_latexmk(bin))
}
