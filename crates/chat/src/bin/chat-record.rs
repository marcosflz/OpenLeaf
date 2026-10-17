//! `chat-record` — graba fixtures SSE reales de `opencode serve` (M0-S4).
//!
//! Uso:
//! ```text
//! cargo run -p lsx-chat --bin chat-record [-- --model provider/modelo] [--no-permission] [--port 14096]
//! ```
//!
//! Flujo: arranca el server del bundle en `fixtures/projects/cv`, crea sesión,
//! graba TODOS los eventos SSE a `fixtures/opencode/events/spike-<ts>.jsonl`
//! (+ `-mini.jsonl` derivado), envía el prompt trivial («Responde exactamente:
//! hola») esperando `session.idle` (90 s, si cuelga → abort) y, salvo
//! `--no-permission`, un segundo prompt que provoca un permiso de edición
//! (se responde `reject` para no escribir en fixtures inmutables). Termina
//! matando el server y comprobando que el puerto quedó libre.
// El linker MSVC localizado (es-ES) imprime por stdout "Creando biblioteca …"
// al generar la .lib/.exp del binario; es ruido del toolchain, no nuestro.
#![allow(linker_messages)]

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use lsx_chat::events::SpikeEvent;
use lsx_chat::protocol::{ModelRef, PermissionReply, RawSseMessage};
use lsx_chat::recorder::{FixtureSummary, FixtureWriter, timestamp_utc};
use lsx_chat::spike::{ModelChoice, SpikeChat, SpikeConfig, WaitOutcome, wait_for_event};

const PROMPT_TRIVIAL: &str = "Responde exactamente: hola";
const PROMPT_PERMISSION: &str = "Usa la herramienta edit para crear el archivo saludo.txt en el directorio actual con el contenido exacto: hola";

struct Args {
    model: Option<ModelRef>,
    no_permission: bool,
    port: u16,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        model: None,
        no_permission: false,
        port: 14096,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--model" => {
                let raw = iter.next().ok_or("--model necesita provider/modelo")?;
                let (provider, model) = raw
                    .split_once('/')
                    .ok_or("--model debe ser provider/modelo")?;
                args.model = Some(ModelRef::new(provider, model));
            }
            "--no-permission" => args.no_permission = true,
            "--port" => {
                let raw = iter.next().ok_or("--port necesita un número")?;
                args.port = raw.parse().map_err(|_| format!("puerto inválido: {raw}"))?;
            }
            "--help" | "-h" => {
                println!("chat-record [--model provider/modelo] [--no-permission] [--port N]");
                std::process::exit(0);
            }
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    Ok(args)
}

fn repo_root() -> PathBuf {
    // Compilado dentro del workspace: crates/chat → ../../ = raíz del repo.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/chat tiene dos padres")
        .to_path_buf()
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("error de argumentos: {error}");
            std::process::exit(2);
        }
    };
    let repo = repo_root();
    let mut cfg = SpikeConfig::from_repo_root(&repo);
    cfg.port_preference = args.port;
    if let Some(model) = &args.model {
        cfg.model = ModelChoice::Explicit(model.clone());
    }

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("no se pudo crear el runtime tokio: {error}");
            std::process::exit(2);
        }
    };
    let exit_code = runtime.block_on(run(&cfg, &args));
    std::process::exit(exit_code);
}

