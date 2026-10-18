//! `xtask` — herramientas de desarrollo de LaTeX Studio.
//!
//! Uso: `cargo run -p xtask -- <tarea>`
//! - `chat-replay [ruta.jsonl]` → reproduce fixtures SSE de opencode contra el
//!   mapeo de dominio (sin red, sin tokens) y resume por tipo de evento.
//! - `fixtures` → restaurar fixtures (M0, pendiente)
//! - `pdf-golden` → comparativa visual de PDFs (M0-S3, pendiente)
//! - `dist [--no-tex] [--no-zip] [--clean] [--tex-dir=…]` → ensambla la
//!   distribución portable en `dist/latex-ide` (M3-1).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod dist;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let task = args.get(1).cloned().unwrap_or_default();
    match task.as_str() {
        "chat-replay" => chat_replay(),
        "fixtures" => println!("TODO(M0): restaurar fixtures"),
        "pdf-golden" => println!("TODO(M0-S3): comparativa PDF"),
        "dist" => dist::run(&args[2..]),
        "" => println!("tareas: chat-replay | fixtures | pdf-golden | dist"),
        other => {
            eprintln!("tarea desconocida: {other}");
            std::process::exit(2);
        }
    }
}

/// Reproduce un fixture `.jsonl` de `fixtures/opencode/events` (o la ruta dada)
/// y resume los eventos de dominio resultantes.
fn chat_replay() {
    let path = match std::env::args().nth(2) {
        Some(arg) => PathBuf::from(arg),
        None => {
            let dir = Path::new("fixtures/opencode/events");
            match lsx_chat::replay::find_latest(dir, "spike-", ".jsonl") {
                Ok(Some(path)) => path,
                Ok(None) => {
                    eprintln!("no hay fixtures en {}", dir.display());
                    std::process::exit(2);
                }
                Err(error) => {
                    eprintln!("error leyendo fixtures: {error}");
                    std::process::exit(2);
                }
            }
        }
    };

    match lsx_chat::replay::replay_file(&path) {
        Ok(events) => {
            let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
            for event in &events {
                *counts
                    .entry(lsx_chat::replay::event_name(event))
                    .or_default() += 1;
            }
            println!("{} → {} eventos", path.display(), events.len());
            for (name, count) in counts {
                println!("  {name:<24} {count}");
            }
        }
        Err(error) => {
            eprintln!("replay falló: {error}");
            std::process::exit(1);
        }
    }
}
