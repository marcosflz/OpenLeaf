//! `fs.rs` — servicio de archivos mínimo con watcher.
//!
//! Implementa el contrato de `docs/plan/INTERFACES.md` §2:
//! - `list_dir` (oculta artefactos auxiliares de LaTeX salvo que se pidan),
//! - `read_text` / `write_text_atomic` (temp + rename con reintentos),
//! - CRUD básico (`create_file`, `create_dir`, `rename`, `delete`),
//! - `watch` con `notify` + debounce de 200 ms que entrega lotes [`FsEvent`].
//!
//! Decisiones de esta iteración (spike M0-S6, a consolidar en G0):
//! - Todas las rutas de entrada son **relativas a la raíz del proyecto** y las
//!   de los eventos también; se rechaza cualquier ruta que escape de la raíz.
//! - Las escrituras atómicas usan temporales `.lsx-tmp-*` en el mismo
//!   directorio; el watcher traduce el `rename` del temporal a
//!   `Modified(destino)` para que la UI no vea ruido interno.
//! - El callback del watcher recibe **lotes** (`Vec<FsEvent>`) una vez por
//!   ventana de debounce; el coalescing agrupa `Created`+`Modified`,
//!   `Modified`+`Removed`, pares `From`/`To` de rename, etc.
//!
//! Regla dura: este crate no conoce UI, tokio, reqwest ni gpui.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

/// Ventana de debounce por defecto del watcher (INTERFACES §2).
pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// Techo de la ventana de debounce: aunque no haya silencio, un lote se
/// entrega como máximo cada este intervalo mientras llegan eventos.
pub const MAX_DEBOUNCE: Duration = Duration::from_millis(1000);

/// Prefijo de los temporales de `write_text_atomic` (mismo volumen → rename atómico).
const TEMP_PREFIX: &str = ".lsx-tmp-";

/// Sufijos (minúsculas) de artefactos auxiliares de LaTeX que `list_dir` oculta.
const AUX_SUFFIXES: &[&str] = &[
    ".aux",
    ".log",
    ".fls",
    ".fdb_latexmk",
    ".synctex",
    ".synctex.gz",
    ".synctex(busy)",
    ".out",
    ".toc",
    ".lof",
    ".lot",
    ".bbl",
    ".blg",
    ".bcf",
    ".run.xml",
    ".spl",
    ".nav",
    ".snm",
    ".vrb",
    ".idx",
    ".ilg",
    ".ind",
    ".acn",
    ".acr",
    ".alg",
    ".glg",
    ".glo",
    ".gls",
    ".brf",
    ".nlo",
    ".nls",
    ".loa",
    ".xdy",
    ".dvi",
    ".tps",
    ".lkps",
];

/// Archivos de basura del sistema operativo que nunca se muestran.
const OS_JUNK: &[&str] = &["Thumbs.db", "desktop.ini", ".DS_Store"];

/// Cuántos reintentos aplica el replace de Windows (antivirus/indexer pueden
/// mantener un handle momentáneo sobre el destino).
const REPLACE_ATTEMPTS: u32 = 5;
const REPLACE_BACKOFF: Duration = Duration::from_millis(20);

/// Tipo de entrada de un directorio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
}

/// Una entrada de `list_dir` (INTERFACES §2).
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// `true` si es un artefacto auxiliar de LaTeX; solo llega marcado cuando
    /// se pide `show_aux`.
    pub ignored: bool,
}

/// Fin de línea detectado en un archivo de texto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Eol {
    #[default]
    Lf,
    CrLf,
    Mixed,
}

/// Contenido de un archivo de texto leído con [`FsService::read_text`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    /// Texto sin BOM.
    pub text: String,
    pub eol: Eol,
    pub had_bom: bool,
}

/// Evento de archivos agrupado por el watcher (INTERFACES §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
    Renamed(PathBuf, PathBuf),
}

impl FsEvent {
    /// Ruta principal del evento (origen en el caso de `Renamed`).
    pub fn path(&self) -> &Path {
        match self {
            FsEvent::Created(p) | FsEvent::Modified(p) | FsEvent::Removed(p) => p,
            FsEvent::Renamed(from, _) => from,
        }
    }
}

