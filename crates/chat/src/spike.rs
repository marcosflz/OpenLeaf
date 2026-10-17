//! Orquestación del spike M0-S4: server → catálogo → SSE → sesión.
//!
//! Uso típico (recorder y demo UI):
//! ```no_run
//! # async fn demo() -> anyhow::Result<()> {
//! use lsx_chat::spike::{SpikeChat, SpikeConfig};
//! let cfg = SpikeConfig::from_repo_root(std::path::Path::new("C:/ruta/al/repo"));
//! let (chat, info) = SpikeChat::start(&cfg, None).await?;
//! chat.send_prompt("Responde exactamente: hola").await?;
//! // los eventos llegan por chat.events() hasta SessionIdle
//! chat.stop().await?;
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_channel::{Receiver, Sender};
use serde_json::Value;

use crate::client::ApiClient;
use crate::error::{ChatError, Result};
use crate::events::{SpikeEvent, map_raw_event};
use crate::protocol::{ModelRef, PermissionReply, RawSseMessage, SessionInfo};
use crate::supervisor::{self, RunningServer, ServerSpec};

/// Modelo a usar en el spike.
#[derive(Debug, Clone)]
pub enum ModelChoice {
    /// Primer modelo disponible del provider (o su `default` del catálogo).
    ProviderDefault(String),
    /// Forzado por CLI.
    Explicit(ModelRef),
    /// Sin modelo (se envía el prompt sin `model` y el server decide).
    Skip,
}

/// Configuración del spike (defaults = layout del repo).
#[derive(Debug, Clone)]
pub struct SpikeConfig {
    pub repo_root: PathBuf,
    pub opencode_exe: PathBuf,
    pub config_json: PathBuf,
    pub project_cwd: PathBuf,
    pub session_title: String,
    pub password: Option<String>,
    pub port_preference: u16,
    pub port_range: (u16, u16),
    pub model: ModelChoice,
}

impl SpikeConfig {
    /// Defaults del repo: exe/config en `agent/`, proyecto en
    /// `fixtures/projects/cv`, puerto 14096 (rango 14000–14100).
    pub fn from_repo_root(repo_root: &Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            opencode_exe: repo_root.join("agent").join("opencode.exe"),
            config_json: repo_root.join("agent").join("opencode.json"),
            project_cwd: repo_root.join("fixtures").join("projects").join("cv"),
            session_title: "spike-s4".to_owned(),
            password: None,
            port_preference: 14096,
            port_range: (14000, 14100),
            model: ModelChoice::ProviderDefault("opencode-go".to_owned()),
        }
    }

    fn server_spec(&self) -> ServerSpec {
        ServerSpec {
            exe: self.opencode_exe.clone(),
            config_json: self.config_json.clone(),
            cwd: self.project_cwd.clone(),
            password: self.password.clone(),
            port_preference: Some(self.port_preference),
            port_range: self.port_range,
            path_prepend: vec![
                self.repo_root.join("tex").join("bin").join("windows"),
                self.repo_root.join("agent"),
            ],
        }
    }
}

/// Datos del arranque, para logs/UI.
#[derive(Debug, Clone)]
pub struct SpikeStartInfo {
    pub port: u16,
    pub version: String,
    pub session_id: String,
    pub directory: Option<String>,
    /// `None` si no había ningún modelo utilizable (flujo marcado, sin prompt).
    pub model: Option<ModelRef>,
}

/// Sesión viva con server, SSE y modelo resueltos.
pub struct SpikeChat {
    client: ApiClient,
    server: RunningServer,
    session: SessionInfo,
    model: Option<ModelRef>,
    events_rx: Receiver<SpikeEvent>,
    sse_task: tokio::task::JoinHandle<()>,
}

