//! Parser tolerante de logs de `pdflatex`/`latexmk` (contrato en INTERFACES §3).
//!
//! Corpus: `fixtures/logs/{missing-package,undefined-command,cv-ok,cv-sidebar-ok}.log`.
//! Sin dependencias nuevas (nada de `regex`): escaneo manual por líneas con captura de
//! `raw_block`. Mejoras de producción (M1-D3):
//!
//! - **Correlación estable de bloques**: un error `!` se lleva sus continuaciones
//!   (incluso cruzando una línea en blanco) hasta el siguiente diagnóstico,
//!   dos líneas en blanco seguidas o un marcador estructural del final del log.
//! - **Línea real del error**: para errores `!` sin `archivo:línea:` (paquetes,
//!   emergencias) se busca el contexto `l.NN` dentro del bloque o en un
//!   *lookahead* acotado.
//! - **Archivo probable**: pila de aperturas `(./main.tex … )` del propio log,
//!   para atribuir el error al archivo abierto en ese punto.
//! - **`Underfull`**, **severidad `Info`** (`Package … Info:`, `LaTeX Font Info:`)
//!   y **`BibError` real** vía [`parse_bib_log`] con el `.blg` de biber/bibtex.

use camino::Utf8PathBuf;

/// Tope de análisis (8 MB). Se recorta en un límite de carácter válido.
pub const MAX_LOG_BYTES: usize = 8 * 1024 * 1024;
/// Máximo de líneas que un bloque de error puede absorber.
const MAX_BLOCK_LINES: usize = 25;
/// Ventana de búsqueda del contexto `l.NN` tras el bloque.
const L_CONTEXT_LOOKAHEAD: usize = 15;
/// Tope de caracteres razonable para un token de ruta en el log.
const MAX_PATH_TOKEN: usize = 260;

/// Severidad de un diagnóstico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

impl DiagnosticSeverity {
    /// Rótulo corto para la UI (español; i18n llega con M1-A6).
    pub fn label(self) -> &'static str {
        match self {
            DiagnosticSeverity::Error => "Error",
            DiagnosticSeverity::Warning => "Advertencia",
            DiagnosticSeverity::Info => "Información",
        }
    }
}

/// Clasificación de un diagnóstico.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    MissingPackage { package: String },
    LatexError,
    BibError,
    UndefinedRef,
    Overfull,
    Underfull,
    RerunNeeded,
    Info,
    Unknown,
}

