//! Panel placeholder para las hojas editor/PDF/chat (M1-A1).
//!
//! Hasta que lleguen sus streams (M1-B/C/E) cada hoja muestra quién es y
//! qué construirá, manteniendo el chrome del panel (título, botón de
//! colapso) idéntico al de una hoja real.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, dock::PanelEvent, v_flex};
use gpui_kit::{
    App, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, WeakEntity, Window, div, px,
};

use crate::layout::SlotKind;
use crate::panels::{Pane, PaneEvent, PaneState, pane_container};

/// Hoja sin contenido real todavía.
pub struct PlaceholderPanel {
    state: PaneState,
    icon: IconName,
}

impl PlaceholderPanel {
    /// Crea la hoja `kind`.
    pub fn new(
        kind: SlotKind,
        dock_area: Option<WeakEntity<gpui_kit::component::dock::DockArea>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let icon = match kind {
            SlotKind::Editor => IconName::FileCode,
            SlotKind::Pdf => IconName::FileType,
            SlotKind::Chat => IconName::Bot,
            // El árbol tiene su propio panel; si esto ocurre es un error de wiring.
            SlotKind::Tree => IconName::FolderClosed,
        };
        Self {
            state: PaneState::new(kind, dock_area, cx),
            icon,
        }
    }

    /// Hoja que representa.
    pub fn kind(&self) -> SlotKind {
        self.state.kind()
    }

    /// Si la hoja está visible.
    pub fn is_visible(&self) -> bool {
        self.state.is_visible()
    }

    /// Muestra/oculta sin avisar al shell.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.state.set_visible(visible, cx);
    }
}

impl Pane for PlaceholderPanel {
    fn pane_state(&self) -> &PaneState {
        &self.state
    }

    fn pane_state_mut(&mut self) -> &mut PaneState {
        &mut self.state
    }
}

impl gpui_kit::component::dock::BasePanel for PlaceholderPanel {
    fn panel_name(&self) -> &'static str {
        self.state.kind().panel_name()
    }

    fn visible(&self, cx: &App) -> bool {
        self.pane_visible(cx)
    }

    fn closable(&self, cx: &App) -> bool {
        self.pane_closable(cx)
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        // El dock avisa a la hoja "mostrada" de cada grupo; con grupos de una
        // sola hoja eso es cierto para las cuatro, así que aquí solo se
        // actualiza el estado visual (la activación real viene del clic:
        // `Pane::activate_by_user`).
        self.state.set_active(active, cx);
    }
}

impl gpui_kit::component::dock::Panel for PlaceholderPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.pane_title().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.pane_title()
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Vec<gpui_kit::component::button::Button>> {
        None
    }
}

impl gpui_kit::EventEmitter<PanelEvent> for PlaceholderPanel {}

impl gpui_kit::EventEmitter<PaneEvent> for PlaceholderPanel {}

impl gpui_kit::Focusable for PlaceholderPanel {
    fn focus_handle(&self, _cx: &App) -> gpui_kit::FocusHandle {
        self.state.focus_handle().clone()
    }
}

impl Render for PlaceholderPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.state.kind();
        let (note, stream) = match kind {
            SlotKind::Editor => (rust_i18n::t!("ui.app.panel.editor_note"), "M1-B"),
            SlotKind::Pdf => (rust_i18n::t!("ui.app.panel.pdf_note"), "M1-C"),
            SlotKind::Chat => (rust_i18n::t!("ui.app.panel.chat_note"), "M1-E"),
            SlotKind::Tree => (rust_i18n::t!("ui.app.panel.tree"), "M1-F"),
        };
        let building = rust_i18n::t!("ui.app.panel.building", version = stream).to_string();

        pane_container(kind, cx)
            .flex()
            .items_center()
            .justify_center()
            .track_focus(self.state.focus_handle())
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.activate_by_user(window, cx)),
            )
            .child(
                v_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Icon::new(self.icon)
                            .size_8()
                            .text_color(cx.theme().muted_foreground.opacity(0.6)),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.pane_title()),
                    )
                    .child(
                        div()
                            .max_w(px(420.))
                            .text_sm()
                            .text_center()
                            .text_color(cx.theme().muted_foreground)
                            .child(note.to_string()),
                    )
                    .child(
                        div()
                            .px_2()
                            .py(px(2.))
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted)
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(building),
                    ),
            )
    }
}
