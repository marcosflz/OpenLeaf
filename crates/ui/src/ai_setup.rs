//! Conexión de IA desde Ajustes: proveedores, claves y prueba de conexión.
//!
//! El agente LaTeX (`python -m lsx_agent`, sobre nas-adk) admite cuatro
//! proveedores y ninguno viene impuesto:
//!
//! | id            | Qué es                              | Credencial                          |
//! |---------------|-------------------------------------|-------------------------------------|
//! | `opencode`    | OpenCode Go                         | clave en `config/secrets/ai.json`   |
//! | `openrouter`  | OpenRouter                          | clave en `config/secrets/ai.json`   |
//! | `anthropic`   | API de Anthropic                    | clave en `config/secrets/ai.json`   |
//! | `claude-code` | Suscripción de Claude (CLI `claude`)| sesión del CLI (`claude auth login`)|
//!
//! Las claves también se aceptan por variable de entorno (`OPENCODE_API_KEY`,
//! …). La tabla [`PROVIDERS`] replica la de `python/lsx_agent/providers.py`;
//! un test comprueba que no divergen.
//!
//! La UI **nunca** bloquea el hilo de GPUI: [`AiSetup::probe`] lanza un
//! proceso y el llamador la ejecuta dentro de `cx.background_spawn`. El resto
//! de operaciones solo leen o escriben un JSON pequeño.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use lsx_core::root::PortableRoot;
use serde_json::{Map, Value};

/// Presupuesto del botón «Probar» (una petición mínima al modelo).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(90);

/// Proveedor de IA que el agente sabe usar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderSpec {
    /// Id de `ai.model` (`<id>/<modelo>`).
    pub id: &'static str,
    /// Nombre visible.
    pub name: &'static str,
    /// Variable de entorno con la clave (`None`: sin clave, p. ej. `claude-code`).
    pub env_key: Option<&'static str>,
    /// Modelos ofrecidos `(id, nombre)`.
    pub models: &'static [(&'static str, &'static str)],
    /// Ayuda corta para Ajustes.
    pub help: &'static str,
}

/// Proveedores soportados (mismo orden y modelos que `providers.py`).
pub const PROVIDERS: [ProviderSpec; 4] = [
    ProviderSpec {
        id: "opencode",
        name: "OpenCode Go",
        env_key: Some("OPENCODE_API_KEY"),
        models: &[
            ("deepseek-v4.1-flash", "DeepSeek V4.1 Flash"),
            ("glm-5.3-flash", "GLM 5.3 Flash"),
            ("qwen3.8-flash", "Qwen 3.8 Flash"),
        ],
        help: "Clave de opencode.ai (suscripción Go).",
    },
    ProviderSpec {
        id: "openrouter",
        name: "OpenRouter",
        env_key: Some("OPENROUTER_API_KEY"),
        models: &[("openrouter/auto", "Auto (OpenRouter elige)")],
        help: "Clave de openrouter.ai; admite cualquier modelo «proveedor/modelo».",
    },
    ProviderSpec {
        id: "anthropic",
        name: "Anthropic (API)",
        env_key: Some("ANTHROPIC_API_KEY"),
        models: &[
            ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
            ("claude-opus-5-5", "Claude Opus 5.5"),
            ("claude-haiku-5-5", "Claude Haiku 5.5"),
        ],
        help: "Clave de la API de Anthropic (facturación por uso).",
    },
    ProviderSpec {
        id: "claude-code",
        name: "Claude (suscripción)",
        env_key: None,
        models: &[
            ("default", "Modelo por defecto del CLI"),
            ("sonnet", "Sonnet"),
            ("opus", "Opus"),
            ("haiku", "Haiku"),
        ],
        help: "Usa tu CLI `claude` ya autenticado (`claude auth login`). Uso personal y local.",
    },
];

/// Busca un proveedor por id.
pub fn provider(id: &str) -> Option<&'static ProviderSpec> {
    PROVIDERS.iter().find(|spec| spec.id == id)
}

/// Estado de un proveedor para pintar en Ajustes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
    pub spec: &'static ProviderSpec,
    /// Hay credencial (clave guardada, variable de entorno o CLI presente).
    pub configured: bool,
    /// De dónde sale la credencial, para explicarlo en la UI.
    pub source: CredentialSource,
}

