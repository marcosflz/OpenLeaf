//! Caché LRU de bitmaps de página, con presupuesto en bytes (M1-C2).
//!
//! Diseño (INTERFACES §4): clave `(generation, page, scale_bucket)`, presupuesto
//! por bytes (default 256 MB, configurable desde `pdf.maxCacheMB`), política LRU
//! con *lazy deletion*: al tocar una entrada se empuja una marca nueva; en la
//! evicción se descartan marcas obsoletas antes de desalojar de verdad. El
//! visor (`lsx-ui`) decide qué páginas entran (visibles + vecinas); esta caché
//! solo recorta por bytes.
//!
//! `scale_bucket` cuantiza la escala a milésimas para que `fit-width` (un f32
//! recalculado) produzca siempre la misma clave entre frames.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use crate::document::PageBitmap;

/// Presupuesto por defecto de la caché (settings `pdf.maxCacheMB`).
pub const DEFAULT_MAX_CACHE_MB: u32 = 256;

/// Clave de una página renderizada: generación de documento, página (0-based)
/// y escala cuantizada a milésimas (`zoom × dpr × 1000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// Generación del documento (se incrementa al recompilar; invalida todo).
    pub generation: u64,
    /// Página 0-based.
    pub page: u32,
    /// Escala px/bp en milésimas (ver [`scale_bucket`]).
    pub scale_milli: u32,
}

/// Cuantiza `scale` (px por bp) a milésimas; mínimo 1 para no colapsar a 0.
pub fn scale_bucket(scale: f32) -> u32 {
    (scale.max(0.0) * 1000.0).round().max(1.0) as u32
}

/// Convierte milésimas a escala f32 (la escala exacta con la que se renderiza).
pub fn bucket_scale(scale_milli: u32) -> f32 {
    (scale_milli.max(1) as f32) / 1000.0
}

/// Estadísticas de la caché (para el overlay de desarrollo y QA).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    /// Entradas vivas.
    pub entries: usize,
    /// Bytes ocupados por bitmaps.
    pub bytes: usize,
    /// Presupuesto máximo.
    pub max_bytes: usize,
    /// Aciertos (`get` con resultado).
    pub hits: u64,
    /// Fallos (`get` sin resultado).
    pub misses: u64,
    /// Entradas desalojadas por presión de memoria.
    pub evictions: u64,
}

struct Entry {
    bitmap: Arc<PageBitmap>,
    stamp: u64,
}

#[derive(Default)]
struct Inner {
    map: HashMap<CacheKey, Entry>,
    /// Cola de marcas `(clave, sello)`; el frente contiene lo menos usado.
    order: VecDeque<(CacheKey, u64)>,
    next_stamp: u64,
    bytes: usize,
    hits: u64,
    misses: u64,
    evictions: u64,
}

/// Caché LRU por bytes, segura para compartir entre hilos (render en background).
pub struct PdfCache {
    inner: Mutex<Inner>,
    max_bytes: usize,
}

impl Default for PdfCache {
    fn default() -> Self {
        Self::from_megabytes(DEFAULT_MAX_CACHE_MB)
    }
}

impl PdfCache {
    /// Caché con presupuesto explícito en bytes (0 = sin retención).
    pub fn new(max_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            max_bytes,
        }
    }

    /// Caché con presupuesto en MiB (settings `pdf.maxCacheMB`).
    pub fn from_megabytes(max_mb: u32) -> Self {
        Self::new(usize::try_from(max_mb).unwrap_or(256) * 1024 * 1024)
    }

    /// Presupuesto máximo en bytes.
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Busca `key` y la marca como la más reciente.
    pub fn get(&self, key: &CacheKey) -> Option<Arc<PageBitmap>> {
        let mut inner = self.lock();
        let stamp = inner.next_stamp;
        let entry = match inner.map.get_mut(key) {
            Some(entry) => entry,
            None => {
                inner.misses += 1;
                return None;
            }
        };
        entry.stamp = stamp;
        let bitmap = entry.bitmap.clone();
        inner.next_stamp += 1;
        inner.order.push_back((*key, stamp));
        inner.hits += 1;
        Some(bitmap)
    }

    /// ¿Está `key` en la caché, sin alterar su recencia?
    pub fn contains(&self, key: &CacheKey) -> bool {
        self.lock().map.contains_key(key)
    }

    /// Inserta (o reemplaza) el bitmap de `key`; desaloja hasta caber.
    ///
    /// Devuelve cuántas entradas se desalojaron.
    pub fn insert(&self, key: CacheKey, bitmap: Arc<PageBitmap>) -> usize {
        let size = bitmap_bytes(&bitmap);
        let mut evicted = 0;
        let mut inner = self.lock();

        if let Some(previous) = inner.map.remove(&key) {
            inner.bytes = inner.bytes.saturating_sub(bitmap_bytes(&previous.bitmap));
        }

        // Una entrada más grande que el presupuesto no se retiene (evita
        // desalojar todo para meter algo que nunca cabrá).
        if size > self.max_bytes {
            inner.evictions += 1;
            return 1;
        }

        let stamp = inner.next_stamp;
        inner.next_stamp += 1;
        inner.map.insert(key, Entry { bitmap, stamp });
        inner.order.push_back((key, stamp));
        inner.bytes += size;

        while inner.bytes > self.max_bytes {
            let Some((key, stamp)) = inner.order.pop_front() else {
                break;
            };
            match inner.map.get(&key) {
                // Marca obsoleta: la entrada se tocó después (sigue viva).
                Some(entry) if entry.stamp != stamp => continue,
                Some(_) => {
                    let entry = inner.map.remove(&key).expect("get comprobado");
                    inner.bytes = inner.bytes.saturating_sub(bitmap_bytes(&entry.bitmap));
                    inner.evictions += 1;
                    evicted += 1;
                }
                None => continue,
            }
        }
        evicted
    }

    /// Descarta todas las entradas de generaciones distintas a `keep`.
    ///
    /// Se llama al recargar un documento (`generation++`): la generación vieja
    /// no vuelve a pintarse y no debe ocupar presupuesto.
    pub fn retain_generation(&self, keep: u64) {
        let mut inner = self.lock();
        let stale: Vec<CacheKey> = inner
            .map
            .keys()
            .filter(|key| key.generation != keep)
            .copied()
            .collect();
        for key in stale {
            if let Some(entry) = inner.map.remove(&key) {
                inner.bytes = inner.bytes.saturating_sub(bitmap_bytes(&entry.bitmap));
            }
        }
        inner.order.retain(|(key, _)| key.generation == keep);
    }

    /// Vacía la caché completa.
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.map.clear();
        inner.order.clear();
        inner.bytes = 0;
    }

    /// Instantánea de estadísticas.
    pub fn stats(&self) -> CacheStats {
        let inner = self.lock();
        CacheStats {
            entries: inner.map.len(),
            bytes: inner.bytes,
            max_bytes: self.max_bytes,
            hits: inner.hits,
            misses: inner.misses,
            evictions: inner.evictions,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // El veneno solo puede venir de un panic con el lock tomado; la caché
        // no tiene invariantes que se puedan romper a medias, así que se sigue.
        self.inner.lock().unwrap_or_else(|err| err.into_inner())
    }
}

