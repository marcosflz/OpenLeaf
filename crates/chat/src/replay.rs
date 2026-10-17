//! Reproductor de fixtures SSE → eventos de dominio.
//!
//! Lo usan los tests de M0-S4 y, en M1, `cargo xtask chat-replay`
//! (`lsx-chat::replay::replay_file`). Sin red, sin tokens.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{ChatError, Result};
use crate::events::{SpikeEvent, map_raw_event};
use crate::protocol::SseRecord;

/// Carga un `.jsonl` de fixtures (una línea JSON por evento).
pub fn load_records(path: &Path) -> Result<Vec<SseRecord>> {
    let text = std::fs::read_to_string(path)?;
    let mut records = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: SseRecord = serde_json::from_str(line).map_err(|error| {
            ChatError::Protocol(format!(
                "{}:{} no es una línea de fixture válida: {error}",
                path.display(),
                line_no + 1
            ))
        })?;
        records.push(record);
    }
    Ok(records)
}

/// Mapea una lista de registros a eventos de dominio (seed del reducer).
pub fn replay_records(records: &[SseRecord]) -> Vec<SpikeEvent> {
    records
        .iter()
        .map(|record| match &record.data {
            Value::Object(_) => map_raw_event(&record.data, Some(&record.event)),
            _ => map_raw_event(&Value::Null, Some(&record.event)),
        })
        .collect()
}

/// Carga + mapea un fichero de fixtures.
pub fn replay_file(path: &Path) -> Result<Vec<SpikeEvent>> {
    Ok(replay_records(&load_records(path)?))
}

/// Busca el fichero más reciente en `dir` cuyo nombre empiece por `prefix` y
/// termine por `suffix` (los timestamps del nombre ordenan lexicográficamente).
pub fn find_latest(dir: &Path, prefix: &str, suffix: &str) -> Result<Option<PathBuf>> {
    if !dir.is_dir() {
        return Ok(None);
    }
    let mut best: Option<PathBuf> = None;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(prefix) && name.ends_with(suffix) && entry.path().is_file() {
            let is_newer = best
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .is_none_or(|current| name > current);
            if is_newer {
                best = Some(entry.path());
            }
        }
    }
    Ok(best)
}

/// Nombre estable del evento para logs/resúmenes.
pub fn event_name(event: &SpikeEvent) -> &'static str {
    match event {
        SpikeEvent::ServerConnected => "server.connected",
        SpikeEvent::Heartbeat => "server.heartbeat",
        SpikeEvent::SessionCreated { .. } => "session.created",
        SpikeEvent::SessionUpdated { .. } => "session.updated",
        SpikeEvent::SessionDeleted { .. } => "session.deleted",
        SpikeEvent::MessageUpdated { .. } => "message.updated",
        SpikeEvent::PartUpdated { .. } => "message.part.updated",
        SpikeEvent::PartDelta { .. } => "message.part.delta",
        SpikeEvent::PartRemoved { .. } => "message.part.removed",
        SpikeEvent::PermissionAsked { .. } => "permission.asked",
        SpikeEvent::PermissionReplied { .. } => "permission.replied",
        SpikeEvent::SessionStatus { .. } => "session.status",
        SpikeEvent::SessionIdle { .. } => "session.idle",
        SpikeEvent::SessionError { .. } => "session.error",
        SpikeEvent::SessionDiff { .. } => "session.diff",
        SpikeEvent::FileEdited { .. } => "file.edited",
        SpikeEvent::Ignored { .. } => "ignored",
    }
}
