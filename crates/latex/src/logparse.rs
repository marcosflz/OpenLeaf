//! Parser tolerante de logs de `pdflatex`/`latexmk` (contrato en INTERFACES §3).
//!
//! Corpus: `fixtures/logs/{missing-package,undefined-command,cv-ok,cv-sidebar-ok}.log`.
//! Sin dependencias nuevas (nada de `regex`): escaneo manual por líneas con captura de
//! `raw_block`. Diferencias con el contrato documentadas en el informe M0-S5.

use camino::Utf8PathBuf;

/// Tope de análisis (8 MB). Se recorta en un límite de carácter válido.
pub const MAX_LOG_BYTES: usize = 8 * 1024 * 1024;

/// Severidad de un diagnóstico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// Clasificación de un diagnóstico.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    MissingPackage { package: String },
    LatexError,
    BibError,
    UndefinedRef,
    Overfull,
    RerunNeeded,
    Unknown,
}

/// Diagnóstico normalizado (contrato INTERFACES §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub kind: DiagnosticKind,
    /// Ruta tal cual aparece en el log (`./main.tex`), sin resolver contra el proyecto.
    pub file: Option<Utf8PathBuf>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
    pub raw_block: String,
}

/// Pista de progreso en vivo (`parse_stdout_stream`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogHint {
    Progress { line: String },
    Error { message: String },
    Warning { message: String },
    Output { pages: u32, bytes: u64 },
}

/// Parsea un log completo (tolerante; última ejecución conocida).
pub fn parse_log(log: &str) -> Vec<Diagnostic> {
    let log = cap_log(log);
    let log = last_run(log);
    let lines: Vec<&str> = log.lines().collect();
    let mut out: Vec<Diagnostic> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        if let Some(mut diag) = parse_file_line_error(line) {
            let (block, consumed) = collect_block(&lines, i);
            if !block.is_empty() {
                diag.raw_block = block;
            }
            out.push(diag);
            i += consumed;
            continue;
        }
        if line.starts_with("! ") || line == "!" {
            let (block, consumed) = collect_block(&lines, i);
            out.push(Diagnostic {
                severity: DiagnosticSeverity::Error,
                kind: classify_message(line),
                file: None,
                line: None,
                column: None,
                message: line.trim_end().to_string(),
                raw_block: block,
            });
            i += consumed;
            continue;
        }
        if let Some(diag) = parse_warning_line(line) {
            out.push(diag);
            i += 1;
            continue;
        }
        if line.starts_with("Overfull ") {
            out.push(parse_overfull(line));
            i += 1;
            continue;
        }
        if let Some(diag) = parse_bib_line(line) {
            out.push(diag);
            i += 1;
            continue;
        }
        i += 1;
    }
    dedupe(out)
}

/// Pista de una línea de stdout de `latexmk`/`pdflatex` (progreso en vivo).
pub fn parse_stdout_stream(line: &str) -> Option<LogHint> {
    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(diag) = parse_file_line_error(trimmed) {
        return Some(LogHint::Error {
            message: diag.message,
        });
    }
    if trimmed.starts_with("! ") {
        return Some(LogHint::Error {
            message: trimmed.to_string(),
        });
    }
    if let Some(diag) = parse_warning_line(trimmed) {
        return Some(LogHint::Warning {
            message: diag.message,
        });
    }
    if trimmed.starts_with("Overfull ") {
        return Some(LogHint::Warning {
            message: trimmed.to_string(),
        });
    }
    if let Some((pages, bytes)) = parse_output_written(trimmed) {
        return Some(LogHint::Output { pages, bytes });
    }
    if trimmed.starts_with("Latexmk:") || trimmed.starts_with("Running '") {
        return Some(LogHint::Progress {
            line: trimmed.to_string(),
        });
    }
    None
}

// ---------------------------------------------------------------------------
// Internos
// ---------------------------------------------------------------------------

fn cap_log(log: &str) -> &str {
    if log.len() <= MAX_LOG_BYTES {
        return log;
    }
    let mut cut = MAX_LOG_BYTES;
    while !log.is_char_boundary(cut) {
        cut -= 1;
    }
    &log[..cut]
}

