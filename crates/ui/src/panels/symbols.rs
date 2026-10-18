//! `SymbolsPanel` — paleta de símbolos LaTeX (M2): rejilla de glifos por
//! categorías con buscador; un clic inserta el comando en el cursor del editor.
//!
//! Los glifos son Unicode (α, ∑, ≤, →): se ven ya "renderizados" sin compilar
//! LaTeX, y el comando real (`\alpha{}`) viaja en la etiqueta y el tooltip. Es
//! el equivalente práctico de la paleta de símbolos de LaTeX Workshop.
//!
//! El panel nunca toca el documento: pide la inserción a [`EditorPanel`]
//! (`insert_active`), que es quien conoce el cursor activo.

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::panels::editor::EditorPanel;

/// Categoría de la paleta (orden de las fichas del encabezado).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolCategory {
    Griegas,
    Operadores,
    Flechas,
    Conjuntos,
    Acentos,
    Entornos,
}

impl SymbolCategory {
    /// Todas las categorías, en orden de presentación.
    pub const ALL: [Self; 6] = [
        Self::Griegas,
        Self::Operadores,
        Self::Flechas,
        Self::Conjuntos,
        Self::Acentos,
        Self::Entornos,
    ];

    /// Etiqueta de la ficha.
    pub fn label(self) -> &'static str {
        match self {
            Self::Griegas => "Griegas",
            Self::Operadores => "Operadores",
            Self::Flechas => "Flechas",
            Self::Conjuntos => "Conjuntos",
            Self::Acentos => "Acentos",
            Self::Entornos => "Entornos",
        }
    }
}

/// Un símbolo: categoría, glifo visible, comando LaTeX y descripción.
#[derive(Debug)]
pub struct Symbol {
    /// Categoría a la que pertenece.
    pub category: SymbolCategory,
    /// Glifo Unicode que se muestra en la rejilla.
    pub glyph: &'static str,
    /// Texto que se inserta en el documento.
    pub insert: &'static str,
    /// Nombre humano (tooltip y búsqueda).
    pub name: &'static str,
}

