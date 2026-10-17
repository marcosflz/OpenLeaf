//! Supervisor del proceso `opencode serve` (INTERFACES §5.1/§5.4).
//!
//! M0-S4 dejó el arranque mínimo (puerto libre, health-poll, stop). M1-E1 añade
//! lo de producción:
//!
//! - **Job object kill-on-close** ([`crate::job`]): matar solo al padre dejaba
//!   huérfanos (los hijos de `opencode serve` sobreviven); con el job, cualquier
//!   salida de la app —incluido un crash— liquida el árbol completo.
//! - **Restart con backoff** ([`RestartPolicy`], [`ChatSupervisor`]): si el
//!   proceso muere (Task Manager, crash), se relanza con 0.5/1/2/5 s y el
//!   estado viaja como [`ServerState`] (contrato de INTERFACES §5.1).
//! - **Password por sesión**: `OPENCODE_SERVER_PASSWORD` + basic auth
//!   `opencode:{password}` (verificado live en M1-E: sin auth el server
//!   responde 401).
//! - **PATH**: se antepone `tex/bin/windows` y `agent/` para que el agente
//!   `latex-fix` encuentre `latexmk`/`tlmgr`/`opencode` (INTERFACES §5.4).

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::process::{Child, Command};

use crate::error::{ChatError, Result};
use crate::job::JobObject;
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
    /// `OPENCODE_SERVER_PASSWORD` (None → server sin auth, solo para spikes).
    pub password: Option<String>,
    /// Puerto preferido; si está ocupado se escanea el rango. `None` → aleatorio.
    pub port_preference: Option<u16>,
    /// Rango de escaneo (inclusive) si el preferido está ocupado.
    pub port_range: (u16, u16),
    /// Directorios que se anteponen al `PATH` del proceso.
    pub path_prepend: Vec<PathBuf>,
}

/// Estado del server observable por la UI (INTERFACES §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    /// Sin proceso.
    Stopped,
    /// Arrancando (primer intento).
    Starting,
    /// Listo y healthy, con la versión reportada por `/global/health`.
    Ready { port: u16, version: String },
    /// Relanzando tras una muerte inesperada.
    Restarting { attempt: u8 },
    /// Se agotaron los reintentos (o el arranque inicial falló).
    Error(String),
}

/// Política de reintentos del supervisor.
#[derive(Debug, Clone)]
pub struct RestartPolicy {
    /// Espera antes de cada reintento (0.5/1/2/5 s por defecto).
    pub delays: Vec<Duration>,
    /// Timeout del health-poll de cada arranque.
    pub health_timeout: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            delays: vec![
                Duration::from_millis(500),
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(5),
            ],
            health_timeout: Duration::from_secs(20),
        }
    }
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
    /// Job con kill-on-close; `None` si el SO no lo permitió (degradado).
    job: Option<JobObject>,
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