/// Errores del servicio de archivos. Mensajes orientados a usuario en español
/// (INTERFACES §2); el detalle técnico viaja en `source`.
#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("no se pudo acceder a «{path}»: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("la ruta «{0}» queda fuera del proyecto abierto")]
    OutsideRoot(PathBuf),
    #[error("«{0}» no es un archivo de texto UTF-8 (codificación no soportada en v1)")]
    NotUtf8(PathBuf),
    #[error("nombre de archivo no válido: {0}")]
    InvalidName(String),
    #[error("ya existe «{0}»")]
    AlreadyExists(PathBuf),
    #[error("no existe «{0}»")]
    NotFound(PathBuf),
    #[error("no se pudo iniciar el vigilante de archivos: {0}")]
    Watch(String),
}

impl FsError {
    fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        FsError::Io {
            path: path.into(),
            source,
        }
    }
}

/// Opciones de listado.
#[derive(Debug, Clone, Copy, Default)]
pub struct ListDirOptions {
    /// Incluir artefactos auxiliares de LaTeX (marcados con `ignored = true`).
    pub show_aux: bool,
}

/// Devuelve `true` si `name` es un artefacto auxiliar de LaTeX conocido.
pub fn is_aux_artifact(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    AUX_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
}

/// Valida un nombre de archivo/carpeta para Windows (M1-F3 anticipado).
///
/// Rechaza vacíos, separadores, caracteres reservados, puntos/espacios
/// finales y nombres de dispositivo (`CON`, `NUL`, `COM1`…).
pub fn validate_name(name: &str) -> Result<(), FsError> {
    if name.is_empty() {
        return Err(FsError::InvalidName(
            "el nombre no puede estar vacío".into(),
        ));
    }
    if name == "." || name == ".." {
        return Err(FsError::InvalidName(format!(
            "«{name}» no es un nombre válido"
        )));
    }
    if let Some(bad) = name.chars().find(|c| {
        *c == '<'
            || *c == '>'
            || *c == ':'
            || *c == '"'
            || *c == '/'
            || *c == '\\'
            || *c == '|'
            || *c == '?'
            || *c == '*'
            || c.is_control()
    }) {
        return Err(FsError::InvalidName(format!(
            "«{name}» contiene el carácter reservado «{bad}»"
        )));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(FsError::InvalidName(format!(
            "«{name}» no puede terminar en punto ni en espacio"
        )));
    }
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(FsError::InvalidName(format!(
            "«{name}» es un nombre reservado por Windows"
        )));
    }
    Ok(())
}

/// Servicio de archivos anclado a la raíz de un proyecto.
pub struct FsService {
    root: PathBuf,
}

