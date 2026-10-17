//! Cola de compilación de producción (M1-D2).
//!
//! [`CompileController`] es la API que la UI (stream A) conecta:
//!
//! - **Single-flight**: como mucho una compilación en curso.
//! - **Coalescing**: si llegan cambios mientras compila, se guarda **una**
//!   única petición pendiente (la última gana; las intermedias se cuentan en
//!   `QueueStatus::dropped` y no emiten eventos) y se ejecuta un rerun al
//!   terminar la actual.
//! - **Generaciones**: cada `submit` devuelve una generación monótona; los
//!   eventos (`Queued`/`Started`/`Progress`/`PackageInstall`/`Finished`/
//!   `Cancelled`) la llevan para que la UI descarte mensajes obsoletos.
//! - **autoCompile con debounce**: `notify_change()` rearma el temporizador
//!   (`latex.debounceMs`, default 400 ms) y, al expirar sin más cambios,
//!   compila la [petición base](CompileController::set_request).
//!
//! La API no conoce GPUI: los eventos salen por canales `async_channel`
//! (fan-out a todos los suscriptores) y el trabajo corre en hilos propios.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender, TrySendError};

use crate::engine::{
    CancelToken, CompileEvent, CompileOutcome, CompileRequest, CompileStatus, EngineError,
    LatexEngine,
};
use crate::settings::LatexSettings;

/// Estado visible de la cola (contrato M1-D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompileState {
    /// Sin nada en curso ni pendiente (arranque, o tras consumir la UI el
    /// último resultado).
    #[default]
    Idle,
    /// Aceptada y esperando el hilo de trabajo.
    Queued,
    /// Compilación en curso (puede haber un rerun pendiente).
    Running,
    /// La última compilación terminó bien.
    Success,
    /// La última compilación falló (incluye `Timeout`).
    Failed,
    /// La última compilación se canceló.
    Cancelled,
}

/// Instantánea del estado de la cola (barata de clonar; la UI la consulta).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueStatus {
    pub state: CompileState,
    /// Última generación aceptada (running, pendiente o terminada).
    pub generation: u64,
    /// Generación en ejecución, si hay.
    pub running: Option<u64>,
    /// Generación pendiente del rerun por coalescing, si hay.
    pub pending: Option<u64>,
    /// Peticiones superadas por otra más reciente (nunca se ejecutaron).
    pub dropped: u64,
}

/// Controlador de la cola de compilación.
///
/// Se comparte con `Arc` (o se guarda en el estado de la app); no es `Clone`
/// para que cada instancia tenga un dueño claro del hilo de debounce.
pub struct CompileController {
    inner: Arc<QueueInner>,
}

impl CompileController {
    /// Cola con la configuración por defecto (autoCompile activo, 400 ms).
    pub fn new(engine: Arc<dyn LatexEngine>) -> Self {
        Self::with_settings(engine, &LatexSettings::default())
    }

    /// Cola con `autoCompile`/`debounceMs` tomados de los settings.
    pub fn with_settings(engine: Arc<dyn LatexEngine>, settings: &LatexSettings) -> Self {
        let inner = Arc::new(QueueInner {
            engine,
            events: Mutex::new(Vec::new()),
            state: Mutex::new(QueueState::default()),
            debounce: Mutex::new(DebounceState::default()),
            debounce_cv: Condvar::new(),
            auto_compile: AtomicBool::new(settings.auto_compile),
            debounce_ms: AtomicU64::new(settings.debounce_ms),
            base_request: Mutex::new(None),
            next_generation: AtomicU64::new(0),
        });
        let controller = Self {
            inner: Arc::clone(&inner),
        };
        inner.spawn_debounce_timer();
        controller
    }

    /// Nuevo receptor de eventos (fan-out: todos los suscriptores reciben todo).
    pub fn subscribe(&self) -> Receiver<CompileEvent> {
        let (tx, rx) = async_channel::unbounded();
        self.inner.lock_events().push(tx);
        rx
    }

    /// Envía una petición nueva. Devuelve su generación.
    ///
    /// Si hay una compilación en curso, la petición queda como **pendiente**
    /// (rerun por coalescing) y reemplaza a cualquier pendiente anterior.
    pub fn submit(&self, req: CompileRequest) -> u64 {
        self.inner.submit(req)
    }

