//! Estado tipado de la conversación (M1-E2): sesiones, mensajes, partes, tool
//! calls y permisos.
//!
//! El servicio ([`crate::service`]) entrega eventos de dominio ya mapeados
//! (tabla I-4 congelada, [`crate::events`]) y este reducer los convierte en la
//! forma que pinta el panel de chat. Es Rust puro (sin GPUI, sin red): se
//! testea con las fixtures de `fixtures/opencode/events/`.
//!
//! Invariantes que reproduce de M0-S4:
//! - `message.part.updated` son **snapshots** completos: la parte se actualiza
//!   si el snapshot es más largo o igual (nunca se corta un texto en curso).
//! - `message.part.delta` **no trae tipo**: se correlaciona por `partID`; si la
//!   parte aún no existe, el delta crea la parte (texto por defecto).
//! - El cierre de turno (`session.idle`) también cierra los snapshots abiertos
//!   (los textos de usuario nunca reciben `time.end` en 1.18.30).
//! - Un refetch de historial reemplaza el transcript de la sesión sin perder
//!   los permisos pendientes.

use std::collections::HashMap;

use serde_json::Value;

use crate::events::{SpikeEvent, SpikePartUpdate, SpikeToolState};
use crate::protocol::{ModelRef, SessionInfo};

/// Rol del mensaje (`message.updated.info.role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// Estado del último turno de la sesión activa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TurnStatus {
    /// Sin información (`session.status` aún no llegó).
    #[default]
    Unknown,
    Idle,
    Busy,
    Retry,
}

impl TurnStatus {
    /// Mapea `session.status.type` del server.
    pub fn from_status(value: &str) -> Self {
        match value {
            "idle" => Self::Idle,
            "busy" => Self::Busy,
            "retry" => Self::Retry,
            _ => Self::Unknown,
        }
    }
}

/// Llamada a herramienta (parte `tool`) con su estado y salida.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub part_id: String,
    pub call_id: String,
    pub name: String,
    pub state: SpikeToolState,
    /// Resumen corto para la fila (`title` del server o derivado de `input`).
    pub summary: String,
    /// Argumentos legibles (patch crudo o JSON compacto, acotado).
    pub args: String,
    pub output: Option<String>,
    pub error: Option<String>,
    /// `state.metadata.diff` si la tool lo aportó.
    pub diff: Option<String>,
}

/// Parte de un mensaje, en orden de llegada.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    /// Texto markdown (usuario o asistente).
    Text {
        part_id: String,
        text: String,
        streaming: bool,
    },
    /// Razonamiento del modelo.
    Reasoning {
        part_id: String,
        text: String,
        done: bool,
    },
    /// Llamada a herramienta.
    Tool(Box<ToolCall>),
    /// Adjunto de fichero.
    File {
        part_id: String,
        mime: String,
        filename: Option<String>,
    },
    /// Frontera de paso (`step-start`/`step-finish`).
    Step { part_id: String, started: bool },
    /// Referencia a una petición de permiso (los datos viven en
    /// [`Conversation::permissions`]).
    Permission { request_id: String },
}

impl Part {
    /// `part_id` de cualquier variante.
    pub fn part_id(&self) -> &str {
        match self {
            Part::Text { part_id, .. }
            | Part::Reasoning { part_id, .. }
            | Part::File { part_id, .. }
            | Part::Step { part_id, .. } => part_id,
            Part::Tool(call) => &call.part_id,
            Part::Permission { request_id } => request_id,
        }
    }
}

/// Mensaje de la conversación (usuario o asistente) con sus partes.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub id: String,
    pub role: Role,
    /// `modelID` del servicio que respondió (etiqueta del asistente).
    pub model: Option<String>,
    pub provider: Option<String>,
    pub agent: Option<String>,
    /// `time.completed` presente.
    pub completed: bool,
    pub error: Option<String>,
    pub parts: Vec<Part>,
}

impl Message {
    fn new(id: String, role: Role) -> Self {
        Self {
            id,
            role,
            model: None,
            provider: None,
            agent: None,
            completed: false,
            error: None,
            parts: Vec::new(),
        }
    }

    /// Primera parte con `part_id` dado.
    pub fn part(&self, part_id: &str) -> Option<&Part> {
        self.parts.iter().find(|part| part.part_id() == part_id)
    }

    /// Primera parte mutable con `part_id` dado.
    pub fn part_mut(&mut self, part_id: &str) -> Option<&mut Part> {
        self.parts.iter_mut().find(|part| part.part_id() == part_id)
    }

    /// Texto plano concatenado del mensaje (búsqueda/copiar).
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            if let Part::Text { text, .. } | Part::Reasoning { text, .. } = part {
                out.push_str(text);
            }
        }
        out
    }
}

/// Petición de permiso (`permission.asked`) con su respuesta, si la hubo.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionEntry {
    pub request_id: String,
    pub session_id: String,
    /// `edit`, `bash`, …
    pub permission: String,
    /// Patrones afectados (relativos al worktree).
    pub patterns: Vec<String>,
    /// Ruta absoluta (`metadata.filepath`).
    pub filepath: Option<String>,
    /// Unified diff propuesto (`metadata.diff`).
    pub diff: Option<String>,
    /// Patrones que aplicaría "always".
    pub always: Vec<String>,
    /// `once|always|reject` cuando llegó `permission.replied`.
    pub reply: Option<String>,
}