impl FsService {
    /// Crea el servicio para `root` (absoluto si es posible).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let root = std::path::absolute(&root).unwrap_or(root);
        Self { root }
    }

    /// Raíz absoluta del proyecto.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Lista un directorio ocultando artefactos auxiliares (INTERFACES §2).
    ///
    /// Orden: carpetas primero y luego por nombre (sin distinguir mayúsculas).
    pub fn list_dir(&self, rel: &Path) -> Result<Vec<Entry>, FsError> {
        self.list_dir_with(rel, ListDirOptions::default())
    }

    /// Variante con opciones (p. ej. mostrar auxiliares).
    pub fn list_dir_with(
        &self,
        rel: &Path,
        options: ListDirOptions,
    ) -> Result<Vec<Entry>, FsError> {
        let dir = self.resolve(rel)?;
        let read = fs::read_dir(&dir).map_err(|e| FsError::io(&dir, e))?;

        let mut entries = Vec::new();
        for item in read {
            let item = item.map_err(|e| FsError::io(&dir, e))?;
            let name = item.file_name().to_string_lossy().into_owned();
            if OS_JUNK.iter().any(|junk| junk.eq_ignore_ascii_case(&name)) {
                continue;
            }
            let ignored = is_aux_artifact(&name);
            if ignored && !options.show_aux {
                continue;
            }
            let meta = item.metadata().map_err(|e| FsError::io(item.path(), e))?;
            let kind = if meta.is_dir() {
                EntryKind::Dir
            } else {
                EntryKind::File
            };
            entries.push(Entry {
                name,
                kind,
                size: meta.len(),
                modified: meta.modified().ok(),
                ignored,
            });
        }

        // Carpetas primero; dentro de cada grupo, orden alfabético estable.
        entries.sort_by_cached_key(|e| (e.kind == EntryKind::File, e.name.to_lowercase()));
        Ok(entries)
    }

    /// Lee un archivo como texto UTF-8 (detecta y quita BOM; conserva el flag).
    pub fn read_text(&self, rel: &Path) -> Result<TextFile, FsError> {
        let path = self.resolve(rel)?;
        let bytes = fs::read(&path).map_err(|e| FsError::io(&path, e))?;
        let (had_bom, body) = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
            Some(body) => (true, body),
            None => (false, bytes.as_slice()),
        };
        let text = std::str::from_utf8(body).map_err(|_| FsError::NotUtf8(path))?;
        Ok(TextFile {
            text: text.to_owned(),
            eol: detect_eol(text),
            had_bom,
        })
    }

    /// Escribe `text` de forma atómica: temp en el mismo directorio, `fsync` y
    /// `rename` con reintentos (Windows puede tener el destino abierto).
    pub fn write_text_atomic(&self, rel: &Path, text: &str) -> Result<(), FsError> {
        let path = self.resolve(rel)?;
        let parent = path
            .parent()
            .ok_or_else(|| FsError::OutsideRoot(path.clone()))?;
        if !parent.is_dir() {
            return Err(FsError::NotFound(parent.to_path_buf()));
        }

        let temp = parent.join(temp_file_name(&path));
        let write_result = (|| -> io::Result<()> {
            let mut file = fs::File::create(&temp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            Ok(())
        })();
        if let Err(e) = write_result {
            let _ = fs::remove_file(&temp);
            return Err(FsError::io(&temp, e));
        }

        match replace_with_retry(&temp, &path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = fs::remove_file(&temp);
                Err(e)
            }
        }
    }

    /// Crea un archivo vacío (falla si ya existe).
    pub fn create_file(&self, rel: &Path) -> Result<(), FsError> {
        let path = self.resolve(rel)?;
        validate_name(&file_name(&path))?;
        self.require_parent(&path)?;
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(FsError::AlreadyExists(path)),
            Err(e) => Err(FsError::io(&path, e)),
        }
    }

    /// Crea un directorio (falla si ya existe).
    pub fn create_dir(&self, rel: &Path) -> Result<(), FsError> {
        let path = self.resolve(rel)?;
        validate_name(&file_name(&path))?;
        self.require_parent(&path)?;
        match fs::create_dir(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(FsError::AlreadyExists(path)),
            Err(e) => Err(FsError::io(&path, e)),
        }
    }

    /// Renombra o mueve dentro del proyecto. Falla si el destino ya existe.
    pub fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        let from_abs = self.resolve(from)?;
        let to_abs = self.resolve(to)?;
        validate_name(&file_name(&to_abs))?;
        if !from_abs.exists() {
            return Err(FsError::NotFound(from_abs));
        }
        if to_abs.exists() {
            return Err(FsError::AlreadyExists(to_abs));
        }
        self.require_parent(&to_abs)?;
        replace_with_retry(&from_abs, &to_abs)
    }

    /// Borra un archivo o un directorio (recursivo) dentro del proyecto.
    pub fn delete(&self, rel: &Path) -> Result<(), FsError> {
        let path = self.resolve(rel)?;
        if path == self.root {
            return Err(FsError::InvalidName(
                "no se puede borrar la raíz del proyecto".into(),
            ));
        }
        let meta = fs::symlink_metadata(&path).map_err(|e| FsError::io(&path, e))?;
        if meta.is_dir() {
            fs::remove_dir_all(&path).map_err(|e| FsError::io(&path, e))
        } else {
            fs::remove_file(&path).map_err(|e| FsError::io(&path, e))
        }
    }

    /// Observa la raíz del proyecto de forma recursiva y entrega lotes
    /// coalescidos una vez por ventana de debounce (200 ms, techo 1 s).
    ///
    /// Al soltar el [`WatcherGuard`] se detiene el hilo y el watcher.
    pub fn watch(
        &self,
        on_batch: impl Fn(Vec<FsEvent>) + Send + 'static,
    ) -> Result<WatcherGuard, FsError> {
        use notify::{RecursiveMode, Watcher as _};

        let (raw_tx, raw_rx) = channel::<notify::Event>();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = raw_tx.send(event);
            }
        })
        .map_err(|e| FsError::Watch(e.to_string()))?;
        watcher
            .watch(&self.root, RecursiveMode::Recursive)
            .map_err(|e| FsError::Watch(e.to_string()))?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let root = self.root.clone();
        let handle = thread::Builder::new()
            .name("lsx-fs-watch".into())
            .spawn(move || debounce_loop(raw_rx, root, thread_stop, on_batch))
            .map_err(|e| FsError::Watch(e.to_string()))?;

        Ok(WatcherGuard {
            watcher: Some(watcher),
            stop,
            thread: Some(handle),
        })
    }

    /// Resuelve una ruta relativa contra la raíz rechazando escapes (`..`,
    /// absolutas, prefijos) por construcción léxica.
    fn resolve(&self, rel: &Path) -> Result<PathBuf, FsError> {
        let mut out = PathBuf::new();
        for component in rel.components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        return Err(FsError::OutsideRoot(rel.to_path_buf()));
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(FsError::OutsideRoot(rel.to_path_buf()));
                }
            }
        }
        Ok(self.root.join(out))
    }

    fn require_parent(&self, path: &Path) -> Result<(), FsError> {
        let parent = path
            .parent()
            .ok_or_else(|| FsError::OutsideRoot(path.to_path_buf()))?;
        if parent.is_dir() {
            Ok(())
        } else {
            Err(FsError::NotFound(parent.to_path_buf()))
        }
    }
}

