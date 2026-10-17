//! Servicio de proyectos (M1-F1/F4): abrir, cerrar, recientes y creación
//! desde plantilla.
//!
//! Vive en `lsx-core` (INTERFACES §2): no necesita UI y es la fuente única de
//! la heurística de `main.tex`, de la lista de recientes (límite 10, dedupe)
//! y del andamiaje de proyectos nuevos (`main.tex` + `AGENTS.md` +
//! `.opencode/agents/latex-fix.md`). El crate `lsx-ui` lo envuelve en un
//! adaptador fino ([`crate::project`](../lsx_ui) en la UI).
//!
//! Las plantillas y el andamiaje van **embebidos** (`include_str!` desde
//! `assets/templates/`, la fuente normativa del repo): así el wizard funciona
//! igual en desarrollo y en la distribución portable, sin depender de que
//! `assets/` acompañe al `.exe`.

use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::fs::validate_name;
use crate::root::PortableRoot;

/// Máximo de proyectos recientes en `settings.project.recent` (M1-F1).
pub const RECENT_LIMIT: usize = 10;

/// Profundidad máxima al buscar `.tex` candidatos a `main`.
const MAX_MAIN_DEPTH: usize = 2;
/// Tamaño máximo de un `.tex` que se sondea buscando `\documentclass`.
const MAX_MAIN_PROBE_BYTES: u64 = 256 * 1024;

/// Error al abrir o crear un proyecto.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// La ruta no existe.
    #[error("la carpeta del proyecto no existe: {}", .0.display())]
    NotFound(PathBuf),
    /// La ruta existe pero no es un directorio.
    #[error("la ruta del proyecto no es una carpeta: {}", .0.display())]
    NotDir(PathBuf),
    /// Ya hay algo con ese nombre en la carpeta destino.
    #[error("ya existe «{}»", .0.display())]
    AlreadyExists(PathBuf),
    /// Nombre de proyecto inválido (mensaje ya orientado al usuario).
    #[error("{0}")]
    InvalidName(String),
    /// Fallo de E/S durante la creación.
    #[error("no se pudo crear el proyecto: {0}")]
    Io(#[from] std::io::Error),
}

/// Proyecto abierto por la app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// Nombre visible (nombre de la carpeta).
    pub name: String,
    /// Ruta absoluta de la raíz del proyecto.
    pub root: PathBuf,
    /// Fichero principal detectado (si lo hay).
    pub main_tex: Option<PathBuf>,
}

/// Nombre histórico del stream A para el mismo tipo.
pub type ProjectInfo = Project;

impl Project {
    /// Proyecto a partir de una raíz, detectando `main.tex` por heurística.
    pub fn from_root(root: PathBuf) -> Self {
        let name = name_of(&root);
        let main_tex = find_main_tex(&root);
        Self {
            name,
            root,
            main_tex,
        }
    }

    /// Como [`Project::from_root`] pero con `latex.mainFile` de los ajustes
    /// si apunta a un archivo existente.
    pub fn from_root_with_settings_main(root: PathBuf, settings_main: Option<&str>) -> Self {
        let name = name_of(&root);
        let main_tex = settings_main
            .filter(|rel| !rel.is_empty())
            .map(|rel| root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))
            .filter(|path| path.is_file())
            .or_else(|| find_main_tex(&root));
        Self {
            name,
            root,
            main_tex,
        }
    }
}

/// Plantillas disponibles en el wizard «Nuevo proyecto» (M1-F4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TemplateId {
    /// Artículo (`article`).
    Article,
    /// Informe (`report`).
    Report,
    /// Presentación (`beamer`).
    Beamer,
}

impl TemplateId {
    /// Todas las plantillas, en el orden del wizard.
    pub const ALL: [Self; 3] = [Self::Article, Self::Report, Self::Beamer];

    /// Identificador estable para `settings`/logs (`article`, `report`, `beamer`).
    pub const fn id(self) -> &'static str {
        match self {
            Self::Article => "article",
            Self::Report => "report",
            Self::Beamer => "beamer",
        }
    }

    /// Plantilla a partir de su [`TemplateId::id`].
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|template| template.id() == id)
    }

    /// Contenido de `main.tex` (embebido desde `assets/templates/<id>/main.tex`).
    pub const fn main_tex(self) -> &'static str {
        match self {
            Self::Article => include_str!("../../../assets/templates/article/main.tex"),
            Self::Report => include_str!("../../../assets/templates/report/main.tex"),
            Self::Beamer => include_str!("../../../assets/templates/beamer/main.tex"),
        }
    }
}

/// `AGENTS.md` que recibe cada proyecto nuevo (producto, no repo de desarrollo).
pub const PROJECT_AGENTS_MD: &str = include_str!("../../../assets/templates/project/AGENTS.md");