impl PermissionEntry {
    /// Ruta visible: el primer patrón o la ruta absoluta.
    pub fn file(&self) -> String {
        self.patterns
            .first()
            .cloned()
            .or_else(|| self.filepath.clone())
            .unwrap_or_else(|| "(sin ruta)".to_owned())
            .replace('\\', "/")
    }

    /// `true` mientras el server no haya confirmado la respuesta.
    pub fn pending(&self) -> bool {
        self.reply.is_none()
    }
}

/// Sesión del server, filtrada al proyecto activo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    pub id: String,
    pub title: String,
    pub directory: Option<String>,
    pub updated: i64,
}

impl From<&SessionInfo> for SessionEntry {
    fn from(info: &SessionInfo) -> Self {
        Self {
            id: info.id.clone(),
            title: info.display_title(),
            directory: info.directory.clone(),
            updated: info.updated_at(),
        }
    }
}

/// Estado completo de la conversación del panel de chat.
#[derive(Debug, Default)]
pub struct Conversation {
    /// Sesiones del proyecto (más reciente primero).
    pub sessions: Vec<SessionEntry>,
    /// Sesión activa (`ses_…`).
    pub session_id: Option<String>,
    /// Título de la sesión activa (`session.created/updated`).
    pub session_title: Option<String>,
    /// Estado del turno (`session.status`).
    pub status: TurnStatus,
    /// Proveedor y modelo vigentes (del último mensaje del asistente o picker).
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Agente con el que se envía el prompt (`build`, `latex-fix`…).
    pub agent: Option<String>,
    /// Mensajes en orden de llegada.
    pub messages: Vec<Message>,
    /// Permisos en orden de llegada.
    pub permissions: Vec<PermissionEntry>,
    /// Ficheros afectados por el último `session.diff`.
    pub diff_files: usize,
    message_ix: HashMap<String, usize>,
    permission_ix: HashMap<String, usize>,
    /// Deltas de texto de partes cuyo tipo aún no conocemos: opencode emite
    /// `message.part.delta` antes del primer `message.part.updated` y sin tipo,
    /// así que un razonamiento en streaming se pintaría como texto normal
    /// (parpadeo). Se guardan aquí y se materializan al llegar el snapshot.
    pending_deltas: HashMap<String, (String, String)>,
}

impl Conversation {
    /// `true` mientras el server trabaja en un turno.
    pub fn streaming(&self) -> bool {
        matches!(self.status, TurnStatus::Busy | TurnStatus::Retry)
    }