async fn run(cfg: &SpikeConfig, args: &Args) -> i32 {
    let fixtures_dir = cfg
        .repo_root
        .join("fixtures")
        .join("opencode")
        .join("events");
    let stamp = timestamp_utc(SystemTime::now());
    let writer = match FixtureWriter::create(&fixtures_dir, &stamp) {
        Ok(writer) => writer,
        Err(error) => {
            eprintln!("no se pudo crear la fixture: {error}");
            return 1;
        }
    };
    println!("[chat-record] repo={}", cfg.repo_root.display());
    println!("[chat-record] fixture={}", writer.paths().0.display());

    // 1) Arranque: server + SSE + sesión (+ modelo).
    let (raw_tx, raw_rx) = async_channel::unbounded::<RawSseMessage>();
    let (chat, info) = match SpikeChat::start(cfg, Some(raw_tx)).await {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("[chat-record] FALLO al arrancar: {error}");
            return 1;
        }
    };
    println!(
        "[chat-record] PORT={} VERSION={} PID={} SESSION={} DIR={}",
        info.port,
        info.version,
        chat.pid(),
        info.session_id,
        info.directory.as_deref().unwrap_or("-")
    );
    match &info.model {
        Some(model) => println!("[chat-record] MODEL={model}"),
        None => println!(
            "[chat-record] MODEL=NONE (aviso: ningún modelo utilizable del provider; \
             se graba el flujo sesión+SSE sin prompt)"
        ),
    }

    // 2) Tarea de grabación (consume el crudo SSE y escribe el .jsonl).
    let writer_task = tokio::spawn(async move {
        let mut writer = writer;
        while let Ok(message) = raw_rx.recv().await {
            if let Err(error) = writer.write_raw_data(&message.data) {
                eprintln!("[chat-record] error escribiendo fixture: {error}");
                break;
            }
        }
        writer
    });

    let events = chat.events();
    let mut idle1_ok = false;
    let mut permission_seen = false;
    let mut permission_replied = false;
    let mut idle2_ok = false;

    // 3) Prompt trivial → session.idle (90 s; si cuelga, abort).
    if info.model.is_some() {
        if let Err(error) = chat.send_prompt(PROMPT_TRIVIAL).await {
            eprintln!("[chat-record] prompt 1 falló: {error}");
        } else {
            match wait_for_event(&events, Duration::from_secs(90), is_idle).await {
                WaitOutcome::Matched(_) => {
                    idle1_ok = true;
                    println!("[chat-record] prompt1 → session.idle OK");
                }
                WaitOutcome::TimedOut => {
                    println!("[chat-record] prompt1 sin idle en 90 s → POST abort");
                    let _ = chat.abort().await;
                    let _ = wait_for_event(&events, Duration::from_secs(15), is_idle).await;
                }
            }
        }

        // 4) Prompt de permiso (edit: ask en agent/opencode.json) → reject.
        if !args.no_permission {
            if let Err(error) = chat.send_prompt(PROMPT_PERMISSION).await {
                eprintln!("[chat-record] prompt 2 falló: {error}");
            } else {
                match wait_for_event(&events, Duration::from_secs(60), is_permission_asked).await {
                    WaitOutcome::Matched(SpikeEvent::PermissionAsked {
                        request_id,
                        permission,
                        patterns,
                        diff,
                        ..
                    }) => {
                        permission_seen = true;
                        println!(
                            "[chat-record] permission.asked id={request_id} permission={permission} patterns={patterns:?}"
                        );
                        if let Some(diff) = &diff {
                            let head = diff.lines().take(2).collect::<Vec<_>>().join(" | ");
                            println!("[chat-record]   metadata.diff (primeras líneas): {head}");
                        }
                        match chat
                            .respond_permission(&request_id, PermissionReply::Reject)
                            .await
                        {
                            Ok(true) => {
                                permission_replied = true;
                                println!("[chat-record] permission.replied=reject OK");
                            }
                            Ok(false) => eprintln!("[chat-record] reply rechazado por el server"),
                            Err(error) => eprintln!("[chat-record] reply falló: {error}"),
                        }
                    }
                    _ => {
                        println!(
                            "[chat-record] sin permission.asked en 60 s (el modelo no pidió tool); TODO lo dejamos documentado"
                        );
                    }
                }
                match wait_for_event(&events, Duration::from_secs(60), is_idle).await {
                    WaitOutcome::Matched(_) => {
                        idle2_ok = true;
                        println!("[chat-record] prompt2 → session.idle OK");
                    }
                    WaitOutcome::TimedOut => println!("[chat-record] prompt2 sin idle en 60 s"),
                }
            }
        }
    } else {
        println!(
            "[chat-record] MODEL=NONE: flujo sesión+SSE grabado sin enviar prompt (marcado como parcial)"
        );
    }

    // 5) Parada limpia + verificación de puerto.
    let port = chat.port();
    let session_id = chat.session_id().to_owned();
    if let Err(error) = chat.stop().await {
        eprintln!("[chat-record] stop con error: {error}");
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let port_released = lsx_chat::supervisor::port_free(port);
    println!("[chat-record] server_stopped=true port_{port}_free={port_released}");

    // 6) Cierre del grabador (el abort del SSE cierra el canal crudo).
    drop(events);
    let summary = match writer_task.await {
        Ok(writer) => writer.finish(Some(&session_id)),
        Err(error) => Err(lsx_chat::ChatError::Protocol(format!(
            "tarea de grabación abortada: {error}"
        ))),
    };
    match summary {
        Ok(summary) => print_summary(&summary),
        Err(error) => {
            eprintln!("[chat-record] no se pudo cerrar la fixture: {error}");
            return 1;
        }
    }

    if info.model.is_none() {
        println!("[chat-record] ESTADO=PARCIAL (sin modelo utilizable)");
        return 2;
    }
    if idle1_ok && (!permission_seen || permission_replied) && port_released {
        println!(
            "[chat-record] ESTADO=OK (permission_seen={permission_seen} permission_replied={permission_replied} idle2={idle2_ok})"
        );
        0
    } else {
        println!(
            "[chat-record] ESTADO=PARCIAL (idle1={idle1_ok} permission_seen={permission_seen} permission_replied={permission_replied} port_free={port_released})"
        );
        1
    }
}

fn is_idle(event: &SpikeEvent) -> bool {
    matches!(event, SpikeEvent::SessionIdle { .. })
}

fn is_permission_asked(event: &SpikeEvent) -> bool {
    matches!(event, SpikeEvent::PermissionAsked { .. })
}

fn print_summary(summary: &FixtureSummary) {
    println!("[chat-record] ---- resumen de fixtures ----");
    println!(
        "[chat-record] completo: {} ({} líneas)",
        summary.path.display(),
        summary.total_lines
    );
    println!(
        "[chat-record] mini:     {} ({} líneas)",
        summary.mini_path.display(),
        summary.mini_lines
    );
    println!("[chat-record] eventos por tipo:");
    let mut counts: Vec<_> = summary.counts.iter().collect();
    counts.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (event, count) in counts {
        println!("[chat-record]   {count:>4}x {event}");
    }
}
