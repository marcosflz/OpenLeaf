//! `lsx-chat` — integración con opencode (`opencode serve`) para el panel de IA.
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §5):
//! - `supervisor`: arranque/health/restart/stop del proceso, job object
//!   kill-on-close y password por sesión (M1-E1)
//! - `client`: REST v1 + stream SSE propio (`sse`), y `events` con el mapeo
//!   crudo → dominio (tabla I-4 congelada)
//! - `conversation`: reducer tipado (sesiones, mensajes, partes, tools,
//!   permisos) que consume la UI
//! - `service`: worker de producción (canales `ChatCmd`/`ChatEvent`, reconexión
//!   SSE con refetch de historial, sesiones/modelo/agente)
//! - `prompt`: composición del prompt «Arreglar errores con IA» (M1-E5)
//! - `recorder`/`replay`: fixtures (`fixtures/opencode/events/*.jsonl`) y su
//!   reproducción sin red (`cargo xtask chat-replay`)
//!
//! Regla dura: sin gpui. Toda la superficie de la API de opencode vive en
//! `protocol.rs` (único módulo a tocar si se migra a OpenCode v2).
//!
//! Estado: M1-E — supervisor de producción + servicio + reducer + panel.
//! I-4 (tabla evento→dominio) e I-5 (instancia global con `?directory=`) siguen
//! congelados; el password basic-auth se verificó live en M1-E.

pub mod client;
pub mod conversation;
pub mod error;
pub mod events;
pub mod job;
pub mod prompt;
pub mod protocol;
pub mod recorder;
pub mod replay;
pub mod service;
pub mod spike;
pub mod sse;
pub mod supervisor;

pub use error::{ChatError, Result};