/// Si hay varios runs concatenados (raro), quédate con el último desde `Job started:`.
fn last_run(log: &str) -> &str {
    match log.rfind("\nJob started:") {
        Some(idx) => &log[idx + 1..],
        None => log,
    }
}

/// Detecta `archivo:línea: mensaje` (modo `-file-line-error`).
fn parse_file_line_error(line: &str) -> Option<Diagnostic> {
    let bytes = line.as_bytes();
    for (idx, byte) in bytes.iter().enumerate() {
        if *byte != b':' {
            continue;
        }
        // tras ':' deben venir dígitos y luego ':' (+ espacio o fin)
        let mut j = idx + 1;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j == idx + 1 || j >= bytes.len() || bytes[j] != b':' {
            continue;
        }
        let file_part = line[..idx].trim();
        let quoted = file_part.starts_with('"') && file_part.ends_with('"') && file_part.len() > 1;
        let file_path = if quoted {
            &file_part[1..file_part.len() - 1]
        } else {
            file_part
        };
        if file_path.is_empty() {
            continue;
        }
        if file_path.contains(char::is_whitespace) && !quoted {
            continue;
        }
        if !looks_like_file(file_path) {
            continue;
        }
        let after = &line[j + 1..];
        let message = after.trim();
        if message.is_empty() {
            continue;
        }
        return Some(Diagnostic {
            severity: DiagnosticSeverity::Error,
            kind: classify_message(message),
            file: Some(Utf8PathBuf::from(file_path)),
            line: line[idx + 1..j].parse().ok(),
            column: None,
            message: message.to_string(),
            raw_block: String::new(),
        });
    }
    None
}

/// ¿La parte izquierda de `a:1:` parece un archivo?
fn looks_like_file(part: &str) -> bool {
    part.starts_with("./")
        || part.starts_with(".\\")
        || part.contains(".tex")
        || part.contains(".sty")
        || part.contains(".cls")
        || part.contains(".ltx")
        || part.contains(".def")
        || part.contains(".cfg")
        || part.contains(".bib")
}

/// Clasifica el texto de un mensaje (`! ...` o `archivo:línea:`).
///
/// Spike: paquete inexistente vs error LaTeX genérico. M1-D3 añade correlación con
/// el contexto (`l.NN`, entorno, paquete) para mensajes más accionables.
fn classify_message(message: &str) -> DiagnosticKind {
    if let Some(package) = extract_missing_package(message) {
        return DiagnosticKind::MissingPackage { package };
    }
    DiagnosticKind::LatexError
}

/// Extrae el paquete de ``File `x.sty' not found`` o ``I can't find file `x'``.
fn extract_missing_package(message: &str) -> Option<String> {
    if !message.contains("not found") && !message.contains("can't find") {
        return None;
    }
    let (idx, skip) = message
        .find("File `")
        .map(|idx| (idx, 6))
        .or_else(|| message.find("can't find file `").map(|idx| (idx, 17)))?;
    let rest = &message[idx + skip..];
    let end = rest.find('\'')?;
    let file = &rest[..end];
    let stem = match file.rsplit_once('.') {
        Some((stem, "sty" | "cls" | "def" | "cfg" | "ltx")) => stem,
        _ => file,
    };
    // `foo/bar.sty` → `bar`
    let name = stem.rsplit(['/', '\\']).next().unwrap_or(stem);
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Warnings tipo `LaTeX Warning: ...`, `Package x Warning: ...`, `Class x Warning: ...`.
fn parse_warning_line(line: &str) -> Option<Diagnostic> {
    let is_warning = line.starts_with("LaTeX Warning:")
        || line.starts_with("LaTeX Font Warning:")
        || warning_suffix(line, " Warning:");
    if !is_warning {
        return None;
    }
    let kind = if line.contains("undefined")
        && (line.contains("Reference") || line.contains("Citation"))
    {
        DiagnosticKind::UndefinedRef
    } else if line.contains("Rerun") || line.contains("may have changed") {
        DiagnosticKind::RerunNeeded
    } else {
        DiagnosticKind::Unknown
    };
    Some(Diagnostic {
        severity: DiagnosticSeverity::Warning,
        kind,
        file: None,
        line: extract_input_line(line),
        column: None,
        message: line.trim_end().to_string(),
        raw_block: line.trim_end().to_string(),
    })
}

fn warning_suffix(line: &str, needle: &str) -> bool {
    // `Package <nombre> Warning:` con algo de texto antes de `Warning:`
    match line.find(needle) {
        Some(idx) => {
            idx > 0
                && (line.starts_with("Package ") || line.starts_with("Class "))
                && !line[..idx].contains("Info:")
        }
        None => false,
    }
}

/// `... on input line 12.` / `... at lines 10--12`
fn extract_input_line(line: &str) -> Option<u32> {
    if let Some(idx) = line.rfind("on input line ") {
        return leading_digits(&line[idx + "on input line ".len()..]);
    }
    if let Some(idx) = line.rfind("at lines ") {
        return leading_digits(&line[idx + "at lines ".len()..]);
    }
    None
}

fn leading_digits(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// `Overfull \hbox (12.0pt too wide) in paragraph at lines 10--12`
fn parse_overfull(line: &str) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Warning,
        kind: DiagnosticKind::Overfull,
        file: None,
        line: extract_input_line(line),
        column: None,
        message: line.trim_end().to_string(),
        raw_block: line.trim_end().to_string(),
    }
}