/// Origen de la credencial de un proveedor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialSource {
    /// Sin credencial.
    None,
    /// Clave guardada en `config/secrets/ai.json`; se muestran sus 4 últimos caracteres.
    Saved { last4: String },
    /// Clave en la variable de entorno indicada.
    Env(&'static str),
    /// CLI `claude` encontrado en esta ruta.
    Cli(PathBuf),
}

/// Error accionable de la capa de conexión.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiSetupError {
    /// No se pudo localizar la raíz portable.
    #[error("no se encontró la carpeta de instalación: {0}")]
    Root(String),
    /// Falta el intérprete del agente.
    #[error(
        "no se encontró el asistente en {0}. Reinstala LaTeX Studio (o ejecuta `make agent` en desarrollo)"
    )]
    MissingExe(String),
    /// Falta una ruta que la acción necesita.
    #[error("no existe {0}")]
    MissingPath(String),
    /// Proveedor o modelo no válido.
    #[error("{0}")]
    Invalid(String),
    /// No se pudo lanzar el proceso.
    #[error("no se pudo ejecutar el asistente: {0}")]
    Spawn(String),
    /// El proceso no terminó dentro del presupuesto.
    #[error("la prueba tardó más de {0} s y se canceló")]
    Timeout(u64),
    /// El proceso terminó con error.
    #[error("la prueba falló (código {code:?}): {message}")]
    Command {
        /// Código de salida (puede faltar si lo mató una señal).
        code: Option<i32>,
        /// Últimas líneas de stderr/stdout, ya recortadas.
        message: String,
    },
    /// Error de E/S.
    #[error("no se pudo usar {path}: {error}")]
    Io {
        /// Ruta implicada.
        path: String,
        /// Detalle del sistema.
        error: String,
    },
}

/// Salida de un comando con timeout.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandOutput {
    stdout: String,
}

/// Configuración de IA de una instalación: intérprete del agente y claves.
#[derive(Debug, Clone)]
pub struct AiSetup {
    root: PortableRoot,
    python: PathBuf,
    python_path: Vec<PathBuf>,
    secrets: PathBuf,
}

impl AiSetup {
    /// Deriva las rutas de la raíz portable (bundle o, en desarrollo, `python/.venv`).
    pub fn from_root(root: PortableRoot) -> Self {
        let (python, python_path) = if root.agent_python().is_file() {
            (root.agent_python(), root.agent_python_path())
        } else {
            root.dev_agent()
                .unwrap_or_else(|| (root.agent_python(), root.agent_python_path()))
        };
        let secrets = root.ai_secrets();
        Self {
            root,
            python,
            python_path,
            secrets,
        }
    }

    /// Detecta la raíz portable y deriva las rutas.
    pub fn detect() -> Result<Self, AiSetupError> {
        PortableRoot::detect()
            .map(Self::from_root)
            .map_err(|error| AiSetupError::Root(error.to_string()))
    }

    /// Intérprete del agente.
    pub fn python_path(&self) -> &Path {
        &self.python
    }

    /// `true` si el agente está instalado (bundle o entorno de desarrollo).
    pub fn agent_exists(&self) -> bool {
        self.python.is_file()
    }

    /// `config/secrets/ai.json`.
    pub fn secrets_path(&self) -> &Path {
        &self.secrets
    }

    /// Estado de los cuatro proveedores (lectura local, sin red).
    pub fn status(&self) -> Vec<ProviderStatus> {
        let secrets = self.load_secrets();
        PROVIDERS
            .iter()
            .map(|spec| {
                let source = credential_source(spec, &secrets);
                ProviderStatus {
                    spec,
                    configured: source != CredentialSource::None,
                    source,
                }
            })
            .collect()
    }

    /// Modelos `proveedor/modelo` de los proveedores configurados.
    pub fn models(&self) -> Vec<String> {
        let secrets = self.load_secrets();
        let mut models = Vec::new();
        for status in self.status().into_iter().filter(|s| s.configured) {
            for (model, _) in status.spec.models {
                models.push(format!("{}/{model}", status.spec.id));
            }
            for extra in extra_models(&secrets, status.spec.id) {
                let id = format!("{}/{extra}", status.spec.id);
                if !models.contains(&id) {
                    models.push(id);
                }
            }
        }
        models
    }

