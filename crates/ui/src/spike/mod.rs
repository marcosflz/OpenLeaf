//! Spike M0-S1: shell de 4 hojas `[árbol | editor | pdf | chat]` sobre `gpui-kit`.
//!
//! Demo: `cargo run -p latex-studio --bin spike-shell`.
//!
//! # Pregunta I-7 — respuestas del spike
//!
//! - **Hojas y splits**: el layout del kit es un árbol de datos (`PaneTree`)
//!   que se describe con [`gpui_base::dock::DockLayout`] (`h_split`, `v_split`,
//!   `tabs().panel(...)`, `child(layout, size)`) y se instala con
//!   `DockArea::set_center` / `set_dock`. El caso `[árbol | editor | pdf |
//!   chat]` es un `h_split` de 4 grupos `tabs()` de un panel cada uno.
//! - **Redimensionado**: nativo (`ResizableState` interno por split); los
//!   anchos se fijan/leen con `DockArea::set_split_sizes(NodeId, Vec<Pixels>)`
//!   y `DockArea::dump`, y sobreviven al resize de ventana.
//! - **Colapso**: los *docks* del kit (Left/Right/Bottom) son colapsables con
//!   `DockArea::toggle_dock`, y `DockSkin` dibuja el affordance en el tab bar
//!   designado; el **split central no tiene colapso nativo por hoja**. La vía
//!   equivalente es `Panel::visible(false)`: la hoja conserva su sitio en el
//!   árbol pero el contenedor cede su slot. Ver [`SpikePanel`].
//! - **Rename**: el título de cada hoja sale de `gpui_component::dock::Panel::
//!   title` / `tab_name` (elementos, no strings fijos); renombrar = cambiar el
//!   estado que alimenta `title`.
//! - **Serialización**: dos capas. `DockArea::dump() -> DockAreaState`
//!   (serde; `{version, center, left/right/bottom_dock}` con `PanelState`s por
//!   panel) y `DockArea::load()` reconstruyen vía `PanelRegistry` +
//!   `register_panel`. Encima, [`SpikeLayout`] fija el esquema estable de
//!   `INTERFACES.md` §7 (`{id, kind, width_fraction, collapsed}`) que usará
//!   `config/settings.json → ui.dock` en M1.
//! - **¿Suficiente para el IDE?** Sí para v1: splits anidables, tabs por
//!   grupo, resize, zoom por grupo, persistencia y builders por nombre. El
//!   código de M1 debe añadir: cablear `DockEvent::LayoutChanged` con debounce
//!   al guardado real, modo focus (`{hidden, previous_fractions}`) y el
//!   mapeo de dock regions si algún panel migra a Left/Right/Bottom.
//!
//! # Tema y chrome
//!
//! - Tema: `Theme::change(ThemeMode::Dark, None, cx)` y luego acento con
//!   `Theme::update(cx, |theme| theme.accent = ...)`; tokens vía
//!   `cx.theme()` (`ActiveTheme`). Acento del spike: `#4C8DFF`.
//! - Title bar: `component::TitleBar` (drag + controles de ventana del SO),
//!   con `TitleBar::window_options()` como base de `WindowOptions`.
//! - Status bar: `component::status_bar::StatusBar` (`left`/`center`/`right`).

mod layout;
mod panel;
mod shell;

pub use layout::{FocusMode, SCHEMA_VERSION, SlotKind, SlotState, SpikeLayout, SpikeLayoutError};
pub use panel::SpikePanel;
pub use shell::{SpikeShell, demo_layout_path};

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, rgb};

/// Acento del producto (`#4C8DFF`) mientras `config/settings.json` no exista.
pub const ACCENT_RGB: u32 = 0x004C_8DFF;

/// Prepara el tema del spike: dark + acento `#4C8DFF`.
///
/// En M1 esto vive en el módulo de temas y lee `settings.ui.theme/accent`.
pub fn init(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.accent = Hsla::from(rgb(ACCENT_RGB));
    });
}