    /// Petición base que usan `autoCompile` y [`Self::submit_stored`].
    pub fn set_request(&self, req: CompileRequest) {
        *self.inner.lock_base_request() = Some(req);
    }

    /// Petición base actual (si la hay).
    pub fn request(&self) -> Option<CompileRequest> {
        self.inner.lock_base_request().clone()
    }

    /// Compila la petición base (acción «Compilar» de la UI).
    ///
    /// `None` si aún no se configuró ninguna petición.
    pub fn submit_stored(&self) -> Option<u64> {
        let req = self.request()?;
        Some(self.submit(req))
    }

    /// Aplica la parte de settings que gobierna la cola (autoCompile/debounce).
    pub fn apply_settings(&self, settings: &LatexSettings) {
        self.inner
            .auto_compile
            .store(settings.auto_compile, Ordering::Relaxed);
        self.inner
            .debounce_ms
            .store(settings.debounce_ms, Ordering::Relaxed);
    }

    pub fn set_auto_compile(&self, enabled: bool) {
        self.inner.auto_compile.store(enabled, Ordering::Relaxed);
    }

    pub fn is_auto_compile(&self) -> bool {
        self.inner.auto_compile.load(Ordering::Relaxed)
    }

    pub fn set_debounce(&self, debounce: Duration) {
        let ms = debounce.as_millis().min(u128::from(u64::MAX)) as u64;
        self.inner.debounce_ms.store(ms, Ordering::Relaxed);
    }

    pub fn debounce(&self) -> Duration {
        Duration::from_millis(self.inner.debounce_ms.load(Ordering::Relaxed))
    }

    /// Avisa de un cambio de fuentes: rearma el debounce de autoCompile.
    ///
    /// Sin petición base o con autoCompile desactivado no hace nada.
    pub fn notify_change(&self) {
        if !self.is_auto_compile() || self.request().is_none() {
            return;
        }
        let mut debounce = self.inner.lock_debounce();
        debounce.deadline = Some(Instant::now() + self.debounce());
        drop(debounce);
        self.inner.debounce_cv.notify_all();
    }

    /// Cancela la generación `generation`: la que corre (kill del árbol) o la
    /// pendiente (se descarta). `false` si ya no existe.
    pub fn cancel(&self, generation: u64) -> bool {
        let mut state = self.inner.lock_state();
        if state.running == Some(generation)
            && let Some(token) = &state.running_cancel
        {
            token.cancel();
            return true;
        }
        if state
            .pending
            .as_ref()
            .is_some_and(|(pending, _)| *pending == generation)
        {
            state.pending = None;
            drop(state);
            self.inner.emit(CompileEvent::Cancelled { generation });
            return true;
        }
        false
    }

    /// Cancela lo que haya en curso y descarta el pendiente.
    pub fn cancel_all(&self) -> Option<u64> {
        let (running, pending) = {
            let mut state = self.inner.lock_state();
            if let Some(token) = &state.running_cancel {
                token.cancel();
            }
            (
                state.running,
                state.pending.take().map(|(pending, _)| pending),
            )
        };
        if let Some(pending) = pending {
            self.inner.emit(CompileEvent::Cancelled {
                generation: pending,
            });
        }
        running.or(pending)
    }

    /// Instantánea del estado (para status bar, tests y gates de la UI).
    pub fn status(&self) -> QueueStatus {
        let state = self.inner.lock_state();
        QueueStatus {
            state: state.status,
            generation: state.latest_generation,
            running: state.running,
            pending: state.pending.as_ref().map(|(pending, _)| *pending),
            dropped: state.dropped,
        }
    }
}

impl Drop for CompileController {
    fn drop(&mut self) {
        let mut debounce = self.inner.lock_debounce();
        debounce.shutdown = true;
        drop(debounce);
        self.inner.debounce_cv.notify_all();
    }
}

/// Datos internos compartidos entre el controlador, los workers y el temporizador.
struct QueueInner {
    engine: Arc<dyn LatexEngine>,
    events: Mutex<Vec<Sender<CompileEvent>>>,
    state: Mutex<QueueState>,
    debounce: Mutex<DebounceState>,
    debounce_cv: Condvar,
    auto_compile: AtomicBool,
    /// Milisegundos de debounce (`latex.debounceMs`).
    debounce_ms: AtomicU64,
    base_request: Mutex<Option<CompileRequest>>,
    next_generation: AtomicU64,
}

