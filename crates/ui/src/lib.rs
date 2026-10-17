//! `lsx-ui` — shell GPUI: dock de 4 paneles, temas, ajustes, atajos, palette.
//!
//! Responsabilidades (ver `docs/plan/PLAN-MAESTRO.md` §4):
//! - layout dock serializable `[árbol | editor | pdf | chat]`
//! - paneles como reducers puros sobre `AppEvent` + intents (INTERFACES §8)
//! - título/toolbar/status bar, notificaciones, modales, command palette
//! - i18n es desde el inicio
//!
//! Estado: M0-S1 — el [`spike`] demuestra el shell de 4 hojas sobre `gpui-kit`
//! (dock, tema oscuro con acento, title bar, status bar y persistencia del
//! layout). M1 (stream A) lo convierte en el shell de producción.

pub mod spike;
