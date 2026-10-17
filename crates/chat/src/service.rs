//! Servicio de chat de producción (M1-E2): hilo propio con runtime tokio que
//! posee el supervisor, el cliente REST/SSE y la sesión activa.
//!
//! Patrón del skill `lsx-gpui-patterns`: la UI nunca bloquea; los comandos
//! viajan por `async_channel` ([`ChatCmd`]) y los eventos vuelven por otro
//! canal ([`ChatEvent`]) que el panel reduce a [`crate::conversation`].
//!
//! Responsabilidades:
//! - arranque perezoso del server (`Start`) y restart con backoff si muere;
//! - reconexión SSE 1/2/5 s y **refetch de historial** tras el gap (la API v1
//!   no soporta `Last-Event-ID`, hallazgo M0-S4);
//! - sesiones por proyecto (`POST /session?directory=`), cambio/borrado,
//!   modelo y agente por sesión, abort y permisos.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use async_channel::{Receiver, Sender, unbounded};
use serde_json::Value;

use crate::client::ApiClient;
use crate::error::{ChatError, Result};
use crate::events::{SpikeEvent, history_events, map_raw_event};
use crate::protocol::{
    AgentInfo, ModelRef, PermissionReply, ProviderCatalog, SessionInfo, parse_agents,
};
use crate::recorder::timestamp_utc;
use crate::supervisor::{ChatSupervisor, RestartPolicy, ServerSpec, ServerState};

/// Configuración del servicio (la construye la UI desde `settings.json`).
#[derive(Debug, Clone)]
pub struct ChatConfig {
    /// `agent\opencode.exe` del bundle.
    pub exe: PathBuf,
    /// `agent\opencode.json`.
    pub config_json: PathBuf,
    /// Raíz del proyecto activo (cwd del server y `?directory=` de las sesiones).
    pub project_cwd: PathBuf,
    /// Nombre visible del proyecto (título de sesiones nuevas).
    pub project_name: String,
    /// Puerto fijo de `ai.port`; `None` → puerto libre aleatorio.
    pub port: Option<u16>,
    /// `OPENCODE_SERVER_PASSWORD` (None → sin auth; solo pruebas).
    pub password: Option<String>,
    /// Modelo preferido `provider/model` de `ai.model`.
    pub model: Option<String>,
    /// Agente por defecto de `ai.agent`.
    pub agent: String,
    /// Timeout del health-poll de cada arranque.
    pub health_timeout: Duration,
    /// Backoff del restart del server.
    pub restart_delays: Vec<Duration>,
    /// Delays de reconexión del SSE (1/2/5 s).
    pub reconnect_delays: Vec<Duration>,
    /// Límite de mensajes del refetch de historial.
    pub history_limit: usize,
    /// `tex/bin/windows` y `agent` para el PATH del server.
    pub path_prepend: Vec<PathBuf>,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            exe: PathBuf::from("agent/opencode.exe"),
            config_json: PathBuf::from("agent/opencode.json"),
            project_cwd: PathBuf::from("."),
            project_name: "LaTeX Studio".to_owned(),
            port: None,
            password: None,
            model: None,
            agent: "build".to_owned(),
            health_timeout: Duration::from_secs(20),
            restart_delays: vec![
                Duration::from_millis(500),
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(5),
            ],
            reconnect_delays: vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(5),
            ],
            history_limit: 200,
            path_prepend: Vec::new(),
        }
    }
}

/// Comandos UI → servicio.
#[derive(Debug, Clone)]
pub enum ChatCmd {
    /// Arranca el server si no está vivo (idempotente).
    Start,
    /// Detiene server y worker (cierre de la app).
    Stop,
    /// Crea una sesión nueva en el proyecto y la activa.
    NewSession,
    /// Cambia a una sesión existente (refetch de historial).
    SwitchSession { id: String },
    /// Borra una sesión del server.
    DeleteSession { id: String },
    /// Recarga la lista de sesiones del proyecto.
    ListSessions,
    /// Recarga `/config/providers`.
    RefreshProviders,
    /// Fija el modelo de la sesión activa.
    SetModel { provider: String, model: String },
    /// Fija el agente de la sesión activa.
    SetAgent { name: String },
    /// Envía un prompt a la sesión activa (la crea si no existe).
    /// `agent` (opcional) fuerza un agente distinto al vigente (`latex-fix`).
    Prompt { text: String, agent: Option<String> },
    /// `POST abort` del turno en curso.
    Abort,
    /// Responde un permiso pendiente.
    Permission {
        request_id: String,
        reply: PermissionReply,
    },
}

