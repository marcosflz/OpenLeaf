//! Mapeo evento SSE crudo → evento de dominio (semilla de `ChatEvent`).
//!
//! ESTA es la tabla I-4 congelada con fixtures reales (M0-S4). El reducer de
//! M1-E2 consume [`SpikeEvent`] (se renombrará a `ChatEvent` al integrarse en
//! `lsx-ui`), no JSON crudo.
//!
//! Hallazgos clave del protocolo (opencode 1.18.30):
//! - `message.part.updated` llega por SNAPSHOTS: la parte aparece vacía y se
//!   re-emite completa al cerrarse (`time.end` presente ⇒ final).
//! - `message.part.delta` es incremental (`field: "text"`) y NO trae el tipo
//!   de parte: el reducer correlaciona `partID → tipo` con los
//!   `message.part.updated` previos.
//! - `permission.asked` trae `patterns` (relativos al worktree), `always` y
//!   `metadata.diff` (unified diff) + `metadata.filepath` (absoluto).
//! - `session.status` con `busy|idle|retry`; el fin de turno es `session.idle`.

use serde_json::{Value, json};

use crate::protocol::event_type;

/// Evento de dominio (semilla de `ChatEvent`, INTERFACES §5.2).
#[derive(Debug, Clone, PartialEq)]
pub enum SpikeEvent {
    /// Conexión SSE establecida (`server.connected`).
    ServerConnected,
    /// Heartbeat de keep-alive (`server.heartbeat`).
    Heartbeat,
    /// Sesión creada (`session.created`).
    SessionCreated {
        session_id: String,
        title: String,
        directory: Option<String>,
    },
    /// Sesión actualizada (`session.updated`).
    SessionUpdated {
        session_id: String,
        title: String,
        directory: Option<String>,
    },
    /// Sesión borrada (`session.deleted`).
    SessionDeleted { session_id: String },
    /// Mensaje creado/cerrado (`message.updated`): rol y `completed`.
    MessageUpdated {
        session_id: String,
        message_id: String,
        role: String,
        completed: bool,
        provider_id: Option<String>,
        model_id: Option<String>,
        error: Option<String>,
    },
    /// Parte actualizada (snapshot completo).
    PartUpdated {
        session_id: String,
        part: SpikePartUpdate,
    },
    /// Delta incremental de una parte (streaming de texto/reasoning).
    PartDelta {
        session_id: String,
        message_id: String,
        part_id: String,
        field: String,
        delta: String,
    },
    /// Parte eliminada (`message.part.removed`).
    PartRemoved {
        session_id: String,
        message_id: String,
        part_id: String,
    },
    /// Solicitud de permiso (`permission.asked`).
    PermissionAsked {
        session_id: String,
        request_id: String,
        /// `edit`, `bash`, `webfetch`, … (en fixtures: `edit`).
        permission: String,
        /// Patrones afectados, relativos al worktree (p.ej. `fixtures\projects\cv\saludo.txt`).
        patterns: Vec<String>,
        /// Ruta absoluta (`metadata.filepath`).
        filepath: Option<String>,
        /// Unified diff propuesto (`metadata.diff`).
        diff: Option<String>,
        /// Patrones que aplicaría "always".
        always: Vec<String>,
    },
    /// Permiso respondido (`permission.replied`): `once|always|reject`.
    PermissionReplied {
        session_id: String,
        request_id: String,
        reply: String,
    },
    /// Estado de la sesión (`session.status`): `busy|idle|retry`.
    SessionStatus { session_id: String, status: String },
    /// Fin de turno (`session.idle`).
    SessionIdle { session_id: String },
    /// Error de sesión (`session.error`).
    SessionError { session_id: String, message: String },
    /// Cambios de la sesión (`session.diff`).
    SessionDiff { session_id: String, files: usize },
    /// Fichero editado por el agente (`file.edited`).
    FileEdited { file: String },
    /// Evento aún no mapeado (ruido o novedad de versión).
    Ignored { event_type: String },
}

