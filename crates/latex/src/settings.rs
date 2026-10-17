//! Sección `latex` de `config/settings.json` (INTERFACES §7) y su traducción a
//! [`CompileRequest`].
//!
//! El schema completo vive en `lsx-core`; este tipo es el **contrato del motor**
//! para que el shell (stream A/F) pueda pasar la sección `latex` tal cual llegue
//! del JSON (`serde` con `camelCase` y `default` por campo: un settings viejo o
//! parcial nunca rompe la carga).
//!
//! Rangos seguros (validación por campo, §7): `timeoutSecs` ∈ [5, 3600] y
//! `debounceMs` ≤ 10 s; el resto se usa tal cual.

use std::time::Duration;

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::engine::{CompileRequest, DEFAULT_ARGS, EngineId};

/// `latex.debounceMs` por defecto (§7).
pub const DEFAULT_DEBOUNCE_MS: u64 = 400;
/// `latex.timeoutSecs` por defecto (§7).
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;
/// Rango aceptado de `timeoutSecs`.
pub const TIMEOUT_SECS_RANGE: (u64, u64) = (5, 3600);
/// Tope de `debounceMs` (evita bloqueos mentales del usuario con un valor absurdo).
pub const MAX_DEBOUNCE_MS: u64 = 10_000;

/// Sección `latex` de los settings (defaults completos del §7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LatexSettings {
    /// `texlive` (default) o `tectonic` (M4; aquí solo se detecta el valor).
    pub engine: String,
    /// Ruta del `main.tex` configurada a mano (`null` → heurística M1-D5).
    pub main_file: Option<Utf8PathBuf>,
    /// Recompilar al guardar (con el debounce de `debounce_ms`).
    pub auto_compile: bool,
    /// Milisegundos de espera tras el último cambio (`latex.debounceMs`).
    pub debounce_ms: u64,
    /// Permitir que el motor instale paquetes faltantes con `tlmgr` (tope 3).
    pub auto_install_packages: bool,
    /// Args extra **añadidos** a los de por defecto.
    pub extra_args: Vec<String>,
    /// Timeout de cada compilación en segundos.
    pub timeout_secs: u64,
    /// Ejecutar `latexmk -c` tras una compilación correcta.
    pub clean_on_success: bool,
}

impl Default for LatexSettings {
    fn default() -> Self {
        Self {
            engine: "texlive".to_string(),
            main_file: None,
            auto_compile: true,
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            auto_install_packages: true,
            extra_args: Vec::new(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            clean_on_success: false,
        }
    }
}

impl LatexSettings {
    /// Motor seleccionado (cualquier valor desconocido cae a TeX Live).
    pub fn engine_id(&self) -> EngineId {
        if self.engine.eq_ignore_ascii_case("tectonic") {
            EngineId::Tectonic
        } else {
            EngineId::TexLive
        }
    }

    /// Timeout saneado (rango [`TIMEOUT_SECS_RANGE`]).
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(
            self.timeout_secs
                .clamp(TIMEOUT_SECS_RANGE.0, TIMEOUT_SECS_RANGE.1),
        )
    }

    /// Debounce saneado (≤ [`MAX_DEBOUNCE_MS`]).
    pub fn debounce(&self) -> Duration {
        Duration::from_millis(self.debounce_ms.min(MAX_DEBOUNCE_MS))
    }

    /// Petición de compilación con `DEFAULT_ARGS + extraArgs`, timeout y
    /// auto-install de esta sección. `out_dir` lo decide el llamador.
    pub fn request(
        &self,
        project: impl Into<Utf8PathBuf>,
        main: impl Into<Utf8PathBuf>,
    ) -> CompileRequest {
        let mut request = CompileRequest::new(project, main);
        request.args = DEFAULT_ARGS
            .iter()
            .map(|arg| (*arg).to_string())
            .chain(self.extra_args.iter().cloned())
            .collect();
        request.timeout = self.timeout();
        request.auto_install = self.auto_install_packages;
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_del_esquema_7() {
        let settings = LatexSettings::default();
        assert_eq!(settings.engine, "texlive");
        assert!(settings.auto_compile);
        assert_eq!(settings.debounce_ms, 400);
        assert_eq!(settings.debounce(), Duration::from_millis(400));
        assert!(settings.auto_install_packages);
        assert!(settings.extra_args.is_empty());
        assert_eq!(settings.timeout_secs, 600);
        assert_eq!(settings.timeout(), Duration::from_secs(600));
        assert!(!settings.clean_on_success);
        assert!(settings.main_file.is_none());
    }

    #[test]
    fn json_parcial_usa_defaults_y_camel_case() {
        let settings: LatexSettings =
            serde_json::from_str(r#"{ "debounceMs": 250, "extraArgs": ["-shell-escape"] }"#)
                .expect("json válido");
        assert_eq!(settings.debounce_ms, 250);
        assert_eq!(settings.extra_args, vec!["-shell-escape".to_string()]);
        assert_eq!(settings.timeout_secs, 600);
        assert!(settings.auto_compile);
    }

    #[test]
    fn json_completo_round_trip() {
        let settings = LatexSettings {
            engine: "texlive".to_string(),
            main_file: Some(Utf8PathBuf::from("src/main.tex")),
            auto_compile: false,
            debounce_ms: 1000,
            auto_install_packages: false,
            extra_args: vec!["-shell-escape".to_string(), "-halt-on-error".to_string()],
            timeout_secs: 900,
            clean_on_success: true,
        };
        let json = serde_json::to_string(&settings).expect("serializa");
        let back: LatexSettings = serde_json::from_str(&json).expect("deserializa");
        assert_eq!(back, settings);
    }

    #[test]
    fn rangos_se_sanean() {
        let too_small = LatexSettings {
            timeout_secs: 1,
            ..LatexSettings::default()
        };
        assert_eq!(too_small.timeout(), Duration::from_secs(5));
        let too_big = LatexSettings {
            timeout_secs: 99_999,
            ..LatexSettings::default()
        };
        assert_eq!(too_big.timeout(), Duration::from_secs(3600));
        let debounce_absurdo = LatexSettings {
            debounce_ms: 99_999,
            ..LatexSettings::default()
        };
        assert_eq!(debounce_absurdo.debounce(), Duration::from_millis(10_000));
    }

    #[test]
    fn request_compone_defaults_mas_extra_args() {
        let settings = LatexSettings {
            extra_args: vec!["-shell-escape".to_string()],
            timeout_secs: 42,
            auto_install_packages: false,
            ..LatexSettings::default()
        };
        let request = settings.request("C:/proyecto", "C:/proyecto/main.tex");
        assert_eq!(
            request.args,
            vec![
                "-pdf",
                "-synctex=1",
                "-interaction=nonstopmode",
                "-file-line-error",
                "-shell-escape",
            ]
        );
        assert_eq!(request.timeout, Duration::from_secs(42));
        assert!(!request.auto_install);
    }

    #[test]
    fn engine_id_desconocido_cae_a_texlive() {
        let mut settings = LatexSettings::default();
        assert_eq!(settings.engine_id(), EngineId::TexLive);
        settings.engine = "tectonic".to_string();
        assert_eq!(settings.engine_id(), EngineId::Tectonic);
        settings.engine = "lo-que-sea".to_string();
        assert_eq!(settings.engine_id(), EngineId::TexLive);
    }
}
