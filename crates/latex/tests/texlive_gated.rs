//! Tests de integración **reales** del motor (spike M0-S5).
//!
//! Requieren el TeX Live portable del repo: activar con
//! `$env:LSX_TEST_TEXLIVE="1"; cargo test -p lsx-latex -- --nocapture`.
//! Sin la variable, los tests se saltan (print + return) y todo queda verde.
//!
//! El test de auto-install (`tlmgr`) además exige `LSX_TEST_TLMGR_INSTALL=1`
//! porque **modifica** el TeX Live portable (descarga/reinstala un paquete y usa red).

use std::sync::Mutex;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};

use lsx_latex::{
    CancelToken, CompileEvent, CompileRequest, CompileStatus, DiagnosticKind, LatexEngine, Synctex,
    TexLiveEngine, Tlmgr,
};

fn fixture_dir(name: &str) -> Utf8PathBuf {
    Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/projects")
        .join(name)
}

fn gated() -> bool {
    std::env::var("LSX_TEST_TEXLIVE").ok().as_deref() == Some("1")
}

fn tlmgr_install_gated() -> bool {
    gated() && std::env::var("LSX_TEST_TLMGR_INSTALL").ok().as_deref() == Some("1")
}

/// Directorio temporal por test; se limpia al terminar salvo `LSX_TEST_KEEP=1`.
struct TempDir(Utf8PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let base = Utf8PathBuf::from_path_buf(std::env::temp_dir())
            .expect("temp dir UTF-8")
            .join("lsx-s5")
            .join(name);
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("crear temp");
        eprintln!("[temp] {}", base);
        Self(base)
    }

    fn path(&self) -> &Utf8Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if std::env::var("LSX_TEST_KEEP").is_err() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// Copia la fixture (sin ensuciarla) al directorio `src` del temp.
fn stage_project(fixture: &str, dest: &Utf8Path) {
    std::fs::create_dir_all(dest).expect("src del temp");
    let origin = fixture_dir(fixture);
    for entry in std::fs::read_dir(&origin).unwrap_or_else(|e| panic!("fixture {origin}: {e}")) {
        let entry = entry.expect("entrada de fixture");
        if entry.file_type().expect("tipo").is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            std::fs::copy(entry.path(), dest.join(name.as_str())).expect("copiar fixture");
        }
    }
}

fn engine() -> TexLiveEngine {
    TexLiveEngine::detect().expect("TeX Live portable en tex/bin/windows")
}

fn compile(
    engine: &TexLiveEngine,
    project: &Utf8Path,
    auto_install: bool,
    events: &Mutex<Vec<CompileEvent>>,
) -> lsx_latex::CompileOutcome {
    let mut req = CompileRequest::new(project, project.join("main.tex"));
    req.out_dir = Some(project.parent().unwrap().join("out"));
    req.timeout = Duration::from_secs(300);
    req.auto_install = auto_install;
    let on_event = |e: CompileEvent| events.lock().expect("mutex").push(e);
    engine
        .compile(&req, &on_event, &CancelToken::new())
        .expect("compilación sin errores de proceso")
}

#[test]
fn probe_detecta_latexmk_y_version() {
    if !gated() {
        eprintln!("skip: define LSX_TEST_TEXLIVE=1");
        return;
    }
    let engine = engine();
    let info = engine.probe().expect("probe");
    eprintln!(
        "[probe] latexmk={} version={:?} available={} texbin={}",
        info.latexmk, info.version, info.available, info.texbin
    );
    assert!(info.available);
    let version = info.version.expect("versión de latexmk");
    assert!(version.starts_with("4."), "versión inesperada: {version}");
    assert!(info.synctex.is_some(), "synctex.exe presente");
    assert!(info.tlmgr.is_some(), "tlmgr.bat presente");
}