/// Seed de `PartUpdate` (INTERFACES §5.2) con los campos reales de la API.
#[derive(Debug, Clone, PartialEq)]
pub enum SpikePartUpdate {
    /// Parte de texto del asistente/usuario.
    Text {
        message_id: String,
        part_id: String,
        markdown: String,
        /// `true` mientras `time.end` no está presente.
        streaming: bool,
    },
    /// Razonamiento del modelo (deltas con `field: "text"`).
    Reasoning {
        message_id: String,
        part_id: String,
        text: String,
        done: bool,
    },
    /// Llamada a herramienta y su estado.
    Tool {
        message_id: String,
        part_id: String,
        name: String,
        call_id: String,
        state: SpikeToolState,
        /// `state.input` crudo (p.ej. `{filePath, content}` para `write`).
        input: Value,
        title: Option<String>,
        output: Option<String>,
        error: Option<String>,
        /// `state.metadata.diff` si la tool lo aportó.
        diff: Option<String>,
    },
    /// Adjunto de fichero.
    File {
        message_id: String,
        part_id: String,
        mime: String,
        filename: Option<String>,
    },
    /// Frontera de paso del agente (`step-start`/`step-finish`).
    Step {
        phase: SpikeStepPhase,
        message_id: String,
        part_id: String,
    },
    /// Parte futura/desconocida.
    Other { part_type: String },
}

/// Fase de un `step` de opencode.
#[derive(Debug, Clone, PartialEq)]
pub enum SpikeStepPhase {
    Started,
    Finished {
        reason: Option<String>,
        cost: Option<f64>,
    },
}

/// Estado de una tool (mapea 1:1 a `ToolState` de INTERFACES).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpikeToolState {
    Pending,
    Running,
    Completed,
    Error,
}

impl SpikeToolState {
    fn from_status(status: Option<&str>) -> Self {
        match status {
            Some("pending") => Self::Pending,
            Some("running") => Self::Running,
            Some("completed") => Self::Completed,
            Some("error") => Self::Error,
            _ => Self::Pending,
        }
    }
}

