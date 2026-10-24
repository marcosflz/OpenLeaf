//! Backups de recuperación (M1-B5).
//!
//! Mientras un documento tiene cambios sin guardar, el panel escribe copias en
//! `config/backups/` (intervalo configurable). Si la app muere con cambios
//! sucios, al reabrir se propone recuperar el backup **más nuevo que el
//! archivo en disco**.
//!
//! Formato de nombre (un archivo por versión, ordenable por timestamp):
//!
//! ```text
//! <clave-saneada>-<hash64>.0000001234567.bak
//! ```
//!
//! - `clave-saneada`: ruta relativa con `/\:*?"<>|` → `_` (nombres válidos en
//!   Windows), recortada a 80 caracteres;
//! - `hash64`: hash estable de la clave completa (evita colisiones entre
//!   `a/b.tex` y `a__b.tex`);
//! - milisegundos desde epoch.
//!
//! [`.gitignore`](../../../.gitignore) ya ignora `/config/backups/`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::document::rel_key;
use crate::session::now_millis;

/// Sufijo de los backups.
const BACKUP_EXT: &str = "bak";
/// Claves largas se recortan (límite de nombre de archivo en Windows).
const KEY_MAX: usize = 80;
/// Backups conservados por archivo; los más viejos se podan al escribir.
pub const BACKUPS_PER_FILE: usize = 5;
/// Edad máxima de un backup (los más viejos dejan de proponerse).
pub const MAX_BACKUP_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// Un backup en disco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupEntry {
    /// Ruta completa del `.bak`.
    pub path: PathBuf,
    /// Clave (ruta relativa con `/`) del archivo de origen.
    pub source: String,
    /// Momento de creación (del nombre, no del filesystem).
    pub created: SystemTime,
}

/// Almacén de backups en `config/backups/`.
#[derive(Debug, Clone)]
pub struct BackupStore {
    dir: PathBuf,
}

