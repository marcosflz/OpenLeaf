//! `ProjectService` mínimo del stream A (M1-A1/F1 seed).
//!
//! Abre carpetas de proyecto dentro de `<root>/workspace`, recuerda recientes
//! (en `settings.project`) y resuelve el `main.tex`.
//!
//! **Nota de arquitectura**: la versión de producción de este servicio es del
//! stream F y vivirá en `lsx-core::project` (INTERFACES §2). Mientras tanto,
//! el shell necesita abrir proyectos ya, así que aquí hay una implementación
//! **de UI** que solo usa `lsx_core::fs::FsService` (listado/lectura). Cuando
//! F aterrice, `lsx-ui` cambia esta dependencia y este módulo queda como
//! adaptador fino.

use std::path::{Path, PathBuf};

use lsx_core::config::Settings;
use lsx_core::root::PortableRoot;

use crate::layout::SlotKind;

/// Error al abrir un proyecto.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// La ruta no existe.
    #[error("la carpeta del proyecto no existe: {}", .0.display())]
    NotFound(PathBuf),
    /// La ruta existe pero no es un directorio.
    #[error("la ruta del proyecto no es una carpeta: {}", .0.display())]
    NotDir(PathBuf),
    /// Error de E/S al inspeccionar la carpeta.
    #[error("no se pudo abrir el proyecto: {0}")]
    Io(#[from] std::io::Error),
}

/// Proyecto abierto por la app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectInfo {
    /// Nombre visible (nombre de la carpeta).
    pub name: String,
    /// Ruta absoluta de la raíz del proyecto.
    pub root: PathBuf,
    /// Fichero principal detectado (si lo hay).
    pub main_tex: Option<PathBuf>,
}

impl ProjectInfo {
    /// Nombre visible; cae al nombre del último componente de la ruta.
    pub fn from_root(root: PathBuf) -> Self {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let main_tex = find_main_tex(&root);
        Self {
            name,
            root,
            main_tex,
        }
    }
}

/// Servicio de proyectos del shell.
#[derive(Debug, Clone)]
pub struct ProjectService {
    workspace: PathBuf,
}

impl ProjectService {
    /// Servicio anclado a `<root>/workspace`.
    pub fn new(root: &PortableRoot) -> Self {
        Self {
            workspace: root.default_workspace(),
        }
    }

    /// Carpeta `workspace/` de la raíz portable.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Proyecto con el que arrancar: el último guardado si sigue existiendo;
    /// si no, la primera carpeta de `workspace/`; si no, `workspace/` mismo.
    pub fn startup_project(&self, settings: &Settings) -> Option<ProjectInfo> {
        if let Some(last) = settings.project.last.as_deref()
            && let Some(path) = self.resolve_relative(last)
            && path.is_dir()
        {
            return Some(ProjectInfo::from_root(path));
        }
        if let Some(first) = self.workspace_children().into_iter().next() {
            return Some(ProjectInfo::from_root(first));
        }
        if self.workspace.is_dir() {
            return Some(ProjectInfo::from_root(self.workspace.clone()));
        }
        None
    }

    /// Abre una ruta absoluta.
    pub fn open(&self, path: &Path) -> Result<ProjectInfo, ProjectError> {
        if !path.exists() {
            return Err(ProjectError::NotFound(path.to_path_buf()));
        }
        if !path.is_dir() {
            return Err(ProjectError::NotDir(path.to_path_buf()));
        }
        Ok(ProjectInfo::from_root(path.to_path_buf()))
    }

    /// Candidatos del diálogo "Abrir proyecto": recientes vivos primero y
    /// después las carpetas de `workspace/` (sin duplicar).
    pub fn candidates(&self, settings: &Settings) -> Vec<ProjectInfo> {
        let mut paths: Vec<PathBuf> = Vec::new();
        for recent in &settings.project.recent {
            if let Some(path) = self.resolve_relative(recent)
                && path.is_dir()
                && !paths.contains(&path)
            {
                paths.push(path);
            }
        }
        for child in self.workspace_children() {
            if !paths.contains(&child) {
                paths.push(child);
            }
        }
        paths.into_iter().map(ProjectInfo::from_root).collect()
    }