/// Mapea el JSON crudo de un evento SSE a [`SpikeEvent`].
///
/// `sse_event` es el campo `event:` del SSE si existiera (opencode 1.18.30 no
/// lo envía); si `data` no es objeto JSON, se devuelve `Ignored`.
pub fn map_raw_event(data: &Value, sse_event: Option<&str>) -> SpikeEvent {
    let Some(object) = data.as_object() else {
        return SpikeEvent::Ignored {
            event_type: sse_event.unwrap_or("unparsed").to_owned(),
        };
    };
    let event_type_name = object
        .get("type")
        .and_then(Value::as_str)
        .or(sse_event)
        .unwrap_or("unknown")
        .to_owned();
    let props = object.get("properties").cloned().unwrap_or(Value::Null);

    match event_type_name.as_str() {
        event_type::SERVER_CONNECTED => SpikeEvent::ServerConnected,
        event_type::SERVER_HEARTBEAT => SpikeEvent::Heartbeat,
        event_type::SESSION_CREATED => {
            let info = props.get("info");
            SpikeEvent::SessionCreated {
                session_id: str_of(&props, "sessionID").unwrap_or_default(),
                title: info.and_then(|i| str_of(i, "title")).unwrap_or_default(),
                directory: info.and_then(|i| str_of(i, "directory")),
            }
        }
        event_type::SESSION_UPDATED => {
            let info = props.get("info");
            SpikeEvent::SessionUpdated {
                session_id: str_of(&props, "sessionID").unwrap_or_default(),
                title: info.and_then(|i| str_of(i, "title")).unwrap_or_default(),
                directory: info.and_then(|i| str_of(i, "directory")),
            }
        }
        event_type::SESSION_DELETED => SpikeEvent::SessionDeleted {
            session_id: str_of(&props, "sessionID").unwrap_or_default(),
        },
        event_type::MESSAGE_UPDATED => {
            let info = props.get("info").cloned().unwrap_or(Value::Null);
            SpikeEvent::MessageUpdated {
                session_id: str_of(&props, "sessionID").unwrap_or_default(),
                message_id: str_of(&info, "id").unwrap_or_default(),
                role: str_of(&info, "role").unwrap_or_default(),
                completed: !info["time"]["completed"].is_null(),
                provider_id: str_of(&info, "providerID"),
                model_id: str_of(&info, "modelID"),
                error: error_message(&info["error"]),
            }
        }
        event_type::MESSAGE_PART_UPDATED => SpikeEvent::PartUpdated {
            session_id: str_of(&props, "sessionID").unwrap_or_default(),
            part: map_part(&props["part"]),
        },
        event_type::MESSAGE_PART_DELTA => SpikeEvent::PartDelta {
            session_id: str_of(&props, "sessionID").unwrap_or_default(),
            message_id: str_of(&props, "messageID").unwrap_or_default(),
            part_id: str_of(&props, "partID").unwrap_or_default(),
            field: str_of(&props, "field").unwrap_or_default(),
            delta: str_of(&props, "delta").unwrap_or_default(),
        },
        event_type::MESSAGE_PART_REMOVED => SpikeEvent::PartRemoved {
            session_id: str_of(&props, "sessionID").unwrap_or_default(),
            message_id: str_of(&props, "messageID").unwrap_or_default(),
            part_id: str_of(&props, "partID").unwrap_or_default(),
        },
        event_type::PERMISSION_ASKED => SpikeEvent::PermissionAsked {
            session_id: str_of(&props, "sessionID").unwrap_or_default(),
            request_id: str_of(&props, "id").unwrap_or_default(),
            permission: str_of(&props, "permission").unwrap_or_default(),
            patterns: string_array(&props, "patterns"),
            filepath: props.get("metadata").and_then(|m| str_of(m, "filepath")),
            diff: props.get("metadata").and_then(|m| str_of(m, "diff")),
            always: string_array(&props, "always"),
        },
        event_type::PERMISSION_REPLIED => SpikeEvent::PermissionReplied {
            session_id: str_of(&props, "sessionID").unwrap_or_default(),
            request_id: str_of(&props, "requestID").unwrap_or_default(),
            reply: str_of(&props, "reply").unwrap_or_default(),
        },
        event_type::SESSION_STATUS => SpikeEvent::SessionStatus {
            session_id: str_of(&props, "sessionID").unwrap_or_default(),
            status: str_of(&props["status"], "type").unwrap_or_default(),
        },
        event_type::SESSION_IDLE => SpikeEvent::SessionIdle {
            session_id: str_of(&props, "sessionID").unwrap_or_default(),
        },
        event_type::SESSION_ERROR => {
            let error = props.get("error").cloned().unwrap_or(Value::Null);
            SpikeEvent::SessionError {
                session_id: str_of(&props, "sessionID").unwrap_or_default(),
                message: error_message(&error).unwrap_or_else(|| compact(&error)),
            }
        }
        event_type::SESSION_DIFF => SpikeEvent::SessionDiff {
            session_id: str_of(&props, "sessionID").unwrap_or_default(),
            files: props["diff"].as_array().map(Vec::len).unwrap_or(0),
        },
        event_type::FILE_EDITED => SpikeEvent::FileEdited {
            file: str_of(&props, "file").unwrap_or_default(),
        },
        _ => SpikeEvent::Ignored {
            event_type: event_type_name,
        },
    }
}

