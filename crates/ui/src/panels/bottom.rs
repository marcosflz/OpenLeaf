//! `BottomPanel` — dock inferior de la columna del Explorador (M2): pestañas
//! **Problemas · Salida · Símbolos** en una sola ventana, colapsable.
//!
//! Reúne lo que antes vivía repartido: el panel de problemas colgaba del
//! editor y la SALIDA era una sección del propio árbol. Ahora el árbol lo
//! embebe como hijo (`TreePanel::attach_bottom`) y le reenvía los eventos del
//! watcher; el editor se queda solo con el documento.
//!
//! `BottomPanel` no conoce el shell: problemas y símbolos son entidades que
//! le pasa `AppRoot`, y la única API de escritura es [`BottomPanel::push_output`].

use std::collections::VecDeque;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::panels::problems::ProblemsPanel;
use crate::panels::symbols::SymbolsPanel;

/// Máximo de líneas del log de SALIDA.
pub const LOG_CAP: usize = 200;
/// Filas visibles del log (más recientes primero).
pub const LOG_ROWS: usize = 40;

/// Pestañas del dock inferior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BottomTab {
    /// Diagnósticos de la última compilación.
    #[default]
    Problems,
    /// Log del watcher/acciones (la antigua SALIDA del árbol).
    Output,
    /// Paleta de símbolos LaTeX.
    Symbols,
}

impl BottomTab {
    /// Todas las pestañas, en orden.
    pub const ALL: [Self; 3] = [Self::Problems, Self::Output, Self::Symbols];

    /// Etiqueta de la pestaña.
    pub fn label(self) -> &'static str {
        match self {
            Self::Problems => "PROBLEMAS",
            Self::Output => "SALIDA",
            Self::Symbols => "SÍMBOLOS",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Problems => "problems",
            Self::Output => "output",
            Self::Symbols => "symbols",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Problems => IconName::TriangleAlert,
            Self::Output => IconName::Terminal,
            Self::Symbols => IconName::Sigma,
        }
    }
}

/// Dock inferior del explorador.
pub struct BottomPanel {
    problems: Entity<ProblemsPanel>,
    symbols: Entity<SymbolsPanel>,
    log: VecDeque<String>,
    tab: BottomTab,
    open: bool,
}

impl BottomPanel {
    /// Crea el dock con las entidades que le pasa el shell.
    pub fn new(
        problems: Entity<ProblemsPanel>,
        symbols: Entity<SymbolsPanel>,
        cx: &mut Context<Self>,
    ) -> Self {
        // La pestaña activa sigue los diagnósticos: si hay errores, PROBLEMAS.
        cx.observe(&problems, |this, problems, cx| {
            if problems.read(cx).total_counts().0 > 0 {
                this.tab = BottomTab::Problems;
                this.open = true;
            }
            cx.notify();
        })
        .detach();
        Self {
            problems,
            symbols,
            log: VecDeque::new(),
            tab: BottomTab::Problems,
            open: true,
        }
    }

    /// Añade una línea al log de SALIDA (la llama el árbol).
    pub fn push_output(&mut self, line: impl Into<String>, cx: &mut Context<Self>) {
        self.log.push_back(line.into());
        while self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
        // Solo repinta si SALIDA está a la vista: el log del watcher es ruidoso.
        if self.open && self.tab == BottomTab::Output {
            cx.notify();
        }
    }

    /// Vacía el log de SALIDA.
    pub fn clear_output(&mut self, cx: &mut Context<Self>) {
        self.log.clear();
        cx.notify();
    }

    /// `true` si el dock está desplegado (tests).
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Pestaña activa (tests).
    pub fn tab(&self) -> BottomTab {
        self.tab
    }

    /// `true` si hay líneas en SALIDA (tests).
    pub fn output_lines(&self) -> usize {
        self.log.len()
    }

