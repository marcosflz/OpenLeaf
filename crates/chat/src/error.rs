//! Errores de `lsx-chat`.
//!
//! Tipados y con mensaje apto para la UI (regla DoD del swarm).

use std::path::PathBuf;

/// Error unificado del supervisor/cliente de opencode.
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    /// No hay ningún puerto libre en el rango configurado.
    #[error("no hay puerto libre en el rango {start}..={end}")]
    NoFreePort { start: u16, end: u16 },

    /// El proceso `opencode.exe` no se pudo lanzar.
    #[error("no se pudo arrancar opencode ({exe}): {source}")]
    Spawn {
        exe: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// El server no llegó a `healthy` en el tiempo de espera.
    #[error("el server no respondió healthy en {timeout_s}s (puerto {port})")]
    HealthTimeout { port: u16, timeout_s: u64 },

    /// Se agotó el backoff de reintentos del supervisor (M1-E1).
    #[error("no se pudo relanzar opencode tras {attempts} intentos: {last}")]
    RestartExhausted { attempts: usize, last: String },

    /// Error de transporte HTTP.
    #[error("HTTP: {0}")]
    Http(#[from] reqwest::Error),

    /// El server respondió con un status inesperado.
    #[error("respuesta inesperada de {route}: HTTP {status}")]
    UnexpectedStatus { route: String, status: u16 },

    /// JSON inválido.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// Error de I/O local (fixtures, ficheros de log...).
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    /// El server habló un protocolo que no entendemos (deriva v1→v2, R5).
    #[error("protocolo: {0}")]
    Protocol(String),

    /// Se agotó un plazo esperando algo.
    #[error("timeout esperando {what}")]
    Timeout { what: String },
}

/// Alias de conveniencia.
pub type Result<T, E = ChatError> = std::result::Result<T, E>;