/// Guardia del watcher: al soltarlo se detiene el hilo de debounce.
pub struct WatcherGuard {
    watcher: Option<notify::RecommendedWatcher>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WatcherGuard {
    /// `true` si el watcher sigue activo (el hilo no ha terminado).
    pub fn is_watching(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
}

impl Drop for WatcherGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Soltar el watcher cierra el canal → el hilo sale del recv.
        drop(self.watcher.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn temp_file_name(target: &Path) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let stem = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{TEMP_PREFIX}{}-{n}-{stem}", std::process::id())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_temp_name(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with(TEMP_PREFIX))
}

fn replace_with_retry(from: &Path, to: &Path) -> Result<(), FsError> {
    let mut last_error = None;
    for attempt in 0..REPLACE_ATTEMPTS {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_error = Some(e);
                if attempt + 1 < REPLACE_ATTEMPTS {
                    thread::sleep(REPLACE_BACKOFF);
                }
            }
        }
    }
    Err(FsError::io(
        to,
        last_error.unwrap_or_else(|| io::Error::other("rename falló")),
    ))
}

fn detect_eol(text: &str) -> Eol {
    let crlf = text.matches("\r\n").count();
    let lf_total = text.matches('\n').count();
    let lonely_lf = lf_total - crlf;
    match (crlf > 0, lonely_lf > 0) {
        (true, true) => Eol::Mixed,
        (true, false) => Eol::CrLf,
        _ => Eol::Lf,
    }
}

