//! Job objects de Windows con `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (M1-E1).
//!
//! Copia deliberada de la FFI mínima de `crates/latex/src/proc.rs` (el chat no
//! depende de `lsx-latex`): `opencode serve` lanza hijos propios y matar solo
//! al padre deja huérfanos (comprobado a mano durante M1-E). Asignando el
//! proceso raíz a un job con kill-on-close, al morir la app —Windows cierra
//! todos sus handles— el árbol completo de opencode desaparece.
//!
//! Fuera de Windows el módulo es un no-op (la app es Windows-first).

/// Handle de job object; al soltarlo, Windows mata a sus miembros (kill-on-close).
#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::io::RawHandle;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };

    /// Job de Windows con `KILL_ON_JOB_CLOSE`.
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

        /// Asigna un proceso (handle crudo) al job; a partir de aquí todos sus
        /// hijos nacen dentro.
        ///
        /// `tokio::process::Child::raw_handle()` y
        /// `std::process::Child::as_raw_handle()` producen este `RawHandle`.
        pub(crate) fn assign(&self, raw: RawHandle) -> io::Result<()> {
            let process: HANDLE = raw;
            // SAFETY: ambos handles son válidos mientras `self` y el proceso vivan.
            let ok = unsafe { AssignProcessToJobObject(self.0, process) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Mata todos los procesos del job.
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

    impl std::fmt::Debug for JobObject {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("JobObject").finish_non_exhaustive()
        }
    }
}

#[cfg(windows)]
pub(crate) use imp::JobObject;

/// Espacio para el guard del job en plataformas sin job objects.
#[cfg(not(windows))]
#[derive(Debug)]
pub(crate) struct JobObject;
#[cfg(not(windows))]
impl JobObject {
    pub(crate) fn create() -> std::io::Result<Self> {
        Ok(Self)
    }

    pub(crate) fn assign<T>(&self, _process: T) -> std::io::Result<()> {
        Ok(())
    }

    pub(crate) fn terminate(&self) {}
}

#[cfg(all(test, windows))]
mod tests {
    use std::os::windows::io::AsRawHandle as _;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use super::*;

    const POLL_INTERVAL: Duration = Duration::from_millis(25);

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
    fn cerrar_el_job_mata_el_proceso() {
        let mut child = hang_command().spawn().expect("spawn");
        let job = JobObject::create().expect("job object");
        job.assign(child.as_raw_handle()).expect("assign");
        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "el proceso debe seguir vivo"
        );

        // Cerrar el handle = lo que hace el SO al morir la app (kill-on-close).
        drop(job);
        assert!(
            poll_exited(&mut child, Duration::from_secs(5)),
            "kill-on-close no terminó el proceso"
        );
    }

    #[test]
    fn terminate_mata_el_arbol_del_job() {
        let mut child = hang_command().spawn().expect("spawn");
        let job = JobObject::create().expect("job object");
        job.assign(child.as_raw_handle()).expect("assign");
        job.terminate();
        assert!(
            poll_exited(&mut child, Duration::from_secs(5)),
            "TerminateJobObject no terminó el proceso"
        );
    }
}
