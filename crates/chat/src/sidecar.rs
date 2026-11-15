//! Proceso del agente LaTeX (`python -m lsx_agent`) y su cliente NDJSON.
//!
//! Sustituye a `opencode serve`: sin puertos, sin contraseña y sin SSE. Rust
//! y Python hablan por stdio, una línea JSON por mensaje:
//!
//! - petición `{"id", "method", "params"}` → respuesta `{"id", "result" | "error"}`;
//! - eventos `{"type", "properties"}` con la forma de la API v1 de opencode,
//!   que [`crate::events::map_raw_event`] traduce sin cambios (tabla I-4).
//!
//! Se mantiene lo de producción del supervisor anterior: job object con
//! kill-on-close ([`crate::job`]), restart con backoff 0.5/1/2/5 s y estados
//! [`ServerState`] para la UI. stderr del agente va a un archivo de log
//! (antes se descartaba).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_channel::{Receiver, Sender, unbounded};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

use crate::error::{ChatError, Result};
use crate::events::{SpikeEvent, map_raw_event};
use crate::job::JobObject;
use crate::protocol::{Health, ModelRef, PermissionReply, ProviderCatalog, SessionInfo};
use crate::recorder::FixtureWriter;

/// Cómo lanzar el agente.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    /// Intérprete: `agent\python\python.exe` del bundle (o `pythonOverride`).
    pub python: PathBuf,
    /// Entradas de `PYTHONPATH`: `agent\lib` (nas-adk y dependencias) y `agent`
    /// (paquete `lsx_agent`). El Python embebido las fija en su `._pth`.
    pub python_path: Vec<PathBuf>,
    /// cwd del proceso. **Nunca** el proyecto del usuario: Python importa desde
    /// el cwd y un `.py` del proyecto podría suplantar un módulo.
    pub cwd: PathBuf,
    /// `config/agent`: sesiones, transcript y checkpoints.
    pub data_dir: PathBuf,
    /// `config/secrets/ai.json` con las claves de proveedor.
    pub secrets: Option<PathBuf>,
    /// `assets/skills`.
    pub skills_dir: Option<PathBuf>,
    /// Log del agente (`logs/agent.log`).
    pub log_file: Option<PathBuf>,
    /// Directorios que se anteponen al `PATH` (`tex/bin/windows`).
    pub path_prepend: Vec<PathBuf>,
    /// Si existe, se graban los eventos en `spike-<stamp>.jsonl` (fixtures).
    pub record_dir: Option<PathBuf>,
}

/// Estado del proceso observable por la UI (INTERFACES §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    /// Sin proceso.
    Stopped,
    /// Arrancando (primer intento).
    Starting,
    /// Listo: versión de `lsx_agent` que respondió a `hello`.
    Ready { version: String },
    /// Relanzando tras una muerte inesperada.
    Restarting { attempt: u8 },
    /// Se agotaron los reintentos (o el arranque inicial falló).
    Error(String),
}

/// Política de reintentos.
#[derive(Debug, Clone)]
pub struct RestartPolicy {
    /// Espera antes de cada reintento (0.5/1/2/5 s por defecto).
    pub delays: Vec<Duration>,
    /// Plazo para que el agente responda a `hello`.
    pub hello_timeout: Duration,
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
            hello_timeout: Duration::from_secs(20),
        }
    }
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

/// Cliente RPC del agente. Barato de clonar; muere con el proceso.
#[derive(Clone)]
pub struct AgentClient {
    tx: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: Arc<AtomicU64>,
    timeout: Duration,
}

