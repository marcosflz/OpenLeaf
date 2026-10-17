//! Superficie del protocolo de opencode (API v1, binario 1.18.30).
//!
//! Único módulo a tocar si se migra a OpenCode v2 (riesgo R5). Todo lo aquí
//! declarado está verificado contra el server real y su `/doc` (OpenAPI 3.1)
//! durante M0-S4; las fixtures viven en `fixtures/opencode/events/`.
//!
//! Forma del stream SSE observada (1.18.30): solo líneas `data: {json}\n\n`;
//! **no** hay `event:`/`id:`/`retry:` ni soporte de `Last-Event-ID` (la
//! reconexión pierde el gap → refetch con `GET /session/:id/message`).
//! El `type` va dentro del JSON: `{ "id": "evt_…", "type": "session.idle",
//! "properties": { … } }`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// Nombres exactos de eventos SSE v1 relevantes (verificados con fixtures).
pub mod event_type {
    pub const SERVER_CONNECTED: &str = "server.connected";
    pub const SERVER_HEARTBEAT: &str = "server.heartbeat";
    pub const SESSION_CREATED: &str = "session.created";
    pub const SESSION_UPDATED: &str = "session.updated";
    pub const SESSION_DELETED: &str = "session.deleted";
    pub const MESSAGE_UPDATED: &str = "message.updated";
    pub const MESSAGE_REMOVED: &str = "message.removed";
    pub const MESSAGE_PART_UPDATED: &str = "message.part.updated";
    pub const MESSAGE_PART_DELTA: &str = "message.part.delta";
    pub const MESSAGE_PART_REMOVED: &str = "message.part.removed";
    pub const PERMISSION_ASKED: &str = "permission.asked";
    pub const PERMISSION_REPLIED: &str = "permission.replied";
    pub const SESSION_STATUS: &str = "session.status";
    pub const SESSION_IDLE: &str = "session.idle";
    pub const SESSION_ERROR: &str = "session.error";
    pub const SESSION_DIFF: &str = "session.diff";
    pub const FILE_EDITED: &str = "file.edited";
}

/// Línea de fixture (`fixtures/opencode/events/*.jsonl`) — **forma contractual**.
///
/// Una línea por evento SSE: `{"event": "<tipo>", "data": <data crudo>}`.
/// `data` es el JSON parseado de la línea `data:` tal cual llegó del server;
/// si no parsea, el string crudo. `event` se toma de `data.type` (o del campo
/// SSE `event:` si algún día existiera; `"unparsed"` si no hay JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SseRecord {
    /// Tipo de evento (`session.idle`, `message.part.updated`, …).
    pub event: String,
    /// Payload crudo (objeto JSON completo del server, con `id`/`type`/`properties`).
    pub data: Value,
}

impl SseRecord {
    /// Crea una línea de fixture desde el `data:` crudo de una línea SSE.
    pub fn from_sse_data(data: &str) -> Self {
        match serde_json::from_str::<Value>(data) {
            Ok(value) => {
                let event = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                Self { event, data: value }
            }
            Err(_) => Self {
                event: "unparsed".to_owned(),
                data: Value::String(data.to_owned()),
            },
        }
    }
}

/// Mensaje SSE crudo tal como salió del parser (`data` sin re-parsear).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSseMessage {
    /// Campo `event:` si existe (opencode 1.18.30 no lo emite).
    pub event: Option<String>,
    /// Campo `id:` si existe (opencode 1.18.30 no lo emite).
    pub id: Option<String>,
    /// Data concatenada (posiblemente multilínea con `\n`).
    pub data: String,
}

/// `GET /global/health`.
#[derive(Debug, Clone, Deserialize)]
pub struct Health {
    pub healthy: bool,
    pub version: String,
}

/// `POST /session` (subconjunto que usamos; el server manda más campos).
#[derive(Debug, Clone, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(rename = "projectID", default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    /// `time: {created, updated}` en ms epoch.
    #[serde(default)]
    pub time: Option<SessionTime>,
}

/// Marcas de tiempo de una sesión (`GET /session`).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct SessionTime {
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub updated: Option<i64>,
}

impl SessionInfo {
    /// Marca de `time.updated` (para ordenar la lista de sesiones).
    pub fn updated_at(&self) -> i64 {
        self.time
            .as_ref()
            .and_then(|time| time.updated.or(time.created))
            .unwrap_or(0)
    }

    /// Título visible (con fallback).
    pub fn display_title(&self) -> String {
        self.title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| self.slug.clone().unwrap_or_else(|| self.id.clone()))
    }
}

/// Referencia de modelo `{providerID, modelID}` (forma de `prompt_async`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

impl ModelRef {
    pub fn new(provider_id: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
        }
    }
}

impl std::fmt::Display for ModelRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.provider_id, self.model_id)
    }
}

/// `GET /config/providers` (subconjunto).
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderCatalog {
    #[serde(default)]
    pub providers: Vec<ProviderInfo>,
    /// `{ "opencode-go": "<modelo por defecto>", … }`
    #[serde(default)]
    pub default: Map<String, Value>,
}

/// Entrada de `providers[]`.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub models: Map<String, Value>,
}