/// Agente `latex-fix` que recibe cada proyecto nuevo (copia del de `workspace/`).
pub const LATEX_FIX_AGENT_MD: &str =
    include_str!("../../../assets/templates/project/opencode/agents/latex-fix.md");

/// Crea un proyecto en `parent/<name>` con la plantilla indicada.
///
/// Deja `main.tex`, `AGENTS.md` y `.opencode/agents/latex-fix.md`. Si algo
/// falla a mitad, borra la carpeta creada para no dejar restos a medias.
pub fn create_project(
    parent: &Path,
    name: &str,
    template: TemplateId,
) -> Result<Project, ProjectError> {
    if !parent.is_dir() {
        return Err(if parent.exists() {
            ProjectError::NotDir(parent.to_path_buf())
        } else {
            ProjectError::NotFound(parent.to_path_buf())
        });
    }
    validate_name(name).map_err(|err| ProjectError::InvalidName(err.to_string()))?;
    let dir = parent.join(name);
    if dir.exists() {
        return Err(ProjectError::AlreadyExists(dir));
    }
    create_at(&dir, template)
}

/// Crea el andamiaje en `dir` y limpia la carpeta si algo falla a mitad.
fn create_at(dir: &Path, template: TemplateId) -> Result<Project, ProjectError> {
    std::fs::create_dir_all(dir)?;
    match scaffold(dir, template) {
        Ok(()) => Ok(Project::from_root(dir.to_path_buf())),
        Err(err) => {
            let _ = std::fs::remove_dir_all(dir);
            Err(err)
        }
    }
}

/// Servicio de proyectos anclado a la raíz portable.
#[derive(Debug, Clone)]
pub struct ProjectService {
    root: PathBuf,
    workspace: PathBuf,
}

impl ProjectService {
    /// Servicio anclado a `<root>/workspace`.
    pub fn new(portable: &PortableRoot) -> Self {
        Self {
            root: portable.path().to_path_buf(),
            workspace: portable.default_workspace(),
        }
    }

