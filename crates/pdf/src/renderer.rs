//! `PdfRenderer` — documento + caché LRU, compartible entre hilos de render.
//!
//! El visor (`lsx-ui::panels::pdf`) lo envuelve en `Arc` y lo mueve a
//! `background_spawn`: `render_page` busca en la caché, y si falla renderiza con
//! hayro y guarda el bitmap. La generación forma parte de la clave, de modo que
//! recargar el documento con `generation++` invalida lo viejo sin bloquear.

use std::sync::Arc;

use crate::cache::{CacheKey, CacheStats, PdfCache, bucket_scale, scale_bucket};
use crate::document::{PageBitmap, PdfDocument, PdfError};

/// Documento abierto más caché de páginas renderizadas.
pub struct PdfRenderer {
    doc: PdfDocument,
    page_sizes_pt: Vec<(f32, f32)>,
    generation: u64,
    cache: Arc<PdfCache>,
}

impl PdfRenderer {
    /// Abre `bytes` como PDF con una caché propia (bytes de presupuesto).
    pub fn open(bytes: Vec<u8>, generation: u64, max_cache_bytes: usize) -> Result<Self, PdfError> {
        Self::open_with_cache(bytes, generation, Arc::new(PdfCache::new(max_cache_bytes)))
    }

    /// Como [`PdfRenderer::open`] con una caché compartida (tests, settings).
    pub fn open_with_cache(
        bytes: Vec<u8>,
        generation: u64,
        cache: Arc<PdfCache>,
    ) -> Result<Self, PdfError> {
        let doc = PdfDocument::open(bytes)?;
        let count = doc.page_count();
        let mut page_sizes_pt = Vec::with_capacity(count as usize);
        for page in 0..count {
            page_sizes_pt.push(doc.page_size_pt(page)?);
        }
        Ok(Self {
            doc,
            page_sizes_pt,
            generation,
            cache,
        })
    }

    /// Páginas del documento.
    pub fn page_count(&self) -> u32 {
        self.doc.page_count()
    }

    /// Tamaño en bp de cada página (rotación aplicada).
    pub fn page_sizes_pt(&self) -> &[(f32, f32)] {
        &self.page_sizes_pt
    }

    /// Tamaño en bp de `page` (0-based).
    pub fn page_size_pt(&self, page: u32) -> Result<(f32, f32), PdfError> {
        self.page_sizes_pt
            .get(page as usize)
            .copied()
            .ok_or(PdfError::PageOutOfRange {
                page,
                count: self.doc.page_count(),
            })
    }

    /// Página más alta del documento en bp (para la altura uniforme de la lista).
    pub fn max_page_height_pt(&self) -> f32 {
        self.page_sizes_pt
            .iter()
            .map(|(_, h)| *h)
            .fold(0.0_f32, f32::max)
    }

    /// Página más ancha del documento en bp.
    pub fn max_page_width_pt(&self) -> f32 {
        self.page_sizes_pt
            .iter()
            .map(|(w, _)| *w)
            .fold(0.0_f32, f32::max)
    }

    /// Generación del documento (contenido).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Renderiza `page` a `scale` (px/bp), cacheado por `(generación, página,
    /// escala cuantizada)` (INTERFACES §4).
    pub fn render_page(&self, page: u32, scale: f32) -> Result<Arc<PageBitmap>, PdfError> {
        let key = CacheKey {
            generation: self.generation,
            page,
            scale_milli: scale_bucket(scale),
        };
        if let Some(bitmap) = self.cache.get(&key) {
            return Ok(bitmap);
        }
        let bucket = bucket_scale(key.scale_milli);
        let bitmap = Arc::new(self.doc.render(page, bucket)?);
        self.cache.insert(key, bitmap.clone());
        Ok(bitmap)
    }

    /// ¿Está la página cacheada a esa escala (sin tocar la LRU)?
    pub fn is_cached(&self, page: u32, scale: f32) -> bool {
        self.cache.contains(&CacheKey {
            generation: self.generation,
            page,
            scale_milli: scale_bucket(scale),
        })
    }

    /// Descarta las generaciones anteriores (tras recargar el documento).
    pub fn retain_generation(&self) {
        self.cache.retain_generation(self.generation);
    }

    /// Estadísticas de la caché (QA / overlay dev).
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }

    /// Caché compartida (tests, inspección).
    pub fn cache(&self) -> &PdfCache {
        &self.cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn multipage_bytes() -> Vec<u8> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf/multipage.pdf");
        std::fs::read(path).expect("fixtures/pdf/multipage.pdf debe existir")
    }

    #[test]
    fn abre_multipage_y_cachea() {
        let renderer = PdfRenderer::open(multipage_bytes(), 1, 64 * 1024 * 1024).unwrap();
        assert_eq!(renderer.page_count(), 30);
        assert_eq!(renderer.page_sizes_pt().len(), 30);

        let first = renderer.render_page(5, 1.0).unwrap();
        assert!(renderer.is_cached(5, 1.0));
        let again = renderer.render_page(5, 1.0).unwrap();
        assert!(
            Arc::ptr_eq(&first, &again),
            "segundo render debe venir de caché"
        );
        let stats = renderer.cache_stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn generacion_distinta_no_reutiliza_cache_y_se_purga() {
        let bytes = multipage_bytes();
        let cache = Arc::new(PdfCache::new(64 * 1024 * 1024));

        let old = PdfRenderer::open_with_cache(bytes.clone(), 1, cache.clone()).unwrap();
        old.render_page(0, 1.0).unwrap();
        assert_eq!(cache.stats().entries, 1);

        let new = PdfRenderer::open_with_cache(bytes, 2, cache.clone()).unwrap();
        // La entrada de la generación 1 no se reutiliza para la 2.
        assert!(!new.is_cached(0, 1.0));
        new.render_page(0, 1.0).unwrap();
        assert!(new.is_cached(0, 1.0));
        assert_eq!(cache.stats().entries, 2);

        // Tras la recarga, lo viejo se descarta.
        new.retain_generation();
        assert_eq!(cache.stats().entries, 1);
    }
}