    /// Guarda (o sustituye) la clave de un proveedor con API.
    pub fn save_key(&self, provider_id: &str, key: &str) -> Result<(), AiSetupError> {
        let spec = provider(provider_id).ok_or_else(|| {
            AiSetupError::Invalid(format!("proveedor desconocido: {provider_id}"))
        })?;
        if spec.env_key.is_none() {
            return Err(AiSetupError::Invalid(format!(
                "{} no usa clave: inicia sesión con `claude auth login`",
                spec.name
            )));
        }
        let key = key.trim();
        if key.len() < 8 || key.chars().any(char::is_whitespace) {
            return Err(AiSetupError::Invalid(
                "la clave parece incompleta (sin espacios, al menos 8 caracteres)".to_owned(),
            ));
        }
        let mut secrets = self.load_secrets();
        let entry = secrets
            .entry(spec.id.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        if let Some(object) = entry.as_object_mut() {
            object.insert("apiKey".to_owned(), Value::String(key.to_owned()));
        }
        self.store_secrets(&secrets)
    }

    /// Borra la clave guardada de un proveedor (las variables de entorno no se tocan).
    pub fn remove_key(&self, provider_id: &str) -> Result<(), AiSetupError> {
        let mut secrets = self.load_secrets();
        if let Some(object) = secrets.get_mut(provider_id).and_then(Value::as_object_mut) {
            object.remove("apiKey");
            if object.is_empty() {
                secrets.remove(provider_id);
            }
        }
        self.store_secrets(&secrets)
    }

    /// Prueba un modelo `proveedor/modelo` con una petición mínima (gasta muy
    /// pocos tokens). Bloquea: ejecútalo en segundo plano.
    pub fn probe(&self, model: &str) -> Result<String, AiSetupError> {
        if !self.agent_exists() {
            return Err(AiSetupError::MissingExe(self.python.display().to_string()));
        }
        let (provider_id, model_id) = model
            .split_once('/')
            .filter(|(p, m)| provider(p).is_some() && !m.is_empty())
            .ok_or_else(|| {
                AiSetupError::Invalid(format!("modelo no válido: «{model}» (proveedor/modelo)"))
            })?;
        let mut command = Command::new(&self.python);
        command
            .args(["-P", "-s", "-m", "lsx_agent", "--probe"])
            .arg(format!("{provider_id}/{model_id}"))
            .arg("--data-dir")
            .arg(self.root.agent_data_dir())
            .arg("--secrets")
            .arg(&self.secrets)
            // Nunca el proyecto: Python podría importar módulos del cwd.
            .current_dir(if self.root.agent_dir().is_dir() {
                self.root.agent_dir()
            } else {
                std::env::temp_dir()
            })
            .env("PYTHONUTF8", "1")
            .env("PYTHONNOUSERSITE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !self.python_path.is_empty() {
            let joined = std::env::join_paths(&self.python_path)
                .map_err(|error| AiSetupError::Spawn(error.to_string()))?;
            command.env("PYTHONPATH", joined);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = run_with_timeout(command, PROBE_TIMEOUT)?;
        let reply: Value =
            serde_json::from_str(output.stdout.trim()).map_err(|_| AiSetupError::Command {
                code: None,
                message: summarize_failure("", &output.stdout),
            })?;
        if reply.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(reply
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned())
        } else {
            Err(AiSetupError::Command {
                code: None,
                message: reply
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("respuesta vacía")
                    .to_owned(),
            })
        }
    }

    /// Abre la carpeta del agente (`agent/`).
    pub fn open_agent_dir(&self) -> Result<(), AiSetupError> {
        let dir = self.root.agent_dir();
        if !dir.is_dir() {
            return Err(AiSetupError::MissingPath(dir.display().to_string()));
        }
        open_folder(&dir)
    }

    /// Abre el log del agente (`logs/agent.log`).
    pub fn open_log(&self) -> Result<(), AiSetupError> {
        let log = self.root.logs_dir().join("agent.log");
        if !log.is_file() {
            return Err(AiSetupError::MissingPath(log.display().to_string()));
        }
        open_file(&log)
    }

    fn load_secrets(&self) -> Map<String, Value> {
        std::fs::read_to_string(&self.secrets)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| match value {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Escritura atómica (temporal + rename) de `config/secrets/ai.json`.
    fn store_secrets(&self, secrets: &Map<String, Value>) -> Result<(), AiSetupError> {
        let io = |error: std::io::Error| AiSetupError::Io {
            path: self.secrets.display().to_string(),
            error: error.to_string(),
        };
        if let Some(parent) = self.secrets.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let text = serde_json::to_string_pretty(&Value::Object(secrets.clone()))
            .map_err(|error| AiSetupError::Invalid(error.to_string()))?;
        let temporary = self.secrets.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(io)?;
        std::fs::rename(&temporary, &self.secrets).map_err(io)
    }
}

fn credential_source(spec: &ProviderSpec, secrets: &Map<String, Value>) -> CredentialSource {
    let Some(env_key) = spec.env_key else {
        let bin = std::env::var("CLAUDE_CODE_BIN").unwrap_or_else(|_| "claude".to_owned());
        return find_on_path(&bin).map_or(CredentialSource::None, CredentialSource::Cli);
    };
    if let Some(key) = secrets
        .get(spec.id)
        .and_then(|entry| entry.get("apiKey"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        let last4: String = key
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return CredentialSource::Saved { last4 };
    }
    if std::env::var(env_key).is_ok_and(|value| !value.trim().is_empty()) {
        return CredentialSource::Env(env_key);
    }
    CredentialSource::None
}

fn extra_models(secrets: &Map<String, Value>, provider_id: &str) -> Vec<String> {
    secrets
        .get(provider_id)
        .and_then(|entry| entry.get("models"))
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(Value::as_str)
                .filter(|model| !model.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Equivalente a `shutil.which`: busca un ejecutable en el `PATH`.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_owned())
            .split(';')
            .map(str::to_owned)
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            extensions.iter().find_map(|extension| {
                let path = dir.join(format!("{name}{}", extension.to_ascii_lowercase()));
                path.is_file().then_some(path)
            })
        })
    })
}