    /// Variante con raíces explícitas (tests y herramientas).
    pub fn with_paths(root: impl Into<PathBuf>, workspace: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            workspace: workspace.into(),
        }
    }

    /// Raíz portable (referencia de las rutas de `settings`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Carpeta `workspace/` de la raíz portable.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Proyecto con el que arrancar: el último guardado si sigue existiendo;
    /// si no, la primera carpeta de `workspace/`; si no, `workspace/` mismo.
    pub fn startup_project(&self, settings: &Settings) -> Option<Project> {
        let settings_main = settings.latex.main_file.as_deref();
        if let Some(last) = settings.project.last.as_deref()
            && let Some(path) = self.resolve_relative(last)
            && path.is_dir()
        {
            return Some(Project::from_root_with_settings_main(path, settings_main));
        }
        if let Some(first) = self.workspace_children().into_iter().next() {
            return Some(Project::from_root_with_settings_main(first, settings_main));
        }
        if self.workspace.is_dir() {
            return Some(Project::from_root_with_settings_main(
                self.workspace.clone(),
                settings_main,
            ));
        }
        None
    }

    /// Abre una ruta absoluta.
    pub fn open(&self, path: &Path) -> Result<Project, ProjectError> {
        if !path.exists() {
            return Err(ProjectError::NotFound(path.to_path_buf()));
        }
        if !path.is_dir() {
            return Err(ProjectError::NotDir(path.to_path_buf()));
        }
        Ok(Project::from_root(path.to_path_buf()))
    }

    /// Cierra el proyecto activo conservando la lista de recientes.
    pub fn close(&self, settings: &mut Settings) {
        settings.project.last = None;
    }

    /// Recientes vivos de `settings`, en orden de uso (más nuevo primero).
    pub fn recent(&self, settings: &Settings) -> Vec<Project> {
        settings
            .project
            .recent
            .iter()
            .filter_map(|relative| self.resolve_relative(relative))
            .filter(|path| path.is_dir())
            .map(Project::from_root)
            .collect()
    }

    /// Candidatos del diálogo "Abrir proyecto": recientes vivos primero y
    /// después las carpetas de `workspace/` (sin duplicar).
    pub fn candidates(&self, settings: &Settings) -> Vec<Project> {
        let mut paths: Vec<PathBuf> = Vec::new();
        for project in self.recent(settings) {
            if !paths.contains(&project.root) {
                paths.push(project.root);
            }
        }
        for child in self.workspace_children() {
            if !paths.contains(&child) {
                paths.push(child);
            }
        }
        paths.into_iter().map(Project::from_root).collect()
    }

    /// Guarda `path` como último proyecto y lo añade a recientes (máx. 10).
    ///
    /// Rutas fuera de la raíz portable no se recuerdan en v1 (`remember_in`
    /// con `root = self.root`).
    pub fn remember(&self, settings: &mut Settings, path: &Path) {
        self.remember_in(settings, &self.root, path);
    }

    /// [`ProjectService::remember`] con raíz explícita (compat con el shell,
    /// que pasa su `PortableRoot`).
    pub fn remember_in(&self, settings: &mut Settings, root: &Path, path: &Path) {
        let Some(relative) = relative_to(root, path) else {
            return;
        };
        settings.project.last = Some(relative.clone());
        settings.project.recent.retain(|entry| entry != &relative);
        settings.project.recent.insert(0, relative);
        settings.project.recent.truncate(RECENT_LIMIT);
    }

    /// Crea un proyecto dentro de `parent` (por defecto usa el método libre
    /// [`create_project`]).
    pub fn create_project(
        &self,
        parent: &Path,
        name: &str,
        template: TemplateId,
    ) -> Result<Project, ProjectError> {
        create_project(parent, name, template)
    }

    /// Resuelve una ruta de settings (relativa a la raíz portable, `/`).
    fn resolve_relative(&self, value: &str) -> Option<PathBuf> {
        let path = PathBuf::from(value.replace('/', std::path::MAIN_SEPARATOR_STR));
        if path.is_absolute() {
            return Some(path);
        }
        Some(self.root.join(path))
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

/// Ruta de `path` relativa a `root` (raíz portable), con separador `/`.
///
/// `None` si está fuera de la raíz (proyectos externos no se recuerdan en v1).
pub fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// Heurística de `main.tex`: `main.tex`, luego el primer `.tex` con
/// `\documentclass`, luego el primer `.tex` por orden alfabético.
pub fn find_main_tex(root: &Path) -> Option<PathBuf> {
    fn is_tex(path: &Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tex"))
    }

    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        if depth > MAX_MAIN_DEPTH {
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
        if size > MAX_MAIN_PROBE_BYTES {
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

/// Escribe `main.tex`, `AGENTS.md` y el agente `latex-fix` del andamiaje.
fn scaffold(dir: &Path, template: TemplateId) -> Result<(), ProjectError> {
    write_file(&dir.join("main.tex"), template.main_tex())?;
    write_file(&dir.join("AGENTS.md"), PROJECT_AGENTS_MD)?;
    write_file(
        &dir.join(".opencode").join("agents").join("latex-fix.md"),
        LATEX_FIX_AGENT_MD,
    )?;
    Ok(())
}

fn write_file(path: &Path, text: &str) -> Result<(), ProjectError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// Nombre visible de una raíz; cae a la ruta completa si no tiene componente.
fn name_of(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
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
                "lsx-core-project-{}-{}-{n}",
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

        fn service(&self) -> ProjectService {
            ProjectService::new(&self.root)
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
        let service = temp.service();

        let mut settings = Settings::default();
        settings.project.last = Some("workspace/beamer".to_owned());
        let project = service.startup_project(&settings).expect("proyecto");
        assert_eq!(project.name, "beamer");

        settings.project.last = Some("workspace/no-existe".to_owned());
        let project = service.startup_project(&settings).expect("proyecto");
        assert_eq!(project.name, "beamer");
    }

    #[test]
    fn startup_honors_latex_main_file_from_settings() {
        let temp = TempRoot::new("settings-main");
        let cv = temp.workspace().join("cv");
        write(&cv.join("main.tex"), "\\documentclass{article}");
        write(&cv.join("otro.tex"), "\\documentclass{article}");
        let service = temp.service();

        let mut settings = Settings::default();
        settings.project.last = Some("workspace/cv".to_owned());
        settings.latex.main_file = Some("otro.tex".to_owned());
        let project = service.startup_project(&settings).expect("proyecto");
        assert_eq!(project.main_tex, Some(cv.join("otro.tex")));
    }

    #[test]
    fn recent_and_candidates_list_live_recents_first_and_deduplicate() {
        let temp = TempRoot::new("candidates");
        std::fs::create_dir_all(temp.workspace().join("cv")).expect("cv");
        std::fs::create_dir_all(temp.workspace().join("articulo")).expect("articulo");
        let service = temp.service();

        let mut settings = Settings::default();
        settings.project.recent = vec![
            "workspace/articulo".to_owned(),
            "workspace/ruta-que-no-existe".to_owned(),
            "workspace/cv".to_owned(),
        ];
        let recent: Vec<String> = service
            .recent(&settings)
            .into_iter()
            .map(|project| project.name)
            .collect();
        assert_eq!(recent, ["articulo", "cv"]);

        let names: Vec<String> = service
            .candidates(&settings)
            .into_iter()
            .map(|project| project.name)
            .collect();
        assert_eq!(names, ["articulo", "cv"]);
    }

    #[test]
    fn close_keeps_the_recent_list() {
        let temp = TempRoot::new("close");
        let service = temp.service();
        let mut settings = Settings::default();
        settings.project.last = Some("workspace/cv".to_owned());
        settings.project.recent = vec!["workspace/cv".to_owned()];
        service.close(&mut settings);
        assert!(settings.project.last.is_none());
        assert_eq!(settings.project.recent, ["workspace/cv"]);
    }

    #[test]
    fn remember_writes_a_relative_path_and_keeps_a_bounded_recent_list() {
        let temp = TempRoot::new("remember");
        let service = temp.service();
        let mut settings = Settings::default();
        for index in 0..12 {
            let path = temp.workspace().join(format!("p{index:02}"));
            std::fs::create_dir_all(&path).expect("crear");
            service.remember(&mut settings, &path);
        }
        assert_eq!(settings.project.recent.len(), RECENT_LIMIT);
        assert_eq!(settings.project.last.as_deref(), Some("workspace/p11"));
        assert_eq!(settings.project.recent[0], "workspace/p11");

        // Recordar de nuevo no duplica.
        service.remember(&mut settings, &temp.workspace().join("p11"));
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
        let service = temp.service();
        let mut settings = Settings::default();
        let outside = std::env::temp_dir().join("lsx-otro-proyecto");
        service.remember(&mut settings, &outside);
        assert!(settings.project.last.is_none());
        assert!(settings.project.recent.is_empty());
    }

    #[test]
    fn open_reports_missing_and_non_directory_paths() {
        let temp = TempRoot::new("open");
        let service = temp.service();
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

    #[test]
    fn create_project_scaffolds_every_template_and_detects_main() {
        let temp = TempRoot::new("create");
        let workspace = temp.workspace();
        for template in TemplateId::ALL {
            let name = format!("proyecto-{}", template.id());
            let project = create_project(&workspace, &name, template)
                .expect("crear proyecto desde plantilla");
            let root = project.root.clone();
            assert_eq!(project.name, name);
            assert_eq!(project.main_tex, Some(root.join("main.tex")));

            let main = std::fs::read_to_string(root.join("main.tex")).expect("leer main");
            assert_eq!(main, template.main_tex());
            assert!(main.contains("\\documentclass"));
            assert!(root.join("AGENTS.md").is_file());
            assert!(
                root.join(".opencode")
                    .join("agents")
                    .join("latex-fix.md")
                    .is_file()
            );
        }
    }

    #[test]
    fn create_project_rejects_invalid_names_and_existing_folders() {
        let temp = TempRoot::new("create-invalid");
        let workspace = temp.workspace();

        for invalid in [
            "", "..", "CON", "con.tex", "carpeta.", "nombre ", "a/b", "a\\b",
        ] {
            let result = create_project(&workspace, invalid, TemplateId::Article);
            assert!(
                matches!(result, Err(ProjectError::InvalidName(_))),
                "«{invalid}» debería ser rechazado: {result:?}"
            );
        }

        std::fs::create_dir_all(workspace.join("ocupado")).expect("crear");
        let result = create_project(&workspace, "ocupado", TemplateId::Article);
        assert!(matches!(result, Err(ProjectError::AlreadyExists(_))));

        let result = create_project(&workspace.join("no-existe"), "x", TemplateId::Article);
        assert!(matches!(result, Err(ProjectError::NotFound(_))));
    }

    #[test]
    fn create_project_removes_the_folder_when_scaffolding_fails() {
        let temp = TempRoot::new("create-rollback");
        let workspace = temp.workspace();
        // `main.tex` como carpeta fuerza el fallo de escritura del andamiaje.
        let dir = workspace.join("roto");
        std::fs::create_dir_all(dir.join("main.tex")).expect("crear trampa");
        let result = create_at(&dir, TemplateId::Article);
        assert!(result.is_err());
        assert!(
            !dir.exists(),
            "la carpeta a medias debe limpiarse tras el fallo"
        );

        // Tras la limpieza, el camino público puede recrear la carpeta.
        let result = create_project(&workspace, "roto", TemplateId::Article);
        assert!(result.is_ok(), "recrear tras el rollback: {result:?}");
        assert!(workspace.join("roto").join("main.tex").is_file());
    }

    #[test]
    fn template_ids_round_trip() {
        for template in TemplateId::ALL {
            assert_eq!(TemplateId::from_id(template.id()), Some(template));
        }
        assert_eq!(TemplateId::from_id("otra"), None);
    }
}
