//! `SpikeShell` — ventana del spike M0-S1: dock de 4 hojas + title bar + status bar.
//!
//! Composición (de arriba abajo):
//!
//! ```text
//! TitleBar (controles de ventana del kit)
//!   ├─ toggles por hoja (mostrar/colapsar)   [spike]
//!   ├─ título "LaTeX Studio — spike S1"
//!   └─ Guardar / Restaurar layout            [spike]
//! DockArea  h_split [árbol | editor | pdf | chat]   ← resizable, 4 hojas fijas
//! StatusBar "Ln 1, Col 1 · listo"            + estado del layout
//! ```
//!
//! Persistencia: [`SpikeLayout`] (esquema estable de `INTERFACES.md` §7) a un
//! JSON de demo. En M1 el blob vive en `config/settings.json → ui.dock`; aquí
//! se escribe en un archivo temporal (o `LSX_SPIKE_LAYOUT`) para no tocar la
//! config del usuario. `DockAreaState` nativo del kit (con los `PanelState` de
//! cada hoja) es la otra vía de persistencia; `SpikeShell` registra un builder
//! en `PanelRegistry` para poder reconstruir hojas con `DockArea::load`.

use std::path::PathBuf;

use gpui_kit::component::{
    ActiveTheme as _, IconName, Selectable as _, Sizable as _, StyledExt as _, TitleBar,
    button::{Button, ButtonVariants as _},
    dock::{DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, PanelInfo, panel_handle},
    h_flex,
    status_bar::StatusBar,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Pixels, Render,
    SharedString, Styled as _, Subscription, Window, div, px,
};

use crate::spike::layout::{SlotKind, SpikeLayout};
use crate::spike::panel::SpikePanel;

/// Ancho de referencia cuando el dock aún no se ha medido (primer frame).
/// El split reescala sus ratios al medirse, así que el valor es solo un ancla.
const DEFAULT_REFERENCE_WIDTH: Pixels = px(1600.);

/// Ruta del JSON de demo del spike.
///
/// `LSX_SPIKE_LAYOUT` la sobrescribe; por defecto usa un archivo temporal. No
/// usa `config/`: el guardado real es de M1.
pub fn demo_layout_path() -> PathBuf {
    if let Some(path) = std::env::var_os("LSX_SPIKE_LAYOUT") {
        return PathBuf::from(path);
    }
    std::env::temp_dir().join("lsx-spike-s1-layout.json")
}

/// Vista raíz del spike.
pub struct SpikeShell {
    dock: Entity<DockArea>,
    panels: [Entity<SpikePanel>; 4],
    layout_path: PathBuf,
    status: SharedString,
    dirty: bool,
    _dock_events: Subscription,
    /// Observadores de cada hoja: cualquier cambio de visibilidad marca el
    /// layout como modificado, venga del botón de la title bar o de la toolbar
    /// del propio panel.
    _panel_observers: Vec<Subscription>,
}

