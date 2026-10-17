//! Modelo de documento del editor (M1-B1, INTERFACES §6).
//!
//! `EditorState` del kit es **un documento** sin path/dirty/save; el modelo de
//! archivo vive aquí, en `lsx-editor`, y el panel de `lsx-ui` lo compone con
//! una entidad `EditorState` por pestaña.
//!
//! Versión de documento (regla dura M1-B1): el dirty **no** se calcula
//! comparando textos (O(n) por tecla). Cada [`DocumentMeta`] lleva una
//! `version` que se incrementa con cada `InputEvent::Change` y una
//! `saved_version` que se sincroniza en cada guardado/recarga; el dirty es
//! comparar dos enteros.

use std::path::{Path, PathBuf};

use lsx_core::fs::Eol;

/// Identificador de documento abierto, estable durante la sesión.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocId(pub u64);

/// Metadatos de un documento abierto (contrato `Document` de §6, sin el
/// contenido: el buffer vive en `EditorState` del kit, I-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentMeta {
    pub id: DocId,
    /// Ruta relativa a la raíz del proyecto (rutas en código: `PathBuf`).
    pub rel: PathBuf,
    /// Se incrementa con cada edición del usuario.
    pub version: u64,
    /// Versión cuya copia está en disco.
    pub saved_version: u64,
    /// EOL detectado al leer (se preserva: el buffer ya lo lleva tal cual).
    pub eol: Eol,
    /// El archivo tenía BOM UTF-8 al leerlo.
    pub had_bom: bool,
}

impl DocumentMeta {
    pub fn new(id: DocId, rel: impl Into<PathBuf>, eol: Eol, had_bom: bool) -> Self {
        Self {
            id,
            rel: rel.into(),
            version: 0,
            saved_version: 0,
            eol,
            had_bom,
        }
    }

    /// `true` si hay ediciones posteriores al último guardado/recarga.
    pub fn is_dirty(&self) -> bool {
        self.version != self.saved_version
    }

    /// Registra una edición del usuario (`InputEvent::Change`).
    pub fn notify_edit(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    /// Marca el buffer como igual a disco (guardado o recarga).
    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }

    /// Nombre visible de la pestaña (último componente de la ruta).
    pub fn display_name(&self) -> String {
        self.rel
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.rel.display().to_string())
    }

    /// Ruta legible con separadores `/` (convención de la UI y de settings).
    pub fn display_path(&self) -> String {
        self.rel.to_string_lossy().replace('\\', "/")
    }
}

/// Configuración de autosave (M1-B5). Off por defecto; el intervalo llega de
/// `settings.json` (`editor.autosave`) cuando el `Settings` de `lsx-core`
/// exista.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutosaveConfig {
    pub enabled: bool,
    pub interval_secs: u64,
}

impl Default for AutosaveConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 30,
        }
    }
}

/// Genera ids de documento crecientes (`DocId(1), DocId(2), …`).
#[derive(Debug, Default)]
pub struct DocIdGenerator(u64);

impl DocIdGenerator {
    pub fn next_id(&mut self) -> DocId {
        self.0 = self.0.wrapping_add(1);
        DocId(self.0)
    }
}

/// Normaliza una ruta para mostrarla/guardarla con separadores `/`.
pub fn rel_key(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_es_comparacion_de_versiones_no_de_textos() {
        let mut meta = DocumentMeta::new(DocId(1), "main.tex", Eol::Lf, false);
        assert!(!meta.is_dirty());

        meta.notify_edit();
        assert!(meta.is_dirty());
        assert_eq!(meta.version, 1);
        assert_eq!(meta.saved_version, 0);

        // Más ediciones siguen siendo dirty (versión, no igualdad).
        meta.notify_edit();
        meta.notify_edit();
        assert!(meta.is_dirty());
        assert_eq!(meta.version, 3);

        meta.mark_saved();
        assert!(!meta.is_dirty());
        assert_eq!(meta.saved_version, 3);
    }

    #[test]
    fn editar_despues_de_guardar_vuelve_a_dirty() {
        let mut meta = DocumentMeta::new(DocId(7), "capitulos/uno.tex", Eol::CrLf, true);
        meta.notify_edit();
        meta.mark_saved();
        assert!(!meta.is_dirty());
        meta.notify_edit();
        assert!(meta.is_dirty());
    }

    #[test]
    fn display_name_y_path_normalizan_separadores() {
        let meta = DocumentMeta::new(
            DocId(1),
            Path::new("capitulos").join("int ro.tex"),
            Eol::Lf,
            false,
        );
        assert_eq!(meta.display_name(), "int ro.tex");
        assert_eq!(meta.display_path(), "capitulos/int ro.tex");
        assert_eq!(rel_key(Path::new("a\\b\\c.tex")), "a/b/c.tex");
    }

    #[test]
    fn generador_de_ids_es_creciente() {
        let mut ids = DocIdGenerator::default();
        assert_eq!(ids.next_id(), DocId(1));
        assert_eq!(ids.next_id(), DocId(2));
        assert_ne!(ids.next_id(), DocId(2));
    }
}
