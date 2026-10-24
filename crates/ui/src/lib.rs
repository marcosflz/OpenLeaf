//! `lsx-ui` — shell GPUI: dock de 4 paneles, temas, ajustes, atajos, palette.
//!
//! Responsabilidades (ver `docs/plan/PLAN-MAESTRO.md` §4):
//! - layout dock serializable `[árbol | editor | pdf | chat]`
//! - paneles como reducers puros sobre eventos + intents (INTERFACES §8)
//! - título/toolbar/status bar, notificaciones, modales, command palette
//! - wizard «Nuevo proyecto» ([`new_project`], M1-F4) y fachada de proyectos
//!   ([`project`], M1-F1)
//! - i18n es desde el inicio ([`rust_i18n`], tablas en `locales/es.yml`)
//!
//! Arranque típico (`app/src/main.rs`):
//!
//! ```ignore
//! gpui_kit::init(cx);
//! lsx_ui::init(cx);
//! let (settings, _) = Settings::load(&root);
//! lsx_ui::theme::apply(&settings, cx);
//! // … open_window con lsx_ui::app::AppRoot
//! ```
//!
//! El módulo [`spike`] conserva la demo M0-S1 (`cargo run -p latex-studio
//! --bin spike-shell`), que sigue siendo la referencia del dock del kit.

pub mod ai_setup;
pub mod app;
pub mod layout;
pub mod new_project;
pub mod notifications;
pub mod palette;
pub mod panels;
pub mod permission;
pub mod project;
pub mod settings_ui;
pub mod spike;
pub mod status;
pub mod status_bar;
pub mod theme;
pub mod toolbar;

use gpui_kit::App;

rust_i18n::i18n!("locales", fallback = "es");

/// Inicializa el soporte de UI: locale español, tema oscuro por defecto y
/// atajos globales del shell.
///
/// `gpui_kit::init(cx)` debe haberse llamado antes. La app completa el tema
/// con [`theme::apply`] cuando ha cargado `Settings`.
pub fn init(cx: &mut App) {
    rust_i18n::set_locale("es");
    theme::init(cx);
    app::register_keybindings(cx);
}