impl SpikeShell {
    /// Shell con el JSON de demo por defecto ([`demo_layout_path`]).
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_layout_path(window, cx, demo_layout_path())
    }

    /// Shell que guarda/restaura su layout en `layout_path`.
    pub fn with_layout_path(
        window: &mut Window,
        cx: &mut Context<Self>,
        layout_path: PathBuf,
    ) -> Self {
        let (dock, _skin) = DockSkin::dock_area("spike-shell-dock", Some(1), window, cx);

        let panels: [Entity<SpikePanel>; 4] = SlotKind::ALL.map(|kind| {
            let dock_area = dock.downgrade();
            cx.new(|cx| SpikePanel::new(kind, Some(dock_area.clone()), cx))
        });

        let mut layout = DockLayout::h_split();
        for kind in SlotKind::ALL {
            layout = layout.child(
                DockLayout::tabs().panel_view(panel_handle(panels[kind.index()].clone()), cx),
                None,
            );
        }
        dock.update(cx, |dock, cx| {
            dock.set_center(layout, window, cx);
            // I-7 v1: 4 hojas fijas, sin drag de pestañas entre grupos. El
            // lock del dock desactiva drag/drop de grupos y conserva el
            // redimensionado de splits.
            dock.set_locked(true, window, cx);
        });

        // Permite que `DockArea::load` reconstruya las hojas por nombre
        // estable (vía nativa del kit; el shell aplica su propio `SpikeLayout`).
        for kind in SlotKind::ALL {
            gpui_kit::component::dock::register_panel(cx, kind.panel_name(), move |ctx, _, cx| {
                let dock_area = ctx.dock_area();
                panel_handle(cx.new(|cx| SpikePanel::new(kind, Some(dock_area.clone()), cx)))
            });
        }

        let dock_events = cx.subscribe(&dock, |this, _dock, event, cx| {
            if matches!(event, DockEvent::LayoutChanged) {
                this.mark_dirty(cx);
            }
        });
        let panel_observers = panels
            .iter()
            .map(|panel| cx.observe(panel, |this, _panel, cx| this.mark_dirty(cx)))
            .collect();

        let mut shell = Self {
            dock,
            panels,
            layout_path,
            status: SharedString::from("layout: por defecto"),
            dirty: false,
            _dock_events: dock_events,
            _panel_observers: panel_observers,
        };
        shell.restore_from_disk(window, cx);
        shell
    }

    /// Entidad del dock (para M1 / tests de integración).
    pub fn dock(&self) -> &Entity<DockArea> {
        &self.dock
    }

    /// Estado actual del layout leído del dock vivo.
    pub fn capture_layout(&self, cx: &App) -> SpikeLayout {
        let dump = self.dock.read(cx).dump(cx);
        let sizes: Vec<f32> = match &dump.center.info {
            PanelInfo::Stack { sizes, .. } => sizes.iter().map(|size| f32::from(*size)).collect(),
            _ => Vec::new(),
        };
        let total: f32 = sizes.iter().sum();
        let observed: Vec<(SlotKind, f32, bool)> = dump
            .center
            .children
            .iter()
            .enumerate()
            .filter_map(|(ix, group)| {
                let leaf = group.children.first()?;
                let kind = SlotKind::from_panel_name(&leaf.panel_name)?;
                let fraction = if total > 0.0 {
                    sizes.get(ix).copied().unwrap_or(0.0) / total
                } else {
                    0.0
                };
                let collapsed = !self.panels[kind.index()].read(cx).is_visible();
                Some((kind, fraction, collapsed))
            })
            .collect();
        SpikeLayout::from_observed(&observed)
    }

    /// Aplica un layout: visibilidad por hoja y anchos del split central.
    pub fn apply_layout(
        &mut self,
        layout: &SpikeLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for state in &layout.panels {
            let Some(kind) = SlotKind::from_id(&state.id) else {
                continue;
            };
            let visible = !state.collapsed;
            let panel = self.panels[kind.index()].clone();
            panel.update(cx, |panel, cx| panel.set_visible(visible, cx));
        }

        let measured = self.dock.read(cx).bounds().size.width;
        let reference = if measured > px(0.) {
            measured
        } else {
            DEFAULT_REFERENCE_WIDTH
        };
        let sizes: Vec<Pixels> = layout
            .sizes_px(f32::from(reference))
            .into_iter()
            .map(px)
            .collect();
        if let Some(node) = self
            .dock
            .read(cx)
            .layout(DockPlacement::Center)
            .map(|tree| tree.root().id())
        {
            self.dock
                .update(cx, |dock, cx| dock.set_split_sizes(node, sizes, window, cx));
        }
        self.dock.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Guarda el layout actual en `layout_path`.
    pub fn save_layout(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let layout = self.capture_layout(cx);
        let result = layout
            .to_json()
            .and_then(|json| std::fs::write(&self.layout_path, json).map_err(Into::into));
        match result {
            Ok(()) => {
                self.dirty = false;
                self.status = SharedString::from(format!(
                    "layout: guardado en {}",
                    self.layout_path.display()
                ));
            }
            Err(err) => {
                self.status = SharedString::from(format!("layout: no se pudo guardar: {err}"));
            }
        }
        cx.notify();
    }

    /// Restaura el layout guardado; sin archivo válido deja el layout por defecto.
    pub fn restore_from_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(text) = std::fs::read_to_string(&self.layout_path) else {
            return;
        };
        match SpikeLayout::from_json(&text) {
            Ok(layout) => {
                self.apply_layout(&layout, window, cx);
                self.dirty = false;
                self.status = SharedString::from(format!(
                    "layout: restaurado de {}",
                    self.layout_path.display()
                ));
            }
            Err(err) => {
                self.status = SharedString::from(format!(
                    "layout: JSON inválido, uso el layout por defecto ({err})"
                ));
            }
        }
        cx.notify();
    }

    /// Muestra u oculta una hoja (botón de la title bar).
    pub fn toggle_slot(&mut self, kind: SlotKind, _window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.panels[kind.index()].clone();
        panel.update(cx, |panel, cx| panel.toggle_visible(cx));
        self.dock.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    fn mark_dirty(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        self.status = SharedString::from("layout: modificado (sin guardar)");
        cx.notify();
    }

    fn render_title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new().child(
            h_flex()
                .flex_1()
                .items_center()
                .gap_1()
                .children(
                    SlotKind::ALL
                        .into_iter()
                        .map(|kind| self.render_slot_toggle(kind, cx)),
                )
                .child(
                    div()
                        .flex_1()
                        .text_center()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("LaTeX Studio — spike S1"),
                )
                .child(
                    Button::new("spike-save-layout")
                        .ghost()
                        .xsmall()
                        .label("Guardar")
                        .tooltip("Guardar el layout del dock en el JSON de demo")
                        .on_click(cx.listener(|this, _, window, cx| this.save_layout(window, cx))),
                )
                .child(
                    Button::new("spike-restore-layout")
                        .ghost()
                        .xsmall()
                        .label("Restaurar")
                        .tooltip("Reaplicar el layout guardado en disco")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.restore_from_disk(window, cx)),
                        ),
                ),
        )
    }

    fn render_slot_toggle(&self, kind: SlotKind, cx: &mut Context<Self>) -> Button {
        let collapsed = !self.panels[kind.index()].read(cx).is_visible();
        let icon = if collapsed {
            IconName::Eye
        } else {
            IconName::EyeOff
        };
        let tooltip = if collapsed {
            format!("Mostrar «{}»", kind.label())
        } else {
            format!("Colapsar «{}»", kind.label())
        };
        Button::new(SharedString::from(format!(
            "spike-slot-toggle-{}",
            kind.id()
        )))
        .icon(icon)
        .ghost()
        .xsmall()
        .tab_stop(false)
        .selected(collapsed)
        .tooltip(tooltip)
        .on_click(cx.listener(move |this, _, window, cx| this.toggle_slot(kind, window, cx)))
    }

    fn render_status_bar(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        StatusBar::new()
            .left("Ln 1, Col 1 · listo")
            .right(self.status.clone())
    }
}