impl DiagnosticKind {
    /// Rótulo corto para la UI (español; i18n llega con M1-A6).
    pub fn label(&self) -> &'static str {
        match self {
            DiagnosticKind::MissingPackage { .. } => "Falta un paquete",
            DiagnosticKind::LatexError => "Error de LaTeX",
            DiagnosticKind::BibError => "Error de bibliografía",
            DiagnosticKind::UndefinedRef => "Referencia sin definir",
            DiagnosticKind::Overfull => "Caja demasiado ancha",
            DiagnosticKind::Underfull => "Caja demasiado estrecha",
            DiagnosticKind::RerunNeeded => "Requiere recompilar",
            DiagnosticKind::Info => "Información",
            DiagnosticKind::Unknown => "Aviso",
        }
    }
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
    // Archivo abierto en cada línea (pila `(…` / `)`), para atribuir errores `!`
    // que no traen `archivo:línea:`.
    let mut tracker = FileTracker::default();
    let mut current_file: Vec<Option<Utf8PathBuf>> = Vec::with_capacity(lines.len());
    for line in &lines {
        tracker.scan(line);
        current_file.push(tracker.current());
    }

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
            let line_no = scan_l_context(block.lines())
                .or_else(|| scan_l_context(lines[i + consumed..].iter().copied()));
            out.push(Diagnostic {
                severity: DiagnosticSeverity::Error,
                kind: classify_message(line),
                file: current_file[i].clone(),
                line: line_no,
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
            out.push(parse_box_warning(line, DiagnosticKind::Overfull));
            i += 1;
            continue;
        }
        if line.starts_with("Underfull ") {
            out.push(parse_box_warning(line, DiagnosticKind::Underfull));
            i += 1;
            continue;
        }
        if let Some(diag) = parse_info_line(line) {
            out.push(diag);
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
    dedupe_diagnostics(out)
}

/// Parsea un `.blg` de biber/bibtex (log aparte que el `.log` no incluye).
///
/// Cubre el subconjunto accionable: `ERROR - …` (biber), `WARN - …`,
/// `I couldn't open database file`, `Repeated entry---line N of file X`,
/// `Illegal, another \bibdata command`, `I found no \bibdata command`.
pub fn parse_bib_log(text: &str) -> Vec<Diagnostic> {
    let text = cap_log(text);
    let mut out: Vec<Diagnostic> = Vec::new();
    for line in text.lines() {
        if let Some(diag) = parse_bib_line(line) {
            out.push(diag);
        }
    }
    dedupe_diagnostics(out)
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
    if trimmed.starts_with("Overfull ") || trimmed.starts_with("Underfull ") {
        return Some(LogHint::Warning {
            message: trimmed.to_string(),
        });
    }
    if let Some(diag) = parse_bib_line(trimmed)
        && diag.severity == DiagnosticSeverity::Error
    {
        return Some(LogHint::Error {
            message: diag.message,
        });
    }
    if let Some(diag) = parse_warning_line(trimmed) {
        return Some(LogHint::Warning {
            message: diag.message,
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

/// Elimina duplicados exactos conservando el orden (usable al fusionar logs).
pub(crate) fn dedupe_diagnostics(mut diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
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
fn classify_message(message: &str) -> DiagnosticKind {
    if let Some(package) = extract_missing_package(message) {
        return DiagnosticKind::MissingPackage { package };
    }
    if message.contains("Biber") || message.contains("biber") {
        return DiagnosticKind::BibError;
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

/// Info tipo `Package x Info: ...`, `Class x Info: ...`, `LaTeX (Font) Info: ...`.
///
/// No llevan `line`: el `on input line` de un paquete se refiere a menudo a una
/// línea interna del paquete (p.ej. `hyperref Info: … on input line 4199`), y
/// usarla para «ir a línea» en el documento del usuario sería engañoso.
fn parse_info_line(line: &str) -> Option<Diagnostic> {
    let is_info = line.starts_with("LaTeX Info:")
        || line.starts_with("LaTeX Font Info:")
        || info_suffix(line, " Info:");
    if !is_info {
        return None;
    }
    Some(Diagnostic {
        severity: DiagnosticSeverity::Info,
        kind: DiagnosticKind::Info,
        file: None,
        line: None,
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

fn info_suffix(line: &str, needle: &str) -> bool {
    matches!(line.find(needle), Some(idx) if idx > 0)
        && (line.starts_with("Package ") || line.starts_with("Class "))
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

/// `Overfull \hbox (12.0pt too wide) in paragraph at lines 10--12` (y su Underfull).
fn parse_box_warning(line: &str, kind: DiagnosticKind) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Warning,
        kind,
        file: None,
        line: extract_input_line(line),
        column: None,
        message: line.trim_end().to_string(),
        raw_block: line.trim_end().to_string(),
    }
}

/// Errores de biber/bibtex (biber usa `ERROR - …`; bibtex `I couldn't open…`).
fn parse_bib_line(line: &str) -> Option<Diagnostic> {
    let trimmed = line.trim_end();
    let (severity, message) = if let Some(rest) = trimmed.strip_prefix("ERROR - ") {
        (DiagnosticSeverity::Error, rest)
    } else if let Some(rest) = trimmed.strip_prefix("FATAL - ") {
        (DiagnosticSeverity::Error, rest)
    } else if let Some(rest) = trimmed.strip_prefix("WARN - ") {
        (DiagnosticSeverity::Warning, rest)
    } else if trimmed.starts_with("I couldn't open database file")
        || trimmed.starts_with("I found no \\bibdata command")
        || trimmed.starts_with("Illegal, another \\bibdata command")
        || trimmed.starts_with("Repeated entry")
    {
        (DiagnosticSeverity::Error, trimmed)
    } else if trimmed.starts_with("Warning--") {
        (DiagnosticSeverity::Warning, trimmed)
    } else {
        return None;
    };
    let (file, line_no) = extract_bib_location(trimmed);
    Some(Diagnostic {
        severity,
        kind: DiagnosticKind::BibError,
        file,
        line: line_no,
        column: None,
        message: message.to_string(),
        raw_block: trimmed.to_string(),
    })
}

/// `Repeated entry---line 12 of file main.bib` → (`main.bib`, 12).
fn extract_bib_location(line: &str) -> (Option<Utf8PathBuf>, Option<u32>) {
    let Some(idx) = line.find("---line ") else {
        return (None, None);
    };
    let rest = &line[idx + "---line ".len()..];
    let line_no = leading_digits(rest);
    let file = rest
        .find(" of file ")
        .map(|i| Utf8PathBuf::from(rest[i + " of file ".len()..].trim()));
    (file, line_no)
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

/// ¿La línea arranca un diagnóstico nuevo? (frontera de bloque)
fn is_diagnostic_anchor(line: &str) -> bool {
    line.starts_with("! ")
        || line == "!"
        || parse_file_line_error(line).is_some()
        || line.starts_with("Overfull ")
        || line.starts_with("Underfull ")
        || parse_warning_line(line).is_some()
        || parse_info_line(line).is_some()
        || parse_bib_line(line).is_some()
}

/// Marcadores estructurales que cierran cualquier bloque (cola del log TeX).
fn is_structural_stop(line: &str) -> bool {
    line.starts_with("Here is how much of TeX's memory")
        || line.starts_with("Output written on")
        || line.starts_with("Transcript written on")
        || line.starts_with("PDF statistics:")
        || line.starts_with("Latexmk:")
}

/// Captura el bloque del diagnóstico que empieza en `start`.
///
/// Reglas estables (mismo orden de líneas ⇒ mismo bloque):
/// 1. la línea `start` siempre entra;
/// 2. sigue mientras la línea no sea un diagnóstico nuevo ni un marcador
///    estructural;
/// 3. cruza **una** línea en blanco si lo que sigue continúa el mensaje (así
///    los errores `!` de paquete se llevan su «Type X to quit…» y su `l.NN`);
///    dos líneas en blanco seguidas siempre cierran;
/// 4. tope de [`MAX_BLOCK_LINES`] líneas.
fn collect_block(lines: &[&str], start: usize) -> (String, usize) {
    let mut block: Vec<&str> = vec![lines[start]];
    let mut i = start + 1;
    let mut pending_blank = false;
    while i < lines.len() && block.len() < MAX_BLOCK_LINES {
        let line = lines[i];
        if line.trim().is_empty() {
            if pending_blank {
                // dos en blanco seguidas: fin
                break;
            }
            let next = lines[i + 1..]
                .iter()
                .position(|l| !l.trim().is_empty())
                .map(|offset| i + 1 + offset);
            let Some(next) = next else {
                break;
            };
            if next > i + 1 || is_diagnostic_anchor(lines[next]) || is_structural_stop(lines[next])
            {
                break;
            }
            pending_blank = true;
            i += 1;
            continue;
        }
        if is_diagnostic_anchor(line) || is_structural_stop(line) {
            break;
        }
        if pending_blank {
            block.push("");
            pending_blank = false;
        }
        block.push(line);
        i += 1;
    }
    (block.join("\n"), i.saturating_sub(start).max(1))
}

/// Busca el contexto `l.NN` que TeX imprime bajo la línea del error.
fn scan_l_context<'a>(lines: impl Iterator<Item = &'a str>) -> Option<u32> {
    for line in lines.take(L_CONTEXT_LOOKAHEAD) {
        if let Some(line_no) = l_context(line) {
            return Some(line_no);
        }
        if is_structural_stop(line) {
            break;
        }
    }
    None
}

/// `l.4 Hola \foo` → 4 (solo con `l.` al inicio de línea).
fn l_context(line: &str) -> Option<u32> {
    let rest = line.trim_start().strip_prefix("l.")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// Pila de archivos abiertos del log TeX (`(./main.tex … )`).
#[derive(Default)]
struct FileTracker {
    stack: Vec<Option<Utf8PathBuf>>,
}

impl FileTracker {
    /// Archivo abierto más reciente (el último `(ruta)` sin cerrar).
    fn current(&self) -> Option<Utf8PathBuf> {
        self.stack.iter().rev().find_map(|entry| entry.clone())
    }

    /// Recorre la línea: `(` empuja (ruta o marcador nulo) y `)` saca.
    fn scan(&mut self, line: &str) {
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'(' => {
                    self.stack.push(read_path_token(&line[i + 1..]));
                    i += 1;
                }
                b')' => {
                    self.stack.pop();
                    i += 1;
                }
                _ => i += 1,
            }
        }
    }
}

/// Lee el token tras un `(` y decide si es una ruta de archivo.
///
/// Acepta solo tokens sin caracteres imposibles en una ruta y con pinta de
/// archivo (separadores o extensión conocida); cualquier otro `(texto` entra
/// como marcador nulo para que su `)` cierre el par correctamente.
fn read_path_token(rest: &str) -> Option<Utf8PathBuf> {
    let end = rest
        .find([')', ' ', '\t', '{', '}', '[', ']', ','])
        .unwrap_or(rest.len());
    let token = &rest[..end];
    if token.is_empty() || token.len() > MAX_PATH_TOKEN {
        return None;
    }
    if token.chars().any(|c| {
        matches!(
            c,
            '=' | '(' | '<' | '>' | '"' | '|' | '?' | '*' | '#' | '%' | '\''
        )
    }) {
        return None;
    }
    let looks_like_path = token.contains('/')
        || token.contains('\\')
        || [
            ".tex", ".sty", ".cls", ".def", ".clo", ".cfg", ".fd", ".ltx", ".bib",
        ]
        .iter()
        .any(|ext| token.ends_with(ext));
    looks_like_path.then(|| Utf8PathBuf::from(token))
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

";

    const WARNINGS: &str = "\
LaTeX Warning: Reference `fig:uno' on page 1 undefined on input line 12.
LaTeX Warning: There were undefined references.
LaTeX Warning: Label(s) may have changed. Rerun to get cross-references right.
Package fancyhdr Warning: \\footskip is too small (0.0pt): 
(fancyhdr)                Make it at least 3.60004pt, for example:
Package rerunfilecheck Info: File `main.out' has not changed.
LaTeX Font Info:    Checking defaults for OML/cmm/m/it on input line 3.
Overfull \\hbox (12.0pt too wide) in paragraph at lines 10--12
Underfull \\vbox (badness 10000) has occurred while \\output is active [1]
Underfull \\hbox (badness 2357) in paragraph at lines 25--27
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
    fn paquete_inexistente_atribuye_archivo_y_linea_del_contexto() {
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
        // Correlación M1-D3: archivo del stack `(./main.tex` y línea del `l.4`.
        assert_eq!(
            mp[0].file.as_deref().and_then(|f| f.file_name()),
            Some("main.tex")
        );
        assert_eq!(mp[0].line, Some(4));
        // El bloque estable cruza el blanco pero se corta en el siguiente error.
        assert!(mp[0].raw_block.contains("Type X to quit"), "{:#?}", mp[0]);
        assert!(!mp[0].raw_block.contains("Emergency stop"));
        assert!(diags.iter().any(|d| d.line == Some(4)));
    }

    #[test]
    fn warnings_infos_y_underfull_clasificados() {
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
        let underfull: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::Underfull)
            .collect();
        assert_eq!(underfull.len(), 2, "{diags:#?}");
        assert!(underfull.iter().any(|d| d.line == Some(25)));
        // El `Info:` de rerunfilecheck ya no es warning, pero sí diagnóstico Info.
        assert!(!diags.iter().any(|d| {
            d.severity == DiagnosticSeverity::Warning && d.message.contains("has not changed")
        }));
        assert!(diags.iter().any(|d| {
            d.severity == DiagnosticSeverity::Info && d.message.contains("has not changed")
        }));
        // La fuente también es Info, nunca warning.
        assert!(diags.iter().any(|d| {
            d.severity == DiagnosticSeverity::Info && d.message.contains("LaTeX Font Info")
        }));
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
    fn bib_log_de_biber_y_bibtex() {
        let biber = "\
INFO - This is Biber 2.20
ERROR - Cannot find 'refs.bib'!
WARN - I didn't find a database entry for 'x'
INFO - WARNINGS: 0
";
        let diags = parse_bib_log(biber);
        assert_eq!(diags.len(), 2, "{diags:#?}");
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diags[0].kind, DiagnosticKind::BibError);
        assert_eq!(diags[0].message, "Cannot find 'refs.bib'!");
        assert_eq!(diags[1].severity, DiagnosticSeverity::Warning);

        let bibtex = "\
The top-level auxiliary file: main.aux
I couldn't open database file refs.bib
Repeated entry---line 12 of file main.bib
";
        let diags = parse_bib_log(bibtex);
        assert_eq!(diags.len(), 2, "{diags:#?}");
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diags[1].line, Some(12));
        assert_eq!(diags[1].file.as_ref().map(|f| f.as_str()), Some("main.bib"));
    }

    #[test]
    fn biber_en_el_log_principal_es_biberror() {
        let log = "./main.tex:9: Package biblatex Error: Please (re)run Biber on the file.\n";
        let diags = parse_log(log);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::BibError);
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
        assert!(matches!(
            parse_stdout_stream("Underfull \\hbox (badness 1000) in paragraph"),
            Some(LogHint::Warning { .. })
        ));
        assert!(matches!(
            parse_stdout_stream("ERROR - Cannot find 'refs.bib'!"),
            Some(LogHint::Error { .. })
        ));
        assert_eq!(parse_stdout_stream("[1{c:/tex/foo.map}]"), None);
        // Los Info no generan hint de progreso (evita ruido en la UI).
        assert_eq!(
            parse_stdout_stream("Package rerunfilecheck Info: File `main.out' has not changed."),
            None
        );
    }

    #[test]
    fn tracker_correlaciona_aperturas_y_cierres() {
        let mut tracker = FileTracker::default();
        tracker.scan("(./main.tex");
        tracker.scan("(c:/tex/article.cls");
        tracker.scan("(c:/tex/size10.clo");
        assert_eq!(
            tracker.current().as_ref().map(|p| p.as_str()),
            Some("c:/tex/size10.clo")
        );
        tracker.scan(")");
        assert_eq!(
            tracker.current().as_ref().map(|p| p.as_str()),
            Some("c:/tex/article.cls")
        );
        // `) (ruta` cierra el anterior y abre el siguiente en la misma línea.
        tracker.scan(") (./capitulo.tex");
        assert_eq!(
            tracker.current().as_ref().map(|p| p.as_str()),
            Some("./capitulo.tex")
        );
        tracker.scan(")");
        assert_eq!(
            tracker.current().as_ref().map(|p| p.as_str()),
            Some("./main.tex")
        );
        // Paréntesis de prosa no contaminan la pila.
        tracker.scan("(preloaded format=pdflatex 2026.9.13)");
        assert_eq!(
            tracker.current().as_ref().map(|p| p.as_str()),
            Some("./main.tex")
        );
    }
}
