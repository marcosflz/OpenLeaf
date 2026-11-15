//! El servicio de chat contra el proceso Python real (`python -m lsx_agent`).
//!
//! Sin red y sin tokens: arranque, `hello`, catálogo, sesión por proyecto,
//! historial vacío y parada limpia. Necesita el entorno de desarrollo
//! (`cd python && uv sync`); si no existe `python/.venv`, el test se salta.

use std::path::{Path, PathBuf};
use std::time::Duration;

use lsx_chat::service::{ChatCmd, ChatConfig, ChatEvent, spawn};
use lsx_chat::sidecar::ServerState;
use tokio::time::timeout;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("raíz del repo")
        .to_path_buf()
}

fn dev_python() -> Option<PathBuf> {
    let venv = repo_root().join("python").join(".venv");
    [
        venv.join("Scripts").join("python.exe"),
        venv.join("bin").join("python"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

async fn recv_until<F>(
    events: &async_channel::Receiver<ChatEvent>,
    seconds: u64,
    predicate: F,
) -> Option<ChatEvent>
where
    F: Fn(&ChatEvent) -> bool,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match timeout(remaining, events.recv()).await {
            Ok(Ok(event)) if predicate(&event) => return Some(event),
            Ok(Ok(_)) => {}
            _ => return None,
        }
    }
}

#[tokio::test]
async fn the_service_drives_the_real_agent_process() {
    let Some(python) = dev_python() else {
        eprintln!("python/.venv no existe: `cd python && uv sync` para activar este test");
        return;
    };
    let scratch = std::env::temp_dir().join(format!("lsx-agent-process-{}", std::process::id()));
    let project = scratch.join("proyecto");
    std::fs::create_dir_all(&project).expect("proyecto temporal");
    std::fs::write(project.join("main.tex"), "\\documentclass{article}\n").expect("main.tex");

    let config = ChatConfig {
        python,
        python_path: vec![repo_root().join("python")],
        agent_cwd: scratch.clone(),
        data_dir: scratch.join("data"),
        secrets: Some(scratch.join("no-hay-claves.json")),
        skills_dir: Some(repo_root().join("assets").join("skills")),
        log_file: Some(scratch.join("agent.log")),
        project_cwd: project.clone(),
        project_name: "Prueba".to_owned(),
        ..ChatConfig::default()
    };
    let service = spawn(config);
    service.cmd.send(ChatCmd::Start).await.expect("Start");

    let ready = recv_until(&service.events, 60, |event| {
        matches!(
            event,
            ChatEvent::Ready { .. } | ChatEvent::Server(ServerState::Error(_))
        )
    })
    .await
    .expect("el agente debía arrancar");
    match ready {
        ChatEvent::Ready { version, agent, .. } => {
            assert!(!version.is_empty());
            assert_eq!(agent, "build");
        }
        other => panic!("arranque fallido: {other:?}"),
    }

    service
        .cmd
        .send(ChatCmd::NewSession)
        .await
        .expect("NewSession");
    let activated = recv_until(&service.events, 20, |event| {
        matches!(event, ChatEvent::SessionActivated { .. })
    })
    .await
    .expect("sesión creada");
    let ChatEvent::SessionActivated { session } = activated else {
        unreachable!()
    };
    let sessions = recv_until(
        &service.events,
        20,
        |event| matches!(event, ChatEvent::Sessions(list) if !list.is_empty()),
    )
    .await
    .expect("lista de sesiones del proyecto");
    let ChatEvent::Sessions(list) = sessions else {
        unreachable!()
    };
    assert!(
        list.iter().any(|item| item.id == session.id),
        "la sesión nueva debe listarse para el proyecto: {list:?}"
    );

    // Revertir un mensaje inexistente es un error recuperable, no una caída.
    service
        .cmd
        .send(ChatCmd::Revert {
            message_id: "msg_no_existe".to_owned(),
        })
        .await
        .expect("Revert");
    let error = recv_until(&service.events, 20, |event| {
        matches!(event, ChatEvent::Error { .. })
    })
    .await
    .expect("error recuperable");
    assert!(matches!(
        error,
        ChatEvent::Error {
            recoverable: true,
            ..
        }
    ));

    service.cmd.send(ChatCmd::Stop).await.expect("Stop");
    assert!(
        recv_until(&service.events, 20, |event| matches!(
            event,
            ChatEvent::Stopped
        ))
        .await
        .is_some(),
        "el worker debía parar"
    );
    service.join();
    let _ = std::fs::remove_dir_all(&scratch);
}
