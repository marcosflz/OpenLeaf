//! Mapeo de diagnósticos `lsx-latex` → marcadores inline del editor del kit
//! (M1-B3, INTERFACES §6).
//!
//! Entrada: [`lsx_latex::Diagnostic`] del `LogParser` (fixtures reales de
//! `fixtures/logs/`). Salida: [`KitDiagnostic`] de `gpui-base` con rango
//! `Position` (línea 0-based, columna en **caracteres**, como el `RopeExt` del
//! kit) y severidad mapeada. El kit pinta el squiggle y el popover.
//!
//! Solo se mapean **errores y warnings** con línea: los `Info` del log
//! (p. ej. `LaTeX Font Info: … on input line 3`) son ruido para el editor y
//! quedan para el panel de problemas (M1-D3). Si el diagnóstico trae archivo y
//! no coincide con el documento que se está marcando, se omite.
//!
//! Resolución del span (el log no siempre trae columna):
//! 1. `column` explícita (1-based, convención `archivo:línea:columna:`);
//! 2. `raw_block` con la línea `l.N <contexto>` de TeX: se subraya el **último
//!    token** del contexto (en `l.4 Hola \undefinedcommand` el error está en
//!    `\undefinedcommand`; en `l.4 \begin` está en `\begin`);
//! 3. si nada de lo anterior aplica: la línea completa sin espacios extremos.

use gpui_kit::base::input::{
    Diagnostic as KitDiagnostic, DiagnosticSeverity as KitSeverity, Position,
};
use gpui_kit::component::input::EditorState;
use lsx_latex::{Diagnostic, DiagnosticSeverity};

/// Convierte diagnósticos de LaTeX en diagnósticos del editor (sin filtro de
/// archivo; ver [`map_diagnostics_for`]).
pub fn map_diagnostics(text: &str, diags: &[Diagnostic]) -> Vec<KitDiagnostic> {
    map_diagnostics_for(text, None, diags)
}

/// Igual que [`map_diagnostics`], omitiendo los diagnósticos cuyo archivo no
/// sea `file_name` (comparación por nombre de archivo; los diagnósticos sin
/// archivo se conservan).
pub fn map_diagnostics_for(
    text: &str,
    file_name: Option<&str>,
    diags: &[Diagnostic],
) -> Vec<KitDiagnostic> {
    let lines: Vec<&str> = text.lines().collect();
    diags
        .iter()
        .filter(|diag| {
            matches!(
                diag.severity,
                DiagnosticSeverity::Error | DiagnosticSeverity::Warning
            )
        })
        .filter(|diag| match (&diag.file, file_name) {
            (Some(file), Some(name)) => {
                file.file_name()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
                    || file.as_str().ends_with(name)
            }
            _ => true,
        })
        .filter_map(|diag| {
            let line = diag.line?;
            if line == 0 {
                return None;
            }
            let content = lines.get((line - 1) as usize).copied()?;
            let (start, end) = span_for(content, diag)?;
            Some(
                KitDiagnostic::new(
                    Position::new(line - 1, start as u32)..Position::new(line - 1, end as u32),
                    diag.message.trim_start_matches('!').trim().to_string(),
                )
                .with_severity(map_severity(diag.severity))
                .with_source("latexmk"),
            )
        })
        .collect()
}

/// Aplica `diags` al `DiagnosticSet` del editor (reemplaza los anteriores).
///
/// Devuelve cuántos marcadores quedaron. El kit limpia los diagnósticos con
/// cada edición; el llamante decide cuándo re-aplicarlos (p. ej. tras una
/// recarga).
pub fn apply_diagnostics(editor: &mut EditorState, diags: &[Diagnostic]) -> usize {
    apply_diagnostics_for(editor, None, diags)
}

/// Igual que [`apply_diagnostics`] con filtro por nombre de archivo.
pub fn apply_diagnostics_for(
    editor: &mut EditorState,
    file_name: Option<&str>,
    diags: &[Diagnostic],
) -> usize {
    let text = editor.text().clone();
    let mapped = map_diagnostics_for(&text.to_string(), file_name, diags);
    let count = mapped.len();
    if let Some(set) = editor.diagnostics_mut() {
        set.reset(&text);
        set.extend(mapped);
    }
    count
}

fn map_severity(severity: DiagnosticSeverity) -> KitSeverity {
    match severity {
        DiagnosticSeverity::Error => KitSeverity::Error,
        DiagnosticSeverity::Warning => KitSeverity::Warning,
        DiagnosticSeverity::Info => KitSeverity::Info,
    }
}

/// Span (columnas de caracteres, 0-based, fin exclusivo) para el diagnóstico.
fn span_for(content: &str, diag: &Diagnostic) -> Option<(usize, usize)> {
    let total = content.chars().count();
    if total == 0 {
        return None;
    }

    if let Some(column) = diag.column {
        let start = (column.saturating_sub(1) as usize).min(total.saturating_sub(1));
        return Some((start, (start + 1).min(total)));
    }

    if let Some(context) = latex_context_line(&diag.raw_block, diag.line)
        && let Some(span) = last_token_span(content, context)
    {
        return Some(span);
    }

    trimmed_span(content)
}