/// Errores de biber/bibtex (subset mínimo; M1-D3 amplía).
fn parse_bib_line(line: &str) -> Option<Diagnostic> {
    let is_bib = line.starts_with("ERROR - ")
        || line.contains("I couldn't open database file")
        || line.contains("Repeated entry---line")
        || line.contains("Illegal, another \\bibdata command");
    if !is_bib {
        return None;
    }
    Some(Diagnostic {
        severity: DiagnosticSeverity::Error,
        kind: DiagnosticKind::BibError,
        file: None,
        line: None,
        column: None,
        message: line.trim_end().to_string(),
        raw_block: line.trim_end().to_string(),
    })
}

/// `Output written on main.pdf (1 page, 79176 bytes).`
fn parse_output_written(line: &str) -> Option<(u32, u64)> {
    let rest = line.strip_prefix("Output written on")?;
    let open = rest.find('(')?;
    let close = rest[open..].find(')')? + open;
    let inside = &rest[open + 1..close];
    let mut parts = inside.split(',');
    let pages = parts.next()?.split_whitespace().next()?.parse().ok()?;
    let bytes = parts.next()?.split_whitespace().next()?.parse().ok()?;
    Some((pages, bytes))
}

/// Captura el bloque de la línea `start` y sus continuaciones hasta la primera línea
/// en blanco (cap 20 líneas). Devuelve (bloque, líneas consumidas).
fn collect_block(lines: &[&str], start: usize) -> (String, usize) {
    let mut block: Vec<&str> = Vec::new();
    let mut i = start;
    while i < lines.len() && block.len() < 20 {
        let line = lines[i];
        if line.trim().is_empty() {
            break;
        }
        block.push(line);
        i += 1;
    }
    (block.join("\n"), block.len().max(1))
}

