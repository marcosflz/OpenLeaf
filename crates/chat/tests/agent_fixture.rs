//! Reducer de producción contra una grabación del agente LaTeX real
//! (`python/tools/record_fixture.py`, modelo guionizado).
//!
//! Garantiza que lo que emite `lsx_agent` encaja con la tabla I-4 y el
//! reductor sin tocar la UI: modo `latex-fix` → `read` → `edit` con permiso
//! aprobado → respuesta en streaming → `session.idle`.

use std::path::{Path, PathBuf};

use lsx_chat::conversation::{Conversation, Part, Role, TurnStatus};
use lsx_chat::events::SpikeToolState;
use lsx_chat::replay::replay_file;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("raíz del repo")
        .join("fixtures/agent/events/agent-fix-edit.jsonl")
}

#[test]
fn the_agent_fixture_reduces_to_a_fixed_document() {
    let events = replay_file(&fixture()).expect("fixture del agente");
    let mut conversation = Conversation::default();
    conversation.apply_streams(&events);

    assert_eq!(conversation.messages.len(), 2, "usuario + asistente");
    let user = &conversation.messages[0];
    assert_eq!(user.role, Role::User);
    assert!(
        user.plain_text().contains("Arregla"),
        "{}",
        user.plain_text()
    );

    let assistant = &conversation.messages[1];
    assert_eq!(assistant.role, Role::Assistant);
    assert!(assistant.completed);
    assert!(assistant.error.is_none(), "{:?}", assistant.error);
    assert!(
        assistant.plain_text().contains("main.tex:3"),
        "{}",
        assistant.plain_text()
    );

    let tools: Vec<_> = assistant
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Tool(call) => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(
        tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["read", "edit"]
    );
    assert!(tools.iter().all(|t| t.state == SpikeToolState::Completed));
    assert_eq!(tools[1].summary, "main.tex");
    let diff = tools[1].diff.as_deref().expect("diff en la tool edit");
    assert!(diff.contains("+\\textbf{Hola}"), "{diff}");

    assert_eq!(conversation.permissions.len(), 1);
    let permission = &conversation.permissions[0];
    assert_eq!(permission.permission, "edit");
    assert_eq!(permission.reply.as_deref(), Some("once"));
    assert!(permission.file().ends_with("main.tex"));
    assert_eq!(conversation.pending_permissions().count(), 0);

    assert_eq!(conversation.status, TurnStatus::Idle);
    assert_eq!(conversation.diff_files, 1);
    assert_eq!(conversation.title(), "Arregla los errores de compilación");
}
