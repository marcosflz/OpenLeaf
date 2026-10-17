//! Demo del spike M0-S2: editor del kit (`component::input::Editor`) con
//! resaltado LaTeX vía tree-sitter, gutter de números de línea, edición,
//! guardado sobre una **copia temporal** del fixture y estado dirty.
//!
//! Uso:
//! ```text
//! cargo run -p latex-studio --bin spike-editor          # ventana
//! cargo run -p latex-studio --bin spike-editor -- --check
//! ```
//! `--check` no abre ventana: parsea la copia y resume nodos `section` y spans
//! de resaltado (útil para smoke tests sin pantalla).
//!
//! NOTA (spike): el guardado escribe directo a disco. El diseño de INTERFACES
//! §6 reserva la escritura a `lsx-core::FsService` con `EditorIntent::Save`;
//! aquí no existe aún, así que M1 debe sustituir `SpikeEditorView::save`.

use std::path::{Path, PathBuf};

use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Editor, EditorState, InputEvent},
    status_bar::StatusBar,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::latex_language::{
    LATEX_LANGUAGE_NAME, collect_nodes_of_kind, parse_latex, register_latex_language,
};

gpui_kit::actions!(lsx_spike, [SaveDocument]);

/// Documento abierto por el spike (copia temporal del fixture).
#[derive(Clone)]
pub struct SpikeDocument {
    pub path: PathBuf,
    pub content: String,
}

/// Vista del editor del kit con resaltado LaTeX y estado dirty.
pub struct SpikeEditorView {
    editor: Entity<EditorState>,
    path: PathBuf,
    saved_text: String,
    dirty: bool,
    status: SharedString,
    _subscription: Subscription,
}

impl SpikeEditorView {
    pub fn new(document: SpikeDocument, window: &mut Window, cx: &mut Context<Self>) -> Self {
        register_latex_language();

        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(LATEX_LANGUAGE_NAME)
                .line_number(true)
                .default_value(document.content.clone())
        });

        // Estado dirty del spike: comparación contra el último texto guardado.
        // En M1 sustituir por versión de documento + evento de FsService.
        let _subscription = cx.subscribe(&editor, |this, _editor, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let dirty = this.editor.read(cx).text() != this.saved_text.as_str();
                if dirty != this.dirty {
                    this.dirty = dirty;
                    cx.notify();
                }
            }
        });

        cx.bind_keys([KeyBinding::new("ctrl-s", SaveDocument, None)]);

        // Foco inicial en el editor (los atajos burbujean hacia la vista raíz).
        let focus_handle = editor.focus_handle(cx);
        window.defer(cx, move |window, cx| {
            focus_handle.focus(window, cx);
        });

        Self {
            editor,
            path: document.path,
            saved_text: document.content,
            dirty: false,
            status: "spike M0-S2 — edita el texto y guarda con el botón o Ctrl+S (escribe en la copia temporal)".into(),
            _subscription,
        }
    }

    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).text().to_string();
        self.status = match std::fs::write(&self.path, text.as_bytes()) {
            Ok(()) => {
                self.saved_text = text;
                self.dirty = false;
                format!("guardado en {}", self.path.display()).into()
            }
            Err(err) => format!("ERROR al guardar: {err}").into(),
        };
        cx.notify();
    }

    fn on_save(&mut self, _: &SaveDocument, window: &mut Window, cx: &mut Context<Self>) {
        self.save(window, cx);
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dirty_marker = if self.dirty {
            "● sin guardar"
        } else {
            "sin cambios"
        };
        h_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_3()
            .items_center()
            .bg(cx.theme().secondary)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .flex_1()
                    .child(
                        div().text_sm().font_weight(FontWeight::SEMIBOLD).child(
                            "LaTeX Studio · spike M0-S2 (editor del kit + tree-sitter-latex)",
                        ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} — {dirty_marker}", self.path.display())),
                    ),
            )
            .child(
                Button::new("save")
                    .primary()
                    .label("Guardar (Ctrl+S)")
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
    }
}