    /// Título visible de la sesión activa.
    pub fn title(&self) -> String {
        self.session_title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "Sesión de chat".to_owned())
    }

    /// `provider/model` si se conocen.
    pub fn model_full(&self) -> Option<String> {
        let model = self.model.clone()?;
        Some(match &self.provider {
            Some(provider) => format!("{provider}/{model}"),
            None => model,
        })
    }

    /// Índice del último mensaje del asistente, si lo hay.
    pub fn last_assistant_ix(&self) -> Option<usize> {
        self.messages
            .iter()
            .rposition(|message| message.role == Role::Assistant)
    }

    /// Permiso por id.
    pub fn permission(&self, request_id: &str) -> Option<&PermissionEntry> {
        self.permission_ix
            .get(request_id)
            .and_then(|index| self.permissions.get(*index))
    }

    /// Permisos aún sin responder.
    pub fn pending_permissions(&self) -> impl Iterator<Item = &PermissionEntry> {
        self.permissions.iter().filter(|entry| entry.pending())
    }

    /// Marca una respuesta de permiso de forma optimista (el evento
    /// `permission.replied` la confirmará).
    pub fn note_reply(&mut self, request_id: &str, reply: &str) {
        if let Some(index) = self.permission_ix.get(request_id).copied()
            && let Some(entry) = self.permissions.get_mut(index)
        {
            entry.reply = Some(reply.to_owned());
        }
    }

    /// Fija el estado del turno (optimista al enviar un prompt).
    pub fn set_status(&mut self, status: TurnStatus) {
        self.status = status;
    }

    /// Vacía el transcript de la sesión activa (no toca permisos ni sesiones).
    pub fn clear_transcript(&mut self) {
        self.messages.clear();
        self.message_ix.clear();
        self.pending_deltas.clear();
        self.diff_files = 0;
        // Los permisos se conservan: una reconexión no debe perder una petición
        // viva. Las partes `Permission` desaparecen con sus mensajes.
    }

    /// Fija la sesión activa (al crearla o cambiarla) y limpia el transcript.
    pub fn set_active_session(&mut self, session: Option<&SessionInfo>) {
        match session {
            Some(info) => {
                self.session_id = Some(info.id.clone());
                self.session_title = Some(info.display_title());
                self.upsert_session(info);
            }
            None => {
                self.session_id = None;
                self.session_title = None;
            }
        }
        self.status = TurnStatus::Unknown;
        self.clear_transcript();
        self.permissions.retain(|entry| {
            self.session_id
                .as_deref()
                .is_some_and(|id| entry.session_id == id)
        });
        self.permission_ix.clear();
        for (index, entry) in self.permissions.iter().enumerate() {
            self.permission_ix.insert(entry.request_id.clone(), index);
        }
    }

    /// Reemplaza la lista de sesiones del proyecto (más reciente primero).
    pub fn apply_sessions(&mut self, sessions: Vec<SessionInfo>) {
        let mut entries: Vec<SessionEntry> = sessions.iter().map(SessionEntry::from).collect();
        entries.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
        self.sessions = entries;
        if let Some(id) = self.session_id.clone()
            && let Some(entry) = self.sessions.iter().find(|entry| entry.id == id)
        {
            self.session_title = Some(entry.title.clone());
        }
    }

    /// Inserta o actualiza una sesión en la lista.
    pub fn upsert_session(&mut self, info: &SessionInfo) {
        let entry = SessionEntry::from(info);
        match self
            .sessions
            .iter_mut()
            .find(|existing| existing.id == entry.id)
        {
            Some(existing) => *existing = entry,
            None => self.sessions.insert(0, entry),
        }
        self.sessions
            .sort_by_key(|session| std::cmp::Reverse(session.updated));
    }

    /// Quita una sesión de la lista; `true` si existía.
    pub fn remove_session(&mut self, session_id: &str) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|entry| entry.id != session_id);
        self.sessions.len() != before
    }

    /// Fija modelo y proveedor vigentes.
    pub fn set_model(&mut self, model: Option<ModelRef>) {
        match model {
            Some(model) => {
                self.provider = Some(model.provider_id);
                self.model = Some(model.model_id);
            }
            None => {
                self.provider = None;
                self.model = None;
            }
        }
    }

    /// Fija el agente vigente.
    pub fn set_agent(&mut self, agent: impl Into<String>) {
        let agent = agent.into();
        if !agent.is_empty() {
            self.agent = Some(agent);
        }
    }

    /// Refetch de historial: reemplaza el transcript de `session_id` con la
    /// secuencia reconstruida desde `GET /session/:id/message`.
    ///
    /// Si la sesión no es la activa, se ignora (llegó tarde).
    pub fn apply_history(&mut self, session_id: &str, events: &[SpikeEvent]) {
        if self.session_id.as_deref() != Some(session_id) {
            return;
        }
        self.clear_transcript();
        for event in events {
            self.apply_stream(event);
        }
        // Tras un refetch, el estado del turno se re-deriva de los mensajes:
        // si el último mensaje del asistente está completo, el server no está
        // trabajando (el `session.status` real llega por SSE igualmente).
        if let Some(index) = self.last_assistant_ix()
            && self.messages[index].completed
        {
            self.status = TurnStatus::Idle;
        }
    }

    /// Aplica una secuencia de eventos (fixtures, historial, replay).
    pub fn apply_streams<'a>(&mut self, events: impl IntoIterator<Item = &'a SpikeEvent>) {
        for event in events {
            self.apply_stream(event);
        }
    }

    /// Materializa como texto los deltas que nunca recibieron snapshot (cierre
    /// de turno): el snapshot es la fuente del tipo, pero el texto ya emitido
    /// no se pierde.
    fn flush_pending_deltas(&mut self) {
        let pending: Vec<(String, String, String)> = self
            .pending_deltas
            .drain()
            .map(|(part_id, (message_id, text))| (part_id, message_id, text))
            .collect();
        for (part_id, message_id, text) in pending {
            if text.is_empty() {
                continue;
            }
            let index = self.ensure_message(&message_id, Role::Assistant);
            self.messages[index].parts.push(Part::Text {
                part_id,
                text,
                streaming: false,
            });
        }
    }

    /// Reduce un evento de dominio sobre el estado.
    pub fn apply_stream(&mut self, event: &SpikeEvent) {
        match event {
            SpikeEvent::SessionCreated {
                session_id,
                title,
                directory,
            }
            | SpikeEvent::SessionUpdated {
                session_id,
                title,
                directory,
            } => {
                if !title.is_empty() {
                    self.upsert_session(&SessionInfo {
                        id: session_id.clone(),
                        title: Some(title.clone()),
                        directory: directory.clone(),
                        project_id: None,
                        slug: None,
                        time: None,
                    });
                    if self.session_id.as_deref() == Some(session_id.as_str())
                        || self.session_id.is_none()
                    {
                        self.session_id = Some(session_id.clone());
                        self.session_title = Some(title.clone());
                    }
                }
            }
            SpikeEvent::SessionDeleted { session_id } => {
                self.remove_session(session_id);
                if self.session_id.as_deref() == Some(session_id.as_str()) {
                    self.set_active_session(None);
                }
            }
            SpikeEvent::MessageUpdated {
                message_id,
                role,
                completed,
                provider_id,
                model_id,
                error,
                ..
            } => {
                let role = if role == "user" {
                    Role::User
                } else {
                    Role::Assistant
                };
                let index = self.ensure_message(message_id, role);
                let message = &mut self.messages[index];
                if *completed {
                    message.completed = true;
                }
                if let Some(error) = error {
                    message.error = Some(error.clone());
                    message.completed = true;
                }
                if role == Role::Assistant {
                    if let Some(provider_id) = provider_id {
                        self.provider = Some(provider_id.clone());
                        message.provider = Some(provider_id.clone());
                    }
                    if let Some(model_id) = model_id {
                        self.model = Some(model_id.clone());
                        message.model = Some(model_id.clone());
                    }
                }
            }
            SpikeEvent::PartUpdated { part, .. } => self.apply_part(part),
            SpikeEvent::PartDelta {
                message_id,
                part_id,
                field,
                delta,
                ..
            } if field == "text" => {
                if delta.is_empty() {
                    return;
                }
                if self.find_part_mut(part_id).is_none() {
                    // El tipo de la parte aún no se conoce (el snapshot llega
                    // después): se acumula sin pintar para no mostrar el
                    // razonamiento como texto mientras se genera.
                    let entry = self
                        .pending_deltas
                        .entry(part_id.clone())
                        .or_insert_with(|| (message_id.clone(), String::new()));
                    entry.0 = message_id.clone();
                    entry.1.push_str(delta);
                    return;
                }
                if let Some(Part::Text { text, .. } | Part::Reasoning { text, .. }) =
                    self.find_part_mut(part_id)
                {
                    text.push_str(delta);
                }
            }
            SpikeEvent::PartRemoved { part_id, .. } => {
                for message in &mut self.messages {
                    message.parts.retain(|part| part.part_id() != part_id);
                }
            }
            SpikeEvent::PermissionAsked {
                session_id,
                request_id,
                permission,
                patterns,
                filepath,
                diff,
                always,
            } => {
                let entry = PermissionEntry {
                    request_id: request_id.clone(),
                    session_id: session_id.clone(),
                    permission: permission.clone(),
                    patterns: patterns.clone(),
                    filepath: filepath.clone(),
                    diff: diff.clone(),
                    always: always.clone(),
                    reply: None,
                };
                let index = self.permissions.len();
                self.permissions.push(entry);
                self.permission_ix.insert(request_id.clone(), index);

                // La parte visual va al último mensaje del asistente (o a uno
                // nuevo si el permiso llegó antes que su mensaje).
                let message_ix = match self.last_assistant_ix() {
                    Some(index) => index,
                    None => {
                        self.messages.push(Message::new(
                            format!("permission-{request_id}"),
                            Role::Assistant,
                        ));
                        self.messages.len() - 1
                    }
                };
                self.messages[message_ix].parts.push(Part::Permission {
                    request_id: request_id.clone(),
                });
            }
            SpikeEvent::PermissionReplied {
                request_id, reply, ..
            } => {
                if let Some(index) = self.permission_ix.get(request_id).copied()
                    && let Some(entry) = self.permissions.get_mut(index)
                {
                    entry.reply = Some(reply.clone());
                }
            }
            SpikeEvent::SessionStatus { status, .. } => {
                self.status = TurnStatus::from_status(status);
            }
            SpikeEvent::SessionIdle { .. } => {
                self.status = TurnStatus::Idle;
                self.close_open_parts();
                // Red de seguridad: si algún delta nunca recibió su snapshot, se
                // materializa como texto para no perder lo ya emitido.
                self.flush_pending_deltas();
            }
            SpikeEvent::SessionError { message, .. } => {
                let mut error_message =
                    Message::new(format!("error-{}", self.messages.len()), Role::Assistant);
                error_message.completed = true;
                error_message.error = Some(message.clone());
                self.messages.push(error_message);
                self.status = TurnStatus::Idle;
            }
            SpikeEvent::SessionDiff { files, .. } => {
                self.diff_files = *files;
            }
            SpikeEvent::ServerConnected
            | SpikeEvent::Heartbeat
            | SpikeEvent::PartDelta { .. }
            | SpikeEvent::FileEdited { .. }
            | SpikeEvent::Ignored { .. } => {}
        }
    }

    /// Aplica un snapshot de parte (`message.part.updated`).
    pub fn apply_part(&mut self, part: &SpikePartUpdate) {
        match part {
            SpikePartUpdate::Text {
                message_id,
                part_id,
                markdown,
                streaming,
            } => {
                if let Some(existing) = self.find_part_mut(part_id) {
                    if let Part::Text {
                        text,
                        streaming: current,
                        ..
                    } = existing
                    {
                        // El snapshot final reconcilia si faltaron deltas.
                        if markdown.len() >= text.len() {
                            *text = markdown.clone();
                        }
                        *current = *streaming && !text.is_empty();
                    }
                    return;
                }
                if markdown.is_empty() {
                    return;
                }
                let index = self.ensure_message(message_id, Role::Assistant);
                // Si hubo deltas antes del snapshot, mandan ellos cuando son
                // más largos (el snapshot puede llegar recortado).
                let text = match self.pending_deltas.remove(part_id) {
                    Some((_, buffered)) if buffered.len() > markdown.len() => buffered,
                    _ => markdown.clone(),
                };
                self.messages[index].parts.push(Part::Text {
                    part_id: part_id.clone(),
                    text,
                    streaming: *streaming,
                });
            }
            SpikePartUpdate::Reasoning {
                message_id,
                part_id,
                text,
                done,
            } => {
                if let Some(existing) = self.find_part_mut(part_id) {
                    if let Part::Reasoning {
                        text: current,
                        done: current_done,
                        ..
                    } = existing
                    {
                        if text.len() >= current.len() {
                            *current = text.clone();
                        }
                        *current_done = *done;
                    } else if let Part::Text { text: streamed, .. } = existing {
                        // Los `message.part.delta` llegan antes que el primer
                        // snapshot y no traen el tipo de parte: hasta ahora se
                        // materializaban como Texto, así que el razonamiento se
                        // veía como texto normal (y luego se duplicaba al llegar
                        // este snapshot). Se promociona la parte conservando lo
                        // ya recibido.
                        let merged = if text.len() >= streamed.len() {
                            text.clone()
                        } else {
                            streamed.clone()
                        };
                        *existing = Part::Reasoning {
                            part_id: part_id.clone(),
                            text: merged,
                            done: *done,
                        };
                    }
                    return;
                }
                if text.is_empty() {
                    return;
                }
                let index = self.ensure_message(message_id, Role::Assistant);
                // Deltas previos al snapshot (típico en el razonamiento): se
                // materializan aquí, ya como `Reasoning`, y nacen plegados.
                let text = match self.pending_deltas.remove(part_id) {
                    Some((_, buffered)) if buffered.len() > text.len() => buffered,
                    _ => text.clone(),
                };
                self.messages[index].parts.push(Part::Reasoning {
                    part_id: part_id.clone(),
                    text,
                    done: *done,
                });
            }
            SpikePartUpdate::Tool {
                message_id,
                part_id,
                name,
                call_id,
                state,
                input,
                title,
                output,
                error,
                diff,
            } => {
                let args = tool_args(input);
                if let Some(existing) = self.find_part_mut(part_id) {
                    if let Part::Tool(call) = existing {
                        call.name = name.clone();
                        call.call_id = call_id.clone();
                        call.state = *state;
                        if !args.is_empty() {
                            call.args = args;
                        }
                        let summary =
                            tool_summary(name, input, title.as_deref(), &call.args.clone());
                        if !summary.is_empty() {
                            call.summary = summary;
                        }
                        if output.is_some() {
                            call.output = output.clone();
                        }
                        if error.is_some() {
                            call.error = error.clone();
                        }
                        if diff.is_some() {
                            call.diff = diff.clone();
                        }
                    }
                    return;
                }
                let summary = tool_summary(name, input, title.as_deref(), &args);
                let index = self.ensure_message(message_id, Role::Assistant);
                self.messages[index]
                    .parts
                    .push(Part::Tool(Box::new(ToolCall {
                        part_id: part_id.clone(),
                        call_id: call_id.clone(),
                        name: name.clone(),
                        state: *state,
                        summary,
                        args,
                        output: output.clone(),
                        error: error.clone(),
                        diff: diff.clone(),
                    })));
            }
            SpikePartUpdate::File {
                message_id,
                part_id,
                mime,
                filename,
            } => {
                if self.find_part_mut(part_id).is_some() {
                    return;
                }
                let index = self.ensure_message(message_id, Role::Assistant);
                self.messages[index].parts.push(Part::File {
                    part_id: part_id.clone(),
                    mime: mime.clone(),
                    filename: filename.clone(),
                });
            }
            SpikePartUpdate::Step {
                phase,
                message_id,
                part_id,
            } => {
                if self.find_part_mut(part_id).is_some() {
                    return;
                }
                let index = self.ensure_message(message_id, Role::Assistant);
                self.messages[index].parts.push(Part::Step {
                    part_id: part_id.clone(),
                    started: matches!(phase, crate::events::SpikeStepPhase::Started),
                });
            }
            SpikePartUpdate::Other { .. } => {}
        }
    }

    fn ensure_message(&mut self, message_id: &str, default_role: Role) -> usize {
        if let Some(index) = self.message_ix.get(message_id) {
            return *index;
        }
        self.messages
            .push(Message::new(message_id.to_owned(), default_role));
        let index = self.messages.len() - 1;
        self.message_ix.insert(message_id.to_owned(), index);
        index
    }

    fn find_part_mut(&mut self, part_id: &str) -> Option<&mut Part> {
        self.messages
            .iter_mut()
            .find_map(|message| message.part_mut(part_id))
    }

    /// Cierra los snapshots abiertos al terminar el turno (los textos de
    /// usuario nunca reciben `time.end` en opencode 1.18.30).
    fn close_open_parts(&mut self) {
        for message in &mut self.messages {
            for part in &mut message.parts {
                match part {
                    Part::Text { streaming, .. } => *streaming = false,
                    Part::Reasoning { done, .. } => *done = true,
                    _ => {}
                }
            }
        }
    }
}