impl SpikeChat {
    /// Arranca el server, conecta `GET /event`, crea la sesión y resuelve el
    /// modelo. `raw_sink` (si se da) recibe el `data:` crudo de cada evento
    /// SSE — lo usa el grabador de fixtures.
    pub async fn start(
        cfg: &SpikeConfig,
        raw_sink: Option<Sender<RawSseMessage>>,
    ) -> Result<(Self, SpikeStartInfo)> {
        let server = supervisor::start(&cfg.server_spec(), Duration::from_secs(20)).await?;
        let mut guard = Cleanup::new(server);
        let client = ApiClient::new(guard.server_url(), cfg.password.clone())?;

        // 1) Catálogo de modelos (antes de conectar SSE para fallar rápido).
        let catalog = client.providers().await?;
        let model = match &cfg.model {
            ModelChoice::ProviderDefault(provider) => {
                let picked = catalog.pick_model(provider);
                if picked.is_none() {
                    tracing::warn!(
                        provider,
                        available = ?catalog.providers.iter().map(|p| &p.id).collect::<Vec<_>>(),
                        "sin modelos utilizables: se registrará el flujo sesión+SSE igualmente"
                    );
                }
                picked
            }
            ModelChoice::Explicit(model) => Some(model.clone()),
            ModelChoice::Skip => None,
        };

        // 2) SSE antes de crear la sesión (para capturar `session.created`).
        let stream = client.open_event_stream().await?;
        let (events_tx, events_rx) = async_channel::unbounded();
        guard.attach(tokio::spawn(sse_reader(stream, raw_sink, events_tx)));

        // 3) Espera `server.connected` (garantiza stream caliente).
        wait_first_connected(&events_rx, Duration::from_secs(10)).await?;

        // 4) Sesión scoped al proyecto activo (`?directory=`; verificado I-5).
        let session = client
            .create_session(&cfg.session_title, Some(&cfg.project_cwd))
            .await?;

        let info = SpikeStartInfo {
            port: guard.server().port,
            version: guard.server().version.clone(),
            session_id: session.id.clone(),
            directory: session.directory.clone(),
            model: model.clone(),
        };
        let (server, sse_task) = guard.disarm();
        Ok((
            Self {
                client,
                server,
                session,
                model,
                events_rx,
                sse_task,
            },
            info,
        ))
    }

    /// Puerto en uso por el server.
    pub fn port(&self) -> u16 {
        self.server.port
    }

    /// PID del proceso `opencode serve` (diagnóstico/limpieza).
    pub fn pid(&self) -> u32 {
        self.server.pid
    }

    pub fn session_id(&self) -> &str {
        &self.session.id
    }

    pub fn model(&self) -> Option<&ModelRef> {
        self.model.as_ref()
    }

    /// Receptor de eventos de dominio (un solo consumidor; en la UI demo
    /// se reenvían a GPUI).
    pub fn events(&self) -> Receiver<SpikeEvent> {
        self.events_rx.clone()
    }

    /// `POST prompt_async` con el modelo resuelto (o sin modelo si no hubo).
    pub async fn send_prompt(&self, text: &str) -> Result<()> {
        self.client
            .prompt_async(&self.session.id, self.model.as_ref(), text)
            .await
    }

    /// `POST abort`.
    pub async fn abort(&self) -> Result<()> {
        self.client.abort(&self.session.id).await
    }

    /// Responde un permiso (`once|always|reject`).
    pub async fn respond_permission(
        &self,
        request_id: &str,
        reply: PermissionReply,
    ) -> Result<bool> {
        self.client
            .respond_permission(&self.session.id, request_id, reply)
            .await
    }

    /// `GET /session/status` (para UI que no consume el stream de eventos).
    pub async fn session_status(&self) -> Result<Option<String>> {
        let statuses = self.client.session_statuses().await?;
        Ok(statuses
            .get(&self.session.id)
            .and_then(|s| s.get("type"))
            .and_then(Value::as_str)
            .map(str::to_owned))
    }

    /// Refetch del historial (recuperación tras gap de SSE; sin `Last-Event-ID`
    /// en 1.18.30).
    pub async fn refetch_history(&self, limit: usize) -> Result<Value> {
        self.client.session_messages(&self.session.id, limit).await
    }