/// Convierte el payload de `GET /session/:id/message` (`[{info, parts}…]`) en
/// la misma secuencia de eventos de dominio que habría emitido el stream.
///
/// Es la base de la recuperación de gaps tras reconectar SSE (la API v1 **no**
/// soporta `Last-Event-ID`, hallazgo M0-S4): el panel limpia la sesión y vuelve
/// a reducir estos eventos.
pub fn history_events(session_id: &str, messages: &Value) -> Vec<SpikeEvent> {
    let Some(entries) = messages.as_array() else {
        return Vec::new();
    };
    let mut events = Vec::new();
    for entry in entries {
        let info = entry.get("info").cloned().unwrap_or(Value::Null);
        if !info.is_null() {
            let raw = json!({
                "id": "history",
                "type": event_type::MESSAGE_UPDATED,
                "properties": { "sessionID": session_id, "info": info },
            });
            events.push(map_raw_event(&raw, None));
        }
        if let Some(parts) = entry.get("parts").and_then(Value::as_array) {
            for part in parts {
                let raw = json!({
                    "id": "history",
                    "type": event_type::MESSAGE_PART_UPDATED,
                    "properties": { "sessionID": session_id, "part": part },
                });
                events.push(map_raw_event(&raw, None));
            }
        }
    }
    events
}

fn map_part(part: &Value) -> SpikePartUpdate {
    let message_id = str_of(part, "messageID").unwrap_or_default();
    let part_id = str_of(part, "id").unwrap_or_default();
    match str_of(part, "type").as_deref() {
        Some("text") => SpikePartUpdate::Text {
            message_id,
            part_id,
            markdown: str_of(part, "text").unwrap_or_default(),
            streaming: part["time"]["end"].is_null(),
        },
        Some("reasoning") => SpikePartUpdate::Reasoning {
            message_id,
            part_id,
            text: str_of(part, "text").unwrap_or_default(),
            done: !part["time"]["end"].is_null(),
        },
        Some("tool") => {
            let state = &part["state"];
            SpikePartUpdate::Tool {
                message_id,
                part_id,
                name: str_of(part, "tool").unwrap_or_default(),
                call_id: str_of(part, "callID").unwrap_or_default(),
                state: SpikeToolState::from_status(str_of(state, "status").as_deref()),
                input: state.get("input").cloned().unwrap_or(Value::Null),
                title: str_of(state, "title"),
                output: str_of(state, "output"),
                error: str_of(state, "error"),
                diff: state.get("metadata").and_then(|m| str_of(m, "diff")),
            }
        }
        Some("file") => SpikePartUpdate::File {
            message_id,
            part_id,
            mime: str_of(part, "mime").unwrap_or_default(),
            filename: str_of(part, "filename"),
        },
        Some("step-start") => SpikePartUpdate::Step {
            phase: SpikeStepPhase::Started,
            message_id,
            part_id,
        },
        Some("step-finish") => SpikePartUpdate::Step {
            phase: SpikeStepPhase::Finished {
                reason: str_of(part, "reason"),
                cost: part["cost"].as_f64(),
            },
            message_id,
            part_id,
        },
        Some(other) => SpikePartUpdate::Other {
            part_type: other.to_owned(),
        },
        None => SpikePartUpdate::Other {
            part_type: "unknown".to_owned(),
        },
    }
}

