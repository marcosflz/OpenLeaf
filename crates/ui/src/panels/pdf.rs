//! Panel PDF de producción (stream C: M1-C1..M1-C4).
//!
//! Visor propio sobre `lsx-pdf` (hayro) + `RenderImage` de GPUI, según la
//! decisión I-2 (M0-S3). El shell (stream A) lo monta directamente: implementa
//! los traits de dock del kit y expone la API estable:
//!
//! ```ignore
//! let pdf = cx.new(|cx| PdfPanel::new(cx));
//! pdf.update(cx, |pdf, cx| {
//!     pdf.set_document("out/main.pdf", generation, cx); // recarga con generation++
//!     pdf.goto_page(3, cx);                              // 1-based
//!     pdf.flash_sync_rect(rect, cx);                     // SyncTeX forward
//!     pdf.on_inverse_jump(|jump, _window, _cx| { /* abrir archivo:línea */ }, cx);
//! });
//! ```
//!
//! Piezas:
//! - **C1**: zoom (fit-width / fit-page / 50–400 % / Ctrl+rueda), scroll
//!   continuo virtualizado con `uniform_list` (solo visibles + vecinas),
//!   indicador de página, doble clic → visor externo.
//! - **C2**: `PdfRenderer` con caché LRU por bytes (default 256 MB, ajustable
//!   con [`PdfPanel::with_cache_mb`]); render progresivo: escala baja inmediata
//!   y alta escala cuando el scroll queda quieto (~160 ms de idle).
//! - **C3**: `set_document(path, generation)` recarga solo si el archivo cambió
//!   (mtime + tamaño), conserva página/offset y muestra el flash "PDF actualizado".
//! - **C4**: SyncTeX real con `lsx_latex::Synctex` (`forward_all` → caja
//!   resaltada animada + scroll) e inverso con Ctrl+clic → callback/evento
//!   [`PdfPanelEvent::InverseJump`] con archivo:línea.
//!
//! Unidades: el PDF trabaja en **bp** (1/72", origen arriba-izquierda); la UI en
//! px lógicos; `escala = zoom × dpr` y `bp × zoom = px lógicos`.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use camino::{Utf8Path, Utf8PathBuf};
use gpui_kit::component::{
    ActiveTheme as _, IconName,
    button::Button,
    dock::{BasePanel, Panel as DockPanel, PanelEvent},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use image::{Frame, RgbaImage};
use lsx_latex::{SourcePoint, Synctex};
use lsx_pdf::{DEFAULT_MAX_CACHE_MB, PageBitmap, PdfCache, PdfRenderer, scale_bucket};

/// Zoom mínimo permitido (50 %).
pub const ZOOM_MIN: f32 = 0.5;
/// Zoom máximo permitido (400 %).
pub const ZOOM_MAX: f32 = 4.0;
/// Escala baja del render progresivo, como fracción de la alta.
const PROGRESSIVE_LOW_FRACTION: f32 = 0.5;
/// Quietud de scroll antes de lanzar el render a alta escala.
const IDLE_RENDER_DELAY: Duration = Duration::from_millis(160);
/// Duración del resaltado SyncTeX.
const SYNC_MARK_TTL: Duration = Duration::from_millis(2000);
/// Duración del flash "PDF actualizado".
const UPDATED_FLASH_TTL: Duration = Duration::from_millis(1600);
/// Separación vertical entre páginas (px lógicos).
const PAGE_GAP: f32 = 16.0;
/// Margen horizontal reservado en fit-width (px lógicos por lado).
const FIT_MARGIN_X: f32 = 12.0;
/// Páginas extra que se piden fuera del rango visible.
const PREFETCH_PAGES: usize = 1;
/// Pasos "bonitos" del zoom manual.
const ZOOM_STEPS: [f32; 9] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0];
/// Cambio mínimo de scroll (px) para avisar al shell del desplazamiento.
const SCROLL_REPORT_EPSILON_PX: f32 = 2.0;

/// Modo de zoom del visor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomMode {
    /// Ajusta el ancho de la página más ancha al ancho del panel.
    #[default]
    FitWidth,
    /// Ajusta la página más alta completa al panel.
    FitPage,
    /// Zoom manual entre [`ZOOM_MIN`] y [`ZOOM_MAX`].
    Manual,
}

impl ZoomMode {
    /// Modo persistible en settings (`pdf.zoomMode`).
    pub fn as_setting(self) -> &'static str {
        match self {
            ZoomMode::FitWidth => "fit-width",
            ZoomMode::FitPage => "fit-page",
            ZoomMode::Manual => "manual",
        }
    }

    /// Parsea `pdf.zoomMode` de settings; desconocido → `FitWidth`.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "fit-page" => ZoomMode::FitPage,
            "manual" => ZoomMode::Manual,
            _ => ZoomMode::FitWidth,
        }
    }
}

/// Rectángulo de resaltado SyncTeX en bp de la página (origen arriba-izquierda).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncRect {
    /// Página 1-based (como la reporta SyncTeX).
    pub page: u32,
    /// Coordenada x de la caja en bp.
    pub x: f32,
    /// Coordenada y del borde superior en bp.
    pub y: f32,
    /// Ancho de la caja en bp.
    pub w: f32,
    /// Alto de la caja en bp.
    pub h: f32,
}

impl SyncRect {
    /// Caja de un registro `synctex view` (M0-S5: caja = `(h, v−H, W, H)`).
    pub fn from_record(record: &lsx_latex::SyncRecord) -> Self {
        Self {
            page: record.page,
            x: record.h,
            y: record.v - record.height,
            w: record.width,
            h: record.height,
        }
    }

    /// Rectángulo que abarca toda la página (fallback sin caja).
    pub fn whole_page(page: u32, page_w_pt: f32, page_h_pt: f32) -> Self {
        Self {
            page,
            x: 0.0,
            y: 0.0,
            w: page_w_pt,
            h: page_h_pt,
        }
    }
}

/// `(page0, x_bp, y_bp)` del borde superior del viewport a partir del scroll.
///
/// - `top`: px de contenido del scroll; `slot`: alto de cada slot de página en
///   px; `page_size(page)`: `(ancho, alto)` de la página en px lógicos.
/// - `x_bp` es el centro horizontal de la página: la columna no es relevante
///   para `synctex edit`.
///
/// Función pura (testeable sin documento abierto); la usa
/// [`PdfPanel::source_point_at_top`] para el scroll sync inverso (M1-C5).
fn source_point_at(
    top: f32,
    slot: f32,
    page_count: u32,
    page_size: impl Fn(u32) -> (f32, f32),
    zoom: f32,
) -> Option<(u32, f32, f32)> {
    if page_count == 0 || slot <= 0.0 || zoom <= 0.0 {
        return None;
    }
    let page = ((top.max(0.0) / slot).floor() as u32).min(page_count - 1);
    let (w_px, h_px) = page_size(page);
    let top_in_slot = (slot - h_px) / 2.0;
    let y_px = (top.max(0.0) - (page as f32 * slot + top_in_slot)).clamp(0.0, h_px.max(0.0));
    Some((page, (w_px / 2.0) / zoom, y_px / zoom))
}

/// Origen en el `.tex` al que saltar desde el PDF (Ctrl+clic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InverseJump {
    /// Archivo fuente (normalizado por `Synctex`).
    pub file: PathBuf,
    /// Línea 1-based.
    pub line: u32,
    /// Columna 1-based; `None` si el CLI no la conoce.
    pub column: Option<u32>,
}

impl From<SourcePoint> for InverseJump {
    fn from(source: SourcePoint) -> Self {
        Self {
            file: source.file.into_std_path_buf(),
            line: source.line,
            column: source.column,
        }
    }
}