/// Extrae el contexto de la línea `l.<n> <contexto>` del bloque crudo de TeX.
fn latex_context_line(raw_block: &str, line: Option<u32>) -> Option<&str> {
    let line = line?;
    let prefix = format!("l.{line}");
    raw_block.lines().find_map(|raw| {
        let rest = raw.strip_prefix(&prefix)?;
        // `l.4 <texto>` (el número va pegado; acepta `l.4 ` y `l.4\t`).
        let rest = rest.strip_prefix(' ').or_else(|| rest.strip_prefix('\t'))?;
        Some(rest.trim_end())
    })
}

/// Subraya el último token del contexto (`\undefinedcommand` en `Hola
/// \undefinedcommand`) ubicándolo en la línea real del documento.
fn last_token_span(content: &str, context: &str) -> Option<(usize, usize)> {
    let context = context.trim_end();
    if context.is_empty() {
        return None;
    }
    let base = content.find(context).or_else(|| {
        // El log puede mostrar sólo un trozo (`Hola \undefinedc`); prueba con
        // los tokens del contexto de más a menos específico.
        context
            .split_whitespace()
            .rev()
            .find_map(|token| content.find(token))
    })?;
    let (token_start, token_end) = context
        .rsplit_once(char::is_whitespace)
        .map(|(_, last)| (context.len() - last.len(), context.len()))
        .unwrap_or((0, context.len()));
    // El token puede no empezar en `base` si el fallback encontró un sufijo.
    let relative = content[base..].find(&context[token_start..token_end])?;
    let start_byte = base + relative;
    let end_byte = start_byte + context[token_start..token_end].len();
    let start = content[..start_byte].chars().count();
    let end = start + content[start_byte..end_byte].chars().count();
    (end > start).then_some((start, end))
}

