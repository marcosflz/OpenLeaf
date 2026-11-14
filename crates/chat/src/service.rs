//! Servicio de chat de producción: hilo propio con runtime tokio que posee el
//! supervisor del agente LaTeX, su cliente RPC y la sesión activa.
//!
//! Patrón del skill `lsx-gpui-patterns`: la UI nunca bloquea; los comandos
//! viajan por `async_channel` ([`ChatCmd`]) y los eventos vuelven por otro
//! canal ([`ChatEvent`]) que el panel reduce a [`crate::conversation`].
//!
//! Responsabilidades:
//! - arranque perezoso del agente (`Start`) y restart con backoff si muere,
//!   con **refetch de historial** de la sesión activa;
//! - sesiones por proyecto, cambio/borrado, modelo y modo por sesión, abort,
//!   permisos y revert.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use async_channel::{Receiver, Sender, unbounded};

use crate::events::{SpikeEvent, history_events};
use crate::protocol::{
    AgentInfo, ModelRef, PermissionReply, ProviderCatalog, SessionInfo, parse_agents,
};
use crate::recorder::timestamp_utc;
use crate::sidecar::{AgentClient, AgentSpec, AgentSupervisor, RestartPolicy, ServerState};

/// Configuración del servicio (la construye la UI desde `settings.json`).
#[derive(Debug, Clone)]
pub struct ChatConfig {
    /// Intérprete del agente (`agent\python\python.exe`).
    pub python: PathBuf,
    /// `PYTHONPATH` del agente (`agent\lib`, `agent`).
    pub python_path: Vec<PathBuf>,
    /// cwd del proceso del agente (la carpeta `agent`, nunca el proyecto).
    pub agent_cwd: PathBuf,
    /// Datos del agente (`config/agent`): sesiones y checkpoints.
    pub data_dir: PathBuf,
    /// Claves de proveedor (`config/secrets/ai.json`).
    pub secrets: Option<PathBuf>,
    /// Directorio de skills del bundle (`assets/skills`), si existe.
    pub skills_dir: Option<PathBuf>,
    /// Log del agente (`logs/agent.log`).
    pub log_file: Option<PathBuf>,
    /// Raíz del proyecto activo (directorio de las sesiones).
    pub project_cwd: PathBuf,
    /// Nombre visible del proyecto (título de sesiones nuevas).
    pub project_name: String,
    /// Modelo preferido `provider/model` de `ai.model` (None → el del catálogo).
    pub model: Option<String>,
    /// Modo por defecto de `ai.agent` (`build`, `latex-fix`, `ask`).
    pub agent: String,
    /// Plazo para que el agente responda a `hello` en cada arranque.
    pub health_timeout: Duration,
    /// Backoff del restart del agente.
    pub restart_delays: Vec<Duration>,
    /// Límite de mensajes del refetch de historial.
    pub history_limit: usize,
    /// `tex/bin/windows` para el PATH del agente (latexmk, tlmgr…).
    pub path_prepend: Vec<PathBuf>,
    /// `LSX_CHAT_RECORD`: carpeta donde grabar los eventos como fixture.
    pub record_dir: Option<PathBuf>,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            python: PathBuf::from("agent/python/python.exe"),
            python_path: Vec::new(),
            agent_cwd: PathBuf::from("agent"),
            data_dir: PathBuf::from("config/agent"),
            secrets: None,
            skills_dir: None,
            log_file: None,
            project_cwd: PathBuf::from("."),
            project_name: "LaTeX Studio".to_owned(),
            model: None,
            agent: "build".to_owned(),
            health_timeout: Duration::from_secs(20),
            restart_delays: RestartPolicy::default().delays,
            history_limit: 200,
            path_prepend: Vec::new(),
            record_dir: None,
        }
    }
}

impl ChatConfig {
    fn spec(&self) -> AgentSpec {
        AgentSpec {
            python: self.python.clone(),
            python_path: self.python_path.clone(),
            cwd: self.agent_cwd.clone(),
            data_dir: self.data_dir.clone(),
            secrets: self.secrets.clone(),
            skills_dir: self.skills_dir.clone(),
            log_file: self.log_file.clone(),
            path_prepend: self.path_prepend.clone(),
            record_dir: self.record_dir.clone(),
        }
    }
}