impl ProviderCatalog {
    /// Modelos de un provider (ids).
    pub fn model_ids(&self, provider_id: &str) -> Vec<String> {
        self.providers
            .iter()
            .find(|p| p.id == provider_id)
            .map(|p| p.models.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Elige un modelo del provider: primero el `default` del catálogo, si no
    /// el primer id listado. `None` si el provider o sus modelos no existen.
    pub fn pick_model(&self, provider_id: &str) -> Option<ModelRef> {
        let provider = self.providers.iter().find(|p| p.id == provider_id)?;
        if provider.models.is_empty() {
            return None;
        }
        let model_id = self
            .default
            .get(provider_id)
            .and_then(Value::as_str)
            .filter(|m| provider.models.contains_key(*m))
            .map(str::to_owned)
            .or_else(|| provider.models.keys().next().cloned())?;
        Some(ModelRef::new(provider_id, model_id))
    }

    /// Proveedor por defecto sugerido: el primero del mapa `default` que tenga
    /// modelos, si no el primer provider con modelos.
    pub fn default_provider(&self) -> Option<&str> {
        for provider in &self.providers {
            if self.default.contains_key(&provider.id) && !provider.models.is_empty() {
                return Some(provider.id.as_str());
            }
        }
        self.providers
            .iter()
            .find(|provider| !provider.models.is_empty())
            .map(|provider| provider.id.as_str())
    }

    /// Elige el modelo por defecto del catálogo (provider sugerido incluido).
    pub fn pick_default_model(&self) -> Option<ModelRef> {
        let provider = self.default_provider()?;
        self.pick_model(provider)
    }

    /// Entradas aplanadas para el picker de modelos (`provider/model` + nombre).
    pub fn model_entries(&self) -> Vec<ModelEntry> {
        let mut entries = Vec::new();
        for provider in &self.providers {
            for (model_id, meta) in &provider.models {
                let name = meta
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned);
                entries.push(ModelEntry {
                    provider_id: provider.id.clone(),
                    provider_name: provider.name.clone(),
                    model_id: model_id.clone(),
                    model_name: name,
                });
            }
        }
        entries.sort_by(|a, b| {
            a.provider_id
                .cmp(&b.provider_id)
                .then_with(|| a.model_id.cmp(&b.model_id))
        });
        entries
    }
}

/// Entrada del picker de modelos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEntry {
    pub provider_id: String,
    pub provider_name: Option<String>,
    pub model_id: String,
    pub model_name: Option<String>,
}

impl ModelEntry {
    /// `provider/model`.
    pub fn id(&self) -> String {
        format!("{}/{}", self.provider_id, self.model_id)
    }

    /// Etiqueta visible (`model_id` y, si difiere, el nombre bonito).
    pub fn label(&self) -> String {
        match &self.model_name {
            Some(name) if name != &self.model_id => format!("{} — {name}", self.model_id),
            _ => self.model_id.clone(),
        }
    }
}

impl AgentInfo {
    /// Etiqueta visible del agente.
    pub fn label(&self) -> String {
        self.name.clone()
    }
}

/// Agente ofrecido por `GET /agent` (picker del chat).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    pub name: String,
    pub description: Option<String>,
    pub mode: Option<String>,
    /// `true` para los agentes nativos de opencode (`native: true` en v1).
    pub built_in: bool,
}

/// `GET /agent` — tolerante a array u objeto indexado por nombre.
pub fn parse_agents(value: &Value) -> Vec<AgentInfo> {
    let mut agents = Vec::new();
    let mut push = |name: String, agent: &Value| {
        if name.trim().is_empty() {
            return;
        }
        agents.push(AgentInfo {
            name,
            description: agent
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned),
            mode: agent.get("mode").and_then(Value::as_str).map(str::to_owned),
            built_in: agent
                .get("native")
                .or_else(|| agent.get("builtIn"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    };
    match value {
        Value::Array(items) => {
            for item in items {
                if let Some(name) = item.get("name").and_then(Value::as_str) {
                    push(name.to_owned(), item);
                }
            }
        }
        Value::Object(map) => {
            for (name, agent) in map {
                if agent.is_object() {
                    push(name.clone(), agent);
                }
            }
        }
        _ => {}
    }
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    agents
}

/// Body de `POST /session/:id/prompt_async` (subconjunto).
pub fn prompt_body(model: Option<&ModelRef>, text: &str) -> Value {
    let model = model.map(|m| json!({ "providerID": m.provider_id, "modelID": m.model_id }));
    json!({
        "model": model,
        "parts": [ { "type": "text", "text": text } ],
    })
}

/// Igual que [`prompt_body`] + `agent` (M1-E5: `latex-fix`, `build`, `plan`).
pub fn prompt_body_with(model: Option<&ModelRef>, agent: Option<&str>, text: &str) -> Value {
    let mut body = prompt_body(model, text);
    if let Some(agent) = agent.filter(|agent| !agent.is_empty())
        && let Some(object) = body.as_object_mut()
    {
        object.insert("agent".to_owned(), json!(agent));
    }
    body
}

/// Respuesta a `POST /session/:id/permissions/:permissionID` (v1, deprecated pero viva).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionReply {
    Once,
    Always,
    Reject,
}

impl PermissionReply {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Reject => "reject",
        }
    }
}