#[test]
fn compila_cv_en_temp_con_outdir() {
    if !gated() {
        eprintln!("skip: define LSX_TEST_TEXLIVE=1");
        return;
    }
    let tmp = TempDir::new("test-cv");
    let src = tmp.path().join("src");
    stage_project("cv", &src);

    let engine = engine();
    let events = Mutex::new(Vec::new());
    let outcome = compile(&engine, &src, false, &events);
    eprintln!(
        "[cv] status={:?} dur={:?} pdf={:?} log={:?} diags={} exit={:?}",
        outcome.status,
        outcome.duration,
        outcome.pdf,
        outcome.log,
        outcome.diagnostics.len(),
        outcome.exit_code
    );
    for d in &outcome.diagnostics {
        eprintln!(
            "[cv-diag] {:?}/{:?} {:?}:{:?}",
            d.severity, d.kind, d.file, d.line
        );
    }
    assert_eq!(
        outcome.status,
        CompileStatus::Success,
        "{}",
        outcome.log_tail
    );
    assert!(outcome.duration > Duration::ZERO);
    let pdf = outcome.pdf.as_ref().expect("pdf generado");
    assert!(pdf.is_file(), "pdf en {pdf}");
    assert!(outcome.log.as_ref().is_some_and(|l| l.is_file()));
    assert!(
        !outcome
            .diagnostics
            .iter()
            .any(|d| d.severity == lsx_latex::DiagnosticSeverity::Error),
        "cv no debe producir errores: {:#?}",
        outcome.diagnostics
    );
    // La compilación va a `out/`: el src queda con solo main.tex (fixture intacta).
    let src_files: Vec<_> = std::fs::read_dir(&src)
        .expect("src")
        .map(|e| e.expect("entrada").file_name())
        .collect();
    assert_eq!(src_files.len(), 1, "src ensuciado: {src_files:?}");
    // Eventos: Started + Progress + Finished.
    let events = events.into_inner().expect("mutex");
    assert!(matches!(events.first(), Some(CompileEvent::Started { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, CompileEvent::Progress { line } if line.contains("Latexmk:") || line.contains("Output written"))));
    assert!(matches!(events.last(), Some(CompileEvent::Finished { .. })));

    // Segunda pasada con `clean = true` (latexmk -C + rebuild) sobre el mismo outdir.
    let mut req = CompileRequest::new(&src, src.join("main.tex"));
    req.out_dir = Some(tmp.path().join("out"));
    req.timeout = Duration::from_secs(300);
    req.clean = true;
    let clean_outcome = engine
        .compile(&req, &(|_| {}), &CancelToken::new())
        .expect("clean compile");
    assert_eq!(
        clean_outcome.status,
        CompileStatus::Success,
        "{}",
        clean_outcome.log_tail
    );
    assert!(clean_outcome.pdf.as_ref().is_some_and(|p| p.is_file()));
}

#[test]
fn broken_undefined_falla_con_linea_4() {
    if !gated() {
        eprintln!("skip: define LSX_TEST_TEXLIVE=1");
        return;
    }
    let tmp = TempDir::new("test-undefined");
    let src = tmp.path().join("src");
    stage_project("broken-undefined", &src);

    let engine = engine();
    let events = Mutex::new(Vec::new());
    let outcome = compile(&engine, &src, false, &events);
    eprintln!(
        "[undefined] status={:?} exit={:?} diags={:#?}",
        outcome.status, outcome.exit_code, outcome.diagnostics
    );
    assert_eq!(outcome.status, CompileStatus::Failed);
    assert_eq!(outcome.exit_code, Some(12));
    let diag = outcome
        .diagnostics
        .iter()
        .find(|d| {
            d.line == Some(4) && d.file.as_ref().and_then(|f| f.file_name()) == Some("main.tex")
        })
        .unwrap_or_else(|| panic!("sin main.tex:4 en {:#?}", outcome.diagnostics));
    assert!(diag.message.contains("Undefined control sequence"));
}

#[test]
fn broken_missingpkg_reporta_el_paquete() {
    if !gated() {
        eprintln!("skip: define LSX_TEST_TEXLIVE=1");
        return;
    }
    let tmp = TempDir::new("test-missingpkg");
    let src = tmp.path().join("src");
    stage_project("broken-missingpkg", &src);

    let engine = engine();
    let events = Mutex::new(Vec::new());
    let outcome = compile(&engine, &src, false, &events);
    eprintln!(
        "[missingpkg] status={:?} exit={:?} diags={:#?}",
        outcome.status, outcome.exit_code, outcome.diagnostics
    );
    assert_eq!(outcome.status, CompileStatus::Failed);
    assert!(outcome.pdf.is_none(), "no debe haber PDF");
    assert!(outcome.package_installs.is_empty(), "auto_install apagado");
    assert!(outcome.diagnostics.iter().any(|d| matches!(
        &d.kind,
        DiagnosticKind::MissingPackage { package } if package == "estepaquete-quenoexiste"
    )));
}

#[test]
fn synctex_roundtrip_sobre_linea_conocida() {
    if !gated() {
        eprintln!("skip: define LSX_TEST_TEXLIVE=1");
        return;
    }
    let tmp = TempDir::new("test-synctex");
    let src = tmp.path().join("src");
    stage_project("cv", &src);
    let main_tex = src.join("main.tex");

    // Línea conocida del cuerpo: la de "Senior Engineer" (buscada, no hardcodeada).
    let body = std::fs::read_to_string(&main_tex).expect("main.tex");
    let line = body
        .lines()
        .position(|l| l.contains("Senior Engineer"))
        .map(|i| i as u32 + 1)
        .expect("línea Senior Engineer en la fixture");

    let engine = engine();
    let events = Mutex::new(Vec::new());
    let outcome = compile(&engine, &src, false, &events);
    assert_eq!(
        outcome.status,
        CompileStatus::Success,
        "{}",
        outcome.log_tail
    );
    let pdf = outcome.pdf.expect("pdf");

    let synctex = Synctex::new(engine.paths().synctex.clone());
    let point = synctex
        .forward(&pdf, &main_tex, line, 0)
        .expect("forward SyncTeX");
    eprintln!(
        "[synctex] forward línea {line} -> page={} x={:.6} y={:.6} w={:.6} h={:.6}",
        point.page, point.x, point.y, point.w, point.h
    );
    assert_eq!(point.page, 1);
    assert!(point.x > 0.0 && point.y > 0.0, "coordenadas bp: {point:?}");
    assert!(point.w > 0.0 && point.h > 0.0, "caja no vacía: {point:?}");

    let records = synctex
        .forward_all(&pdf, &main_tex, line, 0)
        .expect("forward_all");
    eprintln!("[synctex] registros para la línea: {}", records.len());
    assert!(!records.is_empty());

    let source = synctex
        .inverse(&pdf, point.page, point.x, point.y)
        .expect("inverse SyncTeX");
    eprintln!(
        "[synctex] inverse ({}, {:.6}, {:.6}) -> {}:{} (col={:?})",
        point.page, point.x, point.y, source.file, source.line, source.column
    );
    assert!(
        source.file.as_str().ends_with("main.tex"),
        "archivo inesperado: {}",
        source.file
    );
    assert!(
        source.line.abs_diff(line) <= 2,
        "inverse devolvió línea {} (esperada {line} ±2)",
        source.line
    );
}

#[test]
fn tlmgr_autoinstall_repara_paquete_ausente() {
    if !tlmgr_install_gated() {
        eprintln!("skip: define LSX_TEST_TEXLIVE=1 y LSX_TEST_TLMGR_INSTALL=1");
        return;
    }
    let package = "adjmulticol";
    let tmp = TempDir::new("test-tlmgr-install");
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).expect("src");
    std::fs::write(
        src.join("main.tex"),
        "\\documentclass{article}\n\\usepackage{adjmulticol}\n\\begin{document}\nHola tlmgr.\n\\end{document}\n",
    )
    .expect("mini proyecto");

    let engine = engine();
    let tlmgr = Tlmgr::new(engine.paths().tlmgr.clone());
    eprintln!("[tlmgr] {}", tlmgr.version().expect("version"));
    if tlmgr.is_installed(package).expect("is_installed") {
        eprintln!(
            "[tlmgr] {package} instalado; se desinstala para probar el auto-install (reversible)"
        );
        tlmgr.remove(package).expect("remove");
    }
    assert!(!tlmgr.is_installed(package).expect("is_installed"));

    let events = Mutex::new(Vec::new());
    let first = compile(&engine, &src, false, &events);
    eprintln!(
        "[tlmgr] sin auto-install: status={:?} diags={:#?}",
        first.status, first.diagnostics
    );
    assert_eq!(first.status, CompileStatus::Failed);
    assert!(first.diagnostics.iter().any(|d| matches!(
        &d.kind,
        DiagnosticKind::MissingPackage { package: p } if p == package
    )));

    let second = compile(&engine, &src, true, &events);
    eprintln!(
        "[tlmgr] con auto-install: status={:?} installs={:#?}",
        second.status, second.package_installs
    );
    assert_eq!(second.status, CompileStatus::Success, "{}", second.log_tail);
    assert!(
        second
            .package_installs
            .iter()
            .any(|i| i.package == package && i.success)
    );
    assert!(second.pdf.as_ref().is_some_and(|p| p.is_file()));
    assert!(tlmgr.is_installed(package).expect("is_installed"));
}