/// Bucle de debounce con ventana deslizante limitada por [`MAX_DEBOUNCE`].
fn debounce_loop(
    rx: Receiver<notify::Event>,
    root: PathBuf,
    stop: Arc<AtomicBool>,
    on_batch: impl Fn(Vec<FsEvent>),
) {
    let mut pending: Vec<notify::Event> = Vec::new();
    let mut window_start: Option<Instant> = None;
    let mut last_event: Option<Instant> = None;

    loop {
        let timeout = match (window_start, last_event) {
            (Some(start), Some(last)) => {
                let deadline = (last + DEBOUNCE).min(start + MAX_DEBOUNCE);
                deadline.saturating_duration_since(Instant::now())
            }
            _ => DEBOUNCE,
        };

        match rx.recv_timeout(timeout) {
            Ok(event) => {
                let now = Instant::now();
                if window_start.is_none() {
                    window_start = Some(now);
                }
                last_event = Some(now);
                pending.push(event);
            }
            Err(RecvTimeoutError::Timeout) => {
                flush(root.as_path(), &mut pending, &on_batch);
                window_start = None;
                last_event = None;
                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                flush(root.as_path(), &mut pending, &on_batch);
                break;
            }
        }
    }
}

fn flush(root: &Path, pending: &mut Vec<notify::Event>, on_batch: &impl Fn(Vec<FsEvent>)) {
    if pending.is_empty() {
        return;
    }
    let mapped = map_events(root, pending.drain(..));
    let batch = coalesce(mapped);
    if !batch.is_empty() {
        tracing::debug!(target: "lsx::core::fs", events = batch.len(), "lote de fs");
        on_batch(batch);
    }
}

/// Traduce los tipos reales de `notify` a [`FsEvent`] (relativos a `root`).
fn map_events(root: &Path, events: impl Iterator<Item = notify::Event>) -> Vec<FsEvent> {
    use notify::event::{ModifyKind, RenameMode};

    let mut out = Vec::new();
    let mut rename_source: Option<PathBuf> = None;

    for event in events {
        let paths: Vec<PathBuf> = event
            .paths
            .iter()
            .filter_map(|p| p.strip_prefix(root).ok().map(Path::to_path_buf))
            .collect();
        if paths.is_empty() {
            continue;
        }

        match event.kind {
            notify::EventKind::Create(_) => {
                out.extend(paths.into_iter().map(FsEvent::Created));
            }
            notify::EventKind::Modify(ModifyKind::Name(mode)) => match mode {
                RenameMode::From => rename_source = paths.first().cloned(),
                RenameMode::To => {
                    if let (Some(from), Some(to)) = (rename_source.take(), paths.first()) {
                        out.push(FsEvent::Renamed(from, to.clone()));
                    } else {
                        out.extend(paths.into_iter().map(FsEvent::Created));
                    }
                }
                RenameMode::Both => {
                    if paths.len() >= 2 {
                        out.push(FsEvent::Renamed(paths[0].clone(), paths[1].clone()));
                    }
                }
                RenameMode::Any | RenameMode::Other => {
                    if paths.len() >= 2 {
                        out.push(FsEvent::Renamed(paths[0].clone(), paths[1].clone()));
                    } else {
                        out.extend(paths.into_iter().map(FsEvent::Modified));
                    }
                }
            },
            notify::EventKind::Modify(_) => {
                out.extend(paths.into_iter().map(FsEvent::Modified));
            }
            notify::EventKind::Remove(_) => {
                out.extend(paths.into_iter().map(FsEvent::Removed));
            }
            // `notify` sin detalle y accesos no mutantes: solo `Any` puede ser
            // una modificación real (backend "imprecise"); el resto es ruido.
            notify::EventKind::Any => {
                out.extend(paths.into_iter().map(FsEvent::Modified));
            }
            notify::EventKind::Access(_) | notify::EventKind::Other => {}
        }
    }

    out
}