    /// Guarda `path` como último proyecto y lo añade a recientes (máx. 10).
    pub fn remember(&self, settings: &mut Settings, portable: &PortableRoot, path: &Path) {
        let Some(relative) = relative_to(portable, path) else {
            return;
        };
        settings.project.last = Some(relative.clone());
        settings.project.recent.retain(|entry| entry != &relative);
        settings.project.recent.insert(0, relative);
        settings.project.recent.truncate(10);
    }

    /// Resuelve una ruta de settings (relativa a la raíz portable, `/`).
    fn resolve_relative(&self, value: &str) -> Option<PathBuf> {
        let path = PathBuf::from(value.replace('/', std::path::MAIN_SEPARATOR_STR));
        if path.is_absolute() {
            return Some(path);
        }
        // Las rutas de settings cuelgan de la raíz portable, que es el padre
        // de `workspace/`.
        let root = self.workspace.parent()?;
        Some(root.join(path))
    }

    /// Carpetas de primer nivel de `workspace/`, ordenadas por nombre.
    fn workspace_children(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.workspace) else {
            return Vec::new();
        };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .filter(|path| !is_hidden(path))
            .collect();
        dirs.sort_by_key(|path| natural_order(path));
        dirs
    }
}

/// Orden razonable de carpetas: por nombre en minúsculas.
fn natural_order(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// `true` para carpetas ocultas (`.`/`_` inicial).
fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy())
        .is_some_and(|name| name.starts_with('.') || name.starts_with('_'))
}

/// Ruta de `path` relativa a la raíz portable, con separador `/`.
///
/// `None` si está fuera de la raíz (proyectos externos no se recuerdan en v1).
pub fn relative_to(portable: &PortableRoot, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(portable.path()).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// Heurística de `main.tex` (M1-F1 la refinará): `main.tex`, luego el primer
/// `.tex` con `\documentclass`, luego el primer `.tex` por orden alfabético.
pub fn find_main_tex(root: &Path) -> Option<PathBuf> {
    const MAX_DEPTH: usize = 2;
    const MAX_PROBE_BYTES: u64 = 256 * 1024;

    fn is_tex(path: &Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tex"))
    }

    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        if depth > MAX_DEPTH {
            return Ok(());
        }
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                if !is_hidden(&path) {
                    walk(&path, depth + 1, out)?;
                }
            } else if is_tex(&path) {
                out.push(path);
            }
        }
        Ok(())
    }

    let direct = root.join("main.tex");
    if direct.is_file() {
        return Some(direct);
    }

    let mut tex_files = Vec::new();
    walk(root, 0, &mut tex_files).ok()?;
    if tex_files.is_empty() {
        return None;
    }

    // Primer `.tex` con \documentclass (tamaño acotado para no leer libros).
    for candidate in &tex_files {
        let size = std::fs::metadata(candidate).map_or(0, |meta| meta.len());
        if size > MAX_PROBE_BYTES {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(candidate)
            && text.contains("\\documentclass")
        {
            return Some(candidate.clone());
        }
    }
    tex_files.into_iter().next()
}

