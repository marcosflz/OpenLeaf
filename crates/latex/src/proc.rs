//! Ejecución de procesos con timeout, cancelación y captura de salida.
//!
//! Uso interno por `engine`, `synctex` y `tlmgr`. En Windows el kill es de árbol
//! (`taskkill /F /T /PID`), porque `latexmk` lanza `pdflatex` (y `biber`) como hijos.
//!
//! Estado: spike M0-S5. M1-D1 sustituye `taskkill` por un *job object* con
//! kill-on-close y añade `CREATE_NO_WINDOW` (ver `docs/plan/PLAN-MAESTRO.md` §5.3).

use std::io::{ErrorKind, Read};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::engine::CancelToken;

/// Intervalo de sondeo de `try_wait` (timeout/cancelación).
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Resultado crudo de un proceso ejecutado.
#[derive(Debug)]
pub(crate) struct ProcOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub cancelled: bool,
}

impl ProcOutput {
    /// stdout + stderr concatenados (los comandos de tex escriben en ambos).
    pub fn combined(&self) -> String {
        let mut s = self.stdout.clone();
        s.push_str(&self.stderr);
        s
    }
}

/// Ejecuta `cmd` capturando stdout/stderr a `String`.
///
/// - `on_line`: callback por cada línea de stdout (progreso en vivo).
/// - `cancel`: si se activa, mata el árbol de procesos y devuelve `cancelled = true`.
/// - `timeout`: si expira, mata el árbol y devuelve `timed_out = true`.
pub(crate) fn run(
    cmd: &mut Command,
    timeout: Duration,
    cancel: Option<&CancelToken>,
    on_line: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> std::io::Result<ProcOutput> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdout_pipe = child.stdout.take().expect("stdout pipeado");
    let mut stderr_pipe = child.stderr.take().expect("stderr pipeado");

    let deadline = Instant::now() + timeout;
    let (code, timed_out, cancelled, stdout, stderr) =
        std::thread::scope(|scope| -> std::io::Result<_> {
            let handle_out = scope.spawn(move || read_stream(&mut stdout_pipe, on_line));
            let handle_err = scope.spawn(move || read_stream(&mut stderr_pipe, None));
            let mut timed_out = false;
            let mut cancelled = false;
            let code = loop {
                if let Some(status) = child.try_wait()? {
                    break status.code();
                }
                if !cancelled && !timed_out {
                    if cancel.is_some_and(CancelToken::is_cancelled) {
                        cancelled = true;
                        kill_tree(&mut child);
                    } else if Instant::now() >= deadline {
                        timed_out = true;
                        kill_tree(&mut child);
                    }
                }
                std::thread::sleep(POLL_INTERVAL);
            };
            let stdout = handle_out.join().unwrap_or_default();
            let stderr = handle_err.join().unwrap_or_default();
            Ok((code, timed_out, cancelled, stdout, stderr))
        })?;

    Ok(ProcOutput {
        code,
        stdout,
        stderr,
        timed_out,
        cancelled,
    })
}

/// Mata el proceso y sus descendientes.
fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

/// Lee el stream completo, reensamblando líneas y emitiéndolas por `on_line`.
fn read_stream(pipe: &mut impl Read, on_line: Option<&(dyn Fn(&str) + Send + Sync)>) -> String {
    let mut out = String::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                pending.extend_from_slice(&chunk[..n]);
                while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = pending.drain(..=pos).collect();
                    let text = String::from_utf8_lossy(&line);
                    let text = text.trim_end_matches(['\r', '\n']);
                    if let Some(cb) = on_line {
                        cb(text);
                    }
                    out.push_str(text);
                    out.push('\n');
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    if !pending.is_empty() {
        let text = String::from_utf8_lossy(&pending);
        if let Some(cb) = on_line {
            cb(text.trim_end_matches(['\r', '\n']));
        }
        out.push_str(&text);
    }
    out
}