impl Render for SpikeEditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let position = self.editor.read(cx).cursor_position();
        let line_col = format!("Ln {}, Col {}", position.line + 1, position.character + 1);

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .on_action(cx.listener(Self::on_save))
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(Editor::new(&self.editor).bordered(false).h(relative(1.0))),
            )
            .child(
                StatusBar::new()
                    .left(div().text_xs().child(line_col))
                    .left(
                        div()
                            .text_xs()
                            .when(self.dirty, |this| this.text_color(cx.theme().yellow))
                            .child(if self.dirty { "dirty" } else { "saved" }),
                    )
                    .right(div().text_xs().child(self.status.clone())),
            )
    }
}

/// Ruta del fixture CV (inmutable; solo se lee para copiarlo).
fn locate_cv_fixture() -> PathBuf {
    let mut candidates = vec![
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/cv/main.tex"),
        PathBuf::from("fixtures/projects/cv/main.tex"),
        PathBuf::from("../fixtures/projects/cv/main.tex"),
    ];
    candidates.drain(..).find(|path| path.is_file()).unwrap_or_else(|| {
        panic!(
            "no encuentro fixtures/projects/cv/main.tex — ejecuta el spike desde la raíz del repo"
        )
    })
}

/// Copia el fixture a `%TEMP%/latex-studio/spike-s2/main.tex` y devuelve la ruta.
fn copy_fixture_to_temp() -> PathBuf {
    let fixture = locate_cv_fixture();
    let dir = std::env::temp_dir().join("latex-studio").join("spike-s2");
    std::fs::create_dir_all(&dir).expect("no se pudo crear el directorio temporal del spike");
    let copy = dir.join("main.tex");
    std::fs::copy(&fixture, &copy).expect("no se pudo copiar el fixture a %TEMP%");
    copy
}

fn run_check(copy: &Path) {
    register_latex_language();

    let content = std::fs::read_to_string(copy).expect("no se pudo leer la copia");
    let tree = parse_latex(&content).expect("el parser LaTeX debe inicializarse");
    let mut sections = Vec::new();
    collect_nodes_of_kind(&tree.root_node(), "section", &mut sections);

    use gpui_kit::component::highlighter::{HighlightTheme, SyntaxHighlighter};
    use gpui_kit::component::input::Rope;

    let mut highlighter = SyntaxHighlighter::new(LATEX_LANGUAGE_NAME);
    let rope = Rope::from(content.as_str());
    let parsed = highlighter.update(None, &rope, None);
    let styles = highlighter.styles(&(0..content.len()), &*HighlightTheme::default_dark());
    let colored = styles
        .iter()
        .filter(|(_, style)| style.color.is_some())
        .count();

    println!("spike M0-S2 · check sin ventana");
    println!("  copia         : {}", copy.display());
    println!("  bytes         : {}", content.len());
    println!("  parse ok      : {parsed}");
    println!("  nodos section : {}", sections.len());
    for section in &sections {
        let label = section
            .child_by_field_name("text")
            .or_else(|| section.child_by_field_name("command"))
            .and_then(|node| node.utf8_text(content.as_bytes()).ok())
            .unwrap_or("<¿?>");
        println!(
            "    · línea {:>3}: {label}",
            section.start_position().row + 1
        );
    }
    println!("  spans estilo  : {} ({colored} con color)", styles.len());
}

/// Punto de entrada del spike (compartido por los binarios `spike-editor`).
pub fn run_spike() {
    let copy = copy_fixture_to_temp();

    if std::env::args().any(|arg| arg == "--check") {
        run_check(&copy);
        return;
    }

    let content = std::fs::read_to_string(&copy).expect("no se pudo leer la copia temporal");
    println!("spike M0-S2 · abriendo copia {}", copy.display());
    println!("(el fixture original fixtures/projects/cv/main.tex nunca se escribe)");

    let document = SpikeDocument {
        path: copy,
        content,
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let options = WindowOptions {
                window_min_size: Some(size(px(900.), px(600.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("LaTeX Studio — spike M0-S2 (editor + tree-sitter)".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| SpikeEditorView::new(document.clone(), window, cx))
            })
            .expect("no se pudo abrir la ventana del spike");
        });
}
