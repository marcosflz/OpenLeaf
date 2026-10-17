//! `lsx-ui` — shell GPUI: dock de 4 paneles, temas, ajustes, atajos, palette.
//!
//! Responsabilidades (ver `docs/plan/PLAN-MAESTRO.md` §4):
//! - layout dock serializable `[árbol | editor | pdf | chat]`
//! - paneles como reducers puros sobre `AppEvent` + intents (INTERFACES §8)
//! - título/toolbar/status bar, notificaciones, modales, command palette
//! - i18n es desde el inicio
//!
//! Estado: P0 — stub. Se implementa en M0-S1 (spike) y M1 (stream A).
