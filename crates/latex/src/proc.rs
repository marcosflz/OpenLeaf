//! Ejecución de procesos con timeout, cancelación, captura de salida y aislamiento.
//!
//! Producción M1-D1 (sustituye al `taskkill /T` del spike M0-S5):
//!
//! - **Job objects** de Windows con `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: cada
//!   `run()` crea un job y asigna el proceso raíz. Todo hijo (pdflatex, perl,
//!   biber…) nace dentro del job, así que cerrar el handle —o morir la app, que
//!   cierra todos sus handles— mata el árbol completo (criterio G1: sin
//!   huérfanos al cerrar la aplicación con una compilación en curso).
//! - **Cancelación/timeout**: `TerminateJobObject` (síncrono para el SO) y, si
//!   el job no se pudo crear, se degrada al `taskkill /F /T` del spike.
//! - **`CREATE_NO_WINDOW`**: los procesos de consola (latexmk/tlmgr/synctex)
//!   no abren ventana aunque el usuario desactive el modo *nonstopmode*.
//!
//! Uso interno por `engine`, `queue`, `synctex` y `tlmgr`. Nunca bloquea en
//! espera activa de alta frecuencia: sondea `try_wait` cada 25 ms.

use std::io::{ErrorKind, Read};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::engine::CancelToken;

