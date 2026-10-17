//! Demo CLI del spike M0-S5 (sin gpui).
//!
//! ```text
//! cargo run -p lsx-latex --example spike            # todo
//! cargo run -p lsx-latex --example spike probe
//! cargo run -p lsx-latex --example spike compile
//! cargo run -p lsx-latex --example spike synctex
//! cargo run -p lsx-latex --example spike tlmgr
//! ```
//!
//! Copia cada fixture a `%TEMP%\lsx-s5\demo\<nombre>` y compila con `-outdir`
//! (las fixtures del repo nunca se tocan). Los temporales se dejan para inspección;
//! se pueden borrar con `Remove-Item -Recurse -Force $env:TEMP\lsx-s5`.

use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};

use lsx_latex::{
    CancelToken, CompileEvent, CompileOutcome, CompileRequest, CompileStatus, Diagnostic,
    LatexEngine, Synctex, TexLiveEngine, Tlmgr,
};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    match mode.as_str() {
        "probe" => demo_probe(),
        "compile" => demo_compile(),
        "synctex" => demo_synctex(),
        "tlmgr" => demo_tlmgr(),
        "all" => {
            demo_probe();
            demo_compile();
            demo_synctex();
            demo_tlmgr();
        }
        other => eprintln!("uso: spike [all|probe|compile|synctex|tlmgr] (recibido `{other}`)"),
    }
}

fn demo_dir(name: &str) -> Utf8PathBuf {
    let base = Utf8PathBuf::from_path_buf(std::env::temp_dir())
        .expect("temp UTF-8")
        .join("lsx-s5")
        .join("demo")
        .join(name);
    if base.exists() {
        std::fs::remove_dir_all(&base).expect("limpiar demo previo");
    }
    std::fs::create_dir_all(&base).expect("crear demo");
    base
}

fn fixture_dir(name: &str) -> Utf8PathBuf {
    Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/projects")
        .join(name)
}

fn stage(fixture: &str, demo: &Utf8Path) -> Utf8PathBuf {
    let src = demo.join("src");
    std::fs::create_dir_all(&src).expect("src");
    for entry in std::fs::read_dir(fixture_dir(fixture)).expect("fixture") {
        let entry = entry.expect("entrada");
        if entry.file_type().expect("tipo").is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            std::fs::copy(entry.path(), src.join(name.as_str())).expect("copiar");
        }
    }
    src
}

fn demo_probe() {
    println!("== probe ==");
    let engine = match TexLiveEngine::detect() {
        Ok(engine) => engine,
        Err(e) => {
            println!("  TeX Live no detectado: {e}");
            return;
        }
    };
    match engine.probe() {
        Ok(info) => {
            println!(
                "  engine:    {:?} (disponible: {})",
                info.id, info.available
            );
            println!("  latexmk:   {} ({:?})", info.latexmk, info.version);
            println!("  texbin:    {}", info.texbin);
            println!("  synctex:   {:?}", info.synctex);
            println!("  tlmgr:     {:?}", info.tlmgr);
        }
        Err(e) => println!("  probe falló: {e}"),
    }
}

fn demo_compile() {
    println!("== compile (fixtures copiadas a %TEMP%) ==");
    let engine = match TexLiveEngine::detect() {
        Ok(engine) => engine,
        Err(e) => {
            println!("  TeX Live no detectado: {e}");
            return;
        }
    };
    for fixture in ["cv", "broken-undefined", "broken-missingpkg"] {
        let demo = demo_dir(fixture);
        let src = stage(fixture, &demo);
        let mut req = CompileRequest::new(&src, src.join("main.tex"));
        req.out_dir = Some(demo.join("out"));
        req.timeout = Duration::from_secs(300);
        let outcome = match engine.compile(&req, &|_| {}, &CancelToken::new()) {
            Ok(outcome) => outcome,
            Err(e) => {
                println!("  {fixture}: error de proceso: {e}");
                continue;
            }
        };
        print_outcome(fixture, &outcome);
    }
}

