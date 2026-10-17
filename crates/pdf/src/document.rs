//! `PdfDocument` — apertura, medidas y render de páginas con hayro 0.8.
//!
//! Hallazgos del spike M0-S3 (API real de hayro 0.8, no la esperada en el plan):
//! - `hayro::hayro_syntax::Pdf::new(bytes: impl Into<PdfData>)`; las páginas se
//!   obtienen con `pdf.pages()` (deref a `[Page]`).
//! - El render de una página es la función libre
//!   `hayro::render(page, &cache, &settings, &render_settings, &pixmap_settings) -> Pixmap`
//!   (no un método del documento).
//! - `Pixmap` (de `vello_cpu`) usa **RGBA8 premultiplicado**; `data_as_u8_slice()`
//!   devuelve los bytes en orden `[r, g, b, a]` y filas de arriba-abajo.
//! - `RenderCache<'a>` no puede guardarse junto al `Pdf` (su `InterpreterCache`
//!   es invariante en `'a` vía `RefCell<Font<'a>>`): `render()` crea uno por
//!   llamada y `render_with_cache()` permite que el visor reutilice uno externo.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::Pixmap;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

/// Errores de dominio de `lsx-pdf`.
///
/// `LoadPdfError` de hayro no implementa `Display`/`Error`, por eso se guarda
/// tal cual y se muestra con `Debug` (código LSX-PDF en tracing a partir de M1).
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    /// El archivo no es un PDF válido o está cifrado.
    #[error("no se pudo abrir el PDF: {0:?}")]
    Open(hayro::hayro_syntax::LoadPdfError),
    /// Página fuera de rango (0-based).
    #[error("página {page} fuera de rango (el documento tiene {count})")]
    PageOutOfRange {
        /// Índice solicitado (0-based).
        page: u32,
        /// Número de páginas del documento.
        count: u32,
    },
    /// Escala de render no válida.
    #[error("escala de render inválida: {scale} (debe ser finita y > 0)")]
    InvalidScale {
        /// Escala solicitada.
        scale: f32,
    },
}

/// Una página renderizada a RGBA8.
///
/// `scale` es px de bitmap por bp de PDF (`zoom × dpr`); con ella, la conversión
/// bp ↔ px es una sola multiplicación (ver [`crate::coords`]).
#[derive(Debug, Clone)]
pub struct PageBitmap {
    /// Ancho en píxeles del bitmap.
    pub width: u32,
    /// Alto en píxeles del bitmap.
    pub height: u32,
    /// Escala px/bp usada en el render.
    pub scale: f32,
    /// Píxeles en orden RGBA8, fila a fila, origen arriba-izquierda.
    ///
    /// hayro produce RGBA **premultiplicado**; como `render()` compone sobre
    /// fondo blanco opaco, cada alpha es 255 y premultiplicado ≡ straight.
    pub rgba: Vec<u8>,
}

/// Documento PDF abierto (metadatos + páginas de hayro).
pub struct PdfDocument {
    pdf: Pdf,
}

impl PdfDocument {
    /// Abre un PDF desde memoria. I/O y llamada pesada: usar `spawn_blocking`.
    pub fn open(bytes: Vec<u8>) -> Result<Self, PdfError> {
        let pdf = Pdf::new(bytes).map_err(PdfError::Open)?;
        Ok(Self { pdf })
    }

    /// Número de páginas del documento.
    pub fn page_count(&self) -> u32 {
        // `Pages` hace deref a `[Page]`; el índice real cabe en u32.
        self.pdf.pages().len() as u32
    }

    /// Tamaño de página en bp (puntos PDF, 1/72"), ya con la rotación aplicada
    /// (coincide con `ancho/render().width` a cualquier escala).
    pub fn page_size_pt(&self, page: u32) -> Result<(f32, f32), PdfError> {
        Ok(self.page_ref(page)?.render_dimensions())
    }

    /// Renderiza una página a `scale` px/bp (scale = zoom × dpr).
    ///
    /// Crea un `RenderCache` por llamada; para renders repetidos del mismo
    /// documento usa [`PdfDocument::render_with_cache`].
    pub fn render(&self, page: u32, scale: f32) -> Result<PageBitmap, PdfError> {
        let cache = RenderCache::new();
        // El cache local se unifica con el préstamo del documento en la llamada.
        self.render_inner(page, scale, &cache)
    }

    /// Como [`PdfDocument::render`] pero reutilizando un `RenderCache` externo.
    ///
    /// hayro 0.8 no permite almacenar el cache dentro de esta struct (ver nota
    /// del módulo); el llamador debe mantenerlo vivo en el mismo ámbito que el
    /// préstamo del documento, p. ej. el visor en M1-C2.
    pub fn render_with_cache<'doc>(
        &'doc self,
        page: u32,
        scale: f32,
        cache: &RenderCache<'doc>,
    ) -> Result<PageBitmap, PdfError> {
        self.render_inner(page, scale, cache)
    }

    fn render_inner<'doc>(
        &'doc self,
        page: u32,
        scale: f32,
        cache: &RenderCache<'doc>,
    ) -> Result<PageBitmap, PdfError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(PdfError::InvalidScale { scale });
        }

        let page_ref = self.page_ref(page)?;
        let pixmap = hayro::render(
            page_ref,
            cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings {
                x_scale: scale,
                y_scale: scale,
                // Página como "papel": fondo blanco opaco (lo que espera un visor).
                bg_color: WHITE,
            },
        );
        Ok(pixmap_to_bitmap(pixmap, scale))
    }

    /// Página 0-based, con error tipado si el índice no existe.
    fn page_ref(&self, page: u32) -> Result<&hayro::hayro_syntax::page::Page<'_>, PdfError> {
        let count = self.page_count();
        self.pdf
            .pages()
            .get(page as usize)
            .ok_or(PdfError::PageOutOfRange { page, count })
    }
}

/// Convierte el `Pixmap` de hayro a nuestro `PageBitmap` (copia de bytes).
fn pixmap_to_bitmap(pixmap: Pixmap, scale: f32) -> PageBitmap {
    let width = u32::from(pixmap.width());
    let height = u32::from(pixmap.height());
    let rgba = pixmap.data_as_u8_slice().to_vec();
    debug_assert_eq!(rgba.len(), (width as usize) * (height as usize) * 4);
    PageBitmap {
        width,
        height,
        scale,
        rgba,
    }
}