/// Ejecuta un comando con timeout y sin bloquear el hilo que lo llama.
///
/// Los pipes se leen en hilos propios para que un catálogo grande no llene el
/// buffer del sistema y bloquee al hijo antes de salir. Al agotarse el
/// presupuesto se mata el proceso y se devuelve [`AiSetupError::Timeout`].
fn run_with_timeout(
    mut command: Command,
    timeout: Duration,
) -> Result<CommandOutput, AiSetupError> {
    let mut child = command
        .spawn()
        .map_err(|error| AiSetupError::Spawn(error.to_string()))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = stdout.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = pipe.read_to_end(&mut buffer);
            buffer
        })
    });
    let stderr_reader = stderr.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = pipe.read_to_end(&mut buffer);
            buffer
        })
    });

    let started = Instant::now();
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break (Some(exit), false),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AiSetupError::Spawn(format!(
                    "no se pudo esperar a opencode: {error}"
                )));
            }
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            break (child.wait().ok(), true);
        }
        std::thread::sleep(Duration::from_millis(40));
    };

    if timed_out {
        // No se unen los lectores: si el hijo dejó descendientes con el pipe
        // abierto, un join podría bloquear el hilo de fondo. Terminan solos.
        return Err(AiSetupError::Timeout(timeout.as_secs()));
    }

    let stdout = stdout_reader
        .map(|reader| reader.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = stderr_reader
        .map(|reader| reader.join().unwrap_or_default())
        .unwrap_or_default();

    let stdout = String::from_utf8_lossy(&stdout).into_owned();
    let stderr = String::from_utf8_lossy(&stderr).into_owned();

    if let Some(status) = status
        && !status.success()
    {
        return Err(AiSetupError::Command {
            code: status.code(),
            message: summarize_failure(&stderr, &stdout),
        });
    }

    Ok(CommandOutput { stdout })
}

/// Recorta la salida de error a las últimas líneas útiles.
fn summarize_failure(stderr: &str, stdout: &str) -> String {
    let source = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    let lines: Vec<&str> = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let tail = lines
        .iter()
        .rev()
        .take(4)
        .rev()
        .copied()
        .collect::<Vec<_>>();
    let mut message = tail.join(" ");
    if message.is_empty() {
        message = "sin salida de error".to_owned();
    }
    let message: String = message.chars().take(400).collect();
    message
}

/// Abre una carpeta con el gestor de archivos del sistema.
pub fn open_folder(path: &Path) -> Result<(), AiSetupError> {
    #[cfg(windows)]
    {
        Command::new("explorer.exe")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| AiSetupError::Io {
                path: path.display().to_string(),
                error: error.to_string(),
            })
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| AiSetupError::Io {
                path: path.display().to_string(),
                error: error.to_string(),
            })
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| AiSetupError::Io {
                path: path.display().to_string(),
                error: error.to_string(),
            })
    }
}