    /// Despliega o colapsa el dock (el botón del árbol y la propia pestaña).
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        cx.notify();
    }

    /// Alterna desplegado/colapsado.
    pub fn toggle_open(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open;
        cx.notify();
    }

    /// Activa una pestaña (y despliega si estaba colapsado).
    pub fn set_tab(&mut self, tab: BottomTab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.open = true;
        cx.notify();
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (errors, warnings, _) = self.problems.read(cx).total_counts();
        h_flex()
            .id("bottom-tabs")
            .w_full()
            .h(px(26.))
            .flex_none()
            .items_center()
            .gap(px(2.))
            .px(px(4.))
            .border_b_1()
            .border_color(cx.theme().border)
            .children(BottomTab::ALL.into_iter().map(|tab| {
                let selected = self.open && tab == self.tab;
                let counter = match tab {
                    BottomTab::Problems if errors + warnings > 0 => {
                        Some((errors, warnings, errors > 0))
                    }
                    _ => None,
                };
                h_flex()
                    .id(SharedString::from(format!("bottom-tab-{}", tab.id())))
                    .flex_none()
                    .gap(px(4.))
                    .items_center()
                    .px(px(6.))
                    .py(px(3.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .when(selected, |this| {
                        this.bg(cx.theme().foreground.opacity(0.08))
                    })
                    .hover(|style| style.bg(cx.theme().foreground.opacity(0.06)))
                    .child(Icon::new(tab.icon()).size(px(12.)).text_color(if selected {
                        cx.theme().foreground
                    } else {
                        cx.theme().muted_foreground
                    }))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if selected {
                                cx.theme().foreground
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(tab.label()),
                    )
                    .when_some(counter, |this, (errors, warnings, has_errors)| {
                        this.child(
                            div()
                                .text_size(px(10.))
                                .text_color(if has_errors {
                                    cx.theme().danger
                                } else {
                                    cx.theme().warning
                                })
                                .child(if errors > 0 {
                                    format!("{errors}")
                                } else {
                                    format!("{warnings}")
                                }),
                        )
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        // Clic en la pestaña activa = colapsar/desplegar.
                        if this.tab == tab && this.open {
                            this.toggle_open(cx);
                        } else {
                            this.set_tab(tab, cx);
                        }
                    }))
            }))
            .child(div().flex_1())
            .when(self.tab == BottomTab::Output && self.open, |this| {
                this.child(
                    Button::new("bottom-clear-output")
                        .xsmall()
                        .ghost()
                        .compact()
                        .icon(IconName::Trash)
                        .tooltip("Vaciar SALIDA")
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                            this.clear_output(cx);
                        })),
                )
            })
            .child(
                Button::new("bottom-collapse")
                    .xsmall()
                    .ghost()
                    .compact()
                    .icon(if self.open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronUp
                    })
                    .tooltip(if self.open {
                        "Colapsar la ventana inferior"
                    } else {
                        "Desplegar la ventana inferior"
                    })
                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                        this.toggle_open(cx);
                    })),
            )
    }

    fn render_output(&self, cx: &App) -> AnyElement {
        let mono = cx.theme().mono_font_family.clone();
        let lines: Vec<&String> = self.log.iter().rev().take(LOG_ROWS).collect();
        v_flex()
            .id("bottom-output")
            .size_full()
            .overflow_y_scroll()
            .px(px(8.))
            .py(px(4.))
            .gap(px(1.))
            .font_family(mono)
            .text_size(px(11.))
            .text_color(cx.theme().muted_foreground)
            .children(lines.into_iter().map(|line| {
                div()
                    .id(SharedString::from(format!("output-{line}")))
                    .whitespace_nowrap()
                    .child(line.clone())
            }))
            .into_any_element()
    }
}

impl Render for BottomPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body: AnyElement = match self.tab {
            BottomTab::Problems => self.problems.clone().into_any_element(),
            BottomTab::Output => self.render_output(cx),
            BottomTab::Symbols => self.symbols.clone().into_any_element(),
        };
        v_flex()
            .id("bottom-panel")
            .w_full()
            .h_full()
            .flex_none()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(self.render_tabs(cx))
            .when(self.open, |this| {
                this.child(div().flex_1().min_h_0().w_full().child(body))
            })
    }
}

#[cfg(test)]
mod tests {
    // Sin glob: `gpui_kit::*` exporta el macro `test` de GPUI y sombrearía el
    // `#[test]` estándar en este módulo.
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
        TestAppContext, VisualTestContext, Window, div,
    };

    use super::{BottomPanel, BottomTab, LOG_CAP};
    use crate::panels::problems::ProblemsPanel;
    use crate::panels::symbols::SymbolsPanel;

    struct Host {
        bottom: Entity<BottomPanel>,
    }

    impl Render for Host {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.bottom.clone())
        }
    }

    fn with_bottom<R>(
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut VisualTestContext, Entity<BottomPanel>) -> R,
    ) -> R {
        cx.update(gpui_kit::init);
        let mut bottom = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let problems = cx.new(|cx| ProblemsPanel::new(window, cx));
            let symbols = cx.new(|cx| SymbolsPanel::new(window, None, cx));
            let panel = cx.new(|cx| BottomPanel::new(problems, symbols, cx));
            bottom = Some(panel.clone());
            Host { bottom: panel }
        });
        let bottom = bottom.unwrap();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        f(cx, bottom)
    }

    /// El dock arranca en PROBLEMAS, acumula SALIDA acotada y colapsa/despliega.
    #[test]
    fn el_dock_inferior_cambia_de_pestana_y_acota_la_salida() {
        with_bottom(&mut TestAppContext::single(), |cx, bottom| {
            let (open, tab) = cx.read(|cx| (bottom.read(cx).is_open(), bottom.read(cx).tab()));
            assert!(open, "arranca desplegado");
            assert_eq!(tab, BottomTab::Problems, "arranca en PROBLEMAS");

            // La SALIDA acumula líneas y se acota al tope.
            for line in 0..LOG_CAP + 25 {
                cx.update(|_, cx| {
                    bottom.update(cx, |panel, cx| {
                        panel.push_output(format!("línea {line}"), cx)
                    });
                });
            }
            assert_eq!(cx.read(|cx| bottom.read(cx).output_lines()), LOG_CAP);

            // Colapsar desde el encabezado y reabrir en SALIDA.
            cx.update(|_, cx| bottom.update(cx, |panel, cx| panel.toggle_open(cx)));
            assert!(!cx.read(|cx| bottom.read(cx).is_open()));
            cx.update(|_, cx| {
                bottom.update(cx, |panel, cx| panel.set_tab(BottomTab::Output, cx));
            });
            let (open, tab) = cx.read(|cx| (bottom.read(cx).is_open(), bottom.read(cx).tab()));
            assert!(open, "cambiar de pestaña despliega");
            assert_eq!(tab, BottomTab::Output);

            // Se pinta sin pánico con cada pestaña activa.
            for tab in BottomTab::ALL {
                cx.update(|_, cx| bottom.update(cx, |panel, cx| panel.set_tab(tab, cx)));
                cx.update(|window, cx| window.render_frame(cx));
            }
        });
    }
}
