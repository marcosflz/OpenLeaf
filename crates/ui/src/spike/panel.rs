//! Panel placeholder del spike M0-S1.
//!
//! Implementa las dos mitades del contrato de paneles del kit:
//!
//! - `gpui_base::dock::Panel` (comportamiento) — nombre estable, visibilidad,
//!   y `dump` para la persistencia nativa del dock;
//! - `gpui_component::dock::Panel` (presentación) — título, `tab_name` y
//!   botones de toolbar.
//!
//! **Colapso**: el `DockArea` del kit no ofrece plegado para hojas del split
//! central (el `toggle_dock` nativo existe solo para los docks Left/Right/
//! Bottom). La vía que sí ofrece es [`BasePanel::visible`]: una hoja oculta
//! conserva su sitio en el árbol pero el contenedor cede su slot
//! (`DockArea::render_node` → `is_node_visible`). El botón de ojo de la
//! toolbar usa ese mecanismo; como al ocultarse desaparece el propio chrome,
//! el shell ofrece además un toggle por hoja en la title bar (ver `shell.rs`).
//!
//! El panel necesita un `WeakEntity<DockArea>` para pedirle un repintado: la
//! visibilidad se decide al pintar el área, no dentro del panel.

use gpui_kit::component::{
    ActiveTheme as _, IconName,
    button::Button,
    dock::{BasePanel, DockArea, Panel as DockPanel, PanelEvent},
};
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, WeakEntity, Window, div,
};

use crate::spike::layout::SlotKind;

/// Hoja placeholder del spike, con visibilidad conmutable.
pub struct SpikePanel {
    kind: SlotKind,
    focus: FocusHandle,
    visible: bool,
    dock_area: Option<WeakEntity<DockArea>>,
}

impl SpikePanel {
    /// Crea la hoja `kind`; `dock_area` permite notificar al dock al cambiar
    /// la visibilidad (puede ser `None` en tests puros).
    pub fn new(
        kind: SlotKind,
        dock_area: Option<WeakEntity<DockArea>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            kind,
            focus: cx.focus_handle(),
            visible: true,
            dock_area,
        }
    }

    /// Hoja que representa este panel.
    pub fn kind(&self) -> SlotKind {
        self.kind
    }

    /// Si la hoja está actualmente en pantalla.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Muestra u oculta la hoja; si cambia, avisa al dock y repinta.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if let Some(area) = &self.dock_area
            && let Err(err) = area.update(cx, |_, cx| cx.notify())
        {
            // El dock puede no existir todavía durante la construcción o en
            // tests puros; no es un error accionable para el usuario.
            tracing::debug!(slot = self.kind.id(), error = %err, "dock no disponible");
        }
        cx.notify();
    }

    /// Invierte la visibilidad de la hoja.
    pub fn toggle_visible(&mut self, cx: &mut Context<Self>) {
        let next = !self.visible;
        self.set_visible(next, cx);
    }

    fn collapse_button(&self, cx: &Context<Self>) -> Button {
        let icon = if self.visible {
            IconName::EyeOff
        } else {
            IconName::Eye
        };
        let tooltip: SharedString = if self.visible {
            format!("Colapsar «{}»", self.kind.label()).into()
        } else {
            format!("Mostrar «{}»", self.kind.label()).into()
        };
        let id = SharedString::from(format!("spike-panel-collapse-{}", self.kind.id()));
        Button::new(id)
            .icon(icon)
            .tooltip(tooltip)
            .on_click(cx.listener(|this, _, _window, cx| this.toggle_visible(cx)))
    }
}

impl BasePanel for SpikePanel {
    fn panel_name(&self) -> &'static str {
        self.kind.panel_name()
    }

    fn visible(&self, _cx: &App) -> bool {
        self.visible
    }

    /// Las 4 hojas del spike son fijas: no se cierran (I-7).
    fn closable(&self, _cx: &App) -> bool {
        false
    }
}

impl DockPanel for SpikePanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.kind.label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.kind.label()
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        Some(vec![self.collapse_button(cx)])
    }
}

impl EventEmitter<PanelEvent> for SpikePanel {}

impl Focusable for SpikePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SpikePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(SharedString::from(format!(
                "spike-panel-{}",
                self.kind.id()
            )))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .text_color(cx.theme().muted_foreground)
            .text_xl()
            .track_focus(&self.focus)
            .child(self.kind.placeholder())
    }
}