impl AgentClient {
    /// Llamada genérica: `method` + `params` → `result`.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (reply_tx, reply_rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| ChatError::Protocol("lock de peticiones envenenado".to_owned()))?
            .insert(id, reply_tx);
        let line = serde_json::to_string(&json!({"id": id, "method": method, "params": params}))?;
        if self.tx.send(line).is_err() {
            self.forget(id);
            return Err(ChatError::Protocol(
                "el agente no está en marcha".to_owned(),
            ));
        }
        match tokio::time::timeout(self.timeout, reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ChatError::Protocol(
                "el agente se cerró antes de responder".to_owned(),
            )),
            Err(_) => {
                self.forget(id);
                Err(ChatError::Timeout {
                    what: format!("respuesta del agente a `{method}`"),
                })
            }
        }
    }

    fn forget(&self, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }

    async fn call_as<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T> {
        Ok(serde_json::from_value(self.call(method, params).await?)?)
    }

    pub async fn health(&self) -> Result<Health> {
        self.call_as("hello", json!({})).await
    }

    pub async fn providers(&self) -> Result<ProviderCatalog> {
        self.call_as("config.providers", json!({})).await
    }

    /// Estado de todos los proveedores (configurados o no) para Ajustes › IA.
    pub async fn provider_status(&self) -> Result<Value> {
        self.call("provider.status", json!({})).await
    }

    pub async fn agents(&self) -> Result<Value> {
        self.call("agent.list", json!({})).await
    }

    pub async fn list_sessions(&self, directory: Option<&str>) -> Result<Vec<SessionInfo>> {
        self.call_as("session.list", json!({"directory": directory}))
            .await
    }

    pub async fn session(&self, id: &str) -> Result<SessionInfo> {
        self.call_as("session.get", json!({"id": id})).await
    }

    pub async fn create_session(&self, title: &str, directory: &str) -> Result<SessionInfo> {
        self.call_as(
            "session.create",
            json!({"title": title, "directory": directory}),
        )
        .await
    }

    pub async fn delete_session(&self, id: &str) -> Result<bool> {
        self.call_as("session.delete", json!({"id": id})).await
    }

    pub async fn prompt(
        &self,
        id: &str,
        model: Option<&ModelRef>,
        agent: &str,
        text: &str,
    ) -> Result<()> {
        self.call(
            "session.prompt",
            json!({"id": id, "model": model, "agent": agent, "text": text}),
        )
        .await
        .map(|_| ())
    }

    pub async fn abort(&self, id: &str) -> Result<()> {
        self.call("session.abort", json!({"id": id}))
            .await
            .map(|_| ())
    }

    pub async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        reply: PermissionReply,
    ) -> Result<()> {
        self.call(
            "permission.reply",
            json!({"sessionID": session_id, "requestID": request_id, "reply": reply.as_str()}),
        )
        .await
        .map(|_| ())
    }

    pub async fn revert(&self, id: &str, message_id: &str) -> Result<SessionInfo> {
        self.call_as("session.revert", json!({"id": id, "messageID": message_id}))
            .await
    }

    pub async fn unrevert(&self, id: &str) -> Result<SessionInfo> {
        self.call_as("session.unrevert", json!({"id": id})).await
    }

    /// `[{info, parts}]` de la sesión (misma forma que `GET /session/:id/message`).
    pub async fn session_messages(&self, id: &str, limit: usize) -> Result<Value> {
        self.call("session.messages", json!({"id": id, "limit": limit}))
            .await
    }
}

/// Proceso del agente en marcha.
pub struct RunningAgent {
    pub version: String,
    pub pid: u32,
    pub client: AgentClient,
    /// Eventos de dominio (cierra cuando el proceso termina).
    pub events: Receiver<SpikeEvent>,
    child: Child,
    job: Option<JobObject>,
}

impl RunningAgent {
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Cierra stdin (salida limpia) y, si no basta, mata el árbol.
    pub async fn stop(mut self) -> Result<()> {
        drop(self.client);
        match tokio::time::timeout(Duration::from_secs(3), self.child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            _ => {
                terminate(&mut self.child, self.job.as_ref());
                match tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(error)) => Err(ChatError::Io(error)),
                    Err(_) => Err(ChatError::Timeout {
                        what: format!("parada del agente (pid {})", self.pid),
                    }),
                }
            }
        }
    }
}