/// Elimina duplicados exactos conservando el orden.
fn dedupe(mut diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut seen: Vec<Diagnostic> = Vec::new();
    diags.retain(|d| {
        let dup = seen.iter().any(|s| {
            s.severity == d.severity
                && s.kind == d.kind
                && s.file == d.file
                && s.line == d.line
                && s.message == d.message
        });
        if !dup {
            seen.push(d.clone());
        }
        !dup
    });
    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNDEFINED: &str = "\
This is pdfTeX, Version 3.141592653-2.6-1.40.29 (TeX Live 2026) (preloaded format=pdflatex 2026.9.13)  17 OCT 2026 14:12
entering extended mode
(./main.tex
LaTeX2e <2026-06-01>
./main.tex:4: Undefined control sequence.
l.4 Hola \\undefinedcommand
                          {} mundo.
The control sequence at the end of the top line
of your error message was never \\def'ed.

Output written on main.pdf (1 page, 13076 bytes).
";

    const MISSING: &str = "\
(./main.tex
LaTeX2e <2026-06-01>

! LaTeX Error: File `estepaquete-quenoexiste.sty' not found.

Type X to quit or <RETURN> to proceed,
or enter new name. (Default extension: sty)

Enter file name: 
./main.tex:4: Emergency stop.
<read *> 
l.4 \\begin
          {document}^^M
*** (cannot \\read from terminal in nonstop modes)

./main.tex:4:  ==> Fatal error occurred, no output PDF file produced!
";

    const WARNINGS: &str = "\
LaTeX Warning: Reference `fig:uno' on page 1 undefined on input line 12.
LaTeX Warning: There were undefined references.
LaTeX Warning: Label(s) may have changed. Rerun to get cross-references right.
Package fancyhdr Warning: \\footskip is too small (0.0pt): 
(fancyhdr)                Make it at least 3.60004pt, for example:
Package rerunfilecheck Info: File `main.out' has not changed.
Overfull \\hbox (12.0pt too wide) in paragraph at lines 10--12
";

    #[test]
    fn undefined_control_sequence_con_archivo_y_linea() {
        let diags = parse_log(UNDEFINED);
        let d = diags
            .iter()
            .find(|d| d.line == Some(4))
            .expect("diagnóstico con línea 4");
        assert_eq!(d.severity, DiagnosticSeverity::Error);
        assert_eq!(d.kind, DiagnosticKind::LatexError);
        assert_eq!(
            d.file.as_deref().and_then(|f| f.file_name()),
            Some("main.tex")
        );
        assert!(d.raw_block.contains("l.4 Hola"));
    }

    #[test]
    fn paquete_inexistente() {
        let diags = parse_log(MISSING);
        let mp: Vec<_> = diags
            .iter()
            .filter(|d| matches!(d.kind, DiagnosticKind::MissingPackage { .. }))
            .collect();
        assert_eq!(mp.len(), 1, "{diags:#?}");
        assert_eq!(
            mp[0].kind,
            DiagnosticKind::MissingPackage {
                package: "estepaquete-quenoexiste".to_string()
            }
        );
        assert_eq!(mp[0].severity, DiagnosticSeverity::Error);
        assert!(diags.iter().any(|d| d.line == Some(4)));
    }

    #[test]
    fn warnings_clasificados() {
        let diags = parse_log(WARNINGS);
        assert!(
            !diags
                .iter()
                .any(|d| d.severity == DiagnosticSeverity::Error)
        );
        assert!(
            diags
                .iter()
                .any(|d| { d.kind == DiagnosticKind::UndefinedRef && d.line == Some(12) })
        );
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::RerunNeeded));
        assert!(
            diags
                .iter()
                .any(|d| d.kind == DiagnosticKind::Overfull && d.line == Some(10))
        );
        // El `Info:` de rerunfilecheck no se debe colar como warning.
        assert!(!diags.iter().any(|d| d.message.contains("has not changed")));
        // El warning de fancyhdr se captura (kind Unknown).
        assert!(
            diags.iter().any(
                |d| d.severity == DiagnosticSeverity::Warning && d.message.contains("footskip")
            )
        );
    }

    #[test]
    fn sin_duplicados() {
        let log = "./main.tex:7: Emergency stop.\n./main.tex:7: Emergency stop.\n";
        assert_eq!(parse_log(log).len(), 1);
    }

    #[test]
    fn stdout_hints() {
        assert_eq!(
            parse_stdout_stream("Output written on main.pdf (1 page, 79176 bytes)."),
            Some(LogHint::Output {
                pages: 1,
                bytes: 79176
            })
        );
        assert!(matches!(
            parse_stdout_stream("./main.tex:4: Undefined control sequence."),
            Some(LogHint::Error { .. })
        ));
        assert!(matches!(
            parse_stdout_stream("Latexmk: All targets (main.pdf) are up-to-date"),
            Some(LogHint::Progress { .. })
        ));
        assert!(matches!(
            parse_stdout_stream("LaTeX Warning: There were undefined references."),
            Some(LogHint::Warning { .. })
        ));
        assert_eq!(parse_stdout_stream("[1{c:/tex/foo.map}]"), None);
    }
}
