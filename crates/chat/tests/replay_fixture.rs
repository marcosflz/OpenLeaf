//! Replayer mínimo (M0-S4): lee la fixture real grabada con `chat-record`
//! (mini) y verifica la tabla I-4 evento→dominio con evidencia concreta:
//! streaming de texto, ciclo de tool, permiso de edición y `session.idle`.

use std::path::PathBuf;

use lsx_chat::events::{SpikeEvent, SpikePartUpdate, SpikeToolState};
use lsx_chat::replay;

/// Fixture canónica del spike (inmutable; nuevas grabaciones no deben romper
/// estos asserts). Si desaparece, se cae al mini más reciente.
const KNOWN_MINI: &str = "spike-20261017-123649-mini.jsonl";

fn events_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/opencode/events")
}

fn find_mini() -> Option<PathBuf> {
    let dir = events_dir();
    let known = dir.join(KNOWN_MINI);
    if known.is_file() {
        return Some(known);
    }
    replay::find_latest(&dir, "spike-", "-mini.jsonl").expect("scan de fixtures")
}

#[test]
fn mini_fixture_replays_into_domain_events() {
    let dir = events_dir();
    let Some(path) = find_mini() else {
        eprintln!(
            "SKIP: aún no hay fixture mini en {} — se graba con `cargo run -p lsx-chat --bin chat-record`",
            dir.display()
        );
        return;
    };
    let records = replay::load_records(&path).expect("la fixture parsea como SseRecord");
    assert!(
        records.len() >= 10,
        "fixture demasiado corta: {} líneas",
        records.len()
    );
    let events = replay::replay_records(&records);
    assert_eq!(events.len(), records.len());

    // 1) Primer evento contractual: server.connected.
    assert!(
        matches!(events.first(), Some(SpikeEvent::ServerConnected)),
        "primer evento inesperado: {:?}",
        events.first()
    );

    // 2) Sesión creada en el proyecto del fixture.
    let session_id = events
        .iter()
        .find_map(|event| match event {
            SpikeEvent::SessionCreated { session_id, .. } => Some(session_id.clone()),
            _ => None,
        })
        .expect("session.created presente");
    assert!(!session_id.is_empty());

    // 3) Fin de turno: session.idle de esa sesión.
    assert!(
        events.iter().any(|event| matches!(
            event,
            SpikeEvent::SessionIdle { session_id: id } if *id == session_id
        )),
        "sin session.idle para {session_id}"
    );

    // 4) Streaming de texto: la parte final de texto y sus deltas concuerdan.
    let (text_part_id, markdown) = events
        .iter()
        .rev()
        .find_map(|event| match event {
            SpikeEvent::PartUpdated {
                part:
                    SpikePartUpdate::Text {
                        part_id,
                        markdown,
                        streaming: false,
                        ..
                    },
                ..
            } if markdown.contains("hola") => Some((part_id.clone(), markdown.clone())),
            _ => None,
        })
        .expect("snapshot final de texto con «hola»");
    let delta_concat: String = events
        .iter()
        .filter_map(|event| match event {
            SpikeEvent::PartDelta { part_id, delta, .. } if *part_id == text_part_id => {
                Some(delta.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        delta_concat, markdown,
        "los deltas de {text_part_id} deben reconstruir el snapshot final"
    );

    // 5) Ciclo de tool (write/edit/apply_patch…) con estado error tras rechazo.
    let tool_updates: Vec<(&String, SpikeToolState, Option<String>)> = events
        .iter()
        .filter_map(|event| match event {
            SpikeEvent::PartUpdated {
                part:
                    SpikePartUpdate::Tool {
                        name, state, error, ..
                    },
                ..
            } => Some((name, *state, error.clone())),
            _ => None,
        })
        .collect();
    assert!(
        !tool_updates.is_empty(),
        "la fixture debe contener al menos un ciclo de tool (prompt 2)"
    );
    for (name, _, _) in &tool_updates {
        assert!(!name.is_empty(), "tool sin nombre");
    }
    let error = tool_updates
        .iter()
        .find(|(_, state, _)| *state == SpikeToolState::Error)
        .and_then(|(_, _, error)| error.clone())
        .expect("la tool rechazada debe terminar en estado error");
    assert!(
        error.contains("reject"),
        "el error de la tool debería mencionar el rechazo: {error}"
    );

    // 5b) Estados de sesión observados: busy e idle.
    let statuses: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            SpikeEvent::SessionStatus { status, .. } => Some(status.as_str()),
            _ => None,
        })
        .collect();
    assert!(statuses.contains(&"busy"), "statuses={statuses:?}");
    assert!(statuses.contains(&"idle"), "statuses={statuses:?}");

    // 6) Permiso de edición: asked (con diff) antes de replied(reject).
    let asked_index = events
        .iter()
        .position(|event| matches!(event, SpikeEvent::PermissionAsked { .. }))
        .expect("permission.asked presente (prompt 2 de chat-record)");
    let replied_index = events
        .iter()
        .position(|event| matches!(event, SpikeEvent::PermissionReplied { .. }))
        .expect("permission.replied presente");
    assert!(asked_index < replied_index);
    match (&events[asked_index], &events[replied_index]) {
        (
            SpikeEvent::PermissionAsked {
                request_id,
                permission,
                patterns,
                diff,
                ..
            },
            SpikeEvent::PermissionReplied {
                request_id: replied_id,
                reply,
                ..
            },
        ) => {
            assert_eq!(permission, "edit");
            assert_eq!(request_id, replied_id);
            assert_eq!(reply, "reject");
            assert!(
                patterns.iter().any(|p| p.contains("saludo.txt")),
                "patterns={patterns:?}"
            );
            assert!(
                diff.as_deref().is_some_and(|d| d.contains("+hola")),
                "metadata.diff debería contener la línea propuesta"
            );
        }
        other => panic!("pareja de permiso inesperada: {other:?}"),
    }
}

#[test]
fn every_recording_parses_as_jsonl() {
    let dir = events_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("SKIP: {} no existe", dir.display());
        return;
    };
    let mut checked = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "jsonl") {
            continue;
        }
        let records = replay::load_records(&path)
            .unwrap_or_else(|error| panic!("{} no parsea: {error}", path.display()));
        assert!(!records.is_empty(), "{} vacía", path.display());
        checked += 1;
    }
    assert!(checked > 0, "no hay ninguna grabación en {}", dir.display());
}