    /// Cierra el SSE y mata el server (sin huérfanos).
    pub async fn stop(self) -> Result<()> {
        self.sse_task.abort();
        let _ = self.sse_task.await;
        self.server.stop().await
    }
}

async fn sse_reader(
    mut stream: crate::client::SseStream,
    raw_sink: Option<Sender<RawSseMessage>>,
    events_tx: Sender<SpikeEvent>,
) {
    loop {
        match stream.next().await {
            Ok(Some(event)) => {
                if let Some(raw_sink) = &raw_sink {
                    let message = RawSseMessage {
                        event: event.event.clone(),
                        id: event.id.clone(),
                        data: event.data.clone(),
                    };
                    if raw_sink.send(message).await.is_err() {
                        break;
                    }
                }
                let value: Value =
                    serde_json::from_str(&event.data).unwrap_or(Value::String(event.data.clone()));
                let mapped = map_raw_event(&value, event.event.as_deref());
                if events_tx.send(mapped).await.is_err() {
                    break;
                }
            }
            Ok(None) => {
                tracing::info!("SSE cerrado por el server");
                break;
            }
            Err(error) => {
                tracing::warn!(%error, "SSE interrumpido");
                break;
            }
        }
    }
}

/// Guard de limpieza del arranque: si algo falla a mitad, aborta el SSE y
/// deja morir el server (kill_on_drop).
struct Cleanup {
    server: Option<RunningServer>,
    sse: Option<tokio::task::JoinHandle<()>>,
}

impl Cleanup {
    fn new(server: RunningServer) -> Self {
        Self {
            server: Some(server),
            sse: None,
        }
    }

    fn server(&self) -> &RunningServer {
        self.server.as_ref().expect("guard armado")
    }

    fn server_url(&self) -> String {
        self.server().base_url.clone()
    }

    fn attach(&mut self, task: tokio::task::JoinHandle<()>) {
        self.sse = Some(task);
    }

    fn disarm(mut self) -> (RunningServer, tokio::task::JoinHandle<()>) {
        (
            self.server.take().expect("guard armado"),
            self.sse.take().expect("sse adjunto"),
        )
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(task) = self.sse.take() {
            task.abort();
        }
        // server: Drop del Child con kill_on_drop(true) → proceso fuera.
        self.server.take();
    }
}

async fn wait_first_connected(events: &Receiver<SpikeEvent>, timeout: Duration) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(ChatError::Timeout {
                what: "server.connected".to_owned(),
            });
        }
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(SpikeEvent::ServerConnected)) => return Ok(()),
            Ok(Ok(_)) => continue, // otros eventos previos: ignorar
            Ok(Err(_)) => {
                return Err(ChatError::Protocol(
                    "canal de eventos cerrado antes de server.connected".to_owned(),
                ));
            }
            Err(_) => {
                return Err(ChatError::Timeout {
                    what: "server.connected".to_owned(),
                });
            }
        }
    }
}

/// Resultado de esperar un evento con timeout.
#[derive(Debug)]
pub enum WaitOutcome<T> {
    Matched(T),
    TimedOut,
}

/// Espera (consumiendo el canal) hasta que `predicate` acepte un evento.
/// Los eventos que no casan se descartan — el grabador no pierde nada porque
/// graba el crudo por otro canal.
pub async fn wait_for_event<P>(
    events: &Receiver<SpikeEvent>,
    timeout: Duration,
    predicate: P,
) -> WaitOutcome<SpikeEvent>
where
    P: Fn(&SpikeEvent) -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return WaitOutcome::TimedOut;
        }
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(event)) if predicate(&event) => return WaitOutcome::Matched(event),
            Ok(Ok(_)) => continue,
            Ok(Err(_)) => return WaitOutcome::TimedOut, // canal cerrado
            Err(_) => return WaitOutcome::TimedOut,
        }
    }
}