/// Comandos UI → servicio.
#[derive(Debug, Clone)]
pub enum ChatCmd {
    /// Arranca el agente si no está vivo (idempotente).
    Start,
    /// Detiene el agente y el worker (cierre de la app).
    Stop,
    /// Crea una sesión nueva en el proyecto y la activa.
    NewSession,
    /// Cambia a una sesión existente (refetch de historial).
    SwitchSession { id: String },
    /// Borra una sesión.
    DeleteSession { id: String },
    /// Recarga la lista de sesiones del proyecto.
    ListSessions,
    /// Recarga el catálogo de proveedores configurados.
    RefreshProviders,
    /// Fija el modelo de la sesión activa.
    SetModel { provider: String, model: String },
    /// Fija el modo (`build`, `latex-fix`, `ask`).
    SetAgent { name: String },
    /// Envía un prompt a la sesión activa (la crea si no existe).
    /// `agent` (opcional) fuerza un modo distinto al vigente (`latex-fix`).
    Prompt { text: String, agent: Option<String> },
    /// Cancela el turno en curso.
    Abort,
    /// Responde un permiso pendiente.
    Permission {
        request_id: String,
        reply: PermissionReply,
    },
    /// Revierte la sesión hasta `message_id`: restaura los archivos que tocó
    /// el agente desde ese mensaje y oculta el historial posterior.
    Revert { message_id: String },
    /// Restaura los mensajes y archivos revertidos.
    Unrevert,
}

/// Eventos servicio → UI.
#[derive(Debug)]
pub enum ChatEvent {
    /// Transición del proceso (`Stopped/Starting/Ready/Restarting/Error`).
    Server(ServerState),
    /// Agente listo: datos para el header/pickers.
    Ready {
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
    /// Historial reconstruido tras un cambio de sesión o un restart.
    History {
        session_id: String,
        events: Vec<SpikeEvent>,
    },
    /// Lista de sesiones del proyecto (más reciente primero).
    Sessions(Vec<SessionInfo>),
    /// Catálogo de proveedores/modelos configurados.
    Providers(ProviderCatalog),
    /// Modos disponibles.
    Agents(Vec<AgentInfo>),
    /// Modelo/modo vigentes tras un cambio explícito.
    SelectionChanged {
        model: Option<ModelRef>,
        agent: String,
    },
    /// La sesión se revirtió hasta `message_id` (el historial se recarga).
    Reverted { message_id: String },
    /// Se restauraron los mensajes revertidos.
    Unreverted,
    /// Error mostrable; `recoverable` indica que el servicio sigue vivo.
    Error { message: String, recoverable: bool },
    /// El worker terminó (agente parado).
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
    /// Pide el cierre sin bloquear (seguro desde el hilo UI).
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
        // agente al recibir Stop; el job object cubre el peor caso.
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
        .worker_threads(2)
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
    supervisor: AgentSupervisor,
    client: Option<AgentClient>,
    events: Option<Receiver<SpikeEvent>>,
    session: Option<SessionInfo>,
    model: Option<ModelRef>,
    agent: String,
    catalog: Option<ProviderCatalog>,
    /// `true` tras un `Start` correcto: si el proceso muere, se relanza.
    started: bool,
}

impl Worker {
    fn new(config: ChatConfig) -> Self {
        let policy = RestartPolicy {
            delays: config.restart_delays.clone(),
            hello_timeout: config.health_timeout,
        };
        Self {
            agent: config.agent.clone(),
            supervisor: AgentSupervisor::new(config.spec(), policy),
            config,
            client: None,
            events: None,
            session: None,
            model: None,
            catalog: None,
            started: false,
        }
    }

