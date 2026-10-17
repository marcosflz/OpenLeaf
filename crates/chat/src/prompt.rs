//! Composición de prompts del panel de chat (M1-E5).
//!
//! El flujo «Arreglar errores con IA» (skill `lsx-ai-fix-flow`) compone un
//! prompt con: diagnósticos actuales del panel de Problemas + cola del log de
//! compilación + contexto de proyecto. La plantilla vive aquí (Rust, testeable)
//! hasta que M2-6 la mueva a `assets/prompts` con hot reload.
//!
//! El agente `latex-fix` del proyecto ya define el proceso compilar → leer log
//! → `tlmgr install` → edición mínima → verificar; el prompt solo aporta los
//! datos concretos del fallo.

/// Línea de diagnóstico lista para el prompt (desacoplada de `lsx-latex`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixDiagnosticLine {
    /// Ruta relativa del archivo, si se conoce.
    pub file: Option<String>,
    /// Línea (1-based), si se conoce.
    pub line: Option<u32>,
    /// `error` | `warning` | `info`.
    pub severity: String,
    pub message: String,
}

impl FixDiagnosticLine {
    /// Render `archivo:línea [severidad] mensaje`.
    pub fn render(&self) -> String {
        let location = match (&self.file, self.line) {
            (Some(file), Some(line)) => format!("{file}:{line} "),
            (Some(file), None) => format!("{file} "),
            _ => String::new(),
        };
        format!("- {location}[{}] {}", self.severity, self.message)
    }
}

/// Datos de entrada del prompt «Arreglar errores».
#[derive(Debug, Clone, Default)]
pub struct FixPromptInput {
    /// Nombre/ruta del proyecto activo.
    pub project: Option<String>,
    /// Fichero principal (`main.tex`), si se detectó.
    pub main_file: Option<String>,
    /// Archivo activo del editor (contexto extra).
    pub active_file: Option<String>,
    /// Diagnósticos (los de severidad error deberían ir primero).
    pub diagnostics: Vec<FixDiagnosticLine>,
    /// Cola del log de la última compilación (≈200 líneas).
    pub log_tail: Option<String>,
}

/// Máximo de diagnósticos incluidos en el prompt.
pub const MAX_DIAGNOSTICS: usize = 30;

/// Compone el prompt de reparación.
pub fn compose_fix_prompt(input: &FixPromptInput) -> String {
    let mut prompt = String::from(
        "Arregla los errores de compilación de este proyecto LaTeX.\n\
         Sigue tu proceso: diagnostica con el log, instala con tlmgr si falta un \
         paquete y edita el .tex con el cambio mínimo; recompila y verifica.\n",
    );
    if let Some(project) = &input.project {
        prompt.push_str(&format!("\nProyecto: {project}\n"));
    }
    if let Some(main_file) = &input.main_file {
        prompt.push_str(&format!(
            "Fichero principal: {main_file} (compílalo con latexmk -pdf)\n"
        ));
    }
    if let Some(active_file) = &input.active_file {
        prompt.push_str(&format!("Archivo abierto en el editor: {active_file}\n"));
    }

    prompt.push_str("\nDiagnósticos actuales (panel de Problemas):\n");
    if input.diagnostics.is_empty() {
        prompt.push_str("- (el panel no tiene diagnósticos; mira el log)\n");
    } else {
        for line in input.diagnostics.iter().take(MAX_DIAGNOSTICS) {
            prompt.push_str(&line.render());
            prompt.push('\n');
        }
        if input.diagnostics.len() > MAX_DIAGNOSTICS {
            prompt.push_str(&format!(
                "- …y {} diagnósticos más\n",
                input.diagnostics.len() - MAX_DIAGNOSTICS
            ));
        }
    }

    if let Some(tail) = &input.log_tail
        && !tail.trim().is_empty()
    {
        prompt.push_str("\nCola del log de compilación:\n```log\n");
        prompt.push_str(tail.trim_end());
        prompt.push_str("\n```\n");
    }

    prompt.push_str(
        "\nAl terminar informa: paquetes instalados, archivos editados y estado \
         final de la compilación.\n",
    );
    prompt
}

/// Antepone los chips de contexto al texto del usuario.
pub fn compose_prompt_with_context(
    text: &str,
    active_file: Option<&str>,
    selection: Option<&str>,
) -> String {
    let mut prompt = String::new();
    if let Some(file) = active_file {
        prompt.push_str(&format!("Contexto: archivo abierto `{file}`.\n"));
    }
    if let Some(selection) = selection
        && !selection.trim().is_empty()
    {
        prompt.push_str("Selección del editor:\n```latex\n");
        prompt.push_str(selection.trim_end());
        prompt.push_str("\n```\n");
    }
    if !prompt.is_empty() {
        prompt.push('\n');
    }
    prompt.push_str(text);
    prompt
}

/// Últimas `max_lines` líneas de un texto (para la cola del log).
pub fn last_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