#[derive(Default)]
struct QueueState {
    status: CompileState,
    running: Option<u64>,
    running_cancel: Option<CancelToken>,
    pending: Option<(u64, CompileRequest)>,
    latest_generation: u64,
    dropped: u64,
}

#[derive(Default)]
struct DebounceState {
    deadline: Option<Instant>,
    shutdown: bool,
}

impl QueueInner {
    fn lock_events(&self) -> std::sync::MutexGuard<'_, Vec<Sender<CompileEvent>>> {
        self.events.lock().unwrap_or_else(|err| err.into_inner())
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(|err| err.into_inner())
    }

    fn lock_debounce(&self) -> std::sync::MutexGuard<'_, DebounceState> {
        self.debounce.lock().unwrap_or_else(|err| err.into_inner())
    }

    fn lock_base_request(&self) -> std::sync::MutexGuard<'_, Option<CompileRequest>> {
        self.base_request
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    /// Publica a todos los suscriptores; poda los canales cerrados.
    fn emit(&self, event: CompileEvent) {
        let mut subscribers = self.lock_events();
        subscribers.retain(|tx| match tx.try_send(event.clone()) {
            Ok(()) | Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Closed(_)) => false,
        });
    }

    /// Acepta una petición: la arranca o la deja como pendiente (coalescing).
    fn submit(self: &Arc<Self>, req: CompileRequest) -> u64 {
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let mut state = self.lock_state();
        state.latest_generation = generation;
        if state.running.is_some() {
            if state.pending.replace((generation, req)).is_some() {
                state.dropped += 1;
            }
            state.status = CompileState::Running;
            drop(state);
            return generation;
        }
        let token = CancelToken::new();
        state.running = Some(generation);
        state.running_cancel = Some(token.clone());
        state.status = CompileState::Queued;
        drop(state);
        self.emit(CompileEvent::Queued { generation });
        self.spawn_worker(generation, req, token);
        generation
    }

    fn spawn_worker(self: &Arc<Self>, generation: u64, req: CompileRequest, token: CancelToken) {
        let inner = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name(format!("lsx-latex-compile-{generation}"))
            .spawn(move || inner.run_generation(generation, req, token));
        if let Err(err) = spawned {
            tracing::error!(
                target: "lsx::latex",
                error = %err,
                generation,
                "no se pudo lanzar el hilo de compilación"
            );
            let outcome = CompileOutcome::from_engine_error(&EngineError::Io(
                std::io::Error::other(format!("no se pudo lanzar la compilación: {err}")),
            ));
            self.settle(generation, Some(outcome));
        }
    }

    /// Cuerpo del worker: compila una generación y encadena el pendiente.
    fn run_generation(self: &Arc<Self>, generation: u64, req: CompileRequest, token: CancelToken) {
        {
            let mut state = self.lock_state();
            state.status = CompileState::Running;
        }
        self.emit(CompileEvent::Started { generation });

        let on_event = |event: CompileEvent| match event {
            // El engine emite con `generation = 0`; aquí se reescribe la real.
            CompileEvent::Progress { line, .. } => {
                self.emit(CompileEvent::Progress { generation, line });
            }
            CompileEvent::PackageInstall { package, stage, .. } => {
                self.emit(CompileEvent::PackageInstall {
                    generation,
                    package,
                    stage,
                });
            }
            _ => {}
        };
        let outcome = match self.engine.compile(&req, &on_event, &token) {
            Ok(outcome) => outcome,
            Err(error) => {
                tracing::warn!(
                    target: "lsx::latex",
                    error = %error,
                    generation,
                    "la compilación no pudo arrancar"
                );
                CompileOutcome::from_engine_error(&error)
            }
        };
        self.settle(generation, Some(outcome));
    }

    /// Cierra una generación (éxito, fallo o cancelación) y arranca el pendiente.
    fn settle(self: &Arc<Self>, generation: u64, outcome: Option<CompileOutcome>) {
        let cancelled = outcome
            .as_ref()
            .is_some_and(|outcome| outcome.status == CompileStatus::Cancelled);
        {
            let mut state = self.lock_state();
            state.running = None;
            state.running_cancel = None;
            state.status = match &outcome {
                Some(outcome) => outcome_state(outcome.status),
                None => CompileState::Failed,
            };
        }
        match (outcome, cancelled) {
            (Some(_), true) => self.emit(CompileEvent::Cancelled { generation }),
            (Some(outcome), false) => self.emit(CompileEvent::Finished {
                generation,
                outcome,
            }),
            (None, _) => {
                let outcome = CompileOutcome::from_engine_error(&EngineError::Io(
                    std::io::Error::other("la compilación terminó sin resultado"),
                ));
                self.emit(CompileEvent::Finished {
                    generation,
                    outcome,
                });
            }
        }
        self.start_pending();
    }

    /// Arranca el rerun pendiente (si lo hay).
    fn start_pending(self: &Arc<Self>) {
        let next = {
            let mut state = self.lock_state();
            match state.pending.take() {
                Some((generation, req)) => {
                    let token = CancelToken::new();
                    state.running = Some(generation);
                    state.running_cancel = Some(token.clone());
                    state.status = CompileState::Queued;
                    Some((generation, req, token))
                }
                None => None,
            }
        };
        if let Some((generation, req, token)) = next {
            self.emit(CompileEvent::Queued { generation });
            self.spawn_worker(generation, req, token);
        }
    }

    /// Hilo de debounce: ejecuta la petición base cuando vence el temporizador.
    fn spawn_debounce_timer(self: &Arc<Self>) {
        let inner = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("lsx-latex-debounce".to_string())
            .spawn(move || {
                loop {
                    let mut debounce = inner.lock_debounce();
                    if debounce.shutdown {
                        return;
                    }
                    match debounce.deadline {
                        None => {
                            drop(inner.debounce_cv.wait(debounce));
                            continue;
                        }
                        Some(deadline) => {
                            let now = Instant::now();
                            if deadline > now {
                                let (guard, _) = inner
                                    .debounce_cv
                                    .wait_timeout(debounce, deadline - now)
                                    .unwrap_or_else(|err| err.into_inner());
                                drop(guard);
                                continue;
                            }
                            debounce.deadline = None;
                        }
                    }
                    drop(debounce);
                    if inner.auto_compile.load(Ordering::Relaxed) {
                        let request = inner.lock_base_request().clone();
                        if let Some(request) = request {
                            inner.submit(request);
                        }
                    }
                }
            });
        if let Err(err) = spawned {
            tracing::error!(
                target: "lsx::latex",
                error = %err,
                "no se pudo lanzar el temporizador de autoCompile"
            );
        }
    }
}