/// Techo conocido de la matriz de hojas (útil en tests de layout).
pub const SLOT_COUNT: usize = SlotKind::ALL.len();

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempRoot {
        dir: PathBuf,
        root: PortableRoot,
    }

    impl TempRoot {
        fn new(name: &str) -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "lsx-ui-project-{}-{}-{n}",
                std::process::id(),
                name
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("workspace")).expect("crear workspace");
            let root = PortableRoot::from_dir(dir.clone());
            Self { dir, root }
        }

        fn workspace(&self) -> PathBuf {
            self.dir.join("workspace")
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("crear directorio");
        }
        std::fs::write(path, text).expect("escribir archivo");
    }

    #[test]
    fn find_main_tex_prefers_main_tex_and_detects_documentclass() {
        let temp = TempRoot::new("maintex");
        let cv = temp.workspace().join("cv");
        write(&cv.join("otro.tex"), "\\documentclass{article}");
        write(&cv.join("main.tex"), "sin documentclass");
        assert_eq!(find_main_tex(&cv), Some(cv.join("main.tex")));

        std::fs::remove_file(cv.join("main.tex")).expect("borrar main");
        assert_eq!(find_main_tex(&cv), Some(cv.join("otro.tex")));
    }

    #[test]
    fn find_main_tex_returns_none_without_tex_files() {
        let temp = TempRoot::new("notext");
        let dir = temp.workspace().join("vacio");
        std::fs::create_dir_all(&dir).expect("crear");
        assert_eq!(find_main_tex(&dir), None);
    }

    #[test]
    fn startup_prefers_the_last_project_and_falls_back_to_workspace_children() {
        let temp = TempRoot::new("startup");
        std::fs::create_dir_all(temp.workspace().join("cv")).expect("cv");
        std::fs::create_dir_all(temp.workspace().join("beamer")).expect("beamer");
        let service = ProjectService::new(&temp.root);

        let mut settings = Settings::default();
        settings.project.last = Some("workspace/beamer".to_owned());
        let project = service.startup_project(&settings).expect("proyecto");
        assert_eq!(project.name, "beamer");

        settings.project.last = Some("workspace/no-existe".to_owned());
        let project = service.startup_project(&settings).expect("proyecto");
        assert_eq!(project.name, "beamer");
    }

    #[test]
    fn candidates_list_recents_first_and_deduplicate() {
        let temp = TempRoot::new("candidates");
        std::fs::create_dir_all(temp.workspace().join("cv")).expect("cv");
        std::fs::create_dir_all(temp.workspace().join("articulo")).expect("articulo");
        let service = ProjectService::new(&temp.root);

        let mut settings = Settings::default();
        settings.project.recent = vec![
            "workspace/articulo".to_owned(),
            "workspace/ruta-que-no-existe".to_owned(),
            "workspace/cv".to_owned(),
        ];
        let names: Vec<String> = service
            .candidates(&settings)
            .into_iter()
            .map(|project| project.name)
            .collect();
        assert_eq!(names, ["articulo", "cv"]);
    }

    #[test]
    fn remember_writes_a_relative_path_and_keeps_a_bounded_recent_list() {
        let temp = TempRoot::new("remember");
        let service = ProjectService::new(&temp.root);
        let mut settings = Settings::default();
        for index in 0..12 {
            let path = temp.workspace().join(format!("p{index:02}"));
            std::fs::create_dir_all(&path).expect("crear");
            service.remember(&mut settings, &temp.root, &path);
        }
        assert_eq!(settings.project.recent.len(), 10);
        assert_eq!(settings.project.last.as_deref(), Some("workspace/p11"));
        assert_eq!(settings.project.recent[0], "workspace/p11");

        // Recordar de nuevo no duplica.
        service.remember(&mut settings, &temp.root, &temp.workspace().join("p11"));
        assert_eq!(
            settings
                .project
                .recent
                .iter()
                .filter(|entry| entry.as_str() == "workspace/p11")
                .count(),
            1
        );
    }

    #[test]
    fn remember_ignores_paths_outside_the_portable_root() {
        let temp = TempRoot::new("outside");
        let service = ProjectService::new(&temp.root);
        let mut settings = Settings::default();
        let outside = std::env::temp_dir().join("lsx-otro-proyecto");
        service.remember(&mut settings, &temp.root, &outside);
        assert!(settings.project.last.is_none());
        assert!(settings.project.recent.is_empty());
    }

    #[test]
    fn open_reports_missing_and_non_directory_paths() {
        let temp = TempRoot::new("open");
        let service = ProjectService::new(&temp.root);
        let missing = temp.workspace().join("no-existe");
        assert!(matches!(
            service.open(&missing),
            Err(ProjectError::NotFound(_))
        ));
        let file = temp.workspace().join("archivo.tex");
        write(&file, "x");
        assert!(matches!(service.open(&file), Err(ProjectError::NotDir(_))));
        assert!(service.open(&temp.workspace()).is_ok());
    }
}