/// Eventos servicio → UI.
#[derive(Debug)]
pub enum ChatEvent {
    /// Transición del proceso (`Stopped/Starting/Ready/Restarting/Error`).
    Server(ServerState),
    /// Servidor listo: datos para el header/pickers.
    Ready {
        port: u16,
        version: String,
        model: Option<ModelRef>,
        agent: String,
    },
    /// Sesión activada (creada o cambiada); el panel limpia el transcript.
    SessionActivated { session: SessionInfo },
    /// No hay sesión activa (borrada o perdida tras un restart).
    SessionCleared,
    /// Evento de dominio de la tabla I-4.
    Stream(Box<SpikeEvent>),
    /// Historial reconstruido (`GET /session/:id/message`) tras una reconexión.
    History {
        session_id: String,
        events: Vec<SpikeEvent>,
    },
    /// Lista de sesiones del proyecto (más reciente primero).
    Sessions(Vec<SessionInfo>),
    /// Catálogo de proveedores/modelos.
    Providers(ProviderCatalog),
    /// Agentes disponibles (`GET /agent`).
    Agents(Vec<AgentInfo>),
    /// Modelo/agente vigentes tras un cambio explícito.
    SelectionChanged {
        model: Option<ModelRef>,
        agent: String,
    },
    /// Error mostrable; `recoverable` indica que el servicio sigue vivo.
    Error { message: String, recoverable: bool },
    /// El worker terminó (server parado).
    Stopped,
}

/// Handle del servicio: canales + hilo propietario.
pub struct ChatService {
    /// Comandos hacia el servicio.
    pub cmd: Sender<ChatCmd>,
    /// Eventos del servicio.
    pub events: Receiver<ChatEvent>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ChatService {
    /// Pide el cierre del server sin bloquear (seguro desde el hilo UI).
    pub fn shutdown(&self) {
        let _ = self.cmd.try_send(ChatCmd::Stop);
    }

    /// Espera a que el worker termine (tests y cierre ordenado).
    pub fn join(mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ChatService {
    fn drop(&mut self) {
        // Red de seguridad: si el panel se suelta sin join, el worker para el
        // server al recibir Stop; el job object cubre el peor caso.
        let _ = self.cmd.try_send(ChatCmd::Stop);
    }
}

/// Lanza el worker y devuelve el handle con sus canales.
pub fn spawn(config: ChatConfig) -> ChatService {
    let (cmd_tx, cmd_rx) = unbounded::<ChatCmd>();
    let (event_tx, event_rx) = unbounded::<ChatEvent>();
    let handle = std::thread::Builder::new()
        .name("lsx-chat-worker".to_owned())
        .spawn(move || worker_main(config, cmd_rx, event_tx))
        .expect("no se pudo crear el hilo del servicio de chat");
    ChatService {
        cmd: cmd_tx,
        events: event_rx,
        handle: Some(handle),
    }
}

fn worker_main(config: ChatConfig, cmd_rx: Receiver<ChatCmd>, out: Sender<ChatEvent>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = out.send_blocking(ChatEvent::Error {
                message: format!("no se pudo crear el runtime tokio: {error}"),
                recoverable: false,
            });
            let _ = out.send_blocking(ChatEvent::Stopped);
            return;
        }
    };
    runtime.block_on(async move {
        let mut worker = Worker::new(config);
        worker.run(cmd_rx, out.clone()).await;
        let _ = out.send(ChatEvent::Stopped).await;
    });
    runtime.shutdown_timeout(Duration::from_secs(2));
}

struct Worker {
    config: ChatConfig,
    supervisor: ChatSupervisor,
    client: Option<ApiClient>,
    session: Option<SessionInfo>,
    model: Option<ModelRef>,
    agent: String,
    catalog: Option<ProviderCatalog>,
    /// `true` tras un `Start` correcto (o restart en curso).
    started: bool,
    /// `true` mientras la tarea SSE actual está viva.
    sse_open: bool,
    sse_task: Option<tokio::task::JoinHandle<()>>,
    events_rx: Receiver<SpikeEvent>,
    /// Reintento de reconexión SSE (índice dentro de `reconnect_delays`).
    reconnect_attempt: usize,
}

impl Worker {
    fn new(config: ChatConfig) -> Self {
        let spec = ServerSpec {
            exe: config.exe.clone(),
            config_json: config.config_json.clone(),
            cwd: config.project_cwd.clone(),
            password: config.password.clone(),
            port_preference: config.port,
            port_range: (14000, 14100),
            path_prepend: config.path_prepend.clone(),
        };
        let policy = RestartPolicy {
            delays: config.restart_delays.clone(),
            health_timeout: config.health_timeout,
        };
        let (_events_tx, events_rx) = unbounded::<SpikeEvent>();
        Self {
            agent: config.agent.clone(),
            config,
            supervisor: ChatSupervisor::new(spec, policy),
            client: None,
            session: None,
            model: None,
            catalog: None,
            started: false,
            sse_open: false,
            sse_task: None,
            events_rx,
            reconnect_attempt: 0,
        }
    }