/// `CompileStatus` del outcome → estado de la cola (Timeout cuenta como fallo).
fn outcome_state(status: CompileStatus) -> CompileState {
    match status {
        CompileStatus::Success => CompileState::Success,
        CompileStatus::Failed | CompileStatus::Timeout => CompileState::Failed,
        CompileStatus::Cancelled => CompileState::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::engine::{EngineId, EngineInfo, PackageInstallStage};
    use camino::Utf8PathBuf;

    /// Engine de mentira: registra arranques y espera permisos del test.
    struct FakeEngine {
        starts: Sender<String>,
        permits: AtomicU64,
        package_event: bool,
    }

    impl FakeEngine {
        fn new(starts: Sender<String>) -> Self {
            Self {
                starts,
                permits: AtomicU64::new(0),
                package_event: false,
            }
        }

        fn allow(&self, n: u64) {
            self.permits.fetch_add(n, Ordering::SeqCst);
        }
    }

    impl LatexEngine for FakeEngine {
        fn id(&self) -> EngineId {
            EngineId::TexLive
        }

        fn probe(&self) -> Result<EngineInfo, EngineError> {
            Err(EngineError::TexLiveNotFound { tried: vec![] })
        }

        fn compile(
            &self,
            req: &CompileRequest,
            on_event: &(dyn Fn(CompileEvent) + Send + Sync),
            cancel: &CancelToken,
        ) -> Result<CompileOutcome, EngineError> {
            let name = req.main.file_stem().unwrap_or("sin-nombre").to_string();
            let _ = self.starts.try_send(name.clone());
            loop {
                if cancel.is_cancelled() {
                    return Ok(CompileOutcome {
                        status: CompileStatus::Cancelled,
                        duration: Duration::ZERO,
                        pdf: None,
                        log: None,
                        log_tail: String::new(),
                        diagnostics: Vec::new(),
                        package_installs: Vec::new(),
                        exit_code: None,
                    });
                }
                if self.permits.load(Ordering::SeqCst) > 0 {
                    self.permits.fetch_sub(1, Ordering::SeqCst);
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            on_event(CompileEvent::Progress {
                generation: 0,
                line: format!("fake:{name}"),
            });
            if self.package_event {
                on_event(CompileEvent::PackageInstall {
                    generation: 0,
                    package: "adjmulticol".to_string(),
                    stage: PackageInstallStage::Started,
                });
            }
            Ok(CompileOutcome {
                status: CompileStatus::Success,
                duration: Duration::from_millis(1),
                pdf: None,
                log: None,
                log_tail: String::new(),
                diagnostics: Vec::new(),
                package_installs: Vec::new(),
                exit_code: Some(0),
            })
        }
    }

    fn request(name: &str) -> CompileRequest {
        let root = Utf8PathBuf::from("C:/proyecto");
        CompileRequest::new(root.clone(), root.join(format!("{name}.tex")))
    }

    fn wait_status(
        controller: &CompileController,
        timeout: Duration,
        predicate: impl Fn(&QueueStatus) -> bool,
    ) -> QueueStatus {
        let deadline = Instant::now() + timeout;
        loop {
            let status = controller.status();
            if predicate(&status) {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "estado no alcanzado a tiempo: {status:?}"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_events(
        rx: &Receiver<CompileEvent>,
        count: usize,
        timeout: Duration,
    ) -> Vec<CompileEvent> {
        let deadline = Instant::now() + timeout;
        let mut out = Vec::new();
        while out.len() < count {
            match rx.try_recv() {
                Ok(event) => out.push(event),
                Err(async_channel::TryRecvError::Empty) => {
                    assert!(Instant::now() < deadline, "faltan eventos: {out:#?}");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(async_channel::TryRecvError::Closed) => break,
            }
        }
        out
    }

    /// `async-channel` no trae `recv_timeout` en esta versión: sondeo con tope.
    fn recv_with_timeout<T: Clone>(rx: &Receiver<T>, timeout: Duration) -> Option<T> {
        let deadline = Instant::now() + timeout;
        loop {
            match rx.try_recv() {
                Ok(value) => return Some(value),
                Err(async_channel::TryRecvError::Empty) => {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(async_channel::TryRecvError::Closed) => return None,
            }
        }
    }

    /// Generación de un evento (todas las variantes la llevan).
    fn event_generation(event: &CompileEvent) -> Option<u64> {
        match event {
            CompileEvent::Queued { generation }
            | CompileEvent::Started { generation }
            | CompileEvent::Progress { generation, .. }
            | CompileEvent::PackageInstall { generation, .. }
            | CompileEvent::Finished { generation, .. }
            | CompileEvent::Cancelled { generation } => Some(*generation),
        }
    }

    #[test]
    fn estado_inicial_idle() {
        let (starts, _rx) = async_channel::unbounded();
        let controller = CompileController::new(Arc::new(FakeEngine::new(starts)));
        assert_eq!(controller.status().state, CompileState::Idle);
        assert_eq!(controller.status().generation, 0);
    }

    #[test]
    fn single_flight_y_coalescing_de_una_sola_pendiente() {
        let (starts_tx, starts_rx) = async_channel::unbounded();
        let fake = Arc::new(FakeEngine::new(starts_tx));
        let controller = CompileController::new(fake.clone());
        let events = controller.subscribe();

        let gen_a = controller.submit(request("a"));
        wait_status(&controller, Duration::from_secs(2), |s| {
            s.running == Some(gen_a)
        });
        let gen_b = controller.submit(request("b"));
        let gen_c = controller.submit(request("c"));
        let status = controller.status();
        assert_eq!(status.running, Some(gen_a));
        assert_eq!(status.pending, Some(gen_c), "{status:?}");
        assert_eq!(status.dropped, 1, "b fue superada por c");

        fake.allow(2);
        let final_status = wait_status(&controller, Duration::from_secs(5), |s| {
            s.state == CompileState::Success && s.running.is_none() && s.pending.is_none()
        });
        assert_eq!(final_status.generation, gen_c);

        // Arranques: A y C; B nunca corrió (coalescing).
        let mut started = Vec::new();
        while let Ok(name) = starts_rx.try_recv() {
            started.push(name);
        }
        assert_eq!(started, vec!["a", "c"]);

        // Ciclo de vida por generación: 4 eventos por run, sin rastro de B.
        let received = wait_events(&events, 8, Duration::from_secs(2));
        let shape: Vec<(u64, &'static str)> = received
            .iter()
            .map(|event| match event {
                CompileEvent::Queued { generation } => (*generation, "queued"),
                CompileEvent::Started { generation } => (*generation, "started"),
                CompileEvent::Progress { generation, .. } => (*generation, "progress"),
                CompileEvent::PackageInstall { generation, .. } => (*generation, "package"),
                CompileEvent::Finished { generation, .. } => (*generation, "finished"),
                CompileEvent::Cancelled { generation } => (*generation, "cancelled"),
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                (gen_a, "queued"),
                (gen_a, "started"),
                (gen_a, "progress"),
                (gen_a, "finished"),
                (gen_c, "queued"),
                (gen_c, "started"),
                (gen_c, "progress"),
                (gen_c, "finished"),
            ],
            "{received:#?}"
        );
        assert!(!shape.iter().any(|(generation, _)| *generation == gen_b));
    }

    #[test]
    fn cancelar_la_generacion_en_curso_emite_cancelled() {
        let (starts_tx, _starts_rx) = async_channel::unbounded();
        let fake = Arc::new(FakeEngine::new(starts_tx));
        let controller = CompileController::new(fake);
        let events = controller.subscribe();

        let generation = controller.submit(request("a"));
        wait_status(&controller, Duration::from_secs(2), |s| {
            s.running == Some(generation)
        });
        assert!(controller.cancel(generation));
        let status = wait_status(&controller, Duration::from_secs(5), |s| {
            s.state == CompileState::Cancelled
        });
        assert!(status.running.is_none());

        let received = wait_events(&events, 3, Duration::from_secs(2));
        assert!(matches!(
            received.last(),
            Some(CompileEvent::Cancelled { generation: cancelled }) if *cancelled == generation
        ));
        assert!(
            !controller.cancel(generation),
            "ya no existe esa generación"
        );
    }

    #[test]
    fn cancelar_una_pendiente_no_toca_la_que_corre() {
        let (starts_tx, _starts_rx) = async_channel::unbounded();
        let fake = Arc::new(FakeEngine::new(starts_tx));
        let controller = CompileController::new(fake.clone());
        let events = controller.subscribe();

        let gen_a = controller.submit(request("a"));
        wait_status(&controller, Duration::from_secs(2), |s| {
            s.running == Some(gen_a)
        });
        let gen_b = controller.submit(request("b"));
        assert!(controller.cancel(gen_b));
        assert_eq!(controller.status().pending, None);
        assert_eq!(controller.status().running, Some(gen_a));
        // La cancelación de la pendiente se anuncia al suscriptor; el orden con
        // `Started` de la que corre no es determinista entre hilos, así que se
        // espera a que aparezcan ambos.
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut seen = Vec::new();
        loop {
            match events.try_recv() {
                Ok(event) => seen.push(event),
                Err(async_channel::TryRecvError::Empty) => {
                    let started_a = seen.iter().any(
                        |event| matches!(event, CompileEvent::Started { generation } if *generation == gen_a),
                    );
                    let cancelled_b = seen.iter().any(
                        |event| matches!(event, CompileEvent::Cancelled { generation } if *generation == gen_b),
                    );
                    if started_a && cancelled_b {
                        break;
                    }
                    assert!(Instant::now() < deadline, "eventos incompletos: {seen:#?}");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(async_channel::TryRecvError::Closed) => break,
            }
        }
        // A termina normalmente.
        fake.allow(1);
        wait_status(&controller, Duration::from_secs(5), |s| {
            s.state == CompileState::Success
        });
        // La generación cancelada jamás vuelve a aparecer (nunca corrió).
        while let Ok(event) = events.try_recv() {
            if let Some(generation) = event_generation(&event) {
                assert_ne!(generation, gen_b, "evento inesperado: {event:?}");
            }
        }
    }

    #[test]
    fn debounce_de_autocompile_y_rafaga_de_cambios() {
        let (starts_tx, starts_rx) = async_channel::unbounded();
        let fake = Arc::new(FakeEngine::new(starts_tx));
        let settings = LatexSettings {
            auto_compile: true,
            debounce_ms: 150,
            ..LatexSettings::default()
        };
        let controller = CompileController::with_settings(fake.clone(), &settings);
        controller.set_request(request("a"));

        // Tres cambios seguidos: un solo compile al vencer el debounce.
        controller.notify_change();
        thread::sleep(Duration::from_millis(40));
        controller.notify_change();
        thread::sleep(Duration::from_millis(40));
        controller.notify_change();
        thread::sleep(Duration::from_millis(60));
        assert!(starts_rx.try_recv().is_err(), "compiló antes del debounce");
        let name = recv_with_timeout(&starts_rx, Duration::from_secs(2))
            .expect("compile tras el debounce");
        assert_eq!(name, "a");
        fake.allow(1);
        wait_status(&controller, Duration::from_secs(5), |s| {
            s.state == CompileState::Success
        });

        // Con autoCompile apagado no se dispara nada.
        controller.set_auto_compile(false);
        controller.notify_change();
        thread::sleep(Duration::from_millis(400));
        assert!(starts_rx.try_recv().is_err());
    }

    #[test]
    fn submit_stored_usa_la_peticion_base() {
        let (starts_tx, _starts_rx) = async_channel::unbounded();
        let fake = Arc::new(FakeEngine::new(starts_tx));
        let controller = CompileController::new(fake.clone());
        assert_eq!(controller.submit_stored(), None);
        controller.set_request(request("manual"));
        let generation = controller.submit_stored().expect("petición base");
        wait_status(&controller, Duration::from_secs(2), |s| {
            s.running == Some(generation)
        });
        fake.allow(1);
        wait_status(&controller, Duration::from_secs(5), |s| {
            s.state == CompileState::Success
        });
        assert_eq!(
            controller.request().map(|r| r.main),
            Some(Utf8PathBuf::from("C:/proyecto/manual.tex"))
        );
    }

    #[test]
    fn apply_settings_actualiza_debounce_y_auto() {
        let (starts_tx, _starts_rx) = async_channel::unbounded();
        let controller = CompileController::new(Arc::new(FakeEngine::new(starts_tx)));
        assert!(controller.is_auto_compile());
        assert_eq!(controller.debounce(), Duration::from_millis(400));
        let settings = LatexSettings {
            auto_compile: false,
            debounce_ms: 900,
            ..LatexSettings::default()
        };
        controller.apply_settings(&settings);
        assert!(!controller.is_auto_compile());
        assert_eq!(controller.debounce(), Duration::from_millis(900));
    }

    #[test]
    fn los_eventos_del_engine_se_reescriben_con_la_generacion_real() {
        let (starts_tx, _starts_rx) = async_channel::unbounded();
        let mut fake = FakeEngine::new(starts_tx);
        fake.package_event = true;
        let fake = Arc::new(fake);
        let controller = CompileController::new(fake.clone());
        let events = controller.subscribe();
        let generation = controller.submit(request("a"));
        wait_status(&controller, Duration::from_secs(2), |s| {
            s.running == Some(generation)
        });
        fake.allow(1);
        let received = wait_events(&events, 5, Duration::from_secs(2));
        assert!(received.iter().all(|event| match event {
            CompileEvent::Queued {
                generation: event_generation,
            }
            | CompileEvent::Started {
                generation: event_generation,
            }
            | CompileEvent::Progress {
                generation: event_generation,
                ..
            }
            | CompileEvent::PackageInstall {
                generation: event_generation,
                ..
            }
            | CompileEvent::Finished {
                generation: event_generation,
                ..
            }
            | CompileEvent::Cancelled {
                generation: event_generation,
            } => *event_generation == generation,
        }));
        assert!(received
            .iter()
            .any(|event| matches!(event, CompileEvent::PackageInstall { package, .. } if package == "adjmulticol")));
    }
}