/// Eventos emitidos por el panel (el shell puede suscribirse en vez de usar
/// callbacks).
#[derive(Debug, Clone)]
pub enum PdfPanelEvent {
    /// Documento cargado o recargado (generación nueva).
    DocumentReady {
        path: PathBuf,
        generation: u64,
        page_count: u32,
    },
    /// El documento no se pudo abrir.
    DocumentFailed { path: PathBuf, message: String },
    /// La página superior visible cambió.
    PageChanged { page: u32, count: u32 },
    /// El usuario ha desplazado el visor (rueda, barra o teclado). El shell lo
    /// usa para el scroll sync inverso (M1-C5): al asentarse el scroll, pide
    /// [`PdfPanel::source_point_at_top`] y resuelve el origen con SyncTeX.
    ViewportScrolled,
    /// Zoom/modo cambiados (para persistir en settings).
    ZoomChanged { zoom: f32, mode: ZoomMode },
    /// Ctrl+clic sobre el PDF resolvió una posición del `.tex`.
    InverseJump(InverseJump),
}

/// Callback de salto inverso (Ctrl+clic): recibe el origen resuelto.
pub type InverseJumpCallback = Box<dyn Fn(InverseJump, &mut Window, &mut App) + 'static>;

/// Marca de resaltado SyncTeX viva hasta `until`.
#[derive(Debug, Clone, Copy)]
struct SyncMark {
    rect: SyncRect,
    until: Instant,
}

/// Página/offset que se conserva entre zoom y recargas.
#[derive(Debug, Clone, Copy)]
struct ScrollAnchor {
    /// Posición vertical como página fraccionaria (2.5 = mitad de la página 3).
    page_float: f32,
}

/// Huella barata del PDF en disco (para no recargar si no cambió).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    modified: Option<SystemTime>,
    len: u64,
}

impl FileFingerprint {
    fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

/// Documento cargado + metadatos de recarga.
#[derive(Clone)]
struct LoadedDoc {
    path: PathBuf,
    generation: u64,
    renderer: Arc<PdfRenderer>,
    fingerprint: Option<FileFingerprint>,
}

/// Bench de scroll (medición de fluidez, evidencia para QA).
struct ScrollBench {
    started: Instant,
    duration: Duration,
    pages: f32,
    frames_ms: Vec<f64>,
    started_page: f32,
    last_frame: Instant,
}

impl ScrollBench {
    fn progress(&self) -> f32 {
        (self.started.elapsed().as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }

    fn summary(&self) -> String {
        if self.frames_ms.is_empty() {
            return "bench: sin frames".to_string();
        }
        let mut frames = self.frames_ms.clone();
        frames.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mean = frames.iter().sum::<f64>() / frames.len() as f64;
        let p95 = frames[((frames.len() as f64 * 0.95) as usize).min(frames.len() - 1)];
        let fps = if mean > 0.0 { 1000.0 / mean } else { 0.0 };
        format!(
            "scroll {:.0} pág: {:.0} fps · frame medio {:.1} ms · p95 {:.1} ms · {} frames",
            self.pages,
            fps,
            mean,
            p95,
            frames.len()
        )
    }
}

/// Panel del visor PDF.
pub struct PdfPanel {
    /// Documento actual (None = vacío).
    doc: Option<LoadedDoc>,
    /// Caché compartida entre recargas (clave por generación).
    cache: Arc<PdfCache>,
    /// Presupuesto de caché en MiB (settings `pdf.maxCacheMB`).
    cache_mb: u32,
    /// Mensaje de error visible (apertura, synctex…).
    error: Option<String>,
    /// Carga en curso.
    loading: bool,
    /// Zoom aplicado (1.0 = 100 %); en modos fit lo recalcula el layout.
    zoom: f32,
    /// Modo de zoom.
    zoom_mode: ZoomMode,
    /// Tamaño lógico del viewport del visor (px), capturado por el canvas probe.
    viewport: Option<(f32, f32)>,
    /// Origen del viewport en coordenadas de ventana (para clics).
    viewport_origin: (f32, f32),
    /// Factor de escala de la ventana (dpr).
    dpr: f32,
    /// Scroll del `uniform_list` (página virtualizada).
    scroll: UniformListScrollHandle,
    /// Rango visible actual (de la última pasada del callback del list).
    visible_range: Range<usize>,
    /// Último `scroll_top` comunicado al shell (evita repetir
    /// [`PdfPanelEvent::ViewportScrolled`] en cada frame).
    reported_scroll_top: f32,
    /// Página superior visible (1-based) para el indicador.
    current_page: u32,
    /// Texturas por `(página 0-based, escala en milésimas)`.
    images: HashMap<(u32, u32), Arc<RenderImage>>,
    /// Renders en vuelo por `(página, escala en milésimas)`.
    inflight: HashSet<(u32, u32)>,
    /// Secuencia de scroll: invalida renders de alta escala ya obsoletos.
    scroll_seq: u64,
    /// Ancla pendiente de restaurar tras recargar (C3).
    pending_anchor: Option<ScrollAnchor>,
    /// Última duración de render medida (ms).
    last_render_ms: Option<f64>,
    /// Cliente SyncTeX (None = no configurado).
    synctex: Option<Synctex>,
    /// Resaltados SyncTeX vivos.
    sync_marks: Vec<SyncMark>,
    /// Callback de salto inverso.
    inverse_cb: Option<InverseJumpCallback>,
    /// Flash "PDF actualizado" vivo hasta.
    updated_flash_until: Option<Instant>,
    /// Mensaje de estado transitorio (barra inferior).
    status: Option<(String, Instant)>,
    /// Bench de scroll activo.
    bench: Option<ScrollBench>,
    /// Último resumen de bench.
    bench_report: Option<String>,
    /// Frames recientes (ms) para el indicador dev.
    frame_ms: Vec<f64>,
    /// Timestamp del último render (para medir frames).
    last_frame_at: Option<Instant>,
    /// Focus para atajos.
    focus: FocusHandle,
    /// Mostrar barra de herramientas (el shell puede apagarla).
    show_toolbar: bool,
}

impl PdfPanel {
    /// Panel vacío con caché de [`DEFAULT_MAX_CACHE_MB`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::with_cache_mb(DEFAULT_MAX_CACHE_MB, cx)
    }

    /// Panel con presupuesto de caché en MiB (settings `pdf.maxCacheMB`).
    pub fn with_cache_mb(cache_mb: u32, cx: &mut Context<Self>) -> Self {
        Self {
            doc: None,
            cache: Arc::new(PdfCache::from_megabytes(cache_mb)),
            cache_mb,
            error: None,
            loading: false,
            zoom: 1.0,
            zoom_mode: ZoomMode::FitWidth,
            viewport: None,
            viewport_origin: (0.0, 0.0),
            dpr: 1.0,
            scroll: UniformListScrollHandle::new(),
            visible_range: 0..0,
            reported_scroll_top: 0.0,
            current_page: 1,
            images: HashMap::new(),
            inflight: HashSet::new(),
            scroll_seq: 0,
            pending_anchor: None,
            last_render_ms: None,
            synctex: None,
            sync_marks: Vec::new(),
            inverse_cb: None,
            updated_flash_until: None,
            status: None,
            bench: None,
            bench_report: None,
            frame_ms: Vec::new(),
            last_frame_at: None,
            focus: cx.focus_handle(),
            show_toolbar: true,
        }
    }

    /// Muestra u oculta la toolbar propia del panel.
    pub fn set_toolbar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.show_toolbar = visible;
        cx.notify();
    }

    /// Presupuesto de caché actual en MiB.
    pub fn cache_mb(&self) -> u32 {
        self.cache_mb
    }

    /// Ruta del documento cargado.
    pub fn document_path(&self) -> Option<&Path> {
        self.doc.as_ref().map(|doc| doc.path.as_path())
    }