/// Bytes que ocupa un bitmap (RGBA8 = 4 bytes/px).
pub fn bitmap_bytes(bitmap: &PageBitmap) -> usize {
    bitmap.rgba.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(scale: f32, side: u32) -> Arc<PageBitmap> {
        Arc::new(PageBitmap {
            width: side,
            height: side,
            scale,
            rgba: vec![0; (side as usize) * (side as usize) * 4],
        })
    }

    fn key(page: u32, scale: f32) -> CacheKey {
        CacheKey {
            generation: 7,
            page,
            scale_milli: scale_bucket(scale),
        }
    }

    #[test]
    fn bucket_es_estable_y_roundtrip() {
        assert_eq!(scale_bucket(1.0), 1000);
        assert_eq!(scale_bucket(2.0), 2000);
        // f32 recomputado por fit-width: mismos milésimos.
        assert_eq!(scale_bucket(1.234_567_8), scale_bucket(1.234_567_9));
        assert!((bucket_scale(scale_bucket(1.5)) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn insert_y_get() {
        let cache = PdfCache::new(1 << 20);
        let k = key(0, 1.0);
        assert!(cache.get(&k).is_none());
        cache.insert(k, bitmap(1.0, 16));
        assert!(cache.get(&k).is_some());
        let stats = cache.stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn evicta_lo_menos_usado_por_bytes() {
        // Presupuesto para 2 bitmaps de 16×16 RGBA (1 KiB cada uno).
        let cache = PdfCache::new(2 * 16 * 16 * 4);
        let (k0, k1, k2) = (key(0, 1.0), key(1, 1.0), key(2, 1.0));
        cache.insert(k0, bitmap(1.0, 16));
        cache.insert(k1, bitmap(1.0, 16));
        // k0 se usa → k1 es el menos reciente.
        assert!(cache.get(&k0).is_some());
        cache.insert(k2, bitmap(1.0, 16));

        assert!(cache.contains(&k0), "k0 reciente debe sobrevivir");
        assert!(!cache.contains(&k1), "k1 (LRU) debe desalojarse");
        assert!(cache.contains(&k2));
        let stats = cache.stats();
        assert_eq!(stats.evictions, 1);
        assert!(stats.bytes <= stats.max_bytes);
    }

    #[test]
    fn bitmap_mas_grande_que_presupuesto_no_retiene() {
        let cache = PdfCache::new(1024);
        let k = key(0, 1.0);
        cache.insert(k, bitmap(1.0, 64)); // 16 KiB
        assert!(!cache.contains(&k));
        assert_eq!(cache.stats().bytes, 0);
    }

    #[test]
    fn retain_generation_descarta_viejas() {
        let cache = PdfCache::new(1 << 20);
        cache.insert(
            CacheKey {
                generation: 1,
                page: 0,
                scale_milli: 1000,
            },
            bitmap(1.0, 8),
        );
        let fresh = CacheKey {
            generation: 2,
            page: 0,
            scale_milli: 1000,
        };
        cache.insert(fresh, bitmap(1.0, 8));
        cache.retain_generation(2);
        assert_eq!(cache.stats().entries, 1);
        assert!(cache.contains(&fresh));
    }

    #[test]
    fn reemplazo_no_duplica_bytes() {
        let cache = PdfCache::new(1 << 20);
        let k = key(0, 1.0);
        cache.insert(k, bitmap(1.0, 16));
        cache.insert(k, bitmap(1.0, 16));
        assert_eq!(cache.stats().entries, 1);
        assert_eq!(cache.stats().bytes, 16 * 16 * 4);
    }
}
