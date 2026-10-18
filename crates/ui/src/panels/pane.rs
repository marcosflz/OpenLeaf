//! Contrato común de paneles del shell (M1-A1/A2).
//!
//! Cada panel es una entidad GPUI que implementa las dos mitades del panel
//! del kit (`gpui_base::dock::Panel` + `component::dock::Panel`) y, además,
//! [`Pane`]: el contrato pequeño del shell (visibilidad + activación) que
//! evita duplicar el chrome entre hojas.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{BasePanel, DockArea, Panel as DockPanel},
};
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _, Render,
    SharedString, Styled as _, WeakEntity, Window, div,
};

use crate::layout::SlotKind;

/// Mensajes de un panel hacia el shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneEvent {
    /// El panel pasó a ser la pestaña activa de su grupo (lo decide el dock).
    Activated(SlotKind),
    /// El usuario colapsó/expandió la hoja con su botón de ojo.
    VisibilityChanged { kind: SlotKind, visible: bool },
    /// El panel pide al shell el diálogo "abrir proyecto".
    OpenProjectRequested,
}

/// Estado compartido de chrome: visibilidad, foco y referencia al dock.
pub struct PaneState {
    kind: SlotKind,
    visible: bool,
    active: bool,
    focus: FocusHandle,
    dock_area: Option<WeakEntity<DockArea>>,
}

impl PaneState {
    /// Crea el estado de chrome de una hoja.
    pub fn new<T>(
        kind: SlotKind,
        dock_area: Option<WeakEntity<DockArea>>,
        cx: &mut Context<T>,
    ) -> Self {
        Self {
            kind,
            visible: true,
            active: false,
            focus: cx.focus_handle(),
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

    /// Si la hoja es la pestaña activa de su grupo.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Handle de foco del panel.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Muestra u oculta la hoja **sin** avisar al shell (lo usa el shell al
    /// aplicar un layout; emitir allí sería reentrante).
    ///
    /// Notifica también al `DockArea`: la visibilidad se decide al pintar el
    /// área, no dentro del panel (I-7).
    pub fn set_visible<T: Render>(&mut self, visible: bool, cx: &mut Context<T>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if let Some(area) = &self.dock_area
            && let Err(err) = area.update(cx, |_, cx| cx.notify())
        {
            // El dock puede no existir durante la construcción o en tests
            // puros; no es un error accionable para el usuario.
            tracing::debug!(slot = self.kind.id(), error = %err, "dock no disponible");
        }
        cx.notify();
    }

    /// Foco de teclado de la hoja al entrar en ella (focus mode `Ctrl+Alt+M`).
    ///
    /// No roba el foco a un control interno (el editor del panel Editor, el
    /// composer del chat): el clic ya lo enfocó, y quedarse con el foco a nivel
    /// de hoja dejaba al usuario sin poder escribir. Solo toma el foco si
    /// estaba fuera de la hoja.
    pub fn focus_if_outside<T: Render>(&self, window: &mut Window, cx: &mut Context<T>) {
        if !self.focus.contains_focused(window, cx) {
            self.focus.focus(window, cx);
        }
    }

    /// Marca la hoja como (in)activa; solo repinta.
    pub fn set_active<T: Render>(&mut self, active: bool, cx: &mut Context<T>) {
        if self.active == active {
            return;
        }
        self.active = active;
        cx.notify();
    }
}

/// Contrato del shell para una hoja.
pub trait Pane:
    Render + EventEmitter<PaneEvent> + BasePanel + DockPanel + Focusable + Sized + 'static
{
    /// Estado de chrome.
    fn pane_state(&self) -> &PaneState;

    /// Estado de chrome (mutable).
    fn pane_state_mut(&mut self) -> &mut PaneState;

    /// Invierte la visibilidad por acción del usuario y avisa al shell.
    fn toggle_visible_by_user(&mut self, cx: &mut Context<Self>) {
        let next = !self.pane_state().is_visible();
        self.pane_state_mut().set_visible(next, cx);
        let kind = self.pane_state().kind();
        cx.emit(PaneEvent::VisibilityChanged {
            kind,
            visible: next,
        });
    }

    /// El usuario ha hecho clic dentro de la hoja: le damos el foco y la
    /// marcamos como panel activo (lo usa el focus mode `Ctrl+Alt+M`).
    ///
    /// No se usa `set_active` del dock para esto: con 4 grupos de una sola
    /// hoja, todas las hojas son "la activa de su grupo" a la vez.
    fn activate_by_user(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.pane_state().kind();
        self.pane_state().focus_if_outside(window, cx);
        cx.emit(PaneEvent::Activated(kind));
    }

    /// Botón de ojo del toolbar del panel.
    fn collapse_button(&self, cx: &Context<Self>) -> Button {
        let kind = self.pane_state().kind();
        let visible = self.pane_state().is_visible();
        let icon = if visible {
            IconName::EyeOff
        } else {
            IconName::Eye
        };
        let tooltip: SharedString =
            rust_i18n::t!("ui.app.toolbar.toggle_slot", name = kind.title())
                .to_string()
                .into();
        Button::new(SharedString::from(format!("pane-collapse-{}", kind.id())))
            .icon(icon)
            .ghost()
            .xsmall()
            .tab_stop(false)
            .selected(!visible)
            .tooltip(tooltip)
            .on_click(cx.listener(|this, _, _window, cx| this.toggle_visible_by_user(cx)))
    }

    /// Implementación compartida de `BasePanel::visible`.
    fn pane_visible(&self, _cx: &App) -> bool {
        self.pane_state().is_visible()
    }

    /// Las 4 hojas del shell son fijas: no se cierran (I-7).
    fn pane_closable(&self, _cx: &App) -> bool {
        false
    }

    /// Título del tab / título simple del grupo.
    fn pane_title(&self) -> String {
        self.pane_state().kind().title()
    }
}

/// Contenedor raíz común de un panel (fondo y color de texto del tema).
pub(crate) fn pane_container(kind: SlotKind, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(SharedString::from(format!("pane-{}", kind.id())))
        .size_full()
        .bg(cx.theme().background)
        .text_color(cx.theme().foreground)
}