    async fn run(&mut self, cmd_rx: Receiver<ChatCmd>, out: Sender<ChatEvent>) {
        loop {
            if self.started && self.events.is_none() {
                self.recover(&out).await;
            }
            let events = self.events.clone();
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
                event = async { events.as_ref().expect("guardado por la condición").recv().await }, if events.is_some() => {
                    match event {
                        Ok(event) => {
                            let _ = out.send(ChatEvent::Stream(Box::new(event))).await;
                        }
                        Err(_) => {
                            tracing::warn!("el agente terminó inesperadamente; se relanzará");
                            self.events = None;
                            self.client = None;
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
                self.create_session(out).await;
            }
            ChatCmd::SwitchSession { id } => self.switch_session(&id, out).await,
            ChatCmd::DeleteSession { id } => self.delete_session(&id, out).await,
            ChatCmd::ListSessions => self.refresh_sessions(out).await,
            ChatCmd::RefreshProviders => {
                self.refresh_providers(out).await;
                self.resolve_initial_model();
                self.emit_selection(out).await;
            }
            ChatCmd::SetModel { provider, model } => {
                self.model = Some(ModelRef::new(provider, model));
                self.emit_selection(out).await;
            }
            ChatCmd::SetAgent { name } => {
                if !name.trim().is_empty() {
                    self.agent = name;
                }
                self.emit_selection(out).await;
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
            ChatCmd::Abort => self.abort(out).await,
            ChatCmd::Permission { request_id, reply } => {
                self.respond_permission(&request_id, reply, out).await;
            }
            ChatCmd::Revert { message_id } => self.revert(&message_id, out).await,
            ChatCmd::Unrevert => self.unrevert(out).await,
        }
        true
    }

    // --- arranque / recuperación ----------------------------------------------------------

    async fn start(&mut self, out: &Sender<ChatEvent>) {
        let _ = out.send(ChatEvent::Server(ServerState::Starting)).await;
        match self.supervisor.start().await {
            Ok(()) => {
                self.started = true;
                self.attach();
                if let Some(version) = self.version() {
                    let _ = out
                        .send(ChatEvent::Server(ServerState::Ready {
                            version: version.clone(),
                        }))
                        .await;
                }
                self.refresh_providers(out).await;
                self.refresh_agents(out).await;
                self.refresh_sessions(out).await;
                self.resolve_initial_model();
                self.emit_selection(out).await;
                self.emit_ready(out).await;
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

    /// Relanza el agente tras una muerte inesperada y recupera el historial.
    async fn recover(&mut self, out: &Sender<ChatEvent>) {
        let tx = out.clone();
        let result = self
            .supervisor
            .ensure_alive(move |state| {
                let _ = tx.try_send(ChatEvent::Server(state));
            })
            .await;
        match result {
            Ok(()) => {
                self.attach();
                self.emit_ready(out).await;
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
            }
        }
    }

    /// Toma el cliente y el canal de eventos del proceso vivo.
    fn attach(&mut self) {
        match self.supervisor.agent() {
            Some(agent) => {
                self.client = Some(agent.client.clone());
                self.events = Some(agent.events.clone());
            }
            None => {
                self.client = None;
                self.events = None;
            }
        }
    }

    fn version(&mut self) -> Option<String> {
        self.supervisor.agent().map(|agent| agent.version.clone())
    }

    async fn emit_ready(&mut self, out: &Sender<ChatEvent>) {
        let Some(version) = self.version() else {
            return;
        };
        let _ = out
            .send(ChatEvent::Ready {
                version,
                model: self.model.clone(),
                agent: self.agent.clone(),
            })
            .await;
    }

    async fn emit_selection(&self, out: &Sender<ChatEvent>) {
        let _ = out
            .send(ChatEvent::SelectionChanged {
                model: self.model.clone(),
                agent: self.agent.clone(),
            })
            .await;
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
            Err(error) => {
                tracing::warn!(%error, "no se pudo recuperar el historial");
                self.session = None;
                let _ = out.send(ChatEvent::SessionCleared).await;
            }
        }
    }

    async fn stop(&mut self, out: &Sender<ChatEvent>) {
        // Soltar el cliente cierra stdin: el agente termina limpio.
        self.client = None;
        self.events = None;
        if let Err(error) = self.supervisor.stop().await {
            tracing::warn!(%error, "parada del agente con incidencias");
        }
        let _ = out.send(ChatEvent::Stopped).await;
    }

    // --- sesiones ---------------------------------------------------------------------

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
            .create_session(&self.session_title(), &self.project_directory())
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
                let _ = out.send(ChatEvent::SessionActivated { session }).await;
                self.refetch_history(out).await;
            }
            Err(error) => {
                self.emit_error(out, format!("no se pudo abrir la sesión: {error}"))
                    .await;
            }
        }
    }

    async fn revert(&mut self, message_id: &str, out: &Sender<ChatEvent>) {
        let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
            return;
        };
        match client.revert(&session.id, message_id).await {
            Ok(updated) => {
                self.session = Some(updated);
                let _ = out
                    .send(ChatEvent::Reverted {
                        message_id: message_id.to_owned(),
                    })
                    .await;
                self.refetch_history(out).await;
            }
            Err(error) => {
                self.emit_error(out, format!("no se pudo revertir el mensaje: {error}"))
                    .await;
            }
        }
    }

    async fn unrevert(&mut self, out: &Sender<ChatEvent>) {
        let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
            return;
        };
        match client.unrevert(&session.id).await {
            Ok(updated) => {
                self.session = Some(updated);
                let _ = out.send(ChatEvent::Unreverted).await;
                self.refetch_history(out).await;
            }
            Err(error) => {
                self.emit_error(out, format!("no se pudo deshacer el revert: {error}"))
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
        match client.list_sessions(Some(&self.project_directory())).await {
            Ok(mut sessions) => {
                sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at()));
                let _ = out.send(ChatEvent::Sessions(sessions)).await;
            }
            Err(error) => tracing::warn!(%error, "no se pudo listar sesiones"),
        }
    }