impl Render for SpikeShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(window, cx))
            .child(div().flex_1().min_h_0().child(self.dock.clone()))
            .child(self.render_status_bar(cx))
    }
}

/// Tests del ciclo de persistencia del shell sin ventana.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_path_is_a_json_file_in_a_customizable_location() {
        if std::env::var_os("LSX_SPIKE_LAYOUT").is_none() {
            assert!(demo_layout_path().ends_with("lsx-spike-s1-layout.json"));
        }
    }

    #[test]
    fn a_captured_layout_round_trips_through_the_store_format() {
        // El ciclo que ejecutan Guardar/Restaurar, sin tocar disco.
        let captured = SpikeLayout::from_observed(&[
            (SlotKind::Tree, 0.2, false),
            (SlotKind::Editor, 0.5, false),
            (SlotKind::Pdf, 0.2, true),
            (SlotKind::Chat, 0.1, false),
        ]);
        let json = captured.to_json().expect("serializa");
        let restored = SpikeLayout::from_json(&json).expect("restaura");
        assert_eq!(restored, captured);
        assert!(restored.is_collapsed(SlotKind::Pdf));
        assert_eq!(restored.slot(SlotKind::Editor).unwrap().width_fraction, 0.5);
    }

    #[test]
    fn a_corrupt_saved_file_is_reported_as_an_error() {
        let err = SpikeLayout::from_json("{ no soy el layout }");
        assert!(err.is_err(), "el shell usa four_columns() en ese caso");
    }

    #[test]
    fn a_layout_survives_a_round_trip_through_a_temp_file() {
        // El ciclo real de Guardar/Restaurar contra disco (archivo temporal).
        let path = std::env::temp_dir().join(format!(
            "lsx-spike-s1-roundtrip-{}.json",
            std::process::id()
        ));
        let saved = SpikeLayout::from_observed(&[
            (SlotKind::Tree, 0.15, false),
            (SlotKind::Editor, 0.45, false),
            (SlotKind::Pdf, 0.25, false),
            (SlotKind::Chat, 0.15, true),
        ]);
        let result = saved
            .to_json()
            .and_then(|json| std::fs::write(&path, json).map_err(Into::into))
            .and_then(|()| std::fs::read_to_string(&path).map_err(Into::into))
            .and_then(|text| SpikeLayout::from_json(&text));
        let _ = std::fs::remove_file(&path);

        let restored = result.expect("round-trip por archivo");
        assert_eq!(restored, saved);
        assert!(restored.is_collapsed(SlotKind::Chat));
    }
}
