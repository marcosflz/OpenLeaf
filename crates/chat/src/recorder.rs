//! Grabador de fixtures SSE (`fixtures/opencode/events/*.jsonl`).
//!
//! Forma contractual: una línea JSON por evento
//! `{"event": "<tipo>", "data": <data crudo>}` (ver [`SseRecord`]).
//!
//! Además del fichero completo, genera un «mini» derivado (mismo prefijo
//! temporal + `-mini.jsonl`) con los eventos de UNA sesión y sin ruido de
//! arranque (`plugin.added`, `catalog.updated`, …), pensado para tests y para
//! `cargo xtask chat-replay`.

use std::collections::BTreeMap;
use std::fs::{File, create_dir_all};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::error::Result;
use crate::protocol::SseRecord;

/// Tipos que no aportan al reducer y se excluyen del mini fixture.
pub const NOISE_TYPES: &[&str] = &[
    "plugin.added",
    "catalog.updated",
    "reference.updated",
    "integration.updated",
    "models-dev.refreshed",
    "project.updated",
    "lsp.updated",
];

/// Resumen de una grabación.
#[derive(Debug, Clone)]
pub struct FixtureSummary {
    pub path: PathBuf,
    pub mini_path: PathBuf,
    pub total_lines: usize,
    pub mini_lines: usize,
    /// Conteo por tipo en el fichero completo.
    pub counts: BTreeMap<String, usize>,
}

/// Escribe las fixtures (completa + mini diferido).
pub struct FixtureWriter {
    path: PathBuf,
    mini_path: PathBuf,
    file: BufWriter<File>,
    /// Buffers en memoria para el mini (se filtra al cerrar, cuando ya se
    /// conoce el `sessionID`).
    records: Vec<SseRecord>,
    counts: BTreeMap<String, usize>,
    total_lines: usize,
}

impl FixtureWriter {
    /// Crea `spike-<stamp>.jsonl` y `spike-<stamp>-mini.jsonl` en `dir`.
    pub fn create(dir: &Path, stamp: &str) -> Result<Self> {
        create_dir_all(dir)?;
        let path = dir.join(format!("spike-{stamp}.jsonl"));
        let mini_path = dir.join(format!("spike-{stamp}-mini.jsonl"));
        let file = BufWriter::new(File::create(&path)?);
        Ok(Self {
            path,
            mini_path,
            file,
            records: Vec::new(),
            counts: BTreeMap::new(),
            total_lines: 0,
        })
    }

    /// Rutas de los ficheros (disponibles antes de terminar).
    pub fn paths(&self) -> (&Path, &Path) {
        (&self.path, &self.mini_path)
    }

    /// Registra el `data:` crudo de un evento SSE. Devuelve el tipo detectado.
    pub fn write_raw_data(&mut self, raw_data: &str) -> Result<String> {
        let record = SseRecord::from_sse_data(raw_data);
        self.records.push(record.clone());
        writeln!(self.file, "{}", serde_json::to_string(&record)?)?;
        self.total_lines += 1;
        *self.counts.entry(record.event.clone()).or_default() += 1;
        Ok(record.event)
    }

    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    /// Flushea el completo y escribe el mini (eventos de `session_id` +
    /// `server.connected`/`server.heartbeat`, excluyendo [`NOISE_TYPES`]).
    pub fn finish(mut self, session_id: Option<&str>) -> Result<FixtureSummary> {
        self.file.flush()?;
        let mut mini_lines = 0;
        {
            let mut mini = BufWriter::new(File::create(&self.mini_path)?);
            for record in &self.records {
                if keep_in_mini(record, session_id) {
                    writeln!(mini, "{}", serde_json::to_string(record)?)?;
                    mini_lines += 1;
                }
            }
            mini.flush()?;
        }
        Ok(FixtureSummary {
            path: self.path.clone(),
            mini_path: self.mini_path.clone(),
            total_lines: self.total_lines,
            mini_lines,
            counts: self.counts,
        })
    }
}

fn keep_in_mini(record: &SseRecord, session_id: Option<&str>) -> bool {
    if NOISE_TYPES.contains(&record.event.as_str()) {
        return false;
    }
    if matches!(
        record.event.as_str(),
        "server.connected" | "server.heartbeat"
    ) {
        return true;
    }
    let Some(session_id) = session_id else {
        return false;
    };
    record.data["properties"]["sessionID"].as_str() == Some(session_id)
}

/// `spike-<YYYYMMDD-HHMMSS>.jsonl` en UTC — ordenable lexicográficamente.
pub fn timestamp_utc(now: SystemTime) -> String {
    let secs = now
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Días desde epoch → (año, mes, día); algoritmo civil de Howard Hinnant.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Extrae el `sessionID` de un `data` crudo si es objeto JSON.
pub fn session_id_of(data: &Value) -> Option<&str> {
    data["properties"]["sessionID"].as_str()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn timestamp_is_deterministic() {
        // 2026-10-17 00:00:00 UTC = día 20743 desde epoch.
        let t = UNIX_EPOCH + Duration::from_secs(20_743 * 86_400);
        assert_eq!(timestamp_utc(t), "20261017-000000");
        // +12h18m55s
        let t2 = t + Duration::from_secs(44_335);
        assert_eq!(timestamp_utc(t2), "20261017-121855");
    }

    #[test]
    fn mini_filter_keeps_session_and_drops_noise() {
        let noise = SseRecord {
            event: "plugin.added".to_owned(),
            data: serde_json::json!({ "type": "plugin.added", "properties": { "id": "x" } }),
        };
        let other_session = SseRecord {
            event: "session.status".to_owned(),
            data: serde_json::json!({
                "type": "session.status",
                "properties": { "sessionID": "ses_other" }
            }),
        };
        let ours = SseRecord {
            event: "session.idle".to_owned(),
            data: serde_json::json!({
                "type": "session.idle",
                "properties": { "sessionID": "ses_ours" }
            }),
        };
        assert!(!keep_in_mini(&noise, Some("ses_ours")));
        assert!(!keep_in_mini(&other_session, Some("ses_ours")));
        assert!(keep_in_mini(&ours, Some("ses_ours")));
    }
}
