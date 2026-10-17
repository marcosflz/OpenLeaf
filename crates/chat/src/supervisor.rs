//! Supervisor mínimo del proceso `opencode serve` (INTERFACES §5.1/§5.4).
//!
//! Spike M0-S4: arranque con puerto libre, health-poll, parada verificable.
//! La versión de producción (M1-E1) añade restart con backoff, job object
//! kill-on-close y password aleatorio por sesión.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};

use crate::error::{ChatError, Result};
use crate::protocol::Health;

/// Configuración de arranque del server.
#[derive(Debug, Clone)]
pub struct ServerSpec {
    /// Ruta a `agent\opencode.exe` (el del bundle; nunca el del PATH).
    pub exe: PathBuf,
    /// Ruta a `agent\opencode.json` (se exporta como `OPENCODE_CONFIG`).
    pub config_json: PathBuf,
    /// cwd = raíz del proyecto activo.
    pub cwd: PathBuf,
    /// `OPENCODE_SERVER_PASSWORD` (None en el spike → server sin auth).
    pub password: Option<String>,
    /// Puerto preferido; si está ocupado se escanea el rango.
    pub port_preference: u16,
    /// Rango de escaneo (inclusive) si el preferido está ocupado.
    pub port_range: (u16, u16),
}

/// Server en marcha, con la información para el cliente.
#[derive(Debug)]
pub struct RunningServer {
    pub port: u16,
    pub version: String,
    pub base_url: String,
    /// PID del proceso (para diagnósticos y verificación de limpieza).
    pub pid: u32,
    child: Child,
}

/// `true` si se puede escuchar en `127.0.0.1:port` (se libera al soltar).
pub fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Puerto libre: primero el preferido, si no el escaneo del rango.
pub fn pick_free_port(preference: u16, range: (u16, u16)) -> Result<u16> {
    let mut candidates = vec![preference];
    candidates.extend(range.0..=range.1);
    for port in candidates {
        if port_free(port) {
            return Ok(port);
        }
    }
    Err(ChatError::NoFreePort {
        start: range.0,
        end: range.1,
    })
}

/// Arranca `opencode serve --hostname 127.0.0.1 --port {p}` en `spec.cwd` con
/// `OPENCODE_CONFIG` y poletea `GET /global/health` hasta `health_timeout`.
pub async fn start(spec: &ServerSpec, health_timeout: Duration) -> Result<RunningServer> {
    let port = pick_free_port(spec.port_preference, spec.port_range)?;
    let mut command = Command::new(&spec.exe);
    command
        .arg("serve")
        .arg("--hostname")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .current_dir(&spec.cwd)
        .env("OPENCODE_CONFIG", &spec.config_json)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Red de seguridad: si el handle se cae sin stop(), matar al soltar.
        .kill_on_drop(true);
    if let Some(password) = &spec.password {
        command.env("OPENCODE_SERVER_PASSWORD", password);
    }

    let mut child = command.spawn().map_err(|source| ChatError::Spawn {
        exe: spec.exe.clone(),
        source,
    })?;
    let pid = child.id().unwrap_or(0);
    let base_url = format!("http://127.0.0.1:{port}");

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;
    let deadline = Instant::now() + health_timeout;
    loop {
        if Instant::now() >= deadline {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(ChatError::HealthTimeout {
                port,
                timeout_s: health_timeout.as_secs(),
            });
        }
        let mut request = http.get(format!("{base_url}/global/health"));
        if let Some(password) = &spec.password {
            request = request.basic_auth("opencode", Some(password));
        }
        if let Ok(response) = request.send().await
            && response.status().is_success()
            && let Ok(health) = response.json::<Health>().await
            && health.healthy
        {
            tracing::info!(port, pid, version = %health.version, "opencode serve listo");
            return Ok(RunningServer {
                port,
                version: health.version,
                base_url,
                pid,
                child,
            });
        }
        let waited = deadline.saturating_duration_since(Instant::now());
        let pause = if waited > health_timeout.saturating_sub(Duration::from_secs(2)) {
            Duration::from_millis(150)
        } else {
            Duration::from_millis(400)
        };
        tokio::time::sleep(pause).await;
    }
}

impl RunningServer {
    /// Mata el proceso y espera su salida (garantía de no-huérfanos).
    pub async fn stop(mut self) -> Result<()> {
        if let Err(error) = self.child.start_kill() {
            tracing::warn!(%error, pid = self.pid, "start_kill reportó error (puede estar ya muerto)");
        }
        match tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await {
            Ok(Ok(status)) => {
                tracing::info!(pid = self.pid, ?status, "opencode serve detenido");
                Ok(())
            }
            Ok(Err(error)) => Err(ChatError::Io(error)),
            Err(_) => Err(ChatError::Timeout {
                what: format!("kill del server (pid {})", self.pid),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_free_detects_held_listener() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind efímero");
        let port = listener.local_addr().expect("local_addr").port();
        assert!(!port_free(port), "el puerto {port} está ocupado");
        drop(listener);
        assert!(port_free(port), "el puerto {port} quedó libre");
    }

    #[test]
    fn pick_free_port_skips_busy_preference() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind efímero");
        let busy = listener.local_addr().expect("local_addr").port();
        let picked = pick_free_port(busy, (busy, busy.saturating_add(1)));
        match picked {
            Ok(port) => assert_ne!(port, busy),
            Err(ChatError::NoFreePort { .. }) => {
                // Caso extremo: busy+1 también ocupado (improbable).
            }
            Err(other) => panic!("error inesperado: {other}"),
        }
    }
}
