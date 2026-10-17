//! Restauración de sesión del editor (M1-B1).
//!
//! Guarda la lista de pestañas y el cursor de cada una en un JSON propio bajo
//! `config/`:
//!
//! ```text
//! config/editor-session.json
//! ```
//!
//! **Pendiente de migración**: `lsx-core` todavía no tiene el `Settings` de
//! `INTERFACES.md` §7 (`docs/board.md` P0: `lsx-core` es stub de `fs`); cuando
//! exista, este JSON debe pasar a una clave de `settings.json` (p. ej.
//! `ui.editor.session`) y este módulo quedará como lectura de compatibilidad.
//! El archivo debe añadirse a `.gitignore` (`/config/editor-session.json`) —
//! territorio del orchestrator.
//!
//! Escritura atómica (temp + rename) y tolerante: un JSON corrupto o ausente
//! devuelve la sesión vacía con un aviso de `tracing`, nunca un error fatal.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Nombre del archivo de sesión dentro del directorio de configuración.
pub const SESSION_FILE_NAME: &str = "editor-session.json";

/// Versión del esquema del archivo de sesión.
pub const SESSION_SCHEMA_VERSION: u32 = 1;

/// Cursor persistido de una pestaña (0-based, como `Position` del kit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionCursor {
    pub line: u32,
    pub character: u32,
}

/// Una pestaña restaurable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTab {
    /// Ruta relativa a la raíz del proyecto, separadores `/`.
    pub path: String,
    #[serde(default)]
    pub cursor: SessionCursor,
}

/// Estado persistido del editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionState {
    #[serde(rename = "$schemaVersion")]
    pub schema_version: u32,
    pub tabs: Vec<SessionTab>,
    /// Índice de la pestaña activa (se recorta al abrir si ya no existe).
    #[serde(default)]
    pub active: usize,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION,
            tabs: Vec::new(),
            active: 0,
        }
    }
}

impl SessionState {
    /// Pestaña activa, si el índice es válido.
    pub fn active_tab(&self) -> Option<&SessionTab> {
        self.tabs
            .get(self.active.min(self.tabs.len().saturating_sub(1)))
    }

    /// Índice recortado a la lista actual de pestañas.
    pub fn effective_active(&self) -> usize {
        self.active.min(self.tabs.len().saturating_sub(1))
    }
}

/// Almacén del archivo de sesión (`<config_dir>/editor-session.json`).
#[derive(Debug, Clone)]
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    /// `config_dir` es el directorio `config/` de la raíz portable.
    pub fn new(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            path: config_dir.into().join(SESSION_FILE_NAME),
        }
    }

    /// Ruta del archivo (para logs/diagnóstico).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Lee la sesión; ausente o corrupta → [`SessionState::default`].
    pub fn load(&self) -> SessionState {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return SessionState::default(),
            Err(err) => {
                tracing::warn!(target: "lsx::editor::session", path = %self.path.display(), error = %err, "no se pudo leer la sesión");
                return SessionState::default();
            }
        };
        match serde_json::from_slice::<SessionState>(&bytes) {
            Ok(state) => state,
            Err(err) => {
                tracing::warn!(target: "lsx::editor::session", path = %self.path.display(), error = %err, "sesión corrupta; se ignora");
                SessionState::default()
            }
        }
    }

    /// Escribe la sesión de forma atómica (temp + rename en el mismo dir).
    pub fn save(&self, state: &SessionState) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(state)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)
    }

    /// Borra el archivo de sesión (privacidad / reset a mano).
    pub fn clear(&self) -> io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }
}

/// Marca temporal ordenable (milisegundos desde epoch) para backups.
pub(crate) fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
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
                "lsx-editor-session-{tag}-{}-{n}",
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

    #[test]
    fn ida_y_vuelta_con_pestanas_y_cursor() {
        let dir = TempDir::new("roundtrip");
        let store = SessionStore::new(dir.0.join("config"));
        assert_eq!(store.load(), SessionState::default(), "sin archivo → vacío");

        let state = SessionState {
            schema_version: SESSION_SCHEMA_VERSION,
            tabs: vec![
                SessionTab {
                    path: "main.tex".into(),
                    cursor: SessionCursor {
                        line: 3,
                        character: 7,
                    },
                },
                SessionTab {
                    path: "capitulos/intro.tex".into(),
                    cursor: SessionCursor::default(),
                },
            ],
            active: 1,
        };
        store.save(&state).unwrap();
        assert!(store.path().is_file());

        let loaded = store.load();
        assert_eq!(loaded, state);
        assert_eq!(loaded.active_tab().unwrap().path, "capitulos/intro.tex");
    }

    #[test]
    fn json_corrupto_o_ajeno_devuelve_sesion_vacia() {
        let dir = TempDir::new("corrupt");
        let store = SessionStore::new(&dir.0);
        std::fs::write(store.path(), b"{ esto no es json }").unwrap();
        assert_eq!(store.load(), SessionState::default());

        std::fs::write(store.path(), b"null").unwrap();
        assert_eq!(store.load(), SessionState::default());
    }

    #[test]
    fn campos_futuros_y_ausentes_se_toleran() {
        let dir = TempDir::new("forward");
        let store = SessionStore::new(&dir.0);
        std::fs::write(
            store.path(),
            br#"{"$schemaVersion": 99, "tabs": [{"path": "a.tex", "futuro": true}], "active": 0, "otra": 1}"#,
        )
        .unwrap();
        let loaded = store.load();
        assert_eq!(loaded.tabs.len(), 1);
        assert_eq!(loaded.tabs[0].cursor, SessionCursor::default());
    }

    #[test]
    fn active_se_recorta_a_la_lista() {
        let state = SessionState {
            schema_version: 1,
            tabs: vec![SessionTab {
                path: "a.tex".into(),
                cursor: SessionCursor::default(),
            }],
            active: 9,
        };
        assert_eq!(state.effective_active(), 0);
        assert!(SessionState::default().active_tab().is_none());
    }
}