    /// Generación del documento cargado.
    pub fn generation(&self) -> Option<u64> {
        self.doc.as_ref().map(|doc| doc.generation)
    }

    /// Páginas del documento (0 si no hay).
    pub fn page_count(&self) -> u32 {
        self.doc
            .as_ref()
            .map(|doc| doc.renderer.page_count())
            .unwrap_or(0)
    }

    /// Página superior visible (1-based; 1 si no hay documento).
    pub fn current_page(&self) -> u32 {
        self.current_page
    }

    /// Zoom aplicado (1.0 = 100 %).
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Modo de zoom actual.
    pub fn zoom_mode(&self) -> ZoomMode {
        self.zoom_mode
    }

    /// Última duración de render de página medida (ms).
    pub fn last_render_ms(&self) -> Option<f64> {
        self.last_render_ms
    }

    /// Configura el `synctex.exe` para forward/inverse (C4).
    ///
    /// Si la ruta no es UTF-8, SyncTeX queda deshabilitado (v1 solo soporta
    /// rutas UTF-8 en el CLI).
    pub fn set_synctex_exe(&mut self, exe: impl Into<PathBuf>) {
        let exe = exe.into();
        match Utf8PathBuf::from_path_buf(exe) {
            Ok(exe) => self.synctex = Some(Synctex::new(exe)),
            Err(rejected) => {
                tracing::warn!(
                    path = %rejected.display(),
                    "synctex.exe con ruta no UTF-8: SyncTeX deshabilitado"
                );
                self.synctex = None;
            }
        }
    }

    /// ¿Hay SyncTeX configurado?
    pub fn has_synctex(&self) -> bool {
        self.synctex.is_some()
    }

    /// Callback de salto inverso (Ctrl+clic); además se emite
    /// [`PdfPanelEvent::InverseJump`].
    pub fn on_inverse_jump(
        &mut self,
        callback: impl Fn(InverseJump, &mut Window, &mut App) + 'static,
        _cx: &mut Context<Self>,
    ) {
        self.inverse_cb = Some(Box::new(callback));
    }