/// Lanza el agente, conecta stdio y espera a que responda `hello`.
pub async fn start(spec: &AgentSpec, hello_timeout: Duration) -> Result<RunningAgent> {
    std::fs::create_dir_all(&spec.data_dir)?;
    let mut command = Command::new(&spec.python);
    // -P: no anteponer el cwd a sys.path; -s: sin site-packages del usuario.
    command
        .arg("-P")
        .arg("-s")
        .arg("-m")
        .arg("lsx_agent")
        .arg("--data-dir")
        .arg(&spec.data_dir);
    if let Some(secrets) = &spec.secrets {
        command.arg("--secrets").arg(secrets);
    }
    if let Some(skills) = &spec.skills_dir {
        command.arg("--skills").arg(skills);
    }
    if let Some(log) = &spec.log_file {
        command.arg("--log").arg(log);
    }
    command
        .current_dir(&spec.cwd)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if !spec.python_path.is_empty() {
        command.env("PYTHONPATH", join_paths(&spec.python_path, None));
    }
    if !spec.path_prepend.is_empty() {
        command.env(
            "PATH",
            join_paths(&spec.path_prepend, std::env::var_os("PATH")),
        );
    }
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn().map_err(|source| ChatError::Spawn {
        exe: spec.python.clone(),
        source,
    })?;
    let pid = child.id().unwrap_or(0);
    let job = attach_job(&child, pid);
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| ChatError::Protocol("sin stdin del agente".to_owned()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ChatError::Protocol("sin stdout del agente".to_owned()))?;

    let (line_tx, line_rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(writer_task(stdin, line_rx));
    let pending: Pending = Arc::default();
    let (event_tx, event_rx) = unbounded::<SpikeEvent>();
    let recorder = spec.record_dir.as_ref().and_then(|dir| {
        FixtureWriter::create(
            dir,
            &crate::recorder::timestamp_utc(std::time::SystemTime::now()),
        )
        .map_err(|error| tracing::warn!(%error, "no se pudo crear la grabación"))
        .ok()
    });
    tokio::spawn(reader_task(stdout, pending.clone(), event_tx, recorder));

    let client = AgentClient {
        tx: line_tx,
        pending,
        next_id: Arc::new(AtomicU64::new(1)),
        timeout: hello_timeout,
    };
    let health = match client.health().await {
        Ok(health) => health,
        Err(error) => {
            terminate(&mut child, job.as_ref());
            let _ = child.wait().await;
            return Err(match error {
                ChatError::Timeout { .. } => ChatError::HealthTimeout {
                    timeout_s: hello_timeout.as_secs(),
                },
                other => other,
            });
        }
    };
    tracing::info!(pid, version = %health.version, "agente LaTeX listo");
    Ok(RunningAgent {
        version: health.version,
        pid,
        client: AgentClient {
            timeout: Duration::from_secs(60),
            ..client
        },
        events: event_rx,
        child,
        job,
    })
}

fn join_paths(dirs: &[PathBuf], tail: Option<std::ffi::OsString>) -> std::ffi::OsString {
    let separator = if cfg!(windows) { ";" } else { ":" };
    let mut joined = std::ffi::OsString::new();
    for (index, dir) in dirs.iter().enumerate() {
        if index > 0 {
            joined.push(separator);
        }
        joined.push(dir.as_os_str());
    }
    if let Some(tail) = tail {
        joined.push(separator);
        joined.push(tail);
    }
    joined
}

async fn writer_task(
    mut stdin: tokio::process::ChildStdin,
    mut lines: mpsc::UnboundedReceiver<String>,
) {
    while let Some(line) = lines.recv().await {
        if stdin.write_all(line.as_bytes()).await.is_err()
            || stdin.write_all(b"\n").await.is_err()
            || stdin.flush().await.is_err()
        {
            break;
        }
    }
    // Al soltar stdin, el agente ve EOF y termina limpio.
}

async fn reader_task(
    stdout: tokio::process::ChildStdout,
    pending: Pending,
    events: Sender<SpikeEvent>,
    mut recorder: Option<FixtureWriter>,
) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(%error, "stdout del agente ilegible");
                break;
            }
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            tracing::warn!(line = %truncate(&line), "línea no JSON del agente");
            continue;
        };
        if let Some(id) = value.get("id").and_then(Value::as_u64)
            && (value.get("result").is_some() || value.get("error").is_some())
        {
            let reply = match value.get("error") {
                Some(error) => Err(ChatError::Rpc(
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("error desconocido")
                        .to_owned(),
                )),
                None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
            };
            let sender = pending.lock().ok().and_then(|mut map| map.remove(&id));
            if let Some(sender) = sender {
                let _ = sender.send(reply);
            }
            continue;
        }
        if let Some(writer) = recorder.as_mut() {
            let _ = writer.write_raw_data(&line);
        }
        if events.send(map_raw_event(&value, None)).await.is_err() {
            break;
        }
    }
    if let Ok(mut map) = pending.lock() {
        for (_, sender) in map.drain() {
            let _ = sender.send(Err(ChatError::Protocol("el agente terminó".to_owned())));
        }
    }
    if let Some(writer) = recorder {
        match writer.finish(None) {
            Ok(summary) => tracing::info!(path = %summary.path.display(), "grabación guardada"),
            Err(error) => tracing::warn!(%error, "no se pudo cerrar la grabación"),
        }
    }
}

fn truncate(line: &str) -> String {
    crate::conversation::truncate_chars(line, 200)
}

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
        None => None,
    }
}

#[cfg(not(windows))]
fn attach_job(_child: &Child, _pid: u32) -> Option<JobObject> {
    None
}

fn terminate(child: &mut Child, job: Option<&JobObject>) {
    if let Some(job) = job {
        job.terminate();
    }
    let _ = child.start_kill();
}

/// Supervisor con restart: relanza el agente si muere.
pub struct AgentSupervisor {
    spec: AgentSpec,
    policy: RestartPolicy,
    agent: Option<RunningAgent>,
    last_error: Option<String>,
}