/// Cola del log de compilación de `project_root`: `<main>.log` si se conoce su
/// fichero principal; si no, el `.log` más reciente del directorio. ~200 líneas
/// para el prompt de reparación (M1-E5).
pub fn read_log_tail(project_root: &std::path::Path, main_file: Option<&str>) -> Option<String> {
    let log = main_file
        .map(|main| project_root.join(main).with_extension("log"))
        .filter(|log| log.is_file())
        .or_else(|| {
            let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
            for entry in std::fs::read_dir(project_root).ok()?.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|extension| extension != "log") {
                    continue;
                }
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                let Ok(modified) = metadata.modified() else {
                    continue;
                };
                if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
                    newest = Some((modified, path));
                }
            }
            newest.map(|(_, path)| path)
        })?;
    let text = std::fs::read_to_string(&log).ok()?;
    Some(last_lines(&text, 200))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fix_prompt_carries_diagnostics_and_log_tail() {
        let input = FixPromptInput {
            project: Some("cv".to_owned()),
            main_file: Some("main.tex".to_owned()),
            active_file: Some("template.tex".to_owned()),
            diagnostics: vec![
                FixDiagnosticLine {
                    file: Some("template.tex".to_owned()),
                    line: Some(12),
                    severity: "error".to_owned(),
                    message: "Undefined control sequence".to_owned(),
                },
                FixDiagnosticLine {
                    file: None,
                    line: None,
                    severity: "warning".to_owned(),
                    message: "Overfull".to_owned(),
                },
            ],
            log_tail: Some("! Undefined control sequence.\nl.12 \\foo".to_owned()),
        };
        let prompt = compose_fix_prompt(&input);
        assert!(prompt.contains("template.tex:12 [error] Undefined control sequence"));
        assert!(prompt.contains("- [warning] Overfull"));
        assert!(prompt.contains("```log"));
        assert!(prompt.contains("l.12 \\foo"));
        assert!(prompt.contains("main.tex"));
        assert!(prompt.contains("tlmgr"));
    }

    #[test]
    fn the_fix_prompt_without_diagnostics_still_works() {
        let prompt = compose_fix_prompt(&FixPromptInput::default());
        assert!(prompt.contains("(el panel no tiene diagnósticos"));
        assert!(!prompt.contains("```log"));
    }

    #[test]
    fn diagnostics_are_capped_with_a_summary_line() {
        let diagnostics: Vec<FixDiagnosticLine> = (0..40)
            .map(|index| FixDiagnosticLine {
                file: None,
                line: Some(index),
                severity: "error".to_owned(),
                message: format!("e{index}"),
            })
            .collect();
        let prompt = compose_fix_prompt(&FixPromptInput {
            diagnostics,
            ..FixPromptInput::default()
        });
        assert!(prompt.contains("e29"));
        assert!(!prompt.contains("- [error] e30"));
        assert!(prompt.contains("10 diagnósticos más"));
    }

    #[test]
    fn context_chips_prefix_the_user_prompt() {
        let prompt = compose_prompt_with_context(
            "resume el documento",
            Some("template.tex"),
            Some("\\section{A}"),
        );
        assert!(prompt.starts_with("Contexto: archivo abierto `template.tex`."));
        assert!(prompt.contains("```latex\n\\section{A}\n```"));
        assert!(prompt.ends_with("resume el documento"));
    }

    #[test]
    fn context_is_a_no_op_without_chips() {
        assert_eq!(
            compose_prompt_with_context("hola", None, None),
            "hola".to_owned()
        );
    }

    #[test]
    fn last_lines_keeps_the_tail() {
        let text = "1\n2\n3\n4\n5";
        assert_eq!(last_lines(text, 2), "4\n5");
        assert_eq!(last_lines(text, 99), text);
        assert_eq!(last_lines("", 3), "");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "lsx-chat-prompt-{}-{}-{n}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("crear dir temporal");
        dir
    }

    #[test]
    fn log_tail_prefers_the_main_file_log() {
        let dir = temp_dir("log-main");
        std::fs::write(dir.join("main.log"), "linea 1\nlinea 2\n").expect("escribir log");
        std::fs::write(dir.join("otro.log"), "no me leas").expect("escribir log");
        let tail = read_log_tail(&dir, Some("main.tex")).expect("cola del log");
        assert!(tail.contains("linea 2"));
        assert!(!tail.contains("no me leas"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_tail_falls_back_to_the_newest_log() {
        let dir = temp_dir("log-newest");
        std::fs::write(dir.join("viejo.log"), "viejo").expect("escribir log");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("nuevo.log"), "nuevo").expect("escribir log");
        let tail = read_log_tail(&dir, None).expect("cola del log");
        assert_eq!(tail, "nuevo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_tail_is_none_without_logs() {
        let dir = temp_dir("log-none");
        assert!(read_log_tail(&dir, None).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