fn str_of(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn error_message(error: &Value) -> Option<String> {
    if error.is_null() {
        return None;
    }
    error
        .get("data")
        .and_then(|d| str_of(d, "message"))
        .or_else(|| str_of(error, "message"))
        .or_else(|| str_of(error, "name"))
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<error>".to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn maps_session_idle() {
        let raw = json!({
            "id": "evt_1",
            "type": "session.idle",
            "properties": { "sessionID": "ses_1" }
        });
        assert_eq!(
            map_raw_event(&raw, None),
            SpikeEvent::SessionIdle {
                session_id: "ses_1".to_owned()
            }
        );
    }

    #[test]
    fn maps_part_delta() {
        let raw = json!({
            "id": "evt_2",
            "type": "message.part.delta",
            "properties": {
                "sessionID": "ses_1", "messageID": "msg_1",
                "partID": "prt_1", "field": "text", "delta": "hola"
            }
        });
        assert_eq!(
            map_raw_event(&raw, None),
            SpikeEvent::PartDelta {
                session_id: "ses_1".to_owned(),
                message_id: "msg_1".to_owned(),
                part_id: "prt_1".to_owned(),
                field: "text".to_owned(),
                delta: "hola".to_owned(),
            }
        );
    }

    #[test]
    fn maps_tool_lifecycle() {
        let running = json!({
            "id": "evt_3",
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_2", "messageID": "msg_2", "type": "tool",
                    "tool": "write", "callID": "call_1",
                    "state": { "status": "running", "input": { "filePath": "a.txt" } }
                }
            }
        });
        match map_raw_event(&running, None) {
            SpikeEvent::PartUpdated { part, .. } => match part {
                SpikePartUpdate::Tool {
                    name, state, input, ..
                } => {
                    assert_eq!(name, "write");
                    assert_eq!(state, SpikeToolState::Running);
                    assert_eq!(input["filePath"], "a.txt");
                }
                other => panic!("se esperaba Tool, llegó {other:?}"),
            },
            other => panic!("se esperaba PartUpdated, llegó {other:?}"),
        }
    }

    #[test]
    fn maps_permission_asked_with_diff() {
        let raw = json!({
            "id": "evt_4",
            "type": "permission.asked",
            "properties": {
                "id": "per_1", "sessionID": "ses_1", "permission": "edit",
                "patterns": ["fixtures\\projects\\cv\\saludo.txt"],
                "metadata": { "filepath": "C:\\x\\saludo.txt", "diff": "@@ -0,0 +1,1 @@\n+hola\n" },
                "always": ["*"],
                "tool": { "messageID": "msg_2", "callID": "call_1" }
            }
        });
        match map_raw_event(&raw, None) {
            SpikeEvent::PermissionAsked {
                request_id,
                permission,
                patterns,
                diff,
                ..
            } => {
                assert_eq!(request_id, "per_1");
                assert_eq!(permission, "edit");
                assert!(patterns[0].ends_with("saludo.txt"));
                assert!(diff.unwrap().contains("+hola"));
            }
            other => panic!("se esperaba PermissionAsked, llegó {other:?}"),
        }
    }

    #[test]
    fn non_object_data_is_ignored() {
        assert_eq!(
            map_raw_event(&json!("garbage"), Some("weird.event")),
            SpikeEvent::Ignored {
                event_type: "weird.event".to_owned()
            }
        );
    }

    #[test]
    fn history_payload_maps_to_message_and_part_events() {
        let payload = json!([
            {
                "info": {
                    "id": "msg_a", "role": "user",
                    "time": { "created": 1, "completed": 2 }
                },
                "parts": [
                    { "id": "prt_a", "messageID": "msg_a", "type": "text", "text": "hola",
                      "time": { "start": 1, "end": 2 } }
                ]
            },
            {
                "info": {
                    "id": "msg_b", "role": "assistant",
                    "time": { "created": 3 },
                    "providerID": "opencode-go", "modelID": "m"
                },
                "parts": [
                    { "id": "prt_b", "messageID": "msg_b", "type": "tool", "tool": "bash",
                      "callID": "call_1",
                      "state": { "status": "completed", "output": "ok" } }
                ]
            }
        ]);
        let events = history_events("ses_1", &payload);
        assert_eq!(events.len(), 4, "{events:#?}");
        assert!(matches!(
            &events[0],
            SpikeEvent::MessageUpdated { message_id, role, completed, .. }
                if message_id == "msg_a" && role == "user" && *completed
        ));
        assert!(matches!(
            &events[1],
            SpikeEvent::PartUpdated { part: SpikePartUpdate::Text { markdown, .. }, .. }
                if markdown == "hola"
        ));
        assert!(matches!(
            &events[2],
            SpikeEvent::MessageUpdated { message_id, role, .. }
                if message_id == "msg_b" && role == "assistant"
        ));
        assert!(matches!(
            &events[3],
            SpikeEvent::PartUpdated { part: SpikePartUpdate::Tool { name, state, .. }, .. }
                if name == "bash" && *state == SpikeToolState::Completed
        ));
    }

    #[test]
    fn history_payload_tolerates_non_array_json() {
        assert!(history_events("ses_1", &json!({})).is_empty());
        assert!(history_events("ses_1", &Value::Null).is_empty());
    }
}