/// Línea completa sin espacios extremos.
fn trimmed_span(content: &str) -> Option<(usize, usize)> {
    let start = content.chars().position(|c| !c.is_whitespace())?;
    let end = content
        .chars()
        .rev()
        .position(|c| !c.is_whitespace())
        .map(|from_end| content.chars().count() - from_end)?;
    (end > start).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsx_latex::parse_log;

    const UNDEFINED_LOG: &str = include_str!("../../../fixtures/logs/undefined-command.log");
    const MISSING_LOG: &str = include_str!("../../../fixtures/logs/missing-package.log");
    const BROKEN_UNDEFINED_TEX: &str =
        include_str!("../../../fixtures/projects/broken-undefined/main.tex");
    const BROKEN_MISSINGPKG_TEX: &str =
        include_str!("../../../fixtures/projects/broken-missingpkg/main.tex");

    fn slice_marker<'a>(text: &'a str, marker: &KitDiagnostic) -> (&'a str, String) {
        let line = text.lines().nth(marker.range.start.line as usize).unwrap();
        let chars: Vec<char> = line.chars().collect();
        let start = marker.range.start.character as usize;
        let end = marker.range.end.character as usize;
        let range: String = chars[start..end].iter().collect();
        (line, range)
    }

    #[test]
    fn undefined_command_subraya_el_token_del_bloque_l_n() {
        let diags = parse_log(UNDEFINED_LOG);
        assert!(
            diags.iter().any(|d| d.line == Some(4)),
            "el fixture debe traer el error en la línea 4: {diags:#?}"
        );

        let mapped = map_diagnostics_for(BROKEN_UNDEFINED_TEX, Some("main.tex"), &diags);
        let marker = mapped
            .iter()
            .find(|m| m.range.start.line == 3)
            .expect("el error de línea 4 debe mapearse inline");
        assert_eq!(marker.severity, KitSeverity::Error);
        let (_, underlined) = slice_marker(BROKEN_UNDEFINED_TEX, marker);
        assert_eq!(underlined, "\\undefinedcommand");
    }

    #[test]
    fn missing_package_marca_la_linea_del_emergency_stop() {
        let diags = parse_log(MISSING_LOG);
        let mapped = map_diagnostics_for(BROKEN_MISSINGPKG_TEX, Some("main.tex"), &diags);
        // Stream D correlaciona el `! LaTeX Error` con la línea 4; el bloque `l.4
        // \begin` del Emergency stop es el que da el span fino (`\begin`).
        assert!(!mapped.is_empty());
        assert!(
            mapped.iter().all(|m| m.range.start.line == 3),
            "todos los marcadores caen en la línea 4 (0-based 3): {mapped:#?}"
        );
        let underlined: Vec<String> = mapped
            .iter()
            .map(|m| slice_marker(BROKEN_MISSINGPKG_TEX, m).1)
            .collect();
        assert!(
            underlined.iter().any(|text| text == "\\begin"),
            "el contexto `l.4 \\begin` debe dar un span fino: {underlined:?}"
        );
    }

    #[test]
    fn diagnosticos_de_otro_archivo_o_info_se_omiten() {
        let de_otro = Diagnostic {
            severity: DiagnosticSeverity::Error,
            kind: lsx_latex::DiagnosticKind::LatexError,
            file: Some(camino::Utf8PathBuf::from("./otro.tex")),
            line: Some(1),
            column: None,
            message: "error de otro archivo".into(),
            raw_block: String::new(),
        };
        let info = Diagnostic {
            severity: DiagnosticSeverity::Info,
            kind: lsx_latex::DiagnosticKind::Unknown,
            file: None,
            line: Some(1),
            column: None,
            message: "LaTeX Font Info".into(),
            raw_block: String::new(),
        };
        let mapped = map_diagnostics_for("una línea", Some("main.tex"), &[de_otro, info]);
        assert!(mapped.is_empty());
    }

    #[test]
    fn diagnosticos_sin_linea_o_fuera_de_rango_se_omiten() {
        let sin_linea = Diagnostic {
            severity: DiagnosticSeverity::Error,
            kind: lsx_latex::DiagnosticKind::LatexError,
            file: None,
            line: None,
            column: None,
            message: "! sin línea".into(),
            raw_block: String::new(),
        };
        let fuera = Diagnostic {
            line: Some(99),
            ..sin_linea.clone()
        };
        assert!(map_diagnostics("una línea", &[sin_linea, fuera]).is_empty());
    }

    #[test]
    fn columna_explicita_es_1_based_y_se_recorta_a_la_linea() {
        let diag = Diagnostic {
            severity: DiagnosticSeverity::Warning,
            kind: lsx_latex::DiagnosticKind::Overfull,
            file: Some(camino::Utf8PathBuf::from("./main.tex")),
            line: Some(2),
            column: Some(3),
            message: "Overfull \\hbox".into(),
            raw_block: String::new(),
        };
        let mapped = map_diagnostics("primera\nabcdef\n", &[diag]);
        assert_eq!(mapped[0].range.start, Position::new(1, 2));
        assert_eq!(mapped[0].range.end, Position::new(1, 3));
        assert_eq!(mapped[0].severity, KitSeverity::Warning);

        // Columna más allá de la línea: se recorta al último carácter.
        let diag_lejos = Diagnostic {
            column: Some(99),
            ..mapped_helper()
        };
        let mapped = map_diagnostics("primera\nabcdef\n", &[diag_lejos]);
        assert_eq!(mapped[0].range.start, Position::new(1, 5));
        assert_eq!(mapped[0].range.end, Position::new(1, 6));
    }

    fn mapped_helper() -> Diagnostic {
        Diagnostic {
            severity: DiagnosticSeverity::Warning,
            kind: lsx_latex::DiagnosticKind::Overfull,
            file: None,
            line: Some(2),
            column: Some(99),
            message: "Overfull".into(),
            raw_block: String::new(),
        }
    }

    #[test]
    fn sin_columna_ni_bloque_subraya_la_linea_sin_espacios() {
        let lineas = "  \\usepackage{roto}  \n";
        let diag = Diagnostic {
            severity: DiagnosticSeverity::Error,
            kind: lsx_latex::DiagnosticKind::LatexError,
            file: None,
            line: Some(1),
            column: None,
            message: "error".into(),
            raw_block: String::new(),
        };
        let mapped = map_diagnostics(lineas, &[diag]);
        let (_, underlined) = slice_marker(lineas, &mapped[0]);
        assert_eq!(underlined, "\\usepackage{roto}");
    }

    #[gpui_kit::test]
    fn apply_diagnostics_llena_el_diagnostic_set_del_editor(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;

        struct Host {
            state: gpui_kit::Entity<EditorState>,
        }
        impl gpui_kit::Render for Host {
            fn render(
                &mut self,
                _: &mut gpui_kit::Window,
                _: &mut gpui_kit::Context<Self>,
            ) -> impl gpui_kit::IntoElement {
                gpui_kit::component::input::Editor::new(&self.state)
            }
        }

        cx.update(gpui_kit::init);
        cx.update(|_cx| crate::latex_language::register_latex_language());
        let mut state = None;
        let text = BROKEN_UNDEFINED_TEX;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let editor = cx.new(|cx| {
                EditorState::new(window, cx)
                    .language(crate::latex_language::LATEX_LANGUAGE_NAME)
                    .default_value(text)
            });
            let diags = parse_log(UNDEFINED_LOG);
            editor.update(cx, |editor, _| {
                let applied = apply_diagnostics_for(editor, Some("main.tex"), &diags);
                assert!(applied >= 1);
            });
            state = Some(editor.clone());
            Host { state: editor }
        });
        let state = state.unwrap();
        gpui_kit::VisualTestContext::update(cx, |window, cx| window.draw(cx).clear(cx));
        cx.read(|cx| {
            let editor = state.read(cx);
            let set = editor.diagnostics().expect("EditorState del kit");
            assert!(!set.is_empty());
            let entry = set
                .iter()
                .find(|entry| entry.message.contains("Undefined control sequence"))
                .expect("el error del fixture debe estar en el set");
            assert_eq!(
                editor.text().slice(entry.range.clone()).to_string(),
                "\\undefinedcommand"
            );
        });
    }
}