/// Paleta de símbolos (curada: los que se usan de verdad en apuntes y papers).
pub const SYMBOLS: &[Symbol] = &[
    // --- Griegas ---
    s(Griegas, "α", "\\alpha{}", "alfa"),
    s(Griegas, "β", "\\beta{}", "beta"),
    s(Griegas, "γ", "\\gamma{}", "gamma"),
    s(Griegas, "δ", "\\delta{}", "delta"),
    s(Griegas, "ε", "\\epsilon{}", "epsilon"),
    s(Griegas, "θ", "\\theta{}", "theta"),
    s(Griegas, "λ", "\\lambda{}", "lambda"),
    s(Griegas, "μ", "\\mu{}", "mu"),
    s(Griegas, "π", "\\pi{}", "pi"),
    s(Griegas, "ρ", "\\rho{}", "rho"),
    s(Griegas, "σ", "\\sigma{}", "sigma"),
    s(Griegas, "τ", "\\tau{}", "tau"),
    s(Griegas, "φ", "\\phi{}", "fi"),
    s(Griegas, "χ", "\\chi{}", "chi"),
    s(Griegas, "ψ", "\\psi{}", "psi"),
    s(Griegas, "ω", "\\omega{}", "omega"),
    s(Griegas, "Γ", "\\Gamma{}", "Gamma"),
    s(Griegas, "Δ", "\\Delta{}", "Delta"),
    s(Griegas, "Θ", "\\Theta{}", "Theta"),
    s(Griegas, "Λ", "\\Lambda{}", "Lambda"),
    s(Griegas, "Σ", "\\Sigma{}", "Sigma"),
    s(Griegas, "Φ", "\\Phi{}", "Fi"),
    s(Griegas, "Ω", "\\Omega{}", "Omega"),
    // --- Operadores ---
    s(Operadores, "±", "\\pm{}", "más menos"),
    s(Operadores, "∓", "\\mp{}", "menos más"),
    s(Operadores, "×", "\\times{}", "por"),
    s(Operadores, "÷", "\\div{}", "dividido"),
    s(Operadores, "·", "\\cdot{}", "producto punto"),
    s(Operadores, "∗", "\\ast{}", "asterisco"),
    s(Operadores, "∑", "\\sum_{i=1}^{n}", "sumatorio"),
    s(Operadores, "∏", "\\prod_{i=1}^{n}", "productorio"),
    s(Operadores, "∫", "\\int_{a}^{b}", "integral"),
    s(Operadores, "∮", "\\oint", "integral de contorno"),
    s(Operadores, "∂", "\\partial", "derivada parcial"),
    s(Operadores, "∇", "\\nabla", "nabla"),
    s(Operadores, "∞", "\\infty", "infinito"),
    s(Operadores, "√", "\\sqrt{}", "raíz"),
    s(Operadores, "≈", "\\approx", "aproximado"),
    s(Operadores, "≠", "\\neq", "distinto"),
    s(Operadores, "≤", "\\leq", "menor o igual"),
    s(Operadores, "≥", "\\geq", "mayor o igual"),
    s(Operadores, "≪", "\\ll", "mucho menor"),
    s(Operadores, "≫", "\\gg", "mucho mayor"),
    s(Operadores, "∼", "\\sim", "semejante"),
    s(Operadores, "∝", "\\propto", "proporcional"),
    // --- Flechas ---
    s(Flechas, "→", "\\to", "flecha"),
    s(Flechas, "←", "\\leftarrow", "flecha izquierda"),
    s(Flechas, "↔", "\\leftrightarrow", "bidireccional"),
    s(Flechas, "⇒", "\\Rightarrow", "implica"),
    s(Flechas, "⇐", "\\Leftarrow", "se deduce"),
    s(Flechas, "⇔", "\\Leftrightarrow", "si y solo si"),
    s(Flechas, "↦", "\\mapsto", "mapea a"),
    s(Flechas, "↑", "\\uparrow", "flecha arriba"),
    s(Flechas, "↓", "\\downarrow", "flecha abajo"),
    // --- Conjuntos y lógica ---
    s(Conjuntos, "∈", "\\in", "pertenece"),
    s(Conjuntos, "∉", "\\notin", "no pertenece"),
    s(Conjuntos, "⊂", "\\subset", "subconjunto"),
    s(Conjuntos, "⊆", "\\subseteq", "subconjunto o igual"),
    s(Conjuntos, "∪", "\\cup", "unión"),
    s(Conjuntos, "∩", "\\cap", "intersección"),
    s(Conjuntos, "∅", "\\emptyset", "vacío"),
    s(Conjuntos, "∀", "\\forall", "para todo"),
    s(Conjuntos, "∃", "\\exists", "existe"),
    s(Conjuntos, "¬", "\\neg", "no"),
    s(Conjuntos, "∧", "\\land", "y lógico"),
    s(Conjuntos, "∨", "\\lor", "o lógico"),
    s(Conjuntos, "ℝ", "\\mathbb{R}", "reales"),
    s(Conjuntos, "ℕ", "\\mathbb{N}", "naturales"),
    s(Conjuntos, "ℤ", "\\mathbb{Z}", "enteros"),
    s(Conjuntos, "ℂ", "\\mathbb{C}", "complejos"),
    // --- Acentos y estructura ---
    s(Acentos, "á", "\\'a", "acento agudo"),
    s(Acentos, "ñ", "\\~n", "tilde"),
    s(Acentos, "ü", "\\\"u", "diéresis"),
    s(Acentos, "â", "\\^a", "circunflejo"),
    s(Acentos, "ā", "\\=a", "macrón"),
    s(Acentos, "x̂", "\\hat{x}", "sombrero"),
    s(Acentos, "x̄", "\\bar{x}", "barra"),
    s(Acentos, "x⃗", "\\vec{x}", "vector"),
    s(Acentos, "ẋ", "\\dot{x}", "punto"),
    s(Acentos, "x̃", "\\tilde{x}", "tilde matemática"),
    // --- Entornos (bloques de un clic) ---
    s(
        Entornos,
        "•",
        "\\begin{itemize}\n  \\item \n\\end{itemize}\n",
        "lista",
    ),
    s(
        Entornos,
        "1.",
        "\\begin{enumerate}\n  \\item \n\\end{enumerate}\n",
        "lista numerada",
    ),
    s(
        Entornos,
        "=",
        "\\begin{equation}\n  \n\\end{equation}\n",
        "ecuación",
    ),
    s(
        Entornos,
        "≡",
        "\\begin{align}\n   &= \n\\end{align}\n",
        "alineado",
    ),
    s(
        Entornos,
        "▦",
        "\\begin{table}[t]\n  \\centering\n  \\caption{}\n  \\label{tab:}\n  \\begin{tabular}{ll}\n    \\toprule\n     & \\\\\n    \\midrule\n     & \\\\\n    \\bottomrule\n  \\end{tabular}\n\\end{table}\n",
        "tabla booktabs",
    ),
    s(
        Entornos,
        "▣",
        "\\begin{figure}[t]\n  \\centering\n  \\includegraphics[width=.8\\linewidth]{}\n  \\caption{}\n  \\label{fig:}\n\\end{figure}\n",
        "figura",
    ),
    s(Entornos, "❝", "\\begin{quote}\n  \n\\end{quote}\n", "cita"),
    s(Entornos, "§", "\\section{}\n", "sección"),
    s(Entornos, "↵", "\\\\\n", "salto de línea"),
    s(Entornos, "%", "% ", "comentario"),
];