/// Intervalo de sondeo de `try_wait` (timeout/cancelación).
const POLL_INTERVAL: Duration = Duration::from_millis(25);
/// Tras pedir el kill, margen máximo para que el SO liquide el árbol.
const KILL_GRACE: Duration = Duration::from_secs(5);
/// Bandera de `CreateProcess`: sin ventana de consola (apps GUI).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
///
/// El proceso raíz y todos sus descendientes quedan en un job con kill-on-close;
/// al salir de `run` el handle se cierra y nada sobrevive a la función.
pub(crate) fn run(
    cmd: &mut Command,
    timeout: Duration,
    cancel: Option<&CancelToken>,
    on_line: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> std::io::Result<ProcOutput> {
    configure(cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (mut child, job) = spawn_child(cmd)?;
    let mut stdout_pipe = child.stdout.take().expect("stdout pipeado");
    let mut stderr_pipe = child.stderr.take().expect("stderr pipeado");

    let deadline = Instant::now() + timeout;
    let (code, timed_out, cancelled, stdout, stderr) =
        std::thread::scope(|scope| -> std::io::Result<_> {
            let handle_out = scope.spawn(move || read_stream(&mut stdout_pipe, on_line));
            let handle_err = scope.spawn(move || read_stream(&mut stderr_pipe, None));
            let mut timed_out = false;
            let mut cancelled = false;
            let mut killed = false;
            let mut kill_deadline: Option<Instant> = None;
            let code = loop {
                if let Some(status) = child.try_wait()? {
                    break status.code();
                }
                if !killed {
                    let reason = if cancel.is_some_and(CancelToken::is_cancelled) {
                        Some(true)
                    } else if Instant::now() >= deadline {
                        Some(false)
                    } else {
                        None
                    };
                    if let Some(is_cancel) = reason {
                        cancelled = is_cancel;
                        timed_out = !is_cancel;
                        killed = true;
                        kill_deadline = Some(Instant::now() + KILL_GRACE);
                        kill_tree(&mut child, job.as_ref());
                    }
                } else if let Some(at) = kill_deadline
                    && Instant::now() >= at
                {
                    // El SO no liquidó el árbol en el margen; el `Drop` del job
                    // (kill-on-close) hará un último intento al salir de aquí.
                    break None;
                }
                std::thread::sleep(POLL_INTERVAL);
            };
            let stdout = handle_out.join().unwrap_or_default();
            let stderr = handle_err.join().unwrap_or_default();
            Ok((code, timed_out, cancelled, stdout, stderr))
        })?;

    // Cierre explícito del job: si algún descendiente sobrevivió al loop,
    // kill-on-close lo elimina aquí.
    drop(job);

    Ok(ProcOutput {
        code,
        stdout,
        stderr,
        timed_out,
        cancelled,
    })
}

/// Marca una salida de consola como app GUI (sin ventana).
fn configure(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Job de Windows con `KILL_ON_JOB_CLOSE` (M1-D1).
#[cfg(windows)]
mod job {
    use std::io;
    use std::os::windows::io::AsRawHandle as _;
    use std::process::Child;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };

    /// Handle de job object. Al soltarlo, Windows mata a todos sus miembros
    /// (kill-on-close), incluidos los nietos creados por latexmk/perl.
    pub(crate) struct JobObject(HANDLE);

    // El handle es un puntero opaco del SO: seguro de mover entre hilos (solo
    // lo consumen las funciones de job, todas thread-safe).
    unsafe impl Send for JobObject {}
    unsafe impl Sync for JobObject {}

    impl JobObject {
        /// Crea el job y activa `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
        pub(crate) fn create() -> io::Result<Self> {
            // SAFETY: atributos de seguridad nulos (defaults) y nombre nulo son válidos.
            let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
                // SAFETY: estructura POD que solo requiere ceros y luego el flag.
                unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: handle válido; la referencia y el tamaño describen la estructura.
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            };
            if ok == 0 {
                let err = io::Error::last_os_error();
                // SAFETY: handle válido y aún no propiedad de JobObject.
                unsafe { CloseHandle(handle) };
                return Err(err);
            }
            Ok(Self(handle))
        }

        /// Asigna el proceso al job. A partir de aquí, todos sus hijos nacen dentro.
        pub(crate) fn assign(&self, child: &Child) -> io::Result<()> {
            let process: HANDLE = child.as_raw_handle();
            // SAFETY: ambos handles son válidos mientras `self`/`child` vivan.
            let ok = unsafe { AssignProcessToJobObject(self.0, process) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Mata todos los procesos del job (y espera lo que el SO necesita).
        pub(crate) fn terminate(&self) {
            // SAFETY: handle válido; el código de salida 1 es informativo.
            unsafe { TerminateJobObject(self.0, 1) };
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            // SAFETY: handle válido; kill-on-close elimina a los miembros vivos.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(windows)]
use job::JobObject;

/// Espacio para el guard del job en plataformas sin job objects.
#[cfg(not(windows))]
pub(crate) struct JobObject;

/// Lanza el proceso y lo mete en un job (en Windows). Devuelve el job si se pudo.
fn spawn_child(cmd: &mut Command) -> std::io::Result<(Child, Option<JobObject>)> {
    let child = cmd.spawn()?;
    #[cfg(windows)]
    {
        // La asignación es casi inmediata tras el `CreateProcess`; latexmk aún
        // tarda en arrancar perl/pdflatex, así que ningún hijo se escapa.
        match JobObject::create().and_then(|job| job.assign(&child).map(|()| job)) {
            Ok(job) => Ok((child, Some(job))),
            Err(err) => {
                tracing::warn!(
                    target: "lsx::latex",
                    error = %err,
                    "job object no disponible; se degrada a taskkill /T"
                );
                Ok((child, None))
            }
        }
    }
    #[cfg(not(windows))]
    {
        Ok((child, None))
    }
}

/// Mata el proceso y sus descendientes (job si existe; si no, `taskkill /T`).
fn kill_tree(child: &mut Child, job: Option<&JobObject>) {
    #[cfg(windows)]
    {
        if let Some(job) = job {
            job.terminate();
        } else {
            let pid = child.id().to_string();
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    #[cfg(not(windows))]
    {
        let _ = job;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Comando de "colgado" barato y sin TeX Live: `ping` dura décadas en
    /// tiempo de test. `cmd /c` + redirección mantiene la consola muda.
    fn hang_command() -> Command {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/c", "ping -n 120 127.0.0.1 > NUL"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd
    }

    fn poll_exited(child: &mut Child, grace: Duration) -> bool {
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if child.try_wait().expect("try_wait").is_some() {
                return true;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        child.try_wait().expect("try_wait").is_some()
    }

    #[test]
    fn timeout_mata_el_proceso_y_no_cuelga() {
        let started = Instant::now();
        let out = run(&mut hang_command(), Duration::from_millis(400), None, None).expect("run");
        let elapsed = started.elapsed();
        assert!(out.timed_out, "{out:?}");
        assert!(!out.cancelled, "{out:?}");
        assert!(elapsed < Duration::from_secs(6), "kill lento: {elapsed:?}");
    }

    #[test]
    fn cancel_token_mata_el_proceso() {
        let token = CancelToken::new();
        let canceller = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            canceller.cancel();
        });
        let started = Instant::now();
        let out = run(
            &mut hang_command(),
            Duration::from_secs(60),
            Some(&token),
            None,
        )
        .expect("run");
        let elapsed = started.elapsed();
        assert!(out.cancelled, "{out:?}");
        assert!(!out.timed_out, "{out:?}");
        assert!(elapsed < Duration::from_secs(6), "kill lento: {elapsed:?}");
    }

    #[cfg(windows)]
    #[test]
    fn cerrar_el_job_mata_el_arbol() {
        let (mut child, job) = spawn_child(&mut hang_command()).expect("spawn");
        let job = job.expect("job object creado en este sistema");
        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "el proceso debe seguir vivo"
        );

        // Cerrar el handle del job = kill-on-close (lo que ocurre al morir la
        // app que lo posee: el SO cierra todos sus handles).
        drop(job);
        assert!(
            poll_exited(&mut child, Duration::from_secs(5)),
            "kill-on-close no terminó el proceso"
        );
    }

    #[test]
    fn el_proceso_normal_devuelve_codigo_y_salida() {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/c", "echo hola-m1-d1"]);
        let out = run(&mut cmd, Duration::from_secs(15), None, None).expect("run");
        assert_eq!(out.code, Some(0), "{out:?}");
        assert!(out.stdout.contains("hola-m1-d1"), "{out:?}");
        assert!(!out.timed_out && !out.cancelled);
    }
}