impl BackupStore {
    /// `dir` suele ser `<raíz portable>/config/backups`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Directorio del almacén.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Escribe un backup del contenido actual. Devuelve su ruta.
    ///
    /// Poda los backups viejos del mismo archivo (conserva
    /// [`BACKUPS_PER_FILE`] versiones).
    pub fn write(&self, rel: &Path, text: &str) -> io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let key = rel_key(rel);
        let millis = now_millis();
        let name = format!("{}.{millis:013}.{BACKUP_EXT}", file_stem(&key));
        let path = self.dir.join(name);
        std::fs::write(&path, text)?;
        self.prune(&key, BACKUPS_PER_FILE);
        Ok(path)
    }

    /// Backup más reciente de `rel`, sin filtro de antigüedad.
    pub fn latest_for(&self, rel: &Path) -> Option<BackupEntry> {
        self.entries_for(rel)
            .into_iter()
            .max_by_key(|entry| entry.created)
    }

    /// Backup más reciente **más nuevo que** `newer_than` (p. ej. la fecha de
    /// modificación del archivo en disco) y con edad menor que
    /// [`MAX_BACKUP_AGE`]. Es la propuesta de recuperación al abrir.
    pub fn recovery_candidate(&self, rel: &Path, newer_than: SystemTime) -> Option<BackupEntry> {
        let now = SystemTime::now();
        self.entries_for(rel)
            .into_iter()
            .filter(|entry| entry.created > newer_than)
            .filter(|entry| {
                now.duration_since(entry.created)
                    .map(|age| age <= MAX_BACKUP_AGE)
                    .unwrap_or(true)
            })
            .max_by_key(|entry| entry.created)
    }

    /// Todos los backups de `rel`, de más nuevo a más viejo.
    pub fn entries_for(&self, rel: &Path) -> Vec<BackupEntry> {
        let key = rel_key(rel);
        let stem = file_stem(&key);
        let prefix = format!("{stem}.");
        let Ok(read) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<BackupEntry> = read
            .filter_map(|item| item.ok())
            .filter_map(|item| {
                let name = item.file_name().to_string_lossy().into_owned();
                parse_name(&name, &prefix).map(|created| BackupEntry {
                    path: item.path(),
                    source: key.clone(),
                    created,
                })
            })
            .collect();
        out.sort_by_key(|entry| std::cmp::Reverse(entry.created));
        out
    }

    /// Contenido de un backup.
    pub fn read(&self, entry: &BackupEntry) -> io::Result<String> {
        std::fs::read_to_string(&entry.path)
    }

    /// Borra todos los backups de `rel` (tras guardar ya no hacen falta).
    pub fn remove_for(&self, rel: &Path) -> io::Result<usize> {
        let mut removed = 0usize;
        for entry in self.entries_for(rel) {
            std::fs::remove_file(&entry.path)?;
            removed += 1;
        }
        Ok(removed)
    }

    /// Conserva los `keep` backups más nuevos de `key`.
    fn prune(&self, key: &str, keep: usize) {
        let stem = file_stem(key);
        let Ok(read) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let prefix = format!("{stem}.");
        let mut found: Vec<(u128, PathBuf)> = read
            .filter_map(|item| item.ok())
            .filter_map(|item| {
                let name = item.file_name().to_string_lossy().into_owned();
                parse_name(&name, &prefix).map(|created| (to_millis(created), item.path()))
            })
            .collect();
        if found.len() <= keep {
            return;
        }
        found.sort_by_key(|(millis, _)| std::cmp::Reverse(*millis));
        for (_, path) in found.into_iter().skip(keep) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Clave de archivo: `<ruta saneada>-<hash64>`.
fn file_stem(key: &str) -> String {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    let hash = hasher.finish();
    let sanitized: String = key
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let sanitized: String = sanitized.chars().take(KEY_MAX).collect();
    let sanitized = sanitized.trim_end_matches(['.', ' ']);
    let sanitized = if sanitized.is_empty() {
        "backup"
    } else {
        sanitized
    };
    format!("{sanitized}-{hash:016x}")
}

/// `true` si `name` es `<prefix><millis>.bak`; devuelve el instante.
fn parse_name(name: &str, prefix: &str) -> Option<SystemTime> {
    let rest = name.strip_prefix(prefix)?;
    let millis = rest.strip_suffix(&format!(".{BACKUP_EXT}"))?;
    let millis: u128 = millis.parse().ok()?;
    Some(UNIX_EPOCH + Duration::from_millis(millis as u64))
}

fn to_millis(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "lsx-editor-backups-{tag}-{}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Windows entrega el reloj a ~15.6 ms: sin este margen dos escrituras
    /// caen en el mismo milisegundo y comparten nombre.
    fn wait_tick() {
        std::thread::sleep(Duration::from_millis(25));
    }

    #[test]
    fn escribe_lee_y_encuentra_el_mas_nuevo() {
        let dir = TempDir::new("latest");
        let store = BackupStore::new(&dir.0);
        let rel = Path::new("capitulos").join("intro.tex");

        let first = store.write(&rel, "v1").unwrap();
        wait_tick();
        let second = store.write(&rel, "v2").unwrap();
        assert_ne!(first, second);

        let latest = store.latest_for(&rel).expect("hay backups");
        assert_eq!(store.read(&latest).unwrap(), "v2");
        assert_eq!(latest.source, "capitulos/intro.tex");
        assert_eq!(store.entries_for(&rel).len(), 2);
    }

    #[test]
    fn poda_a_cinco_versiones_por_archivo() {
        let dir = TempDir::new("prune");
        let store = BackupStore::new(&dir.0);
        for i in 0..8 {
            store
                .write(Path::new("main.tex"), &format!("v{i}"))
                .unwrap();
            wait_tick();
        }
        assert_eq!(
            store.entries_for(Path::new("main.tex")).len(),
            BACKUPS_PER_FILE
        );
    }

    #[test]
    fn archivos_distintos_no_se_pisan() {
        let dir = TempDir::new("distintos");
        let store = BackupStore::new(&dir.0);
        store.write(Path::new("a/b.tex"), "uno").unwrap();
        store.write(Path::new("a__b.tex"), "dos").unwrap();
        assert_eq!(store.entries_for(Path::new("a/b.tex")).len(), 1);
        assert_eq!(store.entries_for(Path::new("a__b.tex")).len(), 1);
        let first = store.latest_for(Path::new("a/b.tex")).unwrap();
        let second = store.latest_for(Path::new("a__b.tex")).unwrap();
        assert_ne!(first.path, second.path);
    }

    #[test]
    fn candidato_de_recuperacion_solo_si_es_mas_nuevo() {
        let dir = TempDir::new("recovery");
        let store = BackupStore::new(&dir.0);
        let rel = Path::new("main.tex");
        store.write(rel, "sin guardar").unwrap();

        // El archivo en disco es anterior al backup → hay candidato.
        let disk_mtime = SystemTime::now() - Duration::from_secs(60);
        assert!(store.recovery_candidate(rel, disk_mtime).is_some());

        // El archivo en disco es más nuevo que el backup → no se propone.
        wait_tick();
        let disk_mtime = SystemTime::now() + Duration::from_secs(5);
        assert!(store.recovery_candidate(rel, disk_mtime).is_none());

        // Sin backups → None.
        assert!(
            store
                .recovery_candidate(Path::new("otro.tex"), UNIX_EPOCH)
                .is_none()
        );
    }

    #[test]
    fn borrar_tras_guardar_deja_el_archivo_limpio() {
        let dir = TempDir::new("remove");
        let store = BackupStore::new(&dir.0);
        let rel = Path::new("main.tex");
        store.write(rel, "a").unwrap();
        // Los nombres de backup llevan timestamp con resolución de segundos:
        // sin esperar, la segunda escritura puede pisar a la primera y el test
        // se vuelve intermitente (visto en el gate 0.2).
        wait_tick();
        store.write(rel, "b").unwrap();
        assert_eq!(store.remove_for(rel).unwrap(), 2);
        assert!(store.entries_for(rel).is_empty());
        assert!(store.remove_for(rel).unwrap() == 0);
    }

    #[test]
    fn nombres_sanos_para_windows() {
        let dir = TempDir::new("nombres");
        let store = BackupStore::new(&dir.0);
        store
            .write(Path::new("secciones/¿qué: pasa?.tex"), "x")
            .unwrap();
        let entry = store
            .latest_for(Path::new("secciones/¿qué: pasa?.tex"))
            .unwrap();
        let name = entry
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(!name.contains([':', '?', '\\', '<', '>', '|', '"', '*']));
        assert!(name.ends_with(".bak"));
    }
}