    /// Ruta canónica del proyecto, tal como la guarda el agente (`resolve()`).
    fn project_directory(&self) -> String {
        let path = self
            .config
            .project_cwd
            .canonicalize()
            .unwrap_or_else(|_| self.config.project_cwd.clone());
        let text = path.to_string_lossy().into_owned();
        // `canonicalize` en Windows devuelve `\\?\C:\…`; Python no usa ese prefijo.
        text.strip_prefix(r"\\?\")
            .map(str::to_owned)
            .unwrap_or(text)
    }

    // --- prompts / permisos ------------------------------------------------------------

    async fn send_prompt(
        &mut self,
        text: &str,
        agent_override: Option<&str>,
        out: &Sender<ChatEvent>,
    ) {
        let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
            return;
        };
        let agent = agent_override.unwrap_or(self.agent.as_str());
        if let Err(error) = client
            .prompt(&session.id, self.model.as_ref(), agent, text)
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
            self.emit_error(out, format!("no se pudo cancelar: {error}"))
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

    // --- catálogos ---------------------------------------------------------------------

    async fn refresh_providers(&mut self, out: &Sender<ChatEvent>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        match client.providers().await {
            Ok(catalog) => {
                self.catalog = Some(catalog.clone());
                let _ = out.send(ChatEvent::Providers(catalog)).await;
            }
            Err(error) => tracing::warn!(%error, "no se pudo leer el catálogo de proveedores"),
        }
    }

    async fn refresh_agents(&mut self, out: &Sender<ChatEvent>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        match client.agents().await {
            Ok(value) => {
                let _ = out.send(ChatEvent::Agents(parse_agents(&value))).await;
            }
            Err(error) => tracing::warn!(%error, "no se pudo leer la lista de modos"),
        }
    }

    /// Modelo inicial: `ai.model` si su proveedor está configurado; si no, el
    /// primero del catálogo. Sin proveedores configurados → `None` (la UI pide
    /// configurar uno; nunca se fuerza un proveedor).
    fn resolve_initial_model(&mut self) {
        let Some(catalog) = &self.catalog else {
            return;
        };
        let configured = |provider: &str| catalog.providers.iter().any(|p| p.id == provider);
        if let Some(current) = &self.model
            && configured(&current.provider_id)
        {
            return;
        }
        if let Some(model) = &self.config.model
            && let Some((provider, model_id)) = model.split_once('/')
            && !model_id.is_empty()
            && configured(provider)
        {
            self.model = Some(ModelRef::new(provider, model_id));
            return;
        }
        self.model = catalog.pick_default_model();
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
