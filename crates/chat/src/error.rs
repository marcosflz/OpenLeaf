//! Errores de `lsx-chat`.
//!
//! Tipados y con mensaje apto para la UI (regla DoD del swarm).

use std::path::PathBuf;

/// Error unificado del agente LaTeX (proceso + RPC).
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    /// El intérprete del agente no se pudo lanzar.
    #[error("no se pudo arrancar el agente ({exe}): {source}")]
    Spawn {
        exe: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// El agente no respondió a `hello` a tiempo.
    #[error("el agente no respondió en {timeout_s}s")]
    HealthTimeout { timeout_s: u64 },

    /// Se agotó el backoff de reintentos del supervisor.
    #[error("no se pudo relanzar el agente tras {attempts} intentos: {last}")]
    RestartExhausted { attempts: usize, last: String },

    /// El agente devolvió un error a una petición.
    #[error("{0}")]
    Rpc(String),

    /// JSON inválido.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// Error de I/O local (fixtures, ficheros de log...).
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    /// El agente habló un protocolo que no entendemos o se cerró.
    #[error("protocolo: {0}")]
    Protocol(String),

    /// Se agotó un plazo esperando algo.
    #[error("timeout esperando {what}")]
    Timeout { what: String },
}

/// Alias de conveniencia.
pub type Result<T, E = ChatError> = std::result::Result<T, E>;