    async fn run(&mut self, cmd_rx: Receiver<ChatCmd>, out: Sender<ChatEvent>) {
        loop {
            // Reconexión pendiente (SSE caído o server muerto).
            if self.started && !self.sse_open {
                self.reconnect(&out).await;
            }
            tokio::select! {
                command = cmd_rx.recv() => {
                    match command {
                        Ok(command) => {
                            if !self.handle_command(command, &out).await {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                event = self.events_rx.recv(), if self.sse_open => {
                    match event {
                        Ok(event) => {
                            let _ = out.send(ChatEvent::Stream(Box::new(event))).await;
                        }
                        Err(_) => {
                            tracing::warn!("SSE cerrado por el server; se reconectará");
                            self.sse_open = false;
                            self.sse_task = None;
                        }
                    }
                }
            }
        }
        self.stop(&out).await;
    }

    /// Procesa un comando; `false` = parar el worker.
    async fn handle_command(&mut self, command: ChatCmd, out: &Sender<ChatEvent>) -> bool {
        match command {
            ChatCmd::Start => {
                if !self.started {
                    self.start(out).await;
                }
            }
            ChatCmd::Stop => return false,
            ChatCmd::NewSession => {
                if !self.started {
                    self.start(out).await;
                }
                if self.client.is_some() {
                    self.create_session(out).await;
                }
            }
            ChatCmd::SwitchSession { id } => {
                self.switch_session(&id, out).await;
            }
            ChatCmd::DeleteSession { id } => {
                self.delete_session(&id, out).await;
            }
            ChatCmd::ListSessions => {
                self.refresh_sessions(out).await;
            }
            ChatCmd::RefreshProviders => {
                self.refresh_providers(out).await;
            }
            ChatCmd::SetModel { provider, model } => {
                self.model = Some(ModelRef::new(provider, model));
                let _ = out
                    .send(ChatEvent::SelectionChanged {
                        model: self.model.clone(),
                        agent: self.agent.clone(),
                    })
                    .await;
            }
            ChatCmd::SetAgent { name } => {
                if !name.trim().is_empty() {
                    self.agent = name;
                }
                let _ = out
                    .send(ChatEvent::SelectionChanged {
                        model: self.model.clone(),
                        agent: self.agent.clone(),
                    })
                    .await;
            }
            ChatCmd::Prompt { text, agent } => {
                if !self.started {
                    self.start(out).await;
                }
                if self.session.is_none() {
                    self.create_session(out).await;
                }
                self.send_prompt(&text, agent.as_deref(), out).await;
            }
            ChatCmd::Abort => {
                self.abort(out).await;
            }
            ChatCmd::Permission { request_id, reply } => {
                self.respond_permission(&request_id, reply, out).await;
            }
        }
        true
    }

    // --- arranque / reconexión -------------------------------------------------

    async fn start(&mut self, out: &Sender<ChatEvent>) {
        let _ = out.send(ChatEvent::Server(ServerState::Starting)).await;
        match self.supervisor.start().await {
            Ok(()) => {
                self.started = true;
                self.reconnect_attempt = 0;
                self.build_client();
                if let Err(error) = self.connect_sse(out).await {
                    tracing::warn!(%error, "SSE no disponible tras el arranque; se reintentará");
                }
                self.refresh_providers(out).await;
                self.refresh_agents(out).await;
                self.refresh_sessions(out).await;
                self.resolve_initial_model();
                let _ = out
                    .send(ChatEvent::SelectionChanged {
                        model: self.model.clone(),
                        agent: self.agent.clone(),
                    })
                    .await;
                self.emit_ready(out);
            }
            Err(error) => {
                let _ = out
                    .send(ChatEvent::Server(ServerState::Error(error.to_string())))
                    .await;
                let _ = out
                    .send(ChatEvent::Error {
                        message: error.to_string(),
                        recoverable: true,
                    })
                    .await;
            }
        }
    }

    /// Reconecta SSE: primero asegura el server (restart con backoff), después
    /// la conexión del stream (1/2/5 s) y el refetch de historial.
    ///
    /// Un health-check decide si el server está realmente vivo: `opencode
    /// serve` puede mantener el proceso padre en pie aunque el hijo que
    /// escucha haya muerto (visto live en M1-E1), así que la liveness del
    /// proceso raíz no basta.
    async fn reconnect(&mut self, out: &Sender<ChatEvent>) {
        let root_alive = self.supervisor.is_alive();
        let healthy = match &self.client {
            Some(client) => client.health().await.is_ok(),
            None => false,
        };
        if !root_alive || !healthy {
            if root_alive {
                tracing::warn!(
                    "el proceso padre sigue vivo pero el server no responde al health; se relanza"
                );
                let _ = self.supervisor.stop().await;
            }
            let tx = out.clone();
            let result = self
                .supervisor
                .ensure_alive(move |state| {
                    let _ = tx.try_send(ChatEvent::Server(state));
                })
                .await;
            match result {
                Ok(()) => {
                    self.build_client();
                    // El server puede estar en otro puerto tras el restart.
                    if self.session.is_some() {
                        self.refetch_history(out).await;
                    }
                }
                Err(error) => {
                    let _ = out
                        .send(ChatEvent::Server(ServerState::Error(error.to_string())))
                        .await;
                    let _ = out
                        .send(ChatEvent::Error {
                            message: error.to_string(),
                            recoverable: true,
                        })
                        .await;
                    self.started = false;
                    return;
                }
            }
        } else if self.client.is_none() {
            self.build_client();
        }

        match self.connect_sse(out).await {
            Ok(()) => {
                self.reconnect_attempt = 0;
                self.emit_ready(out);
                if self.session.is_some() {
                    self.refetch_history(out).await;
                }
            }
            Err(error) => {
                let delay = self
                    .config
                    .reconnect_delays
                    .get(
                        self.reconnect_attempt
                            .min(self.config.reconnect_delays.len().saturating_sub(1)),
                    )
                    .copied()
                    .unwrap_or(Duration::from_secs(5));
                self.reconnect_attempt = self.reconnect_attempt.saturating_add(1);
                tracing::warn!(%error, ?delay, "SSE no disponible; reintento");
                tokio::time::sleep(delay).await;
            }
        }
    }

    fn build_client(&mut self) {
        let Some(base_url) = self.supervisor.base_url() else {
            self.client = None;
            return;
        };
        match ApiClient::new(base_url, self.config.password.clone()) {
            Ok(client) => self.client = Some(client),
            Err(error) => {
                tracing::error!(%error, "no se pudo crear el cliente HTTP");
                self.client = None;
            }
        }
    }

    /// Abre `GET /event` y lanza el lector; `sse_open` pasa a `true`.
    async fn connect_sse(&mut self, _out: &Sender<ChatEvent>) -> Result<()> {
        let Some(client) = self.client.clone() else {
            return Err(ChatError::Protocol("sin cliente HTTP".to_owned()));
        };
        let stream = client.open_event_stream().await?;
        if let Some(task) = self.sse_task.take() {
            task.abort();
        }
        let (tx, rx) = unbounded::<SpikeEvent>();
        self.events_rx = rx;
        self.sse_task = Some(tokio::spawn(sse_reader(stream, tx)));
        self.sse_open = true;
        Ok(())
    }

    fn emit_ready(&mut self, out: &Sender<ChatEvent>) {
        let Some(server) = self.supervisor.server() else {
            return;
        };
        let _ = out.try_send(ChatEvent::Ready {
            port: server.port,
            version: server.version.clone(),
            model: self.model.clone(),
            agent: self.agent.clone(),
        });
    }

    async fn refetch_history(&mut self, out: &Sender<ChatEvent>) {
        let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
            return;
        };
        match client
            .session_messages(&session.id, self.config.history_limit)
            .await
        {
            Ok(value) => {
                let events = history_events(&session.id, &value);
                let _ = out
                    .send(ChatEvent::History {
                        session_id: session.id,
                        events,
                    })
                    .await;
            }
            Err(ChatError::UnexpectedStatus { status: 404, .. }) => {
                // La sesión no sobrevivió al restart: se pide una nueva.
                self.session = None;
                let _ = out.send(ChatEvent::SessionCleared).await;
            }
            Err(error) => {
                let _ = out
                    .send(ChatEvent::Error {
                        message: format!("no se pudo recuperar el historial: {error}"),
                        recoverable: true,
                    })
                    .await;
            }
        }
    }

    async fn stop(&mut self, out: &Sender<ChatEvent>) {
        if let Some(task) = self.sse_task.take() {
            task.abort();
            let _ = task.await;
        }
        if let Err(error) = self.supervisor.stop().await {
            tracing::warn!(%error, "parada del server con incidencias");
        }
        let _ = out.send(ChatEvent::Stopped).await;
    }

    // --- sesiones ---------------------------------------------------------------

    fn session_title(&self) -> String {
        format!(
            "{} {}",
            self.config.project_name,
            timestamp_utc(SystemTime::now())
        )
    }

    async fn create_session(&mut self, out: &Sender<ChatEvent>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        match client
            .create_session(&self.session_title(), Some(&self.config.project_cwd))
            .await
        {
            Ok(session) => {
                self.session = Some(session.clone());
                let _ = out.send(ChatEvent::SessionActivated { session }).await;
                self.refresh_sessions(out).await;
            }
            Err(error) => {
                self.emit_error(out, format!("no se pudo crear la sesión: {error}"))
                    .await;
            }
        }
    }

    async fn switch_session(&mut self, id: &str, out: &Sender<ChatEvent>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        match client.session(id).await {
            Ok(session) => {
                self.session = Some(session.clone());
                let _ = out
                    .send(ChatEvent::SessionActivated {
                        session: session.clone(),
                    })
                    .await;
                self.refetch_history(out).await;
            }
            Err(error) => {
                self.emit_error(out, format!("no se pudo abrir la sesión: {error}"))
                    .await;
            }
        }
    }

    async fn delete_session(&mut self, id: &str, out: &Sender<ChatEvent>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        match client.delete_session(id).await {
            Ok(_) => {
                if self.session.as_ref().is_some_and(|s| s.id == id) {
                    self.session = None;
                    let _ = out.send(ChatEvent::SessionCleared).await;
                }
                self.refresh_sessions(out).await;
            }
            Err(error) => {
                self.emit_error(out, format!("no se pudo borrar la sesión: {error}"))
                    .await;
            }
        }
    }

    async fn refresh_sessions(&mut self, out: &Sender<ChatEvent>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        match client.list_sessions().await {
            Ok(sessions) => {
                let mut filtered: Vec<SessionInfo> = sessions
                    .into_iter()
                    .filter(|session| {
                        session.directory.as_deref() == Some(self.project_directory().as_str())
                    })
                    .collect();
                filtered.sort_by_key(|session| std::cmp::Reverse(session.updated_at()));
                let _ = out.send(ChatEvent::Sessions(filtered)).await;
            }
            Err(error) => {
                tracing::warn!(%error, "no se pudo listar sesiones");
            }
        }
    }

    fn project_directory(&self) -> String {
        self.config
            .project_cwd
            .canonicalize()
            .unwrap_or_else(|_| self.config.project_cwd.clone())
            .to_string_lossy()
            .into_owned()
    }

    // --- prompts / permisos -------------------------------------------------------

    async fn send_prompt(
        &mut self,
        text: &str,
        agent_override: Option<&str>,
        out: &Sender<ChatEvent>,
    ) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(session) = self.session.clone() else {
            return;
        };
        let agent = agent_override.unwrap_or(self.agent.as_str());
        if let Err(error) = client
            .prompt_async_with(&session.id, self.model.as_ref(), Some(agent), text)
            .await
        {
            self.emit_error(out, format!("no se pudo enviar el prompt: {error}"))
                .await;
        }
    }

    async fn abort(&mut self, out: &Sender<ChatEvent>) {
        let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
            return;
        };
        if let Err(error) = client.abort(&session.id).await {
            self.emit_error(out, format!("no se pudo abortar: {error}"))
                .await;
        }
    }

    async fn respond_permission(
        &mut self,
        request_id: &str,
        reply: PermissionReply,
        out: &Sender<ChatEvent>,
    ) {
        let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
            return;
        };
        if let Err(error) = client
            .respond_permission(&session.id, request_id, reply)
            .await
        {
            self.emit_error(out, format!("no se pudo responder el permiso: {error}"))
                .await;
        }
    }

