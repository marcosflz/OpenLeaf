//! Raíz portable de LaTeX Studio (M1-A0).
//!
//! El producto se distribuye como una carpeta portable:
//!
//! ```text
//! latex-ide/
//!   latex-studio.exe      ← el binario vive en la raíz
//!   tex/                  ← TeX Live reducido
//!   agent/                ← opencode.exe + opencode.json
//!   workspace/            ← proyectos del usuario
//!   config/               ← settings.json (estado de usuario)
//!   logs/
//! ```
//!
//! [`PortableRoot`] localiza esa carpeta una sola vez en el arranque y expone
//! rutas derivadas. Orden de detección (`INTERFACES.md` §2):
//!
//! 1. `LATESTUDIO_ROOT` (override explícita; se acepta también el nombre
//!    histórico `LATEX_STUDIO_ROOT` del borrador de interfaces).
//! 2. Directorio del ejecutable, si **parece** una raíz portable (contiene
//!    `tex/`, `agent/`, `workspace/` o `config/`).
//! 3. Raíz del repo (`CARGO_MANIFEST_DIR/../..`), solo si existe: es el modo
//!    desarrollo, donde el `.exe` vive en `target\debug`.
//!
//! `lsx-core` no conoce UI ni runtime: este módulo solo usa `std`.

use std::path::{Path, PathBuf};

/// Variable de entorno que fuerza la raíz portable.
pub const ENV_ROOT: &str = "LATESTUDIO_ROOT";
/// Nombre histórico del borrador (`INTERFACES.md` §2); se acepta por compat.
pub const ENV_ROOT_LEGACY: &str = "LATEX_STUDIO_ROOT";

/// Constructores de carpetas que identifican una raíz portable.
const ROOT_MARKERS: [&str; 4] = ["tex", "agent", "workspace", "config"];

/// Error de detección de la raíz portable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RootError {
    /// No hay override válida, ni raíz junto al ejecutable, ni repo de desarrollo.
    #[error("no se encontró la raíz portable de LaTeX Studio (prueba a definir {ENV_ROOT})")]
    NotFound,
    /// La raíz existe pero no se puede escribir (config/ y logs/ viven ahí).
    #[error("la raíz portable no admite escritura: {}", .0.display())]
    NotWritable(PathBuf),
}

/// Raíz portable detectada.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PortableRoot(PathBuf);

impl PortableRoot {
    /// Detecta la raíz portable según el orden documentado en el módulo.
    pub fn detect() -> Result<Self, RootError> {
        let env = std::env::var_os(ENV_ROOT)
            .or_else(|| std::env::var_os(ENV_ROOT_LEGACY))
            .map(PathBuf::from);
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        Self::detect_from(env, exe_dir, repo_root())
    }

    /// Construye la raíz a partir de un directorio cualquiera (tests, tools).
    ///
    /// No valida el contenido: la validación de layout es responsabilidad del
    /// primer arranque (M3-3).
    pub fn from_dir(dir: impl Into<PathBuf>) -> Self {
        Self(dir.into())
    }

    /// Lógica de detección sin tocar el entorno del proceso (testeable).
    fn detect_from(
        env: Option<PathBuf>,
        exe_dir: Option<PathBuf>,
        repo: Option<PathBuf>,
    ) -> Result<Self, RootError> {
        if let Some(dir) = env {
            if dir.is_dir() {
                return Ok(Self(dir));
            }
            return Err(RootError::NotFound);
        }
        if let Some(dir) = exe_dir
            && looks_like_root(&dir)
        {
            return Ok(Self(dir));
        }
        if let Some(repo) = repo
            && repo.is_dir()
        {
            return Ok(Self(repo));
        }
        Err(RootError::NotFound)
    }

    /// Directorio raíz.
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// `<root>/tex` — TeX Live reducido.
    pub fn tex_dir(&self) -> PathBuf {
        self.0.join("tex")
    }

    /// `tex/bin/windows/latexmk.exe`.
    pub fn latexmk(&self) -> PathBuf {
        self.tex_dir()
            .join("bin")
            .join("windows")
            .join("latexmk.exe")
    }

    /// `tex/bin/windows/synctex.exe`.
    pub fn synctex_exe(&self) -> PathBuf {
        self.tex_dir()
            .join("bin")
            .join("windows")
            .join("synctex.exe")
    }

    /// `<root>/agent` — bundle de opencode.
    pub fn agent_dir(&self) -> PathBuf {
        self.0.join("agent")
    }

    /// `agent/opencode.exe`.
    pub fn opencode_exe(&self) -> PathBuf {
        self.agent_dir().join("opencode.exe")
    }

    /// `agent/opencode.json`.
    pub fn opencode_config(&self) -> PathBuf {
        self.agent_dir().join("opencode.json")
    }

    /// `<root>/config` — settings.json y backups.
    pub fn config_dir(&self) -> PathBuf {
        self.0.join("config")
    }

    /// `<root>/logs`.
    pub fn logs_dir(&self) -> PathBuf {
        self.0.join("logs")
    }

    /// `<root>/workspace` — proyectos del usuario.
    pub fn default_workspace(&self) -> PathBuf {
        self.0.join("workspace")
    }

    /// Prepara el layout de una instalación recién copiada: crea `<root>/workspace`
    /// y `<root>/config` si faltan (idempotente) y devuelve la ruta del workspace.
    ///
    /// El instalador ligero no crea esas carpetas. Sin `workspace/` el `cwd` de
    /// `opencode serve` no existe — y `Command::current_dir` falla, así que el
    /// chat se quedaría sin agente en el primer arranque (M3-3).
    pub fn ensure_layout(&self) -> std::io::Result<PathBuf> {
        let workspace = self.default_workspace();
        std::fs::create_dir_all(&workspace)?;
        std::fs::create_dir_all(self.config_dir())?;
        Ok(workspace)
    }