impl AgentSupervisor {
    pub fn new(spec: AgentSpec, policy: RestartPolicy) -> Self {
        Self {
            spec,
            policy,
            agent: None,
            last_error: None,
        }
    }

    /// Arranque explícito (sin backoff). Idempotente si ya vive.
    pub async fn start(&mut self) -> Result<()> {
        if self.is_alive() {
            return Ok(());
        }
        match start(&self.spec, self.policy.hello_timeout).await {
            Ok(agent) => {
                self.agent = Some(agent);
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub fn is_alive(&mut self) -> bool {
        self.agent.as_mut().is_some_and(RunningAgent::is_alive)
    }

    /// Agente vivo, si lo hay.
    pub fn agent(&mut self) -> Option<&RunningAgent> {
        if self.is_alive() {
            self.agent.as_ref()
        } else {
            self.agent = None;
            None
        }
    }

    /// Relanza con backoff; `on_state` recibe cada transición.
    pub async fn ensure_alive<F>(&mut self, mut on_state: F) -> Result<()>
    where
        F: FnMut(ServerState),
    {
        if self.is_alive() {
            return Ok(());
        }
        self.agent = None;
        let delays = self.policy.delays.clone();
        let attempts = delays.len().max(1);
        let mut last = self
            .last_error
            .take()
            .unwrap_or_else(|| "el agente no está en marcha".to_owned());
        for attempt in 1..=attempts {
            on_state(ServerState::Restarting {
                attempt: attempt as u8,
            });
            if let Some(delay) = delays.get(attempt - 1).filter(|d| !d.is_zero()) {
                tokio::time::sleep(*delay).await;
            }
            match start(&self.spec, self.policy.hello_timeout).await {
                Ok(agent) => {
                    let state = ServerState::Ready {
                        version: agent.version.clone(),
                    };
                    self.agent = Some(agent);
                    on_state(state);
                    return Ok(());
                }
                Err(error) => {
                    tracing::warn!(attempt, %error, "reintento de arranque del agente fallido");
                    last = error.to_string();
                }
            }
        }
        self.last_error = Some(last.clone());
        Err(ChatError::RestartExhausted { attempts, last })
    }

    pub async fn stop(&mut self) -> Result<()> {
        match self.agent.take() {
            Some(agent) => agent.stop().await,
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_policy_default_matches_the_frozen_backoff() {
        let policy = RestartPolicy::default();
        assert_eq!(
            policy.delays,
            vec![
                Duration::from_millis(500),
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(5),
            ]
        );
    }

    #[test]
    fn join_paths_uses_platform_separator() {
        let joined = join_paths(&[PathBuf::from("a"), PathBuf::from("b")], Some("c".into()));
        let separator = if cfg!(windows) { ";" } else { ":" };
        assert_eq!(
            joined,
            std::ffi::OsString::from(format!("a{separator}b{separator}c"))
        );
    }

    #[tokio::test]
    async fn missing_python_reports_spawn_error() {
        let spec = AgentSpec {
            python: PathBuf::from("python-que-no-existe.exe"),
            python_path: Vec::new(),
            cwd: std::env::temp_dir(),
            data_dir: std::env::temp_dir().join("lsx-agent-test"),
            secrets: None,
            skills_dir: None,
            log_file: None,
            path_prepend: Vec::new(),
            record_dir: None,
        };
        let error = start(&spec, Duration::from_secs(1))
            .await
            .err()
            .expect("debe fallar");
        assert!(matches!(error, ChatError::Spawn { .. }), "{error}");
    }

    #[tokio::test]
    async fn ensure_alive_exhausts_backoff_and_reports_attempts() {
        let spec = AgentSpec {
            python: PathBuf::from("python-que-no-existe.exe"),
            python_path: Vec::new(),
            cwd: std::env::temp_dir(),
            data_dir: std::env::temp_dir().join("lsx-agent-test"),
            secrets: None,
            skills_dir: None,
            log_file: None,
            path_prepend: Vec::new(),
            record_dir: None,
        };
        let policy = RestartPolicy {
            delays: vec![Duration::ZERO, Duration::ZERO],
            hello_timeout: Duration::from_secs(1),
        };
        let mut supervisor = AgentSupervisor::new(spec, policy);
        let mut states = Vec::new();
        let result = supervisor.ensure_alive(|state| states.push(state)).await;
        assert!(matches!(
            result,
            Err(ChatError::RestartExhausted { attempts: 2, .. })
        ));
        assert_eq!(
            states,
            vec![
                ServerState::Restarting { attempt: 1 },
                ServerState::Restarting { attempt: 2 }
            ]
        );
    }
}