    // --- catálogos ------------------------------------------------------------------

    async fn refresh_providers(&mut self, out: &Sender<ChatEvent>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        match client.providers().await {
            Ok(catalog) => {
                self.catalog = Some(catalog.clone());
                let _ = out.send(ChatEvent::Providers(catalog)).await;
            }
            Err(error) => {
                tracing::warn!(%error, "no se pudo leer /config/providers");
            }
        }
    }

    async fn refresh_agents(&mut self, out: &Sender<ChatEvent>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        match client.agents().await {
            Ok(value) => {
                let agents = parse_agents(&value);
                let _ = out.send(ChatEvent::Agents(agents)).await;
            }
            Err(error) => {
                tracing::warn!(%error, "no se pudo leer /agent");
            }
        }
    }

    /// Modelo inicial: `ai.model` si es válido; si no, el default del catálogo.
    fn resolve_initial_model(&mut self) {
        if let Some(model) = &self.config.model
            && let Some((provider, model_id)) = model.split_once('/')
            && !provider.is_empty()
            && !model_id.is_empty()
        {
            self.model = Some(ModelRef::new(provider, model_id));
            return;
        }
        if let Some(catalog) = &self.catalog {
            self.model = catalog.pick_default_model();
        }
    }

    async fn emit_error(&self, out: &Sender<ChatEvent>, message: String) {
        let _ = out
            .send(ChatEvent::Error {
                message,
                recoverable: true,
            })
            .await;
    }
}

/// Tarea lectora del SSE: data cruda → [`SpikeEvent`] (tabla I-4).
async fn sse_reader(mut stream: crate::client::SseStream, tx: Sender<SpikeEvent>) {
    loop {
        match stream.next().await {
            Ok(Some(event)) => {
                let value: Value = serde_json::from_str(&event.data)
                    .unwrap_or_else(|_| Value::String(event.data.clone()));
                let mapped = map_raw_event(&value, event.event.as_deref());
                if tx.send(mapped).await.is_err() {
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