fn print_outcome(fixture: &str, outcome: &CompileOutcome) {
    println!(
        "  {fixture}: {:?} en {:.0} ms (exit {:?}) pdf={} log={}",
        outcome.status,
        outcome.duration.as_millis(),
        outcome.exit_code,
        outcome.pdf.as_ref().map(|p| p.as_str()).unwrap_or("—"),
        outcome.log.as_ref().map(|p| p.as_str()).unwrap_or("—"),
    );
    print_diags(&outcome.diagnostics);
}

fn print_diags(diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        println!("    diagnósticos: 0");
        return;
    }
    println!("    diagnósticos: {}", diagnostics.len());
    for diag in diagnostics.iter().take(8) {
        let location = format!(
            "{}:{}",
            diag.file
                .as_ref()
                .map(|f| f.file_name().unwrap_or(f.as_str()))
                .unwrap_or("—"),
            diag.line
                .map(|l| l.to_string())
                .unwrap_or_else(|| "—".into())
        );
        let message = diag.message.lines().next().unwrap_or("");
        println!(
            "      [{:?}/{:?}] {location} {message}",
            diag.severity, diag.kind
        );
    }
    if diagnostics.len() > 8 {
        println!("      … y {} más", diagnostics.len() - 8);
    }
}

fn demo_synctex() {
    println!("== synctex (forward/inverse sobre cv) ==");
    let engine = match TexLiveEngine::detect() {
        Ok(engine) => engine,
        Err(e) => {
            println!("  TeX Live no detectado: {e}");
            return;
        }
    };
    let demo = demo_dir("synctex-cv");
    let src = stage("cv", &demo);
    let main_tex = src.join("main.tex");
    let body = std::fs::read_to_string(&main_tex).expect("main.tex");
    let line = body
        .lines()
        .position(|l| l.contains("Senior Engineer"))
        .map(|i| i as u32 + 1)
        .expect("línea Senior Engineer");
    let mut req = CompileRequest::new(&src, main_tex.clone());
    req.out_dir = Some(demo.join("out"));
    let events: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let on_event = |e: CompileEvent| {
        if let CompileEvent::Progress { line, .. } = e {
            events.lock().expect("mutex").push(line);
        }
    };
    let outcome = engine
        .compile(&req, &on_event, &CancelToken::new())
        .expect("compilar cv");
    if outcome.status != CompileStatus::Success {
        println!("  compilación no limpia: {:?}", outcome.status);
        return;
    }
    let progress = events.lock().expect("mutex");
    println!(
        "  progreso en vivo: {} líneas con hint, p.ej. {:?}",
        progress.len(),
        progress.first()
    );
    drop(progress);
    let pdf = outcome.pdf.expect("pdf");
    let synctex = Synctex::new(engine.paths().synctex.clone());
    let point = synctex.forward(&pdf, &main_tex, line, 0).expect("forward");
    println!(
        "  forward línea {line} → página {} punto ({:.3}, {:.3}) caja {:.3}×{:.3} bp",
        point.page, point.x, point.y, point.w, point.h
    );
    let source = synctex
        .inverse(&pdf, point.page, point.x, point.y)
        .expect("inverse");
    println!(
        "  inverse ({:.3}, {:.3}) → {}:{} (columna {:?})",
        point.x, point.y, source.file, source.line, source.column
    );
}

fn demo_tlmgr() {
    println!("== tlmgr ==");
    let engine = match TexLiveEngine::detect() {
        Ok(engine) => engine,
        Err(e) => {
            println!("  TeX Live no detectado: {e}");
            return;
        }
    };
    let tlmgr = Tlmgr::new(engine.paths().tlmgr.clone());
    match tlmgr.version() {
        Ok(version) => println!("  {version}"),
        Err(e) => println!("  version falló: {e}"),
    }
    match tlmgr.is_installed("adjmulticol") {
        Ok(installed) => println!("  adjmulticol instalado: {installed}"),
        Err(e) => println!("  is_installed falló: {e}"),
    }
}
