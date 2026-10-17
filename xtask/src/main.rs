//! `xtask` — herramientas de desarrollo de LaTeX Studio.
//!
//! Uso: `cargo run -p xtask -- <tarea>`
//! Tareas planificadas (ver docs/plan/TAREAS.md):
//! - `fixtures`     → restaurar fixtures desde disco/originales (M0)
//! - `pdf-golden`   → comparativa visual de PDFs de fixture (M0-S3)
//! - `chat-replay`  → reproducir fixtures SSE de opencode contra el reducer (M0-S4)
//! - `dist`         → ensamblar la distribución portable + zips (M3)

fn main() {
    let task = std::env::args().nth(1).unwrap_or_default();
    match task.as_str() {
        "fixtures" => println!("TODO(M0): restaurar fixtures"),
        "pdf-golden" => println!("TODO(M0-S3): comparativa PDF"),
        "chat-replay" => println!("TODO(M0-S4): reproductor de fixtures SSE"),
        "dist" => println!("TODO(M3): ensamblar dist portable"),
        "" => println!("tareas: fixtures | pdf-golden | chat-replay | dist"),
        other => {
            eprintln!("tarea desconocida: {other}");
            std::process::exit(2);
        }
    }
}