/// Puerto libre asignado por el SO (bind a `:0` y leer el puerto real).
pub fn pick_random_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Password aleatorio por sesión (sin dependencia de `rand`): entropía de
/// reloj + PID + dirección del heap, expandida con hashes de `DefaultHasher`.
pub fn random_password() -> String {
    let mut seed = String::new();
    seed.push_str(&format!("{}", std::process::id()));
    seed.push_str(&format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    seed.push_str(&format!("{:p}", &seed));
    let mut out = String::new();
    for round in 0..4u64 {
        use std::hash::{Hash as _, Hasher as _};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        seed.hash(&mut hasher);
        round.hash(&mut hasher);
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out
}

/// Arranca `opencode serve --hostname 127.0.0.1 --port {p}` en `spec.cwd` con
/// `OPENCODE_CONFIG`/`OPENCODE_SERVER_PASSWORD`, lo mete en un job con
/// kill-on-close y poletea `GET /global/health` hasta `health_timeout`.
pub async fn start(spec: &ServerSpec, health_timeout: Duration) -> Result<RunningServer> {
    let port = match spec.port_preference {
        Some(preference) => pick_free_port(preference, spec.port_range)?,
        None => pick_random_free_port()?,
    };
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
        // Red de seguridad (además del job): si el handle se cae sin stop().
        .kill_on_drop(true);
    if let Some(password) = &spec.password {
        command.env("OPENCODE_SERVER_PASSWORD", password);
    }
    if !spec.path_prepend.is_empty() {
        let mut path = std::ffi::OsString::new();
        for dir in &spec.path_prepend {
            path.push(dir.as_os_str());
            path.push(";");
        }
        path.push(std::env::var_os("PATH").unwrap_or_default());
        command.env("PATH", path);
    }

    let mut child = command.spawn().map_err(|source| ChatError::Spawn {
        exe: spec.exe.clone(),
        source,
    })?;
    let pid = child.id().unwrap_or(0);
    // Job con kill-on-close: todos los nietos de `opencode serve` nacen dentro.
    let job = attach_job(&child, pid);
    let base_url = format!("http://127.0.0.1:{port}");

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;
    let deadline = Instant::now() + health_timeout;
    loop {
        if Instant::now() >= deadline {
            terminate(&mut child, job.as_ref());
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
                job,
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

/// Asigna el proceso al job con kill-on-close (Windows). Fuera de Windows no
/// hay job objects: se devuelve `None` y queda `kill_on_drop` como red.
#[cfg(windows)]
fn attach_job(child: &Child, pid: u32) -> Option<JobObject> {
    match child.raw_handle() {
        Some(raw) => match JobObject::create().and_then(|job| job.assign(raw).map(|()| job)) {
            Ok(job) => Some(job),
            Err(error) => {
                tracing::warn!(%error, pid, "job object no disponible; se depende de kill_on_drop");
                None
            }
        },
        None => {
            tracing::warn!(pid, "sin handle del proceso; job object omitido");
            None
        }
    }
}

#[cfg(not(windows))]
fn attach_job(_child: &Child, _pid: u32) -> Option<JobObject> {
    None
}

/// Mata el árbol: primero el job (sincrónico para el SO), si no `taskkill /T`.
fn terminate(child: &mut Child, job: Option<&JobObject>) {
    if let Some(job) = job {
        job.terminate();
    } else {
        #[cfg(windows)]
        {
            let pid = child.id().map(|id| id.to_string()).unwrap_or_default();
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    let _ = child.start_kill();
}

impl RunningServer {
    /// `true` mientras el proceso raíz siga vivo (no toca al hijo).
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Mata el proceso (y su árbol) y espera su salida.
    pub async fn stop(mut self) -> Result<()> {
        terminate(&mut self.child, self.job.as_ref());
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
        // `self.job` cae aquí: kill-on-close por si quedara algún nieto.
    }
}

/// Supervisor con restart: envuelve [`start`] y relanza el server si muere.
pub struct ChatSupervisor {
    spec: ServerSpec,
    policy: RestartPolicy,
    server: Option<RunningServer>,
    /// Diagnóstico del último fallo (para el estado `Error`).
    last_error: Option<String>,
}

impl ChatSupervisor {
    /// Crea el supervisor (todavía sin arrancar el proceso).
    pub fn new(spec: ServerSpec, policy: RestartPolicy) -> Self {
        Self {
            spec,
            policy,
            server: None,
            last_error: None,
        }
    }

    /// Arranque explícito (primer intento, sin backoff). Idempotente si ya vive.
    pub async fn start(&mut self) -> Result<()> {
        if self.is_alive() {
            return Ok(());
        }
        match start(&self.spec, self.policy.health_timeout).await {
            Ok(server) => {
                self.server = Some(server);
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    /// `true` mientras el proceso raíz siga vivo.
    pub fn is_alive(&mut self) -> bool {
        self.server.as_mut().is_some_and(RunningServer::is_alive)
    }

    /// Datos del server vivo, si lo hay.
    pub fn server(&mut self) -> Option<&RunningServer> {
        if self.is_alive() {
            self.server.as_ref()
        } else {
            self.server = None;
            None
        }
    }

    pub fn base_url(&mut self) -> Option<String> {
        self.server().map(|server| server.base_url.clone())
    }

    /// Relanza el server con backoff (0.5/1/2/5 s por defecto).
    ///
    /// `on_state` recibe cada transición ([`ServerState`]) para que el servicio
    /// la reenvíe a la UI. Devuelve `Ok(())` cuando hay un server vivo.
    pub async fn ensure_alive<F>(&mut self, mut on_state: F) -> Result<()>
    where
        F: FnMut(ServerState),
    {
        if self.is_alive() {
            return Ok(());
        }
        self.server = None;
        let delays: Vec<Duration> = self.policy.delays.clone();
        let attempts = delays.len().max(1);
        let mut last = self
            .last_error
            .take()
            .unwrap_or_else(|| "el server no está vivo".to_owned());
        for attempt in 1..=attempts {
            let delay = delays.get(attempt - 1).copied().unwrap_or(Duration::ZERO);
            on_state(ServerState::Restarting {
                attempt: attempt as u8,
            });
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            match start(&self.spec, self.policy.health_timeout).await {
                Ok(server) => {
                    let state = ServerState::Ready {
                        port: server.port,
                        version: server.version.clone(),
                    };
                    self.server = Some(server);
                    self.last_error = None;
                    on_state(state);
                    return Ok(());
                }
                Err(error) => {
                    tracing::warn!(attempt, %error, "reintento de arranque fallido");
                    last = error.to_string();
                }
            }
        }
        self.last_error = Some(last.clone());
        Err(ChatError::RestartExhausted { attempts, last })
    }

    /// Para el server (si vive) y deja el supervisor en `Stopped`.
    pub async fn stop(&mut self) -> Result<()> {
        match self.server.take() {
            Some(server) => server.stop().await,
            None => Ok(()),
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

    #[test]
    fn random_port_is_free_and_nonzero() {
        let port = pick_random_free_port().expect("puerto aleatorio");
        assert_ne!(port, 0);
        assert!(port_free(port), "el puerto {port} debe quedar libre");
    }

    #[test]
    fn passwords_are_long_and_differ() {
        let one = random_password();
        let other = random_password();
        assert_eq!(one.len(), 64);
        assert!(one.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(one, other, "dos passwords consecutivos no deben coincidir");
    }

    #[test]
    fn restart_policy_default_matches_the_frozen_backoff() {
        let policy = RestartPolicy::default();
        assert_eq!(policy.delays.len(), 4);
        assert_eq!(policy.delays[0], Duration::from_millis(500));
        assert_eq!(policy.delays[3], Duration::from_secs(5));
    }

    #[tokio::test]
    async fn stopping_an_idle_supervisor_is_a_no_op() {
        let spec = ServerSpec {
            exe: PathBuf::from("no-existe.exe"),
            config_json: PathBuf::from("no.json"),
            cwd: PathBuf::from("."),
            password: None,
            port_preference: None,
            port_range: (15000, 15010),
            path_prepend: Vec::new(),
        };
        let mut supervisor = ChatSupervisor::new(spec, RestartPolicy::default());
        assert!(!supervisor.is_alive());
        assert!(supervisor.base_url().is_none());
        supervisor.stop().await.expect("stop sin server");
    }

    #[tokio::test]
    async fn ensure_alive_exhausts_backoff_and_reports_attempts() {
        let spec = ServerSpec {
            exe: PathBuf::from("no-existe.exe"),
            config_json: PathBuf::from("no.json"),
            cwd: PathBuf::from("."),
            password: None,
            port_preference: None,
            port_range: (15000, 15010),
            path_prepend: Vec::new(),
        };
        let mut supervisor = ChatSupervisor::new(
            spec,
            RestartPolicy {
                delays: vec![Duration::ZERO, Duration::ZERO],
                health_timeout: Duration::from_millis(50),
            },
        );
        let mut states = Vec::new();
        let error = supervisor
            .ensure_alive(|state| states.push(state))
            .await
            .expect_err("sin exe el restart debe agotarse");
        assert!(matches!(
            error,
            ChatError::RestartExhausted { attempts: 2, .. }
        ));
        assert_eq!(states.len(), 2);
        assert!(matches!(states[0], ServerState::Restarting { attempt: 1 }));
        assert!(matches!(states[1], ServerState::Restarting { attempt: 2 }));
    }
}
