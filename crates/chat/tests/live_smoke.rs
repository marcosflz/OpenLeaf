//! Humo live del servicio de chat (M1-E1/E2): `LSX_TEST_OPENCODE=1`.
//!
//! Coste: **1 prompt trivial** («Responde exactamente: hola»). Verifica sin
//! mocks: arranque con password, sesión por proyecto, streaming por SSE hasta
//! `session.idle`, **restart automático** al matar el server a mano y parada
//! limpia. Por defecto se salta (los gates normales no gastan tokens).

use std::path::{Path, PathBuf};
use std::time::Duration;

use lsx_chat::conversation::{Conversation, Role};
use lsx_chat::events::SpikeEvent;
use lsx_chat::service::{ChatCmd, ChatConfig, ChatEvent, spawn};
use lsx_chat::supervisor::{ServerState, random_password};
use tokio::time::timeout;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("raíz del repo")
        .to_path_buf()
}

fn enabled() -> bool {
    std::env::var_os("LSX_TEST_OPENCODE").is_some()
}

/// Recibe eventos hasta que `predicate` acepte uno; `None` si vence el tiempo.
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
        if remaining.is_zero() {
            return None;
        }
        match timeout(remaining, events.recv()).await {
            Ok(Ok(event)) => {
                if predicate(&event) {
                    return Some(event);
                }
            }
            _ => return None,
        }
    }
}

/// Mata el proceso que escucha en `port` (y su árbol) desde el test.
fn kill_listener(port: u16) {
    let script = format!(
        "(Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1).OwningProcess"
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .expect("powershell");
    let pid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    assert!(!pid.is_empty(), "sin PID escuchando en el puerto {port}");
    let status = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid])
        .status()
        .expect("taskkill");
    assert!(status.success(), "taskkill falló para el pid {pid}");
}

#[tokio::test(flavor = "multi_thread")]
async fn service_start_prompt_restart_and_stop() {
    if !enabled() {
        eprintln!("SKIP: define LSX_TEST_OPENCODE=1 para el humo live (gasta 1 prompt)");
        return;
    }
    let root = repo_root();
    let exe = root.join("agent").join("opencode.exe");
    if !exe.is_file() {
        eprintln!("SKIP: falta {}", exe.display());
        return;
    }
    let project = root.join("fixtures").join("projects").join("cv");

    let config = ChatConfig {
        exe: exe.clone(),
        config_json: root.join("agent").join("opencode.json"),
        project_cwd: project.clone(),
        project_name: "m1-e-live".to_owned(),
        port: None,
        // Password aleatorio: ejercita el basic auth verificado en M1-E.
        password: Some(random_password()),
        model: Some("opencode-go/gpt-5.6-luna".to_owned()),
        agent: "build".to_owned(),
        path_prepend: vec![
            root.join("tex").join("bin").join("windows"),
            root.join("agent"),
        ],
        ..ChatConfig::default()
    };
    let service = spawn(config);

    // 1) Arranque → Ready.
    service.cmd.send(ChatCmd::Start).await.expect("send Start");
    let ready = recv_until(&service.events, 120, |event| {
        matches!(event, ChatEvent::Ready { .. })
    })
    .await
    .expect("servidor listo (Ready)");
    let ChatEvent::Ready { port, .. } = ready else {
        unreachable!()
    };
    assert_ne!(port, 0);

    // 2) Nueva sesión por proyecto.
    service
        .cmd
        .send(ChatCmd::NewSession)
        .await
        .expect("send NewSession");
    let activated = recv_until(&service.events, 30, |event| {
        matches!(event, ChatEvent::SessionActivated { .. })
    })
    .await
    .expect("sesión creada");
    let ChatEvent::SessionActivated { session } = activated else {
        unreachable!()
    };
    assert_eq!(
        session.directory.as_deref(),
        Some(project.to_string_lossy().as_ref()),
        "la sesión debe crearse en el directorio del proyecto"
    );

    // 3) Prompt trivial → streaming → session.idle.
    service
        .cmd
        .send(ChatCmd::Prompt {
            text: "Responde exactamente: hola".to_owned(),
            agent: None,
        })
        .await
        .expect("send Prompt");
    let mut conversation = Conversation::default();
    conversation.set_active_session(Some(&session));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    let mut done = false;
    while !done {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!remaining.is_zero(), "el turno no terminó a tiempo");
        match timeout(remaining, service.events.recv()).await {
            Ok(Ok(ChatEvent::Stream(event))) => {
                conversation.apply_stream(&event);
                if matches!(event.as_ref(), SpikeEvent::SessionIdle { .. }) {
                    done = true;
                }
            }
            Ok(Ok(ChatEvent::History { session_id, events })) => {
                conversation.apply_history(&session_id, &events);
            }
            Ok(Ok(_)) => {}
            _ => panic!("canal de eventos cerrado antes de session.idle"),
        }
    }
    let assistant: String = conversation
        .messages
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .map(|message| message.plain_text())
        .collect();
    assert!(
        assistant.to_lowercase().contains("hola"),
        "respuesta inesperada: {assistant:?}"
    );

    // 4) Matar el server a mano → restart con backoff → Ready de nuevo.
    kill_listener(port);
    let restarting = recv_until(&service.events, 60, |event| {
        matches!(event, ChatEvent::Server(ServerState::Restarting { .. }))
    })
    .await
    .expect("el supervisor debe relanzar el server");
    assert!(matches!(
        restarting,
        ChatEvent::Server(ServerState::Restarting { attempt: 1 })
    ));
    let ready_again = recv_until(&service.events, 120, |event| {
        matches!(event, ChatEvent::Server(ServerState::Ready { .. }))
    })
    .await
    .expect("server relanzado");
    let ChatEvent::Server(ServerState::Ready { port: new_port, .. }) = ready_again else {
        unreachable!()
    };
    assert_ne!(new_port, 0);
    // La reconexión SSE debe recuperar el historial de la sesión (sin
    // Last-Event-ID): el transcript vuelve por `ChatEvent::History`.
    recv_until(&service.events, 60, |event| {
        matches!(event, ChatEvent::History { .. })
    })
    .await
    .expect("refetch de historial tras reconectar");

    // 5) Parada limpia.
    service.cmd.send(ChatCmd::Stop).await.expect("send Stop");
    recv_until(&service.events, 20, |event| {
        matches!(event, ChatEvent::Stopped)
    })
    .await
    .expect("el servicio debe parar el server");
    service.join();
}