/// Argumentos legibles de una tool: patch crudo o JSON compacto acotado.
pub fn tool_args(input: &Value) -> String {
    if input.is_null() {
        return String::new();
    }
    if input.as_object().is_some_and(|object| object.is_empty()) {
        return String::new();
    }
    if let Some(patch) = input.get("patchText").and_then(Value::as_str) {
        return truncate_chars(patch, 4000);
    }
    truncate_chars(&input.to_string(), 1200)
}

/// Resumen corto de una tool: `state.title` si existe; si no, claves típicas
/// (`filePath`, `command`, `pattern`…) y, en último lugar, el destino de un
/// patch de `apply_patch`.
pub fn tool_summary(name: &str, input: &Value, title: Option<&str>, args: &str) -> String {
    let mut summary = title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(|title| truncate_chars(title, 72))
        .unwrap_or_default();
    if summary.is_empty() {
        for key in [
            "filePath", "filepath", "file", "path", "pattern", "url", "query",
        ] {
            if let Some(value) = input
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                summary = truncate_chars(value, 72);
                break;
            }
        }
    }
    if summary.is_empty()
        && let Some(command) = input.get("command").and_then(Value::as_str)
    {
        summary = truncate_chars(command.lines().next().unwrap_or_default(), 72);
    }
    if summary.is_empty()
        && name.contains("patch")
        && let Some(target) = patch_target(args)
    {
        summary = truncate_chars(&target, 72);
    }
    summary
}