    /// Comprueba que la raíz existe y admite escribir un archivo de prueba.
    pub fn is_writable(&self) -> bool {
        if !self.0.is_dir() {
            return false;
        }
        let probe = self
            .0
            .join(format!(".lsx-write-probe-{}", std::process::id()));
        match std::fs::write(&probe, b"probe") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
                true
            }
            Err(_) => false,
        }
    }
}

/// Raíz del repo de desarrollo (`CARGO_MANIFEST_DIR/../..`).
///
/// En binarios de release compilados en otra máquina el camino puede no
/// existir; por eso [`PortableRoot::detect`] comprueba `is_dir()` antes de
/// aceptarlo.
pub fn repo_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
}

/// ¿El directorio contiene marcas de una raíz portable?
fn looks_like_root(dir: &Path) -> bool {
    ROOT_MARKERS.iter().any(|marker| dir.join(marker).is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Directorio temporal único por test (los tests corren en paralelo).
    fn temp_dir(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("lsx-core-root-{}-{}-{n}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("crear dir temporal");
        dir
    }

    #[test]
    fn env_override_wins_even_over_a_repo() {
        let dir = temp_dir("env");
        let root = PortableRoot::detect_from(Some(dir.clone()), None, Some(PathBuf::from("C:/no")));
        assert_eq!(root.expect("env válida").path(), dir.as_path());
    }

    #[test]
    fn env_override_to_a_missing_dir_is_not_found() {
        let missing = std::env::temp_dir().join("lsx-root-que-no-existe-jamas");
        let result = PortableRoot::detect_from(Some(missing), None, None);
        assert_eq!(result, Err(RootError::NotFound));
    }

    #[test]
    fn exe_dir_counts_only_when_it_looks_like_a_root() {
        let shippable = temp_dir("exe-ship");
        std::fs::create_dir_all(shippable.join("tex")).expect("crear tex");
        let root = PortableRoot::detect_from(None, Some(shippable.clone()), None);
        assert_eq!(root.expect("exe dir válido").path(), shippable.as_path());

        let plain = temp_dir("exe-plain");
        let root = PortableRoot::detect_from(None, Some(plain), Some(PathBuf::from("C:/no")));
        assert_eq!(root, Err(RootError::NotFound));
    }

    #[test]
    fn falls_back_to_the_repo_root_in_development() {
        let exe = temp_dir("exe-dev");
        let repo = temp_dir("repo-dev");
        let root = PortableRoot::detect_from(None, Some(exe), Some(repo.clone()));
        assert_eq!(root.expect("repo").path(), repo.as_path());
    }

    #[test]
    fn derived_paths_follow_the_portable_layout() {
        let root = PortableRoot::from_dir(PathBuf::from("C:/lsx"));
        assert_eq!(root.tex_dir(), PathBuf::from("C:/lsx/tex"));
        assert_eq!(
            root.latexmk(),
            PathBuf::from("C:/lsx/tex/bin/windows/latexmk.exe")
        );
        assert_eq!(
            root.synctex_exe(),
            PathBuf::from("C:/lsx/tex/bin/windows/synctex.exe")
        );
        assert_eq!(root.agent_dir(), PathBuf::from("C:/lsx/agent"));
        assert_eq!(
            root.opencode_exe(),
            PathBuf::from("C:/lsx/agent/opencode.exe")
        );
        assert_eq!(
            root.opencode_config(),
            PathBuf::from("C:/lsx/agent/opencode.json")
        );
        assert_eq!(root.config_dir(), PathBuf::from("C:/lsx/config"));
        assert_eq!(root.logs_dir(), PathBuf::from("C:/lsx/logs"));
        assert_eq!(root.default_workspace(), PathBuf::from("C:/lsx/workspace"));
    }

    #[test]
    fn ensure_layout_creates_workspace_and_config_without_touching_files() {
        let dir = temp_dir("layout");
        let root = PortableRoot::from_dir(dir.clone());
        assert!(
            !root.default_workspace().is_dir(),
            "el temporal debe empezar vacío"
        );

        let workspace = root.ensure_layout().expect("prepara el layout");
        assert_eq!(workspace, root.default_workspace());
        assert!(workspace.is_dir());
        assert!(root.config_dir().is_dir());

        // Idempotente y no destructivo: un proyecto ya creado sigue ahí.
        let project = workspace.join("mio.tex");
        std::fs::write(&project, b"hola").expect("escribir proyecto");
        assert!(root.ensure_layout().is_ok());
        assert!(project.is_file(), "no debe vaciar el workspace");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_writable_reports_a_writable_dir_and_leaves_no_probe() {
        let dir = temp_dir("writable");
        let root = PortableRoot::from_dir(dir.clone());
        assert!(root.is_writable());
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("leer dir")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !leftovers
                .iter()
                .any(|name| name.starts_with(".lsx-write-probe")),
            "la sonda de escritura no debe quedar en disco: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_writable_is_false_for_a_missing_dir() {
        let missing = std::env::temp_dir().join("lsx-root-no-existe-writable");
        assert!(!PortableRoot::from_dir(missing).is_writable());
    }

    #[test]
    fn the_repo_root_helper_points_at_the_workspace_root() {
        let repo = repo_root().expect("el repo de desarrollo existe");
        assert!(
            repo.join("Cargo.toml").is_file(),
            "esperaba el Cargo.toml del workspace en {}",
            repo.display()
        );
    }
}
