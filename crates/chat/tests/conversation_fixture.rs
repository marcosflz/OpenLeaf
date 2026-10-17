//! Reducer de producción contra las fixtures reales de M0-S4 (M1-E2):
//! dado `events.jsonl` → estado de conversación tipado correcto.
//!
//! Sin red y sin tokens: `lsx_chat::replay` mapea la fixture con la misma tabla
//! I-4 que usa el servicio en vivo.

use std::path::{Path, PathBuf};

use lsx_chat::conversation::{Conversation, Part, Role, TurnStatus};
use lsx_chat::events::SpikeToolState;
use lsx_chat::replay::replay_file;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("raíz del repo")
        .join("fixtures")
        .join("opencode")
        .join("events")
        .join(name)
}

#[test]
fn the_rejected_permission_fixture_reduces_to_a_full_conversation() {
    let events = replay_file(&fixture("spike-20261017-123649-mini.jsonl")).expect("fixture");
    let mut conversation = Conversation::default();
    conversation.apply_streams(&events);

    // Mensajes: turno del usuario + respuesta del asistente.
    assert!(
        conversation.messages.len() >= 2,
        "se esperaban al menos 2 mensajes, llegaron {}",
        conversation.messages.len()
    );
    let user = &conversation.messages[0];
    assert_eq!(user.role, Role::User);
    assert!(user.plain_text().contains("hola"), "{}", user.plain_text());

    let assistant_text: String = conversation
        .messages
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .map(|message| message.plain_text())
        .collect();
    assert!(
        assistant_text.to_lowercase().contains("hola"),
        "el asistente debía responder «hola»: {assistant_text:?}"
    );

    // Modelo del equipo, resuelto del `message.updated` del asistente.
    assert_eq!(conversation.model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(conversation.provider.as_deref(), Some("opencode-go"));

    // Tool `apply_patch` con su ciclo de vida y estado final de error (reject).
    let tool = conversation
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .find_map(|part| match part {
            Part::Tool(call) if call.name == "apply_patch" => Some(call),
            _ => None,
        })
        .expect("tool apply_patch en la fixture");
    assert_eq!(tool.state, SpikeToolState::Error);
    assert!(
        tool.args.contains("saludo.txt"),
        "argumentos con la ruta: {:?}",
        tool.args
    );

    // Permiso rechazado, con el diff propuesto y su archivo.
    assert_eq!(conversation.permissions.len(), 1);
    let permission = &conversation.permissions[0];
    assert_eq!(permission.permission, "edit");
    assert_eq!(permission.reply.as_deref(), Some("reject"));
    assert!(permission.file().ends_with("saludo.txt"));
    assert!(
        permission
            .diff
            .as_deref()
            .is_some_and(|diff| diff.contains("hola")),
        "el diff propuesto debía viajar en el evento"
    );
    assert_eq!(conversation.pending_permissions().count(), 0);

    // El turno terminó y los snapshots quedaron cerrados.
    assert_eq!(conversation.status, TurnStatus::Idle);
    let any_streaming = conversation
        .messages
        .iter()
        .flat_map(|m| m.parts.iter())
        .any(|part| match part {
            Part::Text { streaming, .. } => *streaming,
            Part::Reasoning { done, .. } => !*done,
            _ => false,
        });
    assert!(!any_streaming, "ningún texto debe quedar en streaming");

    // La sesión de la fixture se registra en la lista.
    assert!(conversation.session_id.is_some());
    assert!(
        conversation
            .sessions
            .iter()
            .any(|session| Some(&session.id) == conversation.session_id.as_ref()),
        "la sesión activa debe estar en la lista"
    );
}

#[test]
fn both_recorded_fixtures_reduce_without_panicking() {
    for name in [
        "spike-20261017-123649-mini.jsonl",
        "spike-20261017-132346-mini.jsonl",
        "m1e-edit-permission.jsonl",
    ] {
        let events = replay_file(&fixture(name)).expect("fixture");
        let mut conversation = Conversation::default();
        conversation.apply_streams(&events);
        assert!(
            !conversation.messages.is_empty(),
            "{name}: sin mensajes tras el replay"
        );
    }
}
