//! Registro del lenguaje LaTeX para el editor del kit (spike M0-S2).
//!
//! Piezas (ver `docs/plan/INTERFACES.md` §6 y §11 I-1/I-3):
//! - gramática: `tree-sitter-latex` (upstream latex-lsp; ver la nota de
//!   `Cargo.toml`: crates.io 0.1.0 está roto y se usa el republish
//!   `codebook-tree-sitter-latex` 0.6.1 tras el alias). `LANGUAGE` es un
//!   `LanguageFn`, convertible al `tree_sitter::Language` de la versión con la
//!   que compila `gpui-component` (0.26.13, ver `Cargo.toml` del crate).
//! - query de resaltado: `assets/tree-sitter/latex/highlights.scm` (propia;
//!   cabecera del archivo explica por qué el upstream no sirve).
//! - registro: `LanguageRegistry::singleton().register()` del kit. Después,
//!   `EditorState::set_highlighter("latex")` / `.language("latex")` lo usan.
//!
//! Mapeo capturas → estilos: el kit resuelve `capture` contra
//! `SyntaxColors` (tema) siguiendo `highlighter::HIGHLIGHT_NAMES` (41 nombres),
//! con fallback al prefijo antes del punto (`function.macro` → `function`);
//! capturas desconocidas se pintan con el estilo por defecto.

use gpui_kit::component::highlighter::{GrammarConfig, LanguageRegistry};

/// Nombre con el que el lenguaje queda registrado en el kit.
pub const LATEX_LANGUAGE_NAME: &str = "latex";

/// Query de resaltado incluida en el binario en tiempo de compilación.
pub const LATEX_HIGHLIGHT_QUERY: &str =
    include_str!("../../../assets/tree-sitter/latex/highlights.scm");

/// `tree_sitter::Language` del grammar LaTeX.
pub fn latex_language() -> tree_sitter::Language {
    tree_sitter_latex::LANGUAGE.into()
}

/// Registra el lenguaje LaTeX en el `LanguageRegistry` del kit.
///
/// Idempotente: volver a registrar el mismo nombre sustituye la configuración.
/// Debe llamarse una vez antes de renderizar el editor con `.language("latex")`.
pub fn register_latex_language() {
    LanguageRegistry::singleton().register(
        LATEX_LANGUAGE_NAME,
        &GrammarConfig::new(
            LATEX_LANGUAGE_NAME,
            latex_language(),
            Vec::new(),
            LATEX_HIGHLIGHT_QUERY,
            "",
            "",
        ),
    );
}

/// Parsea `source` con el grammar LaTeX. No requiere UI.
pub fn parse_latex(source: &str) -> Option<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&latex_language()).ok()?;
    parser.parse(source, None)
}

/// Añade a `out` todos los nodos de tipo `kind` del subárbol `root`, en orden.
pub fn collect_nodes_of_kind<'tree>(
    root: &tree_sitter::Node<'tree>,
    kind: &str,
    out: &mut Vec<tree_sitter::Node<'tree>>,
) {
    if root.kind() == kind {
        out.push(*root);
    }
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        collect_nodes_of_kind(&child, kind, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture inmutable del repo (se incluye como str; nunca se escribe).
    const CV_MAIN_TEX: &str = include_str!("../../../fixtures/projects/cv/main.tex");

    #[test]
    fn parsea_el_fixture_cv_y_encuentra_al_menos_dos_sections() {
        let tree = parse_latex(CV_MAIN_TEX).expect("el parser LaTeX debe inicializarse");
        let mut sections = Vec::new();
        collect_nodes_of_kind(&tree.root_node(), "section", &mut sections);
        assert!(
            sections.len() >= 2,
            "esperaba >=2 nodos `section` en fixtures/projects/cv/main.tex, encontré {}",
            sections.len()
        );
        // Las secciones del fixture están antes de \end{document}.
        for section in &sections {
            let is_starred = section
                .child_by_field_name("command")
                .map(|command| {
                    command
                        .utf8_text(CV_MAIN_TEX.as_bytes())
                        .unwrap_or("")
                        .contains('*')
                })
                .unwrap_or(false);
            let _ = is_starred; // \section y \section* comparten nodo; sin assert extra.
        }
    }

    #[test]
    fn la_query_de_resaltado_compila_contra_la_gramatica() {
        tree_sitter::Query::new(&latex_language(), LATEX_HIGHLIGHT_QUERY)
            .expect("la query de resaltado debe compilar sin predicados de nvim");
    }

    #[test]
    fn registra_el_lenguaje_en_el_registry_del_kit() {
        register_latex_language();
        let config = LanguageRegistry::singleton()
            .language(LATEX_LANGUAGE_NAME)
            .expect("`latex` debe quedar registrado");
        assert!(config.has_grammar(), "el registro debe llevar gramática");
    }

    #[test]
    fn el_highlighter_del_kit_produce_estilos_para_el_fixture() {
        use gpui_kit::component::highlighter::{HighlightTheme, SyntaxHighlighter};
        use gpui_kit::component::input::Rope;

        register_latex_language();
        let mut highlighter = SyntaxHighlighter::new(LATEX_LANGUAGE_NAME);
        let rope = Rope::from(CV_MAIN_TEX);
        assert!(highlighter.update(None, &rope, None));

        let theme = HighlightTheme::default_dark();
        let styles = highlighter.styles(&(0..CV_MAIN_TEX.len()), &*theme);
        let colored = styles
            .iter()
            .filter(|(_, style)| style.color.is_some())
            .count();
        assert!(
            colored > 10,
            "esperaba spans con color del tema dark para el fixture, obtuve {colored}"
        );
    }
}