/// Agrupa eventos del mismo path dentro de una ventana de debounce.
///
/// Reglas:
/// - `Created` + `Modified` → `Created`;
/// - `Created` + `Removed` → se descarta (archivo transitorio);
/// - `Removed` + `Created` → `Modified`;
/// - `Modified` + `Removed` → `Removed`;
/// - pares `From`/`To` → `Renamed`, y el `rename` de un temporal propio se
///   traduce a `Modified(destino)`.
fn coalesce(events: Vec<FsEvent>) -> Vec<FsEvent> {
    let mut renamed_from: HashSet<PathBuf> = HashSet::new();
    let mut renamed_to: HashSet<PathBuf> = HashSet::new();
    for event in &events {
        if let FsEvent::Renamed(from, to) = event {
            renamed_from.insert(from.clone());
            renamed_to.insert(to.clone());
        }
    }

    let mut out: Vec<FsEvent> = Vec::new();
    for event in events {
        match event {
            FsEvent::Renamed(from, to) => {
                if is_temp_name(&from) {
                    merge(&mut out, FsEvent::Modified(to));
                } else {
                    merge(&mut out, FsEvent::Renamed(from, to));
                }
            }
            FsEvent::Created(path) => {
                if renamed_from.contains(&path) || renamed_to.contains(&path) || is_temp_name(&path)
                {
                    continue;
                }
                merge(&mut out, FsEvent::Created(path));
            }
            FsEvent::Modified(path) => {
                if renamed_from.contains(&path) || renamed_to.contains(&path) || is_temp_name(&path)
                {
                    continue;
                }
                merge(&mut out, FsEvent::Modified(path));
            }
            FsEvent::Removed(path) => {
                if renamed_from.contains(&path) || renamed_to.contains(&path) || is_temp_name(&path)
                {
                    continue;
                }
                merge(&mut out, FsEvent::Removed(path));
            }
        }
    }
    out
}