/// Primer `*** Add/Update/Delete File: <ruta>` de un patch de apply_patch.
pub fn patch_target(patch: &str) -> Option<String> {
    for line in patch.lines() {
        for prefix in ["*** Add File: ", "*** Update File: ", "*** Delete File: "] {
            if let Some(rest) = line.trim().strip_prefix(prefix) {
                return Some(rest.trim().to_owned());
            }
        }
    }
    None
}

/// Acota por caracteres, sin partir el string en medio de un carácter.
pub fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Estadísticas `+N −M` de un unified diff (cabeceras excluidas).
pub fn diff_stats(diff: Option<&str>) -> (usize, usize) {
    let mut additions = 0;
    let mut deletions = 0;
    for line in diff.unwrap_or_default().lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            additions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
    }
    (additions, deletions)
}

/// Nombre de fichero de una ruta (con `/` normalizado).
pub fn file_label(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .rsplit('/')
        .next()
        .filter(|last| !last.is_empty())
        .unwrap_or(&normalized)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_part(text: &str) -> SpikePartUpdate {
        SpikePartUpdate::Text {
            message_id: "msg_1".to_owned(),
            part_id: "prt_1".to_owned(),
            markdown: text.to_owned(),
            streaming: true,
        }
    }

    /// Mientras el tipo de una parte no se conoce, sus deltas **no** se pintan:
    /// si no, el razonamiento aparece un instante como texto normal y luego se
    /// colapsa (parpadeo reportado por el usuario).
    #[test]
    fn los_deltas_sin_snapshot_no_se_pintan_hasta_saber_el_tipo() {
        let mut conversation = Conversation::default();
        for delta in ["Pensando ", "en la ", "respuesta."] {
            conversation.apply_stream(&SpikeEvent::PartDelta {
                session_id: "ses_1".to_owned(),
                message_id: "msg_1".to_owned(),
                part_id: "prt_reason".to_owned(),
                field: "text".to_owned(),
                delta: delta.to_owned(),
            });
        }
        // Nada visible todavía: no hay parte de texto con el razonamiento.
        let visible = conversation
            .messages
            .iter()
            .flat_map(|message| message.parts.iter())
            .any(|part| matches!(part, Part::Text { .. }));
        assert!(!visible, "los deltas huérfanos no deben pintarse");

        // Llega el snapshot: aparece como razonamiento (plegado por el panel)
        // con el texto acumulado.
        conversation.apply_part(&SpikePartUpdate::Reasoning {
            message_id: "msg_1".to_owned(),
            part_id: "prt_reason".to_owned(),
            text: "Pensando en la respuesta.".to_owned(),
            done: false,
        });
        assert!(matches!(
            &conversation.messages[0].parts[0],
            Part::Reasoning { text, .. } if text == "Pensando en la respuesta."
        ));

        // Al cerrar el turno no queda nada pendiente.
        conversation.apply_stream(&SpikeEvent::SessionIdle {
            session_id: "ses_1".to_owned(),
        });
        assert!(conversation.pending_deltas.is_empty());
    }

    /// Red de seguridad: si un delta nunca recibe snapshot, se materializa como
    /// texto al cerrar el turno (no se pierde lo emitido).
    #[test]
    fn un_delta_huerfano_se_materializa_al_cerrar_el_turno() {
        let mut conversation = Conversation::default();
        conversation.apply_stream(&SpikeEvent::PartDelta {
            session_id: "ses_1".to_owned(),
            message_id: "msg_9".to_owned(),
            part_id: "prt_huerfano".to_owned(),
            field: "text".to_owned(),
            delta: "texto sin snapshot".to_owned(),
        });
        conversation.apply_stream(&SpikeEvent::SessionIdle {
            session_id: "ses_1".to_owned(),
        });
        assert!(matches!(
            &conversation.messages[0].parts[0],
            Part::Text { text, .. } if text == "texto sin snapshot"
        ));
    }

    #[test]
    fn delta_de_razonamiento_antes_del_snapshot_no_duplica_texto() {
        // Caso real (fixture spike-20261017): opencode emite
        // `message.part.delta` de una parte de razonamiento antes de su primer
        // `message.part.updated`. El delta no trae el tipo, así que la parte
        // nace como Texto y el snapshot debe promocionarla, no duplicarla.
        let mut conversation = Conversation::default();
        for delta in ["Voy a ", "revisar ", "el documento."] {
            conversation.apply_stream(&SpikeEvent::PartDelta {
                session_id: "ses_1".to_owned(),
                message_id: "msg_1".to_owned(),
                part_id: "prt_reason".to_owned(),
                field: "text".to_owned(),
                delta: delta.to_owned(),
            });
        }
        conversation.apply_part(&SpikePartUpdate::Reasoning {
            message_id: "msg_1".to_owned(),
            part_id: "prt_reason".to_owned(),
            text: "Voy a revisar el documento.".to_owned(),
            done: false,
        });
        // Y el texto de la respuesta va en otra parte.
        conversation.apply_part(&SpikePartUpdate::Text {
            message_id: "msg_1".to_owned(),
            part_id: "prt_text".to_owned(),
            markdown: "Listo.".to_owned(),
            streaming: false,
        });

        let message = &conversation.messages[0];
        assert_eq!(
            message.parts.len(),
            2,
            "sin duplicados: {:#?}",
            message.parts
        );
        let reasoning = message
            .parts
            .iter()
            .find(|part| matches!(part, Part::Reasoning { .. }))
            .expect("la parte es de razonamiento");
        assert!(matches!(
            reasoning,
            Part::Reasoning { text, .. } if text == "Voy a revisar el documento."
        ));
        // El razonamiento no se cuela como texto visible del mensaje: la única
        // parte de texto es la respuesta.
        let visible: String = message
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(visible.trim(), "Listo.");
    }

    #[test]
    fn deltas_and_snapshots_merge_without_losing_text() {
        let mut conversation = Conversation::default();
        conversation.apply_stream(&SpikeEvent::PartDelta {
            session_id: "ses_1".to_owned(),
            message_id: "msg_1".to_owned(),
            part_id: "prt_1".to_owned(),
            field: "text".to_owned(),
            delta: "ho".to_owned(),
        });
        conversation.apply_stream(&SpikeEvent::PartDelta {
            session_id: "ses_1".to_owned(),
            message_id: "msg_1".to_owned(),
            part_id: "prt_1".to_owned(),
            field: "text".to_owned(),
            delta: "la".to_owned(),
        });
        // Snapshot final más largo: reconcilia.
        conversation.apply_part(&SpikePartUpdate::Text {
            message_id: "msg_1".to_owned(),
            part_id: "prt_1".to_owned(),
            markdown: "hola mundo".to_owned(),
            streaming: false,
        });
        let message = &conversation.messages[0];
        assert_eq!(message.parts.len(), 1);
        assert!(matches!(
            &message.parts[0],
            Part::Text { text, streaming: false, .. } if text == "hola mundo"
        ));
    }

    #[test]
    fn a_snapshot_never_shrinks_an_in_flight_text() {
        let mut conversation = Conversation::default();
        conversation.apply_part(&text_part("hola mundo"));
        conversation.apply_part(&SpikePartUpdate::Text {
            message_id: "msg_1".to_owned(),
            part_id: "prt_1".to_owned(),
            markdown: "hola".to_owned(),
            streaming: false,
        });
        assert!(matches!(
            &conversation.messages[0].parts[0],
            Part::Text { text, .. } if text == "hola mundo"
        ));
    }

    #[test]
    fn session_idle_closes_open_snapshots() {
        let mut conversation = Conversation::default();
        conversation.apply_part(&text_part("en curso"));
        conversation.apply_stream(&SpikeEvent::SessionIdle {
            session_id: "ses_1".to_owned(),
        });
        assert_eq!(conversation.status, TurnStatus::Idle);
        assert!(matches!(
            &conversation.messages[0].parts[0],
            Part::Text {
                streaming: false,
                ..
            }
        ));
    }

    #[test]
    fn tool_lifecycle_updates_in_place() {
        let mut conversation = Conversation::default();
        let running = SpikePartUpdate::Tool {
            message_id: "msg_1".to_owned(),
            part_id: "prt_t".to_owned(),
            name: "apply_patch".to_owned(),
            call_id: "call_1".to_owned(),
            state: SpikeToolState::Running,
            input: serde_json::json!({
                "patchText": "*** Begin Patch\n*** Update File: saludo.txt\n*** End Patch"
            }),
            title: None,
            output: None,
            error: None,
            diff: None,
        };
        conversation.apply_part(&running);
        let completed = SpikePartUpdate::Tool {
            message_id: "msg_1".to_owned(),
            part_id: "prt_t".to_owned(),
            name: "apply_patch".to_owned(),
            call_id: "call_1".to_owned(),
            state: SpikeToolState::Completed,
            input: serde_json::json!({}),
            title: Some("Editado saludo.txt".to_owned()),
            output: Some("ok".to_owned()),
            error: None,
            diff: Some("@@ -0,0 +1 @@\n+hola\n".to_owned()),
        };
        conversation.apply_part(&completed);
        assert_eq!(conversation.messages[0].parts.len(), 1);
        let Part::Tool(call) = &conversation.messages[0].parts[0] else {
            panic!("se esperaba una tool");
        };
        assert_eq!(call.state, SpikeToolState::Completed);
        assert_eq!(call.summary, "Editado saludo.txt");
        assert_eq!(call.output.as_deref(), Some("ok"));
        // El resumen derivado del patch se usó antes de llegar el título.
        assert_eq!(
            call.args.lines().next().unwrap_or_default(),
            "*** Begin Patch"
        );
    }

    #[test]
    fn permission_asked_and_replied_round_trip() {
        let mut conversation = Conversation::default();
        conversation.set_active_session(Some(&SessionInfo {
            id: "ses_1".to_owned(),
            title: Some("spike".to_owned()),
            directory: None,
            project_id: None,
            slug: None,
            time: None,
        }));
        conversation.apply_stream(&SpikeEvent::PermissionAsked {
            session_id: "ses_1".to_owned(),
            request_id: "per_1".to_owned(),
            permission: "edit".to_owned(),
            patterns: vec!["fixtures\\cv\\saludo.txt".to_owned()],
            filepath: None,
            diff: Some("@@ -0,0 +1 @@\n+hola\n".to_owned()),
            always: vec!["*".to_owned()],
        });
        let entry = conversation.permission("per_1").expect("permiso");
        assert_eq!(entry.file(), "fixtures/cv/saludo.txt");
        assert!(entry.pending());
        assert_eq!(
            entry.diff.as_deref().map(|d| diff_stats(Some(d))),
            Some((1, 0))
        );
        conversation.apply_stream(&SpikeEvent::PermissionReplied {
            session_id: "ses_1".to_owned(),
            request_id: "per_1".to_owned(),
            reply: "reject".to_owned(),
        });
        assert_eq!(
            conversation
                .permission("per_1")
                .and_then(|e| e.reply.clone()),
            Some("reject".to_owned())
        );
        assert_eq!(conversation.pending_permissions().count(), 0);
    }

    #[test]
    fn history_refetch_replaces_the_transcript_but_keeps_pending_permissions() {
        let mut conversation = Conversation::default();
        conversation.set_active_session(Some(&SessionInfo {
            id: "ses_1".to_owned(),
            title: None,
            directory: None,
            project_id: None,
            slug: None,
            time: None,
        }));
        conversation.apply_stream(&SpikeEvent::PermissionAsked {
            session_id: "ses_1".to_owned(),
            request_id: "per_1".to_owned(),
            permission: "edit".to_owned(),
            patterns: Vec::new(),
            filepath: None,
            diff: None,
            always: Vec::new(),
        });
        conversation.apply_stream(&SpikeEvent::PartDelta {
            session_id: "ses_1".to_owned(),
            message_id: "msg_viejo".to_owned(),
            part_id: "prt_viejo".to_owned(),
            field: "text".to_owned(),
            delta: "obsoleto".to_owned(),
        });
        let fresh = vec![SpikeEvent::MessageUpdated {
            session_id: "ses_1".to_owned(),
            message_id: "msg_1".to_owned(),
            role: "assistant".to_owned(),
            completed: true,
            provider_id: Some("opencode-go".to_owned()),
            model_id: Some("m".to_owned()),
            error: None,
        }];
        conversation.apply_history("ses_1", &fresh);
        assert!(conversation.messages.iter().all(|m| m.id == "msg_1"));
        assert_eq!(conversation.model_full().as_deref(), Some("opencode-go/m"));
        assert!(
            conversation.permission("per_1").is_some(),
            "permiso pendiente sobrevive"
        );
    }

    #[test]
    fn history_for_another_session_is_ignored() {
        let mut conversation = Conversation::default();
        conversation.set_active_session(Some(&SessionInfo {
            id: "ses_1".to_owned(),
            title: None,
            directory: None,
            project_id: None,
            slug: None,
            time: None,
        }));
        conversation.apply_history("ses_other", &[]);
        assert_eq!(conversation.session_id.as_deref(), Some("ses_1"));
    }

    #[test]
    fn sessions_are_sorted_newer_first_and_removable() {
        let mut conversation = Conversation::default();
        conversation.apply_sessions(vec![
            SessionInfo {
                id: "ses_vieja".to_owned(),
                title: Some("vieja".to_owned()),
                directory: None,
                project_id: None,
                slug: None,
                time: Some(crate::protocol::SessionTime {
                    created: Some(1),
                    updated: Some(1),
                }),
            },
            SessionInfo {
                id: "ses_nueva".to_owned(),
                title: Some("nueva".to_owned()),
                directory: None,
                project_id: None,
                slug: None,
                time: Some(crate::protocol::SessionTime {
                    created: Some(2),
                    updated: Some(9),
                }),
            },
        ]);
        assert_eq!(conversation.sessions[0].id, "ses_nueva");
        assert!(conversation.remove_session("ses_nueva"));
        assert!(!conversation.remove_session("no-existe"));
        assert_eq!(conversation.sessions.len(), 1);
    }

    #[test]
    fn session_deleted_clears_the_active_session() {
        let mut conversation = Conversation::default();
        conversation.set_active_session(Some(&SessionInfo {
            id: "ses_1".to_owned(),
            title: Some("t".to_owned()),
            directory: None,
            project_id: None,
            slug: None,
            time: None,
        }));
        conversation.apply_stream(&SpikeEvent::SessionDeleted {
            session_id: "ses_1".to_owned(),
        });
        assert!(conversation.session_id.is_none());
        assert!(conversation.messages.is_empty());
    }

    #[test]
    fn tool_summary_prefers_title_then_paths_then_command() {
        let input = serde_json::json!({ "filePath": "a/b.tex" });
        assert_eq!(tool_summary("write", &input, None, ""), "a/b.tex");
        assert_eq!(tool_summary("write", &input, Some("Título"), ""), "Título");
        let command = serde_json::json!({ "command": "latexmk -pdf main.tex\nmás" });
        assert_eq!(
            tool_summary("bash", &command, None, ""),
            "latexmk -pdf main.tex"
        );
    }

    #[test]
    fn file_label_normalizes_windows_separators() {
        assert_eq!(file_label("fixtures\\cv\\saludo.txt"), "saludo.txt");
        assert_eq!(file_label("a/b/c.tex"), "c.tex");
        assert_eq!(file_label("sin_ruta"), "sin_ruta");
    }
}
