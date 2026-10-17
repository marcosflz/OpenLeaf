//! `lsx-chat` — integración con opencode (`opencode serve`) para el panel de IA.
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §5):
//! - `ChatSupervisor`: arranque/health/restart/stop del proceso (job object, password por sesión)
//! - cliente REST + stream SSE propio, y `reducer` de eventos crudos → `ChatEvent`
//! - modelo de sesiones/mensajes/partes/tool calls/permisos
//! - composición de prompts (contexto, "arreglar errores")
//!
//! Regla dura: sin gpui. Toda la superficie de la API de opencode vive en `protocol.rs`
//! (único módulo a tocar si se migra a OpenCode v2).
//!
//! Estado: P0 — stub. Se implementa en M0-S4 (spike) y M1 (stream E).