    /// Carga (o recarga) `path` con la `generation` del compile queue.
    ///
    /// Si el archivo no cambió desde la última carga (mtime + tamaño), no hace
    /// nada: evita parpadeo y re-render tras compilaciones incrementales sin
    /// cambios reales (M1-C3). Al recargar conserva página/offset y muestra el
    /// flash "PDF actualizado".
    pub fn set_document(
        &mut self,
        path: impl Into<PathBuf>,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let path = path.into();
        let fingerprint = FileFingerprint::of(&path);

        if let Some(doc) = &self.doc
            && doc.path == path
            && fingerprint.is_some()
            && fingerprint == doc.fingerprint
        {
            // Mismo contenido: la caché sigue siendo válida; solo se registra
            // la intención del caller.
            tracing::debug!(
                path = %path.display(),
                generation,
                "set_document sin cambios en disco: no se recarga"
            );
            return;
        }

        let anchor = self.scroll_anchor();
        let cache = self.cache.clone();
        self.loading = true;
        self.error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let read_path = path.clone();
            let loaded = cx
                .background_spawn(async move {
                    let bytes = std::fs::read(&read_path)?;
                    let renderer = PdfRenderer::open_with_cache(bytes, generation, cache)
                        .map_err(|err| std::io::Error::other(format!("{err}")))?;
                    Ok::<_, std::io::Error>((renderer, fingerprint))
                })
                .await;

            this.update(cx, |this, cx| {
                this.loading = false;
                match loaded {
                    Ok((renderer, fingerprint)) => {
                        let page_count = renderer.page_count();
                        // La generación vieja ya no se pinta: liberar su caché.
                        renderer.retain_generation();
                        this.doc = Some(LoadedDoc {
                            path: path.clone(),
                            generation,
                            renderer: Arc::new(renderer),
                            fingerprint,
                        });
                        this.images.clear();
                        this.inflight.clear();
                        this.error = None;
                        this.pending_anchor = Some(anchor);
                        this.restore_pending_anchor(cx);
                        this.updated_flash_until = Some(Instant::now() + UPDATED_FLASH_TTL);
                        this.set_status(format!("PDF actualizado ({page_count} págs)"));
                        this.reflash_later(UPDATED_FLASH_TTL + Duration::from_millis(60), cx);
                        cx.emit(PdfPanelEvent::DocumentReady {
                            path,
                            generation,
                            page_count,
                        });
                        // El aviso de página no llega si la visible sigue siendo
                        // la 1 (p. ej. tras recargar): se publica siempre para
                        // que la status bar muestre la página y el total.
                        this.current_page = this.current_page.clamp(1, page_count.max(1));
                        cx.emit(PdfPanelEvent::PageChanged {
                            page: this.current_page,
                            count: page_count,
                        });
                        cx.notify();
                    }
                    Err(err) => {
                        this.error = Some(format!("no se pudo abrir el PDF: {err}"));
                        this.set_status("error al abrir el PDF");
                        cx.emit(PdfPanelEvent::DocumentFailed {
                            path,
                            message: err.to_string(),
                        });
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Salta a `page` (1-based) alineada arriba.
    pub fn goto_page(&mut self, page: u32, cx: &mut Context<Self>) {
        let count = self.page_count();
        if count == 0 {
            return;
        }
        let page0 = page.clamp(1, count) - 1;
        self.set_scroll_top(page0 as f32 * self.slot_height());
        cx.notify();
    }

    /// Resalta `rect` (SyncTeX forward) y hace scroll para dejarlo visible.
    pub fn flash_sync_rect(&mut self, rect: SyncRect, cx: &mut Context<Self>) {
        let now = Instant::now();
        // Un nuevo forward reemplaza los resaltados de la página anterior.
        self.sync_marks
            .retain(|mark| mark.until > now && mark.rect.page == rect.page);
        self.sync_marks.push(SyncMark {
            rect,
            until: now + SYNC_MARK_TTL,
        });
        self.scroll_to_rect(rect);
        self.reflash_later(SYNC_MARK_TTL + Duration::from_millis(60), cx);
        cx.notify();
    }

    /// Scroll silencioso al `rect` de SyncTeX, **sin** resaltado: es el scroll
    /// sync automático (M1-C5), donde el salto continuo haría ruido visual.
    pub fn scroll_to_sync_rect(&mut self, rect: SyncRect, cx: &mut Context<Self>) {
        self.scroll_to_rect(rect);
        cx.notify();
    }

    /// Posición del `.tex` correspondiente al borde superior del viewport
    /// (`page0`, `x_bp`, `y_bp`). Base del scroll sync inverso (M1-C5).
    pub fn source_point_at_top(&self) -> Option<(u32, f32, f32)> {
        source_point_at(
            self.scroll_top(),
            self.slot_height(),
            self.page_count(),
            |page| self.page_size_px(page),
            self.zoom,
        )
    }

    /// SyncTeX forward: resuelve `src:line` con el CLI real y resalta la caja.
    ///
    /// Requiere [`PdfPanel::set_synctex_exe`] y un sidecar `.synctex.gz` junto
    /// al PDF (M0-S5). La ruta `src` se normaliza (sin `./` ni `../`): el CLI
    /// la compara contra la ruta grabada en el sidecar.
    pub fn sync_forward(&mut self, src: &Path, line: u32, col: u32, cx: &mut Context<Self>) {
        let (Some(doc), Some(synctex)) = (self.doc.as_ref(), self.synctex.clone()) else {
            self.set_status("SyncTeX no configurado");
            cx.notify();
            return;
        };
        let pdf = Utf8Path::from_path(&doc.path).map(|p| p.to_path_buf());
        let src = Utf8Path::from_path(&normalize_lexical(src)).map(|p| p.to_path_buf());
        let (Some(pdf), Some(src)) = (pdf, src) else {
            self.set_status("rutas no UTF-8: SyncTeX no soportado en v1");
            cx.notify();
            return;
        };

        let t0 = Instant::now();
        let src_name = src.file_name().unwrap_or_default().to_owned();
        let pdf_name = pdf.file_name().unwrap_or_default().to_owned();
        cx.spawn(async move |this, cx| {
            let records = cx
                .background_spawn(async move { synctex.forward_all(&pdf, &src, line, col) })
                .await;
            this.update(cx, |this, cx| match records {
                Ok(records) if !records.is_empty() => {
                    let elapsed = t0.elapsed();
                    let rects: Vec<SyncRect> = records.iter().map(SyncRect::from_record).collect();
                    let first = rects[0];
                    for rect in rects.iter().take(3) {
                        this.flash_sync_rect(*rect, cx);
                    }
                    this.set_status(format!(
                        "SyncTeX → pág {} ({:.0} ms, línea {line})",
                        first.page,
                        elapsed.as_secs_f64() * 1000.0
                    ));
                    cx.notify();
                }
                Ok(_) => {
                    tracing::debug!(%src_name, pdf = %pdf_name, line, "synctex forward sin coincidencia");
                    this.set_status(format!(
                        "SyncTeX sin coincidencia: {}:{line} sobre {pdf_name}",
                        src_name
                    ));
                    cx.notify();
                }
                Err(err) => {
                    this.set_status(format!("SyncTeX: {err}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Aplica el zoom del modo activo.
    pub fn set_zoom_mode(&mut self, mode: ZoomMode, cx: &mut Context<Self>) {
        if self.zoom_mode == mode {
            return;
        }
        self.zoom_mode = mode;
        let anchor = self.scroll_anchor();
        self.apply_fit_zoom();
        if mode == ZoomMode::Manual {
            self.after_zoom_change(anchor);
        }
        cx.emit(PdfPanelEvent::ZoomChanged {
            zoom: self.zoom,
            mode: self.zoom_mode,
        });
        cx.notify();
    }

    /// Fija el zoom (pasa a modo [`ZoomMode::Manual`]).
    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        let zoom = quantize_zoom(zoom.clamp(ZOOM_MIN, ZOOM_MAX));
        if (zoom - self.zoom).abs() < 1e-4 && self.zoom_mode == ZoomMode::Manual {
            return;
        }
        let anchor = self.scroll_anchor();
        self.zoom_mode = ZoomMode::Manual;
        self.zoom = zoom;
        self.after_zoom_change(anchor);
        cx.emit(PdfPanelEvent::ZoomChanged {
            zoom: self.zoom,
            mode: self.zoom_mode,
        });
        cx.notify();
    }

    /// Sube un paso de zoom.
    pub fn zoom_in(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(next_zoom_step(self.zoom, 1), cx);
    }

    /// Baja un paso de zoom.
    pub fn zoom_out(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(next_zoom_step(self.zoom, -1), cx);
    }

    /// Zoom 100 % manual.
    pub fn zoom_reset(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(1.0, cx);
    }

    /// Zoom multiplicativo (Ctrl+rueda).
    pub fn zoom_by_factor(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom * factor, cx);
    }

    /// Lanza un bench de scroll sobre `pages` páginas en `duration`.
    pub fn bench_scroll(&mut self, pages: u32, duration: Duration, cx: &mut Context<Self>) {
        let count = self.page_count();
        if count == 0 {
            return;
        }
        let start_page = self.current_page.saturating_sub(1) as f32;
        let target = (start_page + pages as f32).min((count - 1) as f32);
        let pages_moved = (target - start_page).max(1.0);
        self.bench_report = None;
        self.bench = Some(ScrollBench {
            started: Instant::now(),
            duration,
            pages: pages_moved,
            frames_ms: Vec::new(),
            started_page: start_page,
            last_frame: Instant::now(),
        });

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
                let finished = this
                    .update(cx, |this, cx| {
                        let Some(bench) = this.bench.as_mut() else {
                            return true;
                        };
                        let t = bench.progress();
                        // EaseInOutCubic para un desplazamiento continuo.
                        let eased = if t < 0.5 {
                            4.0 * t * t * t
                        } else {
                            1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                        };
                        let page = bench.started_page + bench.pages * eased;
                        this.set_scroll_top(page * this.slot_height());
                        if t >= 1.0 {
                            let bench = this.bench.take().expect("bench en curso");
                            let summary = bench.summary();
                            println!("[m1-c1] {summary}");
                            this.bench_report = Some(summary);
                            true
                        } else {
                            cx.notify();
                            false
                        }
                    })
                    .unwrap_or(true);
                if finished {
                    break;
                }
            }
            this.update(cx, |this, cx| {
                this.set_status(this.bench_report.clone().unwrap_or_default());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Último resumen de bench (para tests/QA).
    pub fn bench_report(&self) -> Option<&str> {
        self.bench_report.as_deref()
    }

    /// Milisegundos de frame p95 recientes (indicador dev).
    pub fn frame_ms_p95(&self) -> Option<f64> {
        if self.frame_ms.len() < 10 {
            return None;
        }
        let mut frames = self.frame_ms.clone();
        frames.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(frames[((frames.len() as f64 * 0.95) as usize).min(frames.len() - 1)])
    }

    // ------------------------------------------------------------------
    // Internos
    // ------------------------------------------------------------------

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some((message.into(), Instant::now() + Duration::from_secs(6)));
    }

    /// Relanza un repaint tras `delay` para limpiar marcas/flash caducados.
    fn reflash_later(&mut self, delay: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| {
                let now = Instant::now();
                this.sync_marks.retain(|mark| mark.until > now);
                if this.updated_flash_until.is_some_and(|until| until <= now) {
                    this.updated_flash_until = None;
                }
                if let Some((_, until)) = &this.status
                    && *until <= now
                {
                    this.status = None;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Alta escala actual (px/bp).
    fn scale_hi(&self) -> f32 {
        (self.zoom * self.dpr).max(0.05)
    }

    /// Baja escala del render progresivo (px/bp).
    fn scale_lo(&self) -> f32 {
        (self.scale_hi() * PROGRESSIVE_LOW_FRACTION).max(0.25)
    }

    /// Altura de cada slot de página en la lista (px lógicos).
    fn slot_height(&self) -> f32 {
        self.max_page_height_pt() * self.zoom + PAGE_GAP
    }

    /// Tamaño lógico de una página en px.
    fn page_size_px(&self, page: u32) -> (f32, f32) {
        let Some(doc) = &self.doc else {
            return (0.0, 0.0);
        };
        let (w, h) = doc.renderer.page_size_pt(page).unwrap_or((612.0, 792.0));
        (w * self.zoom, h * self.zoom)
    }

    /// Top del scroll actual (px de contenido; ≥ 0).
    fn scroll_top(&self) -> f32 {
        let offset = self.scroll.0.borrow().base_handle.offset().y;
        f32::from(-offset).max(0.0)
    }

    /// Fija el scroll a `top` px de contenido.
    fn set_scroll_top(&mut self, top: f32) {
        let top = top.max(0.0);
        self.scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.0), px(-top)));
    }

    /// Ancla actual (página fraccionaria).
    fn scroll_anchor(&self) -> ScrollAnchor {
        let slot = self.slot_height();
        let count = self.page_count().max(1) as f32;
        let page_float = if slot > 0.0 {
            (self.scroll_top() / slot).clamp(0.0, (count - 1.0).max(0.0))
        } else {
            0.0
        };
        ScrollAnchor { page_float }
    }

    /// Aplica el ancla pendiente tras una recarga (C3).
    fn restore_pending_anchor(&mut self, _cx: &mut Context<Self>) {
        if let Some(anchor) = self.pending_anchor.take() {
            let count = self.page_count();
            if count > 0 {
                let clamped = anchor.page_float.min((count - 1) as f32);
                self.set_scroll_top(clamped * self.slot_height());
            }
        }
    }

    /// Tras cambiar el zoom: recalcula derivados y conserva el ancla.
    fn after_zoom_change(&mut self, anchor: ScrollAnchor) {
        self.prune_images();
        let count = self.page_count();
        if count > 0 {
            let clamped = anchor.page_float.min((count - 1) as f32);
            self.set_scroll_top(clamped * self.slot_height());
        }
    }

    /// Ajusta el zoom si el modo es fit y el viewport cambió.
    fn apply_fit_zoom(&mut self) {
        let target = match self.zoom_mode {
            ZoomMode::Manual => return,
            ZoomMode::FitWidth => fit_width_zoom(
                self.viewport.map(|(w, _)| w).unwrap_or(900.0),
                self.max_page_width_pt(),
            ),
            ZoomMode::FitPage => {
                let (w, h) = self.viewport.unwrap_or((900.0, 700.0));
                fit_page_zoom(w, h, self.max_page_width_pt(), self.max_page_height_pt())
            }
        };
        let target = quantize_zoom(target.clamp(ZOOM_MIN, ZOOM_MAX));
        if (target - self.zoom).abs() > 0.0005 {
            let anchor = self.scroll_anchor();
            self.zoom = target;
            self.after_zoom_change(anchor);
        }
    }

    fn max_page_width_pt(&self) -> f32 {
        self.doc
            .as_ref()
            .map(|doc| doc.renderer.max_page_width_pt())
            .unwrap_or(612.0)
    }

    fn max_page_height_pt(&self) -> f32 {
        self.doc
            .as_ref()
            .map(|doc| doc.renderer.max_page_height_pt())
            .unwrap_or(792.0)
    }

    /// Deja `rect` (bp) visible tras el flash (margen de 1/4 de viewport).
    fn scroll_to_rect(&mut self, rect: SyncRect) {
        let count = self.page_count();
        if count == 0 || rect.page == 0 || rect.page > count {
            return;
        }
        let margin = self.viewport.map(|(_, h)| h * 0.25).unwrap_or(120.0);
        let slot = self.slot_height();
        let page0 = rect.page - 1;
        let (_page_w, page_h) = self.page_size_px(page0);
        let top_in_slot = (slot - page_h) / 2.0;
        let target = page0 as f32 * slot + top_in_slot + rect.y * self.zoom - margin;
        self.set_scroll_top(target.max(0.0));
    }

    /// Viewport medido por el canvas probe.
    fn on_viewport(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let width: f32 = bounds.size.width.into();
        let height: f32 = bounds.size.height.into();
        self.viewport_origin = (bounds.origin.x.into(), bounds.origin.y.into());
        let size = (width, height);
        if self.viewport != Some(size) {
            self.viewport = Some(size);
            if self.zoom_mode != ZoomMode::Manual {
                self.apply_fit_zoom();
            }
            cx.notify();
        }
    }

    /// Solicita en background las páginas visibles + vecinas (C2).
    fn request_pages(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else {
            return;
        };
        let count = doc.renderer.page_count();
        if count == 0 {
            return;
        }
        let start = range.start.saturating_sub(PREFETCH_PAGES);
        let end = (range.end + PREFETCH_PAGES).min(count as usize);
        let seq = self.scroll_seq;
        let scale_hi = self.scale_hi();
        let scale_lo = self.scale_lo();
        let hi_milli = scale_bucket(scale_hi);
        let lo_milli = scale_bucket(scale_lo);

        for page in start..end {
            let page = page as u32;
            if self.images.contains_key(&(page, hi_milli)) {
                continue;
            }
            if !self.inflight.insert((page, hi_milli)) {
                continue;
            }
            let renderer = doc.renderer.clone();

            cx.spawn(async move |this, cx| {
                // 1) Escala baja inmediata si no está en caché ni en vuelo.
                let need_lo = this
                    .update(cx, |this, _| {
                        if this.images.contains_key(&(page, lo_milli)) {
                            false
                        } else {
                            this.inflight.insert((page, lo_milli))
                        }
                    })
                    .unwrap_or(false);

                if need_lo {
                    let bitmap = cx
                        .background_spawn({
                            let renderer = renderer.clone();
                            async move {
                                renderer
                                    .render_page(page, scale_lo)
                                    .map_err(|err| err.to_string())
                            }
                        })
                        .await;
                    let image = match bitmap {
                        Ok(bitmap) => Some(
                            cx.background_spawn(async move { bitmap_to_render_image(&bitmap) })
                                .await,
                        ),
                        Err(err) => {
                            tracing::debug!(page, scale = scale_lo, %err, "render progresivo falló");
                            None
                        }
                    };
                    this.update(cx, |this, cx| {
                        this.inflight.remove(&(page, lo_milli));
                        if let Some(image) = image {
                            this.images.insert((page, lo_milli), Arc::new(image));
                            cx.notify();
                        }
                    })
                    .ok();
                }

                // 2) Alta escala tras quietud de scroll.
                cx.background_executor().timer(IDLE_RENDER_DELAY).await;
                let still_wanted = this
                    .update(cx, |this, _| {
                        this.scroll_seq == seq && !this.images.contains_key(&(page, hi_milli))
                    })
                    .unwrap_or(false);
                if !still_wanted {
                    let _ = this.update(cx, |this, _| {
                        this.inflight.remove(&(page, hi_milli));
                    });
                    return;
                }

                let t0 = Instant::now();
                let bitmap = cx
                    .background_spawn({
                        let renderer = renderer.clone();
                        async move {
                            renderer
                                .render_page(page, scale_hi)
                                .map_err(|err| err.to_string())
                        }
                    })
                    .await;
                let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
                let image = match bitmap {
                    Ok(bitmap) => Some(
                        cx.background_spawn(async move { bitmap_to_render_image(&bitmap) })
                            .await,
                    ),
                    Err(err) => {
                        tracing::warn!(page, %err, "render de alta escala falló");
                        None
                    }
                };
                this.update(cx, |this, cx| {
                    this.inflight.remove(&(page, hi_milli));
                    if let Some(image) = image {
                        this.images.insert((page, hi_milli), Arc::new(image));
                        this.last_render_ms = Some(elapsed_ms);
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// Elimina texturas y renders que no corresponden al zoom actual.
    fn prune_images(&mut self) {
        let hi = scale_bucket(self.scale_hi());
        let lo = scale_bucket(self.scale_lo());
        self.images
            .retain(|(_, milli), _| *milli == hi || *milli == lo);
        self.inflight
            .retain(|(_, milli)| *milli == hi || *milli == lo);
    }

    /// Textura a pintar para `page`: prefiere alta escala, luego baja.
    fn image_for(&self, page: u32) -> Option<Arc<RenderImage>> {
        let hi = scale_bucket(self.scale_hi());
        let lo = scale_bucket(self.scale_lo());
        self.images
            .get(&(page, hi))
            .or_else(|| self.images.get(&(page, lo)))
            .cloned()
    }

    /// Callback del list: el rango del callback NO se usa (`uniform_list`
    /// también lo invoca para medir items, p. ej. `0..1`); el rango real se
    /// calcula con el offset de scroll y el viewport medido.
    fn on_visible_range(&mut self, _callback_range: Range<usize>, cx: &mut Context<Self>) {
        let range = self.computed_visible_range();
        // El visor se ha desplazado (rueda, barra o scroll programático): el
        // shell decide si es un scroll del usuario para el sync inverso.
        let top = self.scroll_top();
        if (top - self.reported_scroll_top).abs() > SCROLL_REPORT_EPSILON_PX {
            self.reported_scroll_top = top;
            cx.emit(PdfPanelEvent::ViewportScrolled);
        }
        if range != self.visible_range {
            self.scroll_seq += 1;
            self.visible_range = range.clone();
            // Suelta texturas fuera de visible±3: los bitmaps siguen en la
            // caché LRU (presupuesto en bytes) y se re-suben si se vuelve.
            let keep_start = range.start.saturating_sub(3) as u32;
            let keep_end = (range.end + 3) as u32;
            self.images
                .retain(|(page, _), _| *page >= keep_start && *page <= keep_end);
            let page = range.start as u32 + 1;
            if page != self.current_page {
                self.current_page = page;
                let count = self.page_count();
                cx.emit(PdfPanelEvent::PageChanged { page, count });
            }
        }
        self.request_pages(range, cx);
    }

    /// Rango de páginas visible según el offset real (0-based, fin exclusivo).
    fn computed_visible_range(&self) -> Range<usize> {
        let count = self.page_count() as usize;
        if count == 0 {
            return 0..0;
        }
        let slot = self.slot_height();
        if slot <= 0.0 {
            return 0..count;
        }
        let (_, viewport_h) = self.viewport.unwrap_or((900.0, 700.0));
        let scroll_top = self.scroll_top();
        let first = (scroll_top / slot).floor().max(0.0) as usize;
        let last = ((scroll_top + viewport_h) / slot).ceil().max(1.0) as usize;
        let first = first.min(count.saturating_sub(1));
        let last = last.clamp(first + 1, count);
        first..last
    }

    /// Ctrl+rueda sobre una página: zoom sin scroll (C1).
    fn on_page_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if !event.modifiers.control {
            return;
        }
        let delta = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / 40.0,
        };
        if delta == 0.0 {
            return;
        }
        let factor = if delta > 0.0 { 1.12 } else { 1.0 / 1.12 };
        self.zoom_by_factor(factor, cx);
        // Evita que el wheel haga scroll mientras se hace zoom.
        cx.stop_propagation();
    }

    /// Clic sobre una página: doble clic abre externo; Ctrl+clic = inverse.
    fn on_page_click(
        &mut self,
        page: u32,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let click_count = match event {
            ClickEvent::Mouse(mouse) => mouse.down.click_count,
            _ => 1,
        };
        let Some(position) = event.mouse_position() else {
            return;
        };

        if event.modifiers().control {
            if let Some((x_bp, y_bp)) = self.window_to_page_bp(page, position) {
                self.inverse_at(page, x_bp, y_bp, window, cx);
            }
            return;
        }
        if click_count >= 2 {
            self.open_external();
            cx.notify();
        }
    }

    /// Convierte un punto de ventana a bp de la página `page` (si cae dentro).
    fn window_to_page_bp(&self, page: u32, position: Point<Pixels>) -> Option<(f32, f32)> {
        if page >= self.page_count() {
            return None;
        }
        let (list_x, list_y) = self.viewport_origin;
        let (list_w, _list_h) = self.viewport?;
        let slot = self.slot_height();
        let (page_w, page_h) = self.page_size_px(page);
        let page_left = list_x + (list_w - page_w) / 2.0;
        let page_top = list_y + page as f32 * slot + (slot - page_h) / 2.0 - self.scroll_top();
        let px_x: f32 = position.x.into();
        let px_y: f32 = position.y.into();
        let local_x = px_x - page_left;
        let local_y = px_y - page_top;
        if local_x < 0.0 || local_y < 0.0 || local_x > page_w || local_y > page_h {
            return None;
        }
        Some((local_x / self.zoom, local_y / self.zoom))
    }

    /// Inverse SyncTeX asíncrono: resuelve `(page0, x_bp, y_bp)` a archivo:línea
    /// y dispara callback + [`PdfPanelEvent::InverseJump`].
    ///
    /// Lo usa el Ctrl+clic del visor; queda público para que el shell (o la demo)
    /// pueda dispararlo por otros medios (p. ej. "ir a la fuente" del menú).
    pub fn inverse_at(
        &mut self,
        page0: u32,
        x_bp: f32,
        y_bp: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(doc), Some(synctex)) = (self.doc.as_ref(), self.synctex.clone()) else {
            self.set_status("SyncTeX no configurado");
            cx.notify();
            return;
        };
        let Some(pdf) = Utf8Path::from_path(&doc.path).map(|p| p.to_path_buf()) else {
            self.set_status("ruta del PDF no UTF-8: SyncTeX no soportado en v1");
            cx.notify();
            return;
        };
        let page1 = page0 + 1;
        let t0 = Instant::now();

        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { synctex.inverse(&pdf, page1, x_bp, y_bp) })
                .await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(source) => {
                    let elapsed = t0.elapsed();
                    let jump = InverseJump::from(source);
                    this.set_status(format!(
                        "SyncTeX inverse → {}:{} ({:.0} ms)",
                        jump.file
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        jump.line,
                        elapsed.as_secs_f64() * 1000.0
                    ));
                    cx.emit(PdfPanelEvent::InverseJump(jump.clone()));
                    if let Some(callback) = &this.inverse_cb {
                        callback(jump, window, cx);
                    }
                    cx.notify();
                }
                Err(err) => {
                    this.set_status(format!("SyncTeX inverse: {err}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Abre el PDF en el visor externo (doble clic, M1-C1).
    pub fn open_external(&mut self) {
        let Some(path) = self.doc.as_ref().map(|doc| doc.path.clone()) else {
            return;
        };
        match open_external_path(&path) {
            Ok(()) => self.set_status("PDF abierto en el visor externo"),
            Err(err) => self.set_status(format!("no se pudo abrir el PDF externo: {err}")),
        }
    }

    /// Controles de zoom/modo de la toolbar.
    fn zoom_controls(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let zoom = self.zoom;
        let mode = self.zoom_mode;
        vec![
            Button::new("pdf-zoom-out")
                .icon(IconName::Minus)
                .tooltip("Alejar (Ctrl+rueda)")
                .on_click(cx.listener(|this, _, _, cx| this.zoom_out(cx)))
                .into_any_element(),
            Button::new("pdf-zoom-reset")
                .outline()
                .label(format!("{:.0}%", zoom * 100.0))
                .tooltip("100 %")
                .on_click(cx.listener(|this, _, _, cx| this.zoom_reset(cx)))
                .into_any_element(),
            Button::new("pdf-zoom-in")
                .icon(IconName::Plus)
                .tooltip("Acercar (Ctrl+rueda)")
                .on_click(cx.listener(|this, _, _, cx| this.zoom_in(cx)))
                .into_any_element(),
            Button::new("pdf-fit-width")
                .outline()
                .label("Ancho")
                .toggled(mode == ZoomMode::FitWidth)
                .tooltip("Ajustar al ancho")
                .on_click(cx.listener(|this, _, _, cx| this.set_zoom_mode(ZoomMode::FitWidth, cx)))
                .into_any_element(),
            Button::new("pdf-fit-page")
                .outline()
                .label("Página")
                .toggled(mode == ZoomMode::FitPage)
                .tooltip("Ajustar a la página")
                .on_click(cx.listener(|this, _, _, cx| this.set_zoom_mode(ZoomMode::FitPage, cx)))
                .into_any_element(),
        ]
    }

    /// Elemento de una página del list (virtualizado).
    fn render_page_item(
        &mut self,
        page: u32,
        slot: f32,
        zoom: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (page_w, page_h) = self.page_size_px(page);
        let image = self.image_for(page);
        let now = Instant::now();
        let marks: Vec<SyncRect> = self
            .sync_marks
            .iter()
            .filter(|mark| mark.until > now && mark.rect.page == page + 1)
            .map(|mark| mark.rect)
            .collect();
        let (muted, muted_foreground, accent) = {
            let theme = cx.theme();
            (theme.muted, theme.muted_foreground, theme.accent)
        };

        let mut body = div()
            .relative()
            .w(px(page_w))
            .h(px(page_h))
            .shadow_lg()
            .bg(white())
            .overflow_hidden();
        match image {
            Some(image) => {
                body = body.child(img(image).w(px(page_w)).h(px(page_h)));
            }
            None => {
                // Placeholder mientras llega el render progresivo.
                body = body.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(muted)
                        .text_xs()
                        .text_color(muted_foreground)
                        .child(format!("página {} · renderizando…", page + 1)),
                );
            }
        }
        for (mark_index, rect) in marks.iter().enumerate() {
            body = body.child(
                div()
                    .absolute()
                    .left(px(rect.x * zoom))
                    .top(px(rect.y * zoom))
                    .w(px((rect.w * zoom).max(4.0)))
                    .h(px((rect.h * zoom).max(4.0)))
                    .rounded(px(2.0))
                    .bg(accent.opacity(0.18))
                    .border_2()
                    .border_color(accent)
                    .with_animation(
                        // Id estable por (página, marca): varias páginas pueden
                        // tener resaltado a la vez sin colisión de estado.
                        ("pdf-sync-mark", page as usize * 8 + mark_index),
                        Animation::new(Duration::from_millis(700))
                            .repeat()
                            .with_easing(ease_out_quint()),
                        |el, progress| el.opacity(0.45 + 0.55 * (1.0 - progress)),
                    ),
            );
        }

        div()
            .id(("pdf-page", page as usize))
            .w_full()
            .h(px(slot))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.on_page_click(page, event, window, cx);
            }))
            .on_scroll_wheel(
                cx.listener(move |this, event: &ScrollWheelEvent, _window, cx| {
                    this.on_page_wheel(event, cx);
                }),
            )
            .child(body)
            .into_any_element()
    }

    /// Texto de la zona derecha de la toolbar (métricas + status).
    fn format_status(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some((w, h)) = self.viewport {
            parts.push(format!("vp {w:.0}×{h:.0}"));
        }
        parts.push(format!(
            "vis {}..{}",
            self.visible_range.start, self.visible_range.end
        ));
        parts.push(format!(
            "img {} · infl {} · sync {}",
            self.images.len(),
            self.inflight.len(),
            self.sync_marks.len()
        ));
        if let Some(report) = &self.bench_report {
            parts.push(report.clone());
        } else if let Some(ms) = self.last_render_ms {
            parts.push(format!("render {ms:.0} ms"));
        }
        if let Some(p95) = self.frame_ms_p95() {
            parts.push(format!("frame p95 {p95:.1} ms"));
        }
        if let Some(doc) = &self.doc {
            let stats = doc.renderer.cache_stats();
            parts.push(format!(
                "caché {:.0}/{:.0} MB · {} entradas",
                stats.bytes as f64 / (1024.0 * 1024.0),
                stats.max_bytes as f64 / (1024.0 * 1024.0),
                stats.entries
            ));
        }
        if let Some((status, until)) = &self.status
            && *until > Instant::now()
        {
            parts.push(status.clone());
        }
        if let Some(until) = self.updated_flash_until {
            parts.push(format!(
                "flash {:.1}s",
                until
                    .saturating_duration_since(Instant::now())
                    .as_secs_f32()
            ));
        }
        parts.join(" · ")
    }
}

/// `PageBitmap` (RGBA premultiplicado de hayro) → `RenderImage` (BGRA) de GPUI.
///
/// Hallazgo M0-S3: el `RenderImage` de GPUI 0.3.9 en Windows espera **BGRA** y,
/// como el render compone sobre blanco opaco (alpha 255), basta el swap R↔B.
fn bitmap_to_render_image(bitmap: &PageBitmap) -> RenderImage {
    let mut data = bitmap.rgba.clone();
    for pixel in data.as_chunks_mut::<4>().0.iter_mut() {
        pixel.swap(0, 2);
        let alpha = pixel[3] as u32;
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel = (((*channel as u32) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    let buffer = RgbaImage::from_raw(bitmap.width, bitmap.height, data)
        .expect("el bitmap debe tener width × height × 4 bytes");
    RenderImage::new(vec![Frame::new(buffer)])
}

/// Cuantiza el zoom a pasos de 0.5 % (estabilidad de las claves de caché).
fn quantize_zoom(zoom: f32) -> f32 {
    (zoom * 200.0).round() / 200.0
}

/// Zoom fit-width: la página más ancha cabe en el viewport.
fn fit_width_zoom(viewport_w: f32, page_w_pt: f32) -> f32 {
    if page_w_pt <= 0.0 || viewport_w <= 0.0 {
        return 1.0;
    }
    ((viewport_w - 2.0 * FIT_MARGIN_X) / page_w_pt).max(0.01)
}

/// Zoom fit-page: la página más alta (y ancha) cabe completa.
fn fit_page_zoom(viewport_w: f32, viewport_h: f32, page_w_pt: f32, page_h_pt: f32) -> f32 {
    if page_w_pt <= 0.0 || page_h_pt <= 0.0 {
        return 1.0;
    }
    fit_width_zoom(viewport_w, page_w_pt).min((viewport_h - PAGE_GAP) / page_h_pt)
}

/// Siguiente paso de zoom en la lista de niveles "bonitos".
fn next_zoom_step(current: f32, direction: i32) -> f32 {
    if direction > 0 {
        ZOOM_STEPS
            .iter()
            .copied()
            .find(|step| *step > current + 1e-4)
            .unwrap_or(ZOOM_MAX)
    } else {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|step| *step < current - 1e-4)
            .unwrap_or(ZOOM_MIN)
    }
}

/// Normaliza una ruta de forma léxica (quita `.` y resuelve `..` sin tocar el
/// disco). El CLI de SyncTeX compara `-i <src>` contra la ruta grabada en el
/// sidecar; si recibe `...\app\..\fixtures\x.tex` no coincide aunque el archivo
/// sea el mismo.
fn normalize_lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // No subir por encima de la raíz/relatividad existente.
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !path.is_absolute() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Abre `path` con el visor asociado del sistema (ShellExecute vía `cmd /C start`).
fn open_external_path(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
}

impl EventEmitter<PanelEvent> for PdfPanel {}

impl EventEmitter<PdfPanelEvent> for PdfPanel {}

impl BasePanel for PdfPanel {
    fn panel_name(&self) -> &'static str {
        "pdf"
    }

    fn visible(&self, _cx: &App) -> bool {
        true
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }
}

impl DockPanel for PdfPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("PDF".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "PDF"
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        Some(vec![
            Button::new("pdf-open-external")
                .icon(IconName::ExternalLink)
                .tooltip("Abrir en visor externo")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_external();
                    cx.notify();
                })),
        ])
    }
}

impl Focusable for PdfPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PdfPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Medición de frames (evidencia de fluidez, M1-C1).
        let now = Instant::now();
        if let Some(last) = self.last_frame_at.replace(now) {
            let ms = (now - last).as_secs_f64() * 1000.0;
            if ms < 1000.0 {
                self.frame_ms.push(ms);
                if self.frame_ms.len() > 240 {
                    self.frame_ms.remove(0);
                }
            }
        }
        if let Some(bench) = self.bench.as_mut() {
            let ms = (now - bench.last_frame).as_secs_f64() * 1000.0;
            if ms < 1000.0 {
                bench.frames_ms.push(ms);
            }
            bench.last_frame = now;
        }

        self.dpr = window.scale_factor();
        // Los modos fit se recalculan al vuelo (resize de ventana/panel).
        self.apply_fit_zoom();

        let (background, foreground, border, muted_foreground, accent, accent_foreground) = {
            let theme = cx.theme();
            (
                theme.background,
                theme.foreground,
                theme.border,
                theme.muted_foreground,
                theme.accent,
                theme.accent_foreground,
            )
        };

        let count = self.page_count();
        let slot = self.slot_height();
        let zoom = self.zoom;

        let content: AnyElement = if count == 0 {
            let message = if self.loading {
                "Cargando PDF…"
            } else if self.error.is_some() {
                "No se pudo abrir el PDF"
            } else {
                "Sin documento PDF"
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(muted_foreground)
                .child(message.to_string())
                .when_some(self.error.clone(), |el, error| {
                    el.child(div().text_xs().max_w(px(520.0)).child(error))
                })
                .into_any_element()
        } else {
            let list = uniform_list(
                "pdf-pages",
                count as usize,
                cx.processor(move |this, range: Range<usize>, _window, cx| {
                    this.on_visible_range(range.clone(), cx);
                    let mut items: Vec<AnyElement> = Vec::with_capacity(range.len());
                    for index in range {
                        items.push(this.render_page_item(index as u32, slot, zoom, cx));
                    }
                    items
                }),
            )
            .track_scroll(&self.scroll)
            .flex_1()
            .w_full();

            let canvas_probe = canvas(
                cx.processor(|this: &mut PdfPanel, bounds: Bounds<Pixels>, _window, cx| {
                    this.on_viewport(bounds, cx);
                }),
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full();

            div()
                .relative()
                .size_full()
                .flex()
                .flex_col()
                .child(list)
                .child(canvas_probe)
                .into_any_element()
        };

        let toolbar = self.show_toolbar.then(|| {
            let mut controls = self.zoom_controls(cx);
            let page_label = format!("pág {} / {}", self.current_page.min(count.max(1)), count);
            let mut bar = h_flex()
                .flex_shrink_0()
                .h(px(40.0))
                .px_3()
                .gap_2()
                .border_b_1()
                .border_color(border)
                .child(div().text_sm().child(page_label));
            for control in controls.drain(..) {
                bar = bar.child(control);
            }
            bar.child(div().flex_1()).child(
                div()
                    .text_xs()
                    .text_color(muted_foreground)
                    .child(self.format_status()),
            )
        });

        let mut root = v_flex()
            .relative()
            .size_full()
            .bg(background)
            .text_color(foreground)
            .track_focus(&self.focus);
        if let Some(toolbar) = toolbar {
            root = root.child(toolbar);
        }
        let mut root = root.child(content);

        // Flash "PDF actualizado" (C3).
        if let Some(until) = self.updated_flash_until
            && until > now
        {
            let remaining = until.saturating_duration_since(now).as_secs_f32();
            root = root.child(
                div()
                    .absolute()
                    .top(px(52.0))
                    .right(px(18.0))
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .bg(accent)
                    .text_color(accent_foreground)
                    .text_xs()
                    .opacity((remaining / UPDATED_FLASH_TTL.as_secs_f32()).clamp(0.0, 1.0))
                    .child("PDF actualizado"),
            );
        }

        root
    }
}

#[cfg(test)]
mod tests {
    // Sin glob: `gpui_kit::*` exporta el macro `test` de GPUI y sombrearía el
    // `#[test]` estándar en este módulo.
    use super::{
        PAGE_GAP, SyncRect, ZOOM_MAX, ZOOM_MIN, ZoomMode, fit_page_zoom, fit_width_zoom,
        next_zoom_step, normalize_lexical, quantize_zoom, source_point_at,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn source_point_at_top_mapea_scroll_a_pagina_y_bp() {
        // 2 páginas de 100×200 px, slots de 216 px (200 + PAGE_GAP), zoom 1.
        let size = |_page: u32| (100.0, 200.0);
        let slot = 200.0 + PAGE_GAP;
        // Arriba del todo: primera página, y = 8 px (centrado del slot) → bp.
        let (page, x, y) = source_point_at(0.0, slot, 2, size, 1.0).unwrap();
        assert_eq!(page, 0);
        assert!((x - 50.0).abs() < 1e-4, "x centrado: {x}");
        assert!(y.abs() < 1e-4, "y recortado a 0 dentro del margen: {y}");
        // Mitad del slot de la 2ª página.
        let middle_page2 = slot + 8.0 + 100.0;
        let (page, _x, y) = source_point_at(middle_page2, slot, 2, size, 1.0).unwrap();
        assert_eq!(page, 1);
        assert!((y - 100.0).abs() < 1e-4, "y en px = bp con zoom 1: {y}");
        // Con zoom 2 los px lógicos doblan y los bp vuelven a su escala.
        let zoom2 = |_page: u32| (200.0, 400.0);
        let (page, _x, y) = source_point_at(108.0, 416.0, 2, zoom2, 2.0).unwrap();
        assert_eq!(page, 0);
        assert!((y - 50.0).abs() < 1e-4, "y en bp con zoom 2: {y}");
        // Más allá del final: se recorta a la última página.
        let (page, _x, y) = source_point_at(slot * 9.0, slot, 2, size, 1.0).unwrap();
        assert_eq!(page, 1);
        assert!(y <= 200.0);
        // Sin páginas o sin layout no hay punto.
        assert!(source_point_at(0.0, slot, 0, size, 1.0).is_none());
        assert!(source_point_at(0.0, 0.0, 2, size, 1.0).is_none());
    }

    #[test]
    fn fit_width_y_fit_page() {
        // A4 595×842 bp en un viewport de 800×600 px lógicos.
        let zoom = fit_width_zoom(800.0, 595.0);
        assert!((zoom - (800.0 - 24.0) / 595.0).abs() < 1e-5);
        let fit_page = fit_page_zoom(800.0, 600.0, 595.0, 842.0);
        assert!(fit_page <= zoom, "fit-page nunca mayor que fit-width");
        assert!((fit_page - (600.0 - PAGE_GAP) / 842.0).abs() < 1e-5);
    }

    #[test]
    fn pasos_de_zoom_respetan_limites() {
        assert_eq!(next_zoom_step(1.0, 1), 1.25);
        assert_eq!(next_zoom_step(1.0, -1), 0.75);
        assert_eq!(next_zoom_step(4.0, 1), ZOOM_MAX);
        assert_eq!(next_zoom_step(0.5, -1), ZOOM_MIN);
        assert_eq!(quantize_zoom(1.2345), 1.235);
    }

    #[test]
    fn zoom_mode_as_setting_roundtrip() {
        for mode in [ZoomMode::FitWidth, ZoomMode::FitPage, ZoomMode::Manual] {
            assert_eq!(ZoomMode::from_setting(mode.as_setting()), mode);
        }
        assert_eq!(ZoomMode::from_setting("otra"), ZoomMode::FitWidth);
    }

    #[test]
    fn normalize_lexical_quita_puntos() {
        assert_eq!(
            normalize_lexical(Path::new(r"C:\a\b\..\c\main.tex")),
            PathBuf::from(r"C:\a\c\main.tex")
        );
        assert_eq!(
            normalize_lexical(Path::new("rel/./x.tex")),
            PathBuf::from("rel/x.tex")
        );
        assert_eq!(
            normalize_lexical(Path::new("../rel/x.tex")),
            PathBuf::from("../rel/x.tex")
        );
    }

    #[test]
    fn sync_rect_desde_registro_usa_la_caja() {
        let record = lsx_latex::SyncRecord {
            page: 1,
            x: 46.799_55,
            y: 311.051_82,
            h: 46.799_55,
            v: 314.638_4,
            width: 523.800_6,
            height: 11.955_17,
        };
        let rect = SyncRect::from_record(&record);
        assert_eq!(rect.page, 1);
        assert!((rect.x - 46.799_55).abs() < 1e-3);
        assert!((rect.w - 523.800_6).abs() < 1e-3);
        assert!((rect.h - 11.955_17).abs() < 1e-3);
        // y = v − H (borde superior de la caja).
        assert!((rect.y - (314.638_4 - 11.955_17)).abs() < 1e-3);
    }
}