/// Const helper para la tabla (evita repetir el tipo).
const fn s(
    category: SymbolCategory,
    glyph: &'static str,
    insert: &'static str,
    name: &'static str,
) -> Symbol {
    Symbol {
        category,
        glyph,
        insert,
        name,
    }
}

use SymbolCategory::*;

/// Símbolos de `category` cuyo nombre o comando contiene `query` (minúsculas).
fn filtered(category: SymbolCategory, query: &str) -> Vec<&'static Symbol> {
    SYMBOLS
        .iter()
        .filter(|symbol| symbol.category == category)
        .filter(|symbol| {
            if query.is_empty() {
                return true;
            }
            symbol.name.to_lowercase().contains(query)
                || symbol.insert.to_lowercase().contains(query)
        })
        .collect()
}

/// Paleta de símbolos LaTeX del dock inferior.
pub struct SymbolsPanel {
    editor: Option<Entity<EditorPanel>>,
    query: Entity<InputState>,
    query_text: String,
    category: SymbolCategory,
    _sub: Subscription,
}

impl SymbolsPanel {
    /// Crea la paleta; `editor` puede faltar en tests/demos.
    pub fn new(
        window: &mut Window,
        editor: Option<Entity<EditorPanel>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Buscar símbolo (p. ej. «alfa», «suma», «flecha»)…")
                .clean_on_escape()
        });
        let sub = cx.subscribe_in(&query, window, |this, state, event, _window, cx| {
            if matches!(event, InputEvent::Change) {
                this.query_text = state.read(cx).value().to_lowercase();
                cx.notify();
            }
        });
        Self {
            editor,
            query,
            query_text: String::new(),
            category: SymbolCategory::Griegas,
            _sub: sub,
        }
    }

    /// Categoría activa (tests).
    pub fn category(&self) -> SymbolCategory {
        self.category
    }

    /// Inserta el comando de los símbolos que cumplen `query` y la categoría
    /// activa en el cursor del editor. Devuelve cuántos insertó.
    pub fn insert_first_match(&mut self, window: &mut Window, cx: &mut Context<Self>) -> usize {
        let Some(symbol) = filtered(self.category, &self.query_text).first().copied() else {
            return 0;
        };
        self.insert(symbol.insert, window, cx);
        1
    }

    /// Inserta `text` en el documento activo (y deja el foco en el editor).
    fn insert(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.clone() else {
            return;
        };
        editor.update(cx, |panel, cx| {
            panel.insert_active(text.to_owned(), window, cx);
        });
    }

    fn render_categories(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .gap(px(4.))
            .flex_wrap()
            .children(SymbolCategory::ALL.into_iter().map(|category| {
                let selected = category == self.category;
                Button::new(SharedString::from(format!(
                    "symbols-cat-{}",
                    category.label()
                )))
                .xsmall()
                .ghost()
                .selected(selected)
                .label(category.label())
                .tooltip(category.label())
                .on_click(cx.listener(
                    move |this, _: &ClickEvent, _window, cx| {
                        this.category = category;
                        cx.notify();
                    },
                ))
            }))
    }

    fn render_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let symbols = filtered(self.category, &self.query_text);
        if symbols.is_empty() {
            return v_flex()
                .p_3()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child("Sin símbolos para esa búsqueda.")
                .into_any_element();
        }
        v_flex()
            .w_full()
            .gap(px(2.))
            .children(symbols.chunks(10).map(|row| {
                h_flex()
                    .w_full()
                    .gap(px(2.))
                    .children(row.iter().map(|symbol| {
                        let insert = symbol.insert.to_string();
                        let tooltip: SharedString =
                            format!("{}\n{}", symbol.name, symbol.insert).into();
                        div()
                            .id(SharedString::from(format!(
                                "symbol-{}-{}",
                                symbol.category.label(),
                                symbol.name
                            )))
                            .flex_none()
                            .w(px(26.))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .cursor_pointer()
                            .hover(|style| style.bg(cx.theme().foreground.opacity(0.08)))
                            .text_size(px(15.))
                            .child(symbol.glyph)
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.insert(&insert, window, cx);
                            }))
                    }))
                    .child(div().flex_1())
            }))
            .into_any_element()
    }
}