/// Abre un archivo con la aplicación asociada del sistema.
pub fn open_file(path: &Path) -> Result<(), AiSetupError> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = Command::new("cmd");
        command
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW);
        command
            .spawn()
            .map(|_| ())
            .map_err(|error| AiSetupError::Io {
                path: path.display().to_string(),
                error: error.to_string(),
            })
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| AiSetupError::Io {
                path: path.display().to_string(),
                error: error.to_string(),
            })
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| AiSetupError::Io {
                path: path.display().to_string(),
                error: error.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> (PortableRoot, PathBuf) {
        let dir = std::env::temp_dir().join(format!("lsx-ai-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).expect("config temporal");
        (PortableRoot::from_dir(dir.clone()), dir)
    }

    #[test]
    fn la_tabla_de_proveedores_coincide_con_la_del_agente() {
        let python =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/lsx_agent/providers.py");
        let source = std::fs::read_to_string(python).expect("providers.py");
        for spec in &PROVIDERS {
            assert!(
                source.contains(&format!("id=\"{}\"", spec.id)),
                "{} falta en providers.py",
                spec.id
            );
            for (model, _) in spec.models {
                assert!(
                    source.contains(&format!("(\"{model}\"")),
                    "{}/{model} falta en providers.py",
                    spec.id
                );
            }
        }
    }

    #[test]
    fn guardar_y_quitar_claves_conserva_el_resto_del_json() {
        let (root, dir) = temp_root("keys");
        let setup = AiSetup::from_root(root);
        std::fs::create_dir_all(setup.secrets_path().parent().unwrap()).unwrap();
        std::fs::write(
            setup.secrets_path(),
            r#"{"openrouter": {"models": ["x/y"]}, "otro": 1}"#,
        )
        .unwrap();

        setup
            .save_key("openrouter", "sk-or-abcdef1234")
            .expect("guardar");
        let status = setup
            .status()
            .into_iter()
            .find(|s| s.spec.id == "openrouter")
            .unwrap();
        assert!(status.configured);
        assert_eq!(
            status.source,
            CredentialSource::Saved {
                last4: "1234".to_owned()
            }
        );
        assert!(setup.models().contains(&"openrouter/x/y".to_owned()));

        setup.remove_key("openrouter").expect("quitar");
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(setup.secrets_path()).unwrap()).unwrap();
        assert_eq!(saved["otro"], 1);
        assert_eq!(saved["openrouter"]["models"][0], "x/y");
        assert!(saved["openrouter"].get("apiKey").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn claves_invalidas_y_claude_code_se_rechazan() {
        let (root, dir) = temp_root("invalid");
        let setup = AiSetup::from_root(root);
        assert!(matches!(
            setup.save_key("opencode", "corta"),
            Err(AiSetupError::Invalid(_))
        ));
        assert!(matches!(
            setup.save_key("opencode", "con espacios 12345"),
            Err(AiSetupError::Invalid(_))
        ));
        assert!(matches!(
            setup.save_key("claude-code", "loquesea12345"),
            Err(AiSetupError::Invalid(_))
        ));
        assert!(matches!(
            setup.save_key("nope", "loquesea12345"),
            Err(AiSetupError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn probe_valida_el_modelo_antes_de_lanzar_nada() {
        let (root, dir) = temp_root("probe");
        let setup = AiSetup {
            python: std::env::current_exe().unwrap(),
            ..AiSetup::from_root(root)
        };
        assert!(matches!(
            setup.probe("sin-barra"),
            Err(AiSetupError::Invalid(_))
        ));
        assert!(matches!(
            setup.probe("desconocido/modelo"),
            Err(AiSetupError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sin_agente_la_prueba_explica_que_falta() {
        let (root, dir) = temp_root("missing");
        let setup = AiSetup {
            python: dir.join("no-existe.exe"),
            ..AiSetup::from_root(root)
        };
        let error = setup.probe("opencode/glm-5.3-flash").unwrap_err();
        assert!(matches!(error, AiSetupError::MissingExe(_)), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn el_resumen_de_errores_toma_las_ultimas_lineas() {
        let message = summarize_failure("a\n\nb\nc\nd\ne\n", "");
        assert_eq!(message, "b c d e");
        assert_eq!(summarize_failure("", ""), "sin salida de error");
    }
}
