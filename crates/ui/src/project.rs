//! Adaptador de UI del servicio de proyectos (M1-F1).
//!
//! La implementación definitiva vive en [`lsx_core::project`] (INTERFACES §2:
//! «ProjectService», abrir/cerrar, recientes y creación desde plantilla).
//! Este módulo es la fachada fina que consume el shell:
//!
//! - conserva la firma histórica [`ProjectService::remember`] de 3 argumentos
//!   (`settings`, `portable`, `path`) que usa `crate::app::AppRoot`;
//! - re-exporta los tipos del núcleo para que los paneles no importen
//!   `lsx-core` directamente por este motivo.
//!
//! La heurística de `main.tex`, el límite de recientes (10) y el andamiaje
//! del wizard «Nuevo proyecto» (M1-F4) están en el núcleo; aquí no hay lógica.

use std::path::Path;

use lsx_core::config::Settings;
use lsx_core::root::PortableRoot;

pub use lsx_core::project::{
    Project, ProjectError, ProjectInfo, RECENT_LIMIT, TemplateId, create_project, find_main_tex,
    relative_to,
};

use crate::layout::SlotKind;

/// Fachada de UI del [`lsx_core::project::ProjectService`].
#[derive(Debug, Clone)]
pub struct ProjectService(lsx_core::project::ProjectService);

impl ProjectService {
    /// Servicio anclado a `<root>/workspace`.
    pub fn new(root: &PortableRoot) -> Self {
        Self(lsx_core::project::ProjectService::new(root))
    }

    /// Carpeta `workspace/` de la raíz portable.
    pub fn workspace(&self) -> &Path {
        self.0.workspace()
    }

    /// Proyecto con el que arrancar (último guardado → primera carpeta de
    /// `workspace/` → `workspace/` mismo).
    pub fn startup_project(&self, settings: &Settings) -> Option<ProjectInfo> {
        self.0.startup_project(settings)
    }

    /// Abre una ruta absoluta.
    pub fn open(&self, path: &Path) -> Result<ProjectInfo, ProjectError> {
        self.0.open(path)
    }

    /// Candidatos del diálogo "Abrir proyecto": recientes vivos + `workspace/`.
    pub fn candidates(&self, settings: &Settings) -> Vec<ProjectInfo> {
        self.0.candidates(settings)
    }

    /// Cierra el proyecto activo conservando la lista de recientes.
    pub fn close(&self, settings: &mut Settings) {
        self.0.close(settings);
    }

    /// Guarda `path` como último proyecto y lo añade a recientes (máx. 10,
    /// dedupe). La raíz portable se pasa explícitamente por compatibilidad
    /// con el shell (`app.rs`); el servicio ya está anclado a ella.
    pub fn remember(&self, settings: &mut Settings, portable: &PortableRoot, path: &Path) {
        self.0.remember_in(settings, portable.path(), path);
    }

    /// Servicio de núcleo subyacente (wizard «Nuevo proyecto», M1-F4).
    pub fn core(&self) -> &lsx_core::project::ProjectService {
        &self.0
    }
}

/// Techo conocido de la matriz de hojas (útil en tests de layout).
pub const SLOT_COUNT: usize = SlotKind::ALL.len();
