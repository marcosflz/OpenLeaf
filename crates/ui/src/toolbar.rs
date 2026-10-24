//! Widgets de la toolbar del shell (M1-A2).
//!
//! Funciones puras de presentación: el shell decide qué hacen los botones y
//! esta capa fija el estilo (icon-only, ghost, 24 px, tooltip siempre).
//! También vive aquí la estructura del menú del proyecto (M1-F1/F4).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::{App, ClickEvent, Hsla, IntoElement, Styled as _, Window, div, px};

use crate::layout::SlotKind;
use crate::project::ProjectInfo;

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
    // Icono descriptivo del panel, no un ojo: el estado se comunica con el
    // tooltip y con `selected` (activo = visible).
    let icon = match kind {
        SlotKind::Tree => IconName::FolderTree,
        SlotKind::Editor => IconName::FileCode,
        SlotKind::Pdf => IconName::BookOpen,
        SlotKind::Chat => IconName::Bot,
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

/// Indicador de estado del motor LaTeX (icono + texto) que abre el detalle al
/// pulsarlo. El tooltip explica el estado y la acción.
pub fn engine_indicator(
    cx: &App,
    label: &str,
    available: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    let (icon, color, hint) = if available {
        (
            IconName::CircleCheck,
            cx.theme().success,
            "Motor LaTeX disponible. Pulsa para ver la versión y la ruta.",
        )
    } else {
        (
            IconName::TriangleAlert,
            cx.theme().warning,
            "No hay un motor LaTeX utilizable. Pulsa para reinstalar o revisar la ruta.",
        )
    };
    Button::new("toolbar-engine")
        .ghost()
        .small()
        .icon(Icon::new(icon).size(px(12.)).text_color(color))
        .label(label.to_owned())
        .tooltip(hint)
        .on_click(on_click)
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

/// Callback que abre UI (diálogo o wizard) desde el menú.
type OpenDialogCallback = dyn Fn(&mut Window, &mut App);

/// Callback que abre una raíz del historial de recientes.
type OpenRecentCallback = dyn Fn(PathBuf, &mut Window, &mut App);

/// Callbacks del menú del proyecto (los conecta el shell con sus métodos).
pub struct ProjectMenuCallbacks {
    /// Abre el diálogo «Abrir proyecto».
    pub on_open_dialog: Rc<OpenDialogCallback>,
    /// Abre el wizard «Nuevo proyecto».
    pub on_new_project: Rc<OpenDialogCallback>,
    /// Abre una raíz de la lista de recientes.
    pub on_open_recent: Rc<OpenRecentCallback>,
}

impl ProjectMenuCallbacks {
    /// Envuelve las clausuras del shell.
    pub fn new(
        on_open_dialog: impl Fn(&mut Window, &mut App) + 'static,
        on_new_project: impl Fn(&mut Window, &mut App) + 'static,
        on_open_recent: impl Fn(PathBuf, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            on_open_dialog: Rc::new(on_open_dialog),
            on_new_project: Rc::new(on_new_project),
            on_open_recent: Rc::new(on_open_recent),
        }
    }
}

/// Estructura completa del menú del proyecto (M1-F1/F4): «Abrir carpeta…»,
/// «Nuevo proyecto…» y el submenú «Recientes» (con ✓ en el activo).
///
/// - `recents`: proyectos recientes vivos (`ProjectService::candidates`).
/// - `current`: raíz del proyecto activo.
/// - `callbacks`: acciones del shell ([`ProjectMenuCallbacks`]).
///
/// **Punto de enganche del shell** para el que edita `app.rs`: hoy
/// `render_toolbar` construye este menú inline; sustituir ese bloque por
/// `toolbar::project_menu(window, cx, menu, &recents, current, callbacks)`
/// deja «Nuevo proyecto…» disponible junto a «Abrir carpeta…» sin duplicar
/// estructura ni iconos.
pub fn project_menu(
    window: &mut Window,
    cx: &mut App,
    menu: PopupMenu,
    recents: &[ProjectInfo],
    current: Option<&Path>,
    callbacks: ProjectMenuCallbacks,
) -> PopupMenu {
    let ProjectMenuCallbacks {
        on_open_dialog,
        on_new_project,
        on_open_recent,
    } = callbacks;
    let mut menu = menu.item(
        PopupMenuItem::new(rust_i18n::t!("ui.app.toolbar.open_folder").to_string())
            .icon(IconName::FolderOpen)
            .on_click(move |_, window, cx| on_open_dialog(window, cx)),
    );
    menu = menu.item(
        PopupMenuItem::new(rust_i18n::t!("ui.app.toolbar.new_project").to_string())
            .icon(IconName::FolderPlus)
            .on_click(move |_, window, cx| on_new_project(window, cx)),
    );

    if recents.is_empty() {
        return menu.separator().item(
            PopupMenuItem::new(rust_i18n::t!("ui.app.toolbar.no_recents").to_string())
                .disabled(true),
        );
    }

    let recents: Vec<ProjectInfo> = recents.to_vec();
    let current = current.map(Path::to_path_buf);
    let submenu = PopupMenu::build(window, cx, move |mut sub, _window, _cx| {
        for info in &recents {
            let on_open = on_open_recent.clone();
            let path = info.root.clone();
            let is_current = current.as_deref() == Some(path.as_path());
            sub = sub.item(
                PopupMenuItem::new(info.name.clone())
                    .icon(IconName::FolderClosed)
                    .checked(is_current)
                    .on_click(move |_, window, cx| on_open(path.clone(), window, cx)),
            );
        }
        sub
    });
    menu.separator().item(
        PopupMenuItem::submenu(rust_i18n::t!("ui.app.toolbar.recents").to_string(), submenu)
            .icon(IconName::Clock),
    )
}
