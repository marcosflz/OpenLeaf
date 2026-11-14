//! `lsx-chat`: integración con el agente LaTeX (`python -m lsx_agent`, sobre
//! nas-adk) para el panel de IA.
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §5 y
//! `docs/decisions/ADR-001-agente-nas-adk.md`):
//! - `sidecar`: arranque, restart y parada del proceso (job object
//!   kill-on-close) y cliente RPC NDJSON por stdio.
//! - `events`: mapeo de los eventos crudos a dominio (tabla I-4). El agente
//!   emite la forma de la API v1 de opencode, así que la tabla no cambia.
//! - `conversation`: reducer tipado (sesiones, mensajes, partes, tools,
//!   permisos) que consume la UI.
//! - `service`: worker de producción (canales `ChatCmd`/`ChatEvent`, restart
//!   con refetch de historial, sesiones/modelo/modo).
//! - `prompt`: composición del prompt «Arreglar errores con IA».
//! - `recorder`/`replay`: fixtures (`fixtures/agent/*.jsonl`) y su
//!   reproducción sin red (`cargo xtask chat-replay`).
//!
//! Regla dura: sin gpui.

pub mod conversation;
pub mod error;
pub mod events;
pub mod job;
pub mod prompt;
pub mod protocol;
pub mod recorder;
pub mod replay;
pub mod service;
pub mod sidecar;

pub use error::{ChatError, Result};
