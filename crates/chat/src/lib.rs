//! `lsx-chat` — integración con opencode (`opencode serve`) para el panel de IA.
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §5):
//! - `supervisor`: arranque/health/stop del proceso (job object y restart con
//!   backoff llegan en M1-E1)
//! - `client`: REST v1 + stream SSE propio (`sse`), y `events` con el mapeo
//!   crudo → dominio
//! - `recorder`/`replay`: fixtures (`fixtures/opencode/events/*.jsonl`) y su
//!   reproducción sin red (`cargo xtask chat-replay`)
//! - `spike` (M0-S4): orquestación mínima server → SSE → sesión → prompt
//!
//! Regla dura: sin gpui. Toda la superficie de la API de opencode vive en
//! `protocol.rs` (único módulo a tocar si se migra a OpenCode v2).
//!
//! Estado: M0-S4 — supervisor mínimo + grabación/replay de fixtures reales.
//! Hallazgos I-4 (tabla evento→`ChatEvent`) y I-5 (instancia global con
//! `?directory=` por sesión) verificados contra 1.18.30; pendiente consolidar
//! en INTERFACES.md/skills al cerrar el spike.

pub mod client;
pub mod error;
pub mod events;
pub mod protocol;
pub mod recorder;
pub mod replay;
pub mod spike;
pub mod sse;
pub mod supervisor;

pub use error::{ChatError, Result};