impl Render for SymbolsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("symbols-panel")
            .size_full()
            .gap(px(4.))
            .p(px(6.))
            .child(
                h_flex()
                    .w_full()
                    .gap(px(6.))
                    .items_center()
                    .child(div().w(px(200.)).child(Input::new(&self.query).small()))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Clic para insertar en el cursor"),
                    ),
            )
            .child(self.render_categories(cx))
            .child(
                div()
                    .id("symbols-grid")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.render_grid(cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    // Sin glob: `gpui_kit::*` exporta el macro `test` de GPUI y sombrearía el
    // `#[test]` estándar en este módulo.
    use super::{SYMBOLS, SymbolCategory, SymbolsPanel, filtered};

    #[test]
    fn la_paleta_cubre_todas_las_categorias_y_no_tiene_comandos_rotos() {
        for category in SymbolCategory::ALL {
            assert!(
                !filtered(category, "").is_empty(),
                "categoría {} sin símbolos",
                category.label()
            );
        }
        for symbol in SYMBOLS {
            assert!(
                !symbol.glyph.is_empty() && !symbol.insert.is_empty() && !symbol.name.is_empty(),
                "símbolo incompleto: {symbol:?}"
            );
        }
    }

    #[test]
    fn el_buscador_encuentra_por_nombre_y_por_comando() {
        let griegas = filtered(SymbolCategory::Griegas, "alfa");
        assert_eq!(griegas.len(), 1);
        assert_eq!(griegas[0].insert, "\\alpha{}");

        // Por comando (sin la barra: el usuario escribe «alpha»).
        let por_comando = filtered(SymbolCategory::Griegas, "alpha");
        assert_eq!(por_comando.len(), 1);

        // Sin coincidencias.
        assert!(filtered(SymbolCategory::Griegas, "zzz").is_empty());
    }

    #[test]
    fn cada_entorno_inserta_un_begin_y_su_end() {
        for symbol in filtered(SymbolCategory::Entornos, "") {
            if symbol.insert.contains("\\begin{") {
                let env = symbol
                    .insert
                    .split("\\begin{")
                    .nth(1)
                    .and_then(|rest| rest.split('}').next())
                    .expect("entorno");
                assert!(
                    symbol.insert.contains(&format!("\\end{{{env}}}")),
                    "{} no cierra {env}",
                    symbol.name
                );
            }
        }
    }

    // --- UI: la paleta inserta en el documento activo (M2) -------------------

    struct Host {
        panel: gpui_kit::Entity<SymbolsPanel>,
    }

    impl gpui_kit::Render for Host {
        fn render(
            &mut self,
            _window: &mut gpui_kit::Window,
            _cx: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            self.panel.clone()
        }
    }

    /// Un clic en la paleta inserta el comando en el cursor del documento.
    #[test]
    fn la_paleta_inserta_en_el_documento_activo() {
        use gpui_kit::{AppContext as _, TestAppContext};

        use crate::panels::editor::EditorPanel;

        let root = std::env::temp_dir().join(format!("lsx-ui-symbols-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("dir");
        std::fs::write(root.join("main.tex"), "\\documentclass{article}\n").expect("main.tex");

        let mut app = TestAppContext::single();
        app.update(gpui_kit::init);
        let mut panel = None;
        let mut editor_panel = None;
        let (_, cx) = app.add_window_view(|window, cx| {
            let editor = cx.new(|cx| EditorPanel::new(&root, root.join("config"), window, cx));
            editor.update(cx, |panel, cx| {
                assert!(panel.open_file("main.tex", None, window, cx));
            });
            editor_panel = Some(editor.clone());
            let symbols = cx.new(|cx| SymbolsPanel::new(window, Some(editor), cx));
            panel = Some(symbols.clone());
            Host { panel: symbols }
        });
        let symbols = panel.unwrap();
        let editor = editor_panel.unwrap();
        cx.update(|window, cx| window.draw(cx).clear(cx));

        // Primera coincidencia de «Griegas» = \alpha{}.
        let inserted = cx.update(|window, cx| {
            symbols.update(cx, |panel, cx| panel.insert_first_match(window, cx))
        });
        assert_eq!(inserted, 1);
        let text = cx.read(|cx| editor.read(cx).active_text(cx).unwrap_or_default());
        assert!(
            text.contains("\\alpha{}"),
            "el símbolo debe acabar en el documento; texto: {text:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