fn merge(out: &mut Vec<FsEvent>, event: FsEvent) {
    let path = match &event {
        FsEvent::Created(p) | FsEvent::Modified(p) | FsEvent::Removed(p) => p.clone(),
        FsEvent::Renamed(from, _) => from.clone(),
    };
    let Some(pos) = out.iter().position(|existing| existing.path() == path) else {
        out.push(event);
        return;
    };
    let existing = out[pos].clone();
    match (&existing, &event) {
        // Crear y volver a escribir en la misma ventana es una sola creación.
        (FsEvent::Created(_), FsEvent::Modified(_)) => {}
        // Archivo transitorio: se creó y se borró en la misma ventana.
        (FsEvent::Created(_), FsEvent::Removed(_)) => {
            out.remove(pos);
        }
        // Se borró y se volvió a crear: el contenido cambió.
        (FsEvent::Removed(_), FsEvent::Created(_)) => out[pos] = FsEvent::Modified(path),
        (FsEvent::Modified(_), FsEvent::Removed(_))
        | (FsEvent::Removed(_), FsEvent::Modified(_)) => {
            out[pos] = FsEvent::Removed(path);
        }
        _ => out[pos] = event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use std::sync::mpsc;

    /// Directorio temporal de test (se limpia en `Drop`).
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("lsx-core-{tag}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("crear tempdir de test");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            // El watcher puede tardar unos ms en soltar handles; reintenta.
            for _ in 0..5 {
                if fs::remove_dir_all(&self.path).is_ok() {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    fn next_batch(rx: &mpsc::Receiver<Vec<FsEvent>>) -> Vec<FsEvent> {
        rx.recv_timeout(Duration::from_secs(10))
            .expect("el watcher no entregó ningún lote")
    }

    #[test]
    fn list_dir_sorts_dirs_first_and_hides_aux() {
        let dir = TempDir::new("list");
        let svc = FsService::new(dir.path());
        fs::create_dir(svc.root().join("secciones")).unwrap();
        fs::write(svc.root().join("main.tex"), "hola").unwrap();
        fs::write(svc.root().join("main.aux"), "aux").unwrap();
        fs::write(svc.root().join("main.synctex(busy)"), "busy").unwrap();
        fs::write(svc.root().join("Thumbs.db"), "junk").unwrap();

        let entries = svc.list_dir(Path::new("")).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["secciones", "main.tex"]);
        assert_eq!(entries[0].kind, EntryKind::Dir);
        assert!(!entries[1].ignored);

        let all = svc
            .list_dir_with(Path::new(""), ListDirOptions { show_aux: true })
            .unwrap();
        let aux = all
            .iter()
            .find(|e| e.name == "main.aux")
            .expect("aux visible con show_aux");
        assert!(aux.ignored);
    }

    #[test]
    fn resolve_rejects_paths_outside_root() {
        let dir = TempDir::new("resolve");
        let svc = FsService::new(dir.path());
        assert!(matches!(
            svc.read_text(Path::new("../fuera.txt")),
            Err(FsError::OutsideRoot(_))
        ));
        assert!(matches!(
            svc.list_dir(Path::new("C:\\Windows")),
            Err(FsError::OutsideRoot(_))
        ));
    }

    #[test]
    fn read_text_detects_bom_and_eol() {
        let dir = TempDir::new("read");
        let svc = FsService::new(dir.path());
        fs::write(svc.root().join("crlf.tex"), b"a\r\nb\r\n").unwrap();
        fs::write(svc.root().join("bom.tex"), b"\xEF\xBB\xBFhola").unwrap();
        fs::write(svc.root().join("latin1.tex"), b"caf\xE9").unwrap();

        let crlf = svc.read_text(Path::new("crlf.tex")).unwrap();
        assert_eq!(crlf.eol, Eol::CrLf);
        assert!(!crlf.had_bom);

        let bom = svc.read_text(Path::new("bom.tex")).unwrap();
        assert!(bom.had_bom);
        assert_eq!(bom.text, "hola");

        assert!(matches!(
            svc.read_text(Path::new("latin1.tex")),
            Err(FsError::NotUtf8(_))
        ));
    }

    #[test]
    fn write_text_atomic_keeps_content_intact_while_reading() {
        const SMALL: &str = "v0";
        const BIG_LEN: usize = 256 * 1024;
        let big = "x".repeat(BIG_LEN);
        let dir = TempDir::new("atomic");
        let svc = FsService::new(dir.path());
        svc.write_text_atomic(Path::new("grande.tex"), SMALL)
            .unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let reader_path = svc.root().join("grande.tex");
        let reader = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                let mut reads = 0u32;
                while !stop.load(Ordering::Relaxed) {
                    let bytes = match fs::read(&reader_path) {
                        Ok(bytes) => bytes,
                        Err(e) => panic!("lectura falló en pleno guardado atómico: {e}"),
                    };
                    // El archivo solo puede verse en una de las dos versiones
                    // publicadas: nunca a medias.
                    if bytes.len() == SMALL.len() {
                        assert_eq!(&bytes, SMALL.as_bytes());
                    } else {
                        assert_eq!(bytes.len(), BIG_LEN, "longitud intermedia inesperada");
                        assert!(bytes.iter().all(|b| *b == b'x'), "contenido corrupto");
                    }
                    reads += 1;
                }
                reads
            })
        };

        for i in 0..150 {
            let payload = if i % 2 == 0 { big.as_str() } else { SMALL };
            svc.write_text_atomic(Path::new("grande.tex"), payload)
                .unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        let reads = reader.join().expect("el lector paniqueó");
        assert!(reads > 0, "el lector no llegó a leer");
        assert_eq!(svc.read_text(Path::new("grande.tex")).unwrap().text, SMALL);
    }

    #[test]
    fn create_rename_delete_roundtrip() {
        let dir = TempDir::new("crud");
        let svc = FsService::new(dir.path());
        svc.create_dir(Path::new("capitulos")).unwrap();
        svc.create_file(Path::new("capitulos/intro.tex")).unwrap();
        assert!(matches!(
            svc.create_file(Path::new("capitulos/intro.tex")),
            Err(FsError::AlreadyExists(_))
        ));

        svc.rename(
            Path::new("capitulos/intro.tex"),
            Path::new("capitulos/uno.tex"),
        )
        .unwrap();
        svc.write_text_atomic(Path::new("capitulos/uno.tex"), "hola")
            .unwrap();
        assert_eq!(
            svc.read_text(Path::new("capitulos/uno.tex")).unwrap().text,
            "hola"
        );

        svc.rename(
            Path::new("capitulos/uno.tex"),
            Path::new("capitulos/uno.tex"),
        )
        .unwrap_err(); // el destino existe → AlreadyExists
        svc.delete(Path::new("capitulos")).unwrap();
        assert!(matches!(
            svc.read_text(Path::new("capitulos/uno.tex")),
            Err(FsError::Io { .. })
        ));
    }

    #[test]
    fn validate_name_blocks_windows_reserved_cases() {
        assert!(validate_name("main.tex").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("a/b.tex").is_err());
        assert!(validate_name("informe:").is_err());
        assert!(validate_name("final.").is_err());
        assert!(validate_name("final ").is_err());
        assert!(validate_name("CON").is_err());
        assert!(validate_name("nul.log").is_err());
        assert!(validate_name("COM7").is_err());
    }

    #[test]
    fn watcher_coalesces_create_and_modify_in_a_single_batch() {
        let dir = TempDir::new("watch-create");
        let svc = FsService::new(dir.path());
        let (tx, rx) = mpsc::channel();
        let guard = svc
            .watch(move |batch| {
                let _ = tx.send(batch);
            })
            .expect("watch");
        assert!(guard.is_watching());

        fs::write(svc.root().join("nuevo.tex"), "hola mundo").unwrap();

        let batch = next_batch(&rx);
        assert_eq!(
            batch,
            vec![FsEvent::Created(PathBuf::from("nuevo.tex"))],
            "create + write deben coalescer en un único Created"
        );
        // Nada más se entrega: una sola llamada para toda la ventana.
        assert!(
            rx.recv_timeout(Duration::from_millis(700)).is_err(),
            "llegó un segundo lote para el mismo cambio"
        );
        drop(guard);
    }

    #[test]
    fn watcher_reports_rename_and_remove() {
        let dir = TempDir::new("watch-rename");
        let svc = FsService::new(dir.path());
        let (tx, rx) = mpsc::channel();
        let guard = svc
            .watch(move |batch| {
                let _ = tx.send(batch);
            })
            .expect("watch");

        fs::write(svc.root().join("a.tex"), "x").unwrap();
        assert_eq!(
            next_batch(&rx),
            vec![FsEvent::Created(PathBuf::from("a.tex"))]
        );

        fs::rename(svc.root().join("a.tex"), svc.root().join("b.tex")).unwrap();
        assert_eq!(
            next_batch(&rx),
            vec![FsEvent::Renamed(
                PathBuf::from("a.tex"),
                PathBuf::from("b.tex")
            )]
        );

        fs::remove_file(svc.root().join("b.tex")).unwrap();
        assert_eq!(
            next_batch(&rx),
            vec![FsEvent::Removed(PathBuf::from("b.tex"))]
        );
        drop(guard);
    }

    #[test]
    fn atomic_writes_surface_as_modified_target_not_temp_noise() {
        let dir = TempDir::new("watch-atomic");
        let svc = FsService::new(dir.path());
        svc.write_text_atomic(Path::new("main.tex"), "v1").unwrap();

        let (tx, rx) = mpsc::channel();
        let guard = svc
            .watch(move |batch| {
                let _ = tx.send(batch);
            })
            .expect("watch");

        svc.write_text_atomic(Path::new("main.tex"), "v2").unwrap();
        let batch = next_batch(&rx);
        assert_eq!(batch, vec![FsEvent::Modified(PathBuf::from("main.tex"))]);
        drop(guard);
    }

    #[test]
    fn list_dir_with_5k_files_stays_within_budget() {
        // Reutiliza el directorio entre ejecuciones para no pagar la creación
        // de 5000 archivos en cada `cargo test`.
        let root = std::env::temp_dir().join("lsx-core-listdir-5k");
        fs::create_dir_all(&root).unwrap();
        let existing = fs::read_dir(&root).map(|d| d.count()).unwrap_or(0);
        if existing < 5000 {
            for i in existing..5000 {
                fs::write(root.join(format!("f{i:04}.tex")), "x").unwrap();
            }
        }

        let svc = FsService::new(&root);
        let start = Instant::now();
        let entries = svc.list_dir(Path::new("")).unwrap();
        let elapsed = start.elapsed();
        eprintln!("list_dir 5k: {elapsed:?} para {} entradas", entries.len());
        assert_eq!(entries.len(), 5000);
        assert!(
            elapsed < Duration::from_secs(1),
            "list_dir con 5k archivos tardó {elapsed:?}"
        );
    }
}
