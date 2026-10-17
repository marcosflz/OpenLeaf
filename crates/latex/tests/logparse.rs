//! `parse_log` contra los fixtures reales de `fixtures/logs/` (P0.6).
//!
//! Si los fixtures no existen (checkout sin `cargo xtask fixtures`), los tests se
//! saltan sin fallar (regla del skill `lsx-latex-engine`).

use std::path::{Path, PathBuf};

use lsx_latex::{DiagnosticKind, DiagnosticSeverity, LogHint, parse_log, parse_stdout_stream};

fn fixtures_logs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/logs")
}

fn read_fixture(name: &str) -> Option<String> {
    let path = fixtures_logs_dir().join(name);
    if !path.is_file() {
        eprintln!("skip: falta fixture {}", path.display());
        return None;
    }
    Some(std::fs::read_to_string(&path).expect("fixture legible"))
}

#[test]
fn missing_package_log_detalla_el_paquete() {
    let Some(log) = read_fixture("missing-package.log") else {
        return;
    };
    let diagnostics = parse_log(&log);
    let missing: Vec<_> = diagnostics
        .iter()
        .filter(|d| matches!(d.kind, DiagnosticKind::MissingPackage { .. }))
        .collect();
    assert_eq!(missing.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        missing[0].kind,
        DiagnosticKind::MissingPackage {
            package: "estepaquete-quenoexiste".to_string()
        }
    );
    assert_eq!(missing[0].severity, DiagnosticSeverity::Error);
    // M1-D3: el `!` sin `archivo:línea:` se correlaciona con el stack `(./main.tex`
    // y el contexto `l.4` del propio log.
    assert_eq!(
        missing[0].file.as_deref().and_then(|f| f.file_name()),
        Some("main.tex"),
        "{:#?}",
        missing[0]
    );
    assert_eq!(missing[0].line, Some(4), "{:#?}", missing[0]);
    // El bloque estable cruza el blanco («Type X to quit…») y se corta en el
    // siguiente error (`Emergency stop`).
    assert!(missing[0].raw_block.contains("Type X to quit"));
    assert!(!missing[0].raw_block.contains("Emergency stop"));
    // El resto del log (Emergency stop / fatal) también es error severo.
    assert!(
        diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error && d.line == Some(4))
    );
}

#[test]
fn undefined_command_log_trae_archivo_y_linea() {
    let Some(log) = read_fixture("undefined-command.log") else {
        return;
    };
    let diagnostics = parse_log(&log);
    let target = diagnostics
        .iter()
        .find(|d| {
            d.line == Some(4) && d.file.as_ref().and_then(|f| f.file_name()) == Some("main.tex")
        })
        .unwrap_or_else(|| panic!("sin diagnóstico main.tex:4 en {diagnostics:#?}"));
    assert_eq!(target.severity, DiagnosticSeverity::Error);
    assert_eq!(target.kind, DiagnosticKind::LatexError);
    assert!(target.raw_block.contains("\\undefinedcommand"));
    // El PDF se produjo pese al error (nonstopmode): el parseo no inventa missing package.
    assert!(
        !diagnostics
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::MissingPackage { .. }))
    );
}

#[test]
fn logs_ok_sin_errores_severos() {
    for name in ["cv-ok.log", "cv-sidebar-ok.log"] {
        let Some(log) = read_fixture(name) else {
            return;
        };
        let diagnostics = parse_log(&log);
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();
        assert!(errors.is_empty(), "{name} produjo errores: {errors:#?}");
        // M1-D3: los Info de fuentes/paquetes existen pero nunca como warning.
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == DiagnosticSeverity::Info),
            "{name} sin diagnósticos Info: {diagnostics:#?}"
        );
        assert!(
            !diagnostics.iter().any(|d| {
                d.severity == DiagnosticSeverity::Warning && d.message.contains("Info:")
            }),
            "{name} coló un Info como warning"
        );
    }
}

#[test]
fn undefined_command_log_trae_bloque_multilinea_estable() {
    let Some(log) = read_fixture("undefined-command.log") else {
        return;
    };
    let diagnostics = parse_log(&log);
    let target = diagnostics
        .iter()
        .find(|d| {
            d.line == Some(4) && d.file.as_ref().and_then(|f| f.file_name()) == Some("main.tex")
        })
        .expect("diagnóstico main.tex:4");
    // El bloque se lleva las líneas de contexto del error, no las de memoria.
    assert!(target.raw_block.contains("l.4 Hola"));
    assert!(target.raw_block.contains("never \\def'ed"));
    assert!(!target.raw_block.contains("Here is how much"));
}

#[test]
fn cv_sidebar_ok_produce_overfull() {
    let Some(log) = read_fixture("cv-sidebar-ok.log") else {
        return;
    };
    let diagnostics = parse_log(&log);
    let overfull = diagnostics
        .iter()
        .find(|d| d.kind == DiagnosticKind::Overfull)
        .expect("overfull del fixture");
    assert_eq!(overfull.severity, DiagnosticSeverity::Warning);
    assert_eq!(overfull.line, Some(147));
}

#[test]
fn cv_ok_produce_el_warning_de_fancyhdr() {
    let Some(log) = read_fixture("cv-ok.log") else {
        return;
    };
    let diagnostics = parse_log(&log);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("Package fancyhdr Warning"))
    );
}

#[test]
fn stdout_stream_reconoce_output_y_errores() {
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
        parse_stdout_stream("! LaTeX Error: File `x.sty' not found."),
        Some(LogHint::Error { .. })
    ));
    assert!(matches!(
        parse_stdout_stream("Latexmk: All targets (main.pdf) are up-to-date"),
        Some(LogHint::Progress { .. })
    ));
}
