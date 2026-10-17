//! Widgets de la toolbar del shell (M1-A2).
//!
//! Funciones puras de presentación: el shell decide qué hacen los botones y
//! esta capa fija el estilo (icon-only, ghost, 24 px, tooltip siempre).

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
};
use gpui_kit::{
    App, ClickEvent, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    Window, div, px,
};

use crate::layout::SlotKind;

/// Altura de la toolbar (VS Code ≈ 35 px lógicos).
pub const HEIGHT: f32 = 36.;

/// Botón solo-icono de la toolbar (fondo transparente, hover sutil).
pub fn icon_button(
    _cx: &App,
    id: impl Into<gpui_kit::ElementId>,
    icon: IconName,
    tooltip: impl Into<gpui_kit::SharedString>,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .icon(icon)
        .ghost()
        .small()
        .rounded(px(3.))
        .tab_stop(false)
        .selected(selected)
        .tooltip(tooltip.into())
        .on_click(on_click)
}

/// Botón de toggle de una hoja: icono de ojo + atajo en el tooltip.
pub fn slot_toggle(
    cx: &App,
    kind: SlotKind,
    collapsed: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    let icon = if collapsed {
        IconName::Eye
    } else {
        IconName::EyeOff
    };
    let shortcut = match kind {
        SlotKind::Tree => "Ctrl+Alt+1",
        SlotKind::Editor => "Ctrl+Alt+2",
        SlotKind::Pdf => "Ctrl+Alt+3 / Ctrl+Alt+V",
        SlotKind::Chat => "Ctrl+Alt+4",
    };
    let tooltip = format!(
        "{} ({shortcut})",
        rust_i18n::t!("ui.app.toolbar.toggle_slot", name = kind.title())
    );
    icon_button(
        cx,
        format!("toolbar-slot-{}", kind.id()),
        icon,
        tooltip,
        !collapsed,
        on_click,
    )
}

/// Indicador de estado del motor LaTeX (icono + texto; el detalle va en el
/// bloque de motor de la status bar).
pub fn engine_indicator(cx: &App, label: &str, available: bool) -> impl IntoElement {
    let (icon, color) = if available {
        (IconName::CircleCheck, cx.theme().success)
    } else {
        (IconName::TriangleAlert, cx.theme().warning)
    };
    div()
        .id("toolbar-engine")
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(icon).size(px(12.)).text_color(color))
        .child(label.to_owned())
}

/// Punto de color para el estado de la IA (dot de la status bar).
pub fn dot(color: Hsla) -> impl IntoElement {
    div().size(px(8.)).rounded_full().bg(color).flex_none()
}

/// Separador vertical fino entre grupos de la toolbar.
pub fn separator(cx: &App) -> impl IntoElement {
    div().w(px(1.)).h(px(18.)).bg(cx.theme().border).flex_none()
}

/// Variante de botón de compilar (contorno + icono Play).
pub fn compile_button(
    label: String,
    busy: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    let icon = if busy {
        IconName::Loader
    } else {
        IconName::Play
    };
    Button::new("toolbar-compile")
        .icon(icon)
        .label(label)
        .small()
        .outline()
        .tab_stop(false)
        .on_click(on_click)
}

/// Botón con menú dropdown anclado (para "proyecto ▾").
pub fn custom_menu_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(cx.theme().foreground)
        .hover(cx.theme().foreground.opacity(0.08))
        .active(cx.theme().foreground.opacity(0.13))
}
