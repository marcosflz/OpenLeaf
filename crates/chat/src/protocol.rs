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
}

/// Body de `POST /session/:id/prompt_async` (subconjunto).
pub fn prompt_body(model: Option<&ModelRef>, text: &str) -> Value {
    let model = model.map(|m| json!({ "providerID": m.provider_id, "modelID": m.model_id }));
    json!({
        "model": model,
        "parts": [ { "type": "text", "text": text } ],
    })
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
