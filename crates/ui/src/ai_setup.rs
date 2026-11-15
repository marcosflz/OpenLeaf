//! Gestión de proveedores y modelos de IA (Ajustes › Claves de API y Suscripciones).
//!
//! El agente LaTeX (`python -m lsx_agent`, sobre nas-adk) admite varias familias de
//! proveedores y ninguna viene impuesta. Como en nexus, se separan en dos:
//!
//! | Familia            | Proveedores                                              | Credencial                         |
//! |--------------------|----------------------------------------------------------|------------------------------------|
//! | **Claves de API**  | OpenRouter, OpenAI, Anthropic, Gemini, DeepSeek, LM Studio | clave en `config/secrets/ai.json`  |
//! | **Suscripciones**  | OpenCode Go (clave), Claude (CLI `claude`)               | clave o sesión del CLI             |
//!
//! Varios proveedores pueden estar activos a la vez. Por proveedor se guarda, en
//! `config/secrets/ai.json`:
//!
//! ```json
//! {"<id>": {"apiKey": "...", "enabled": true, "baseUrl": "...",
//!           "models": [{"id": "...", "name": "..."}]}}
//! ```
//!
//! - `enabled`: interruptor; `false` lo saca del chat sin borrar la clave.
//! - `models`: los modelos **activos** (los que ve el selector del chat). Sin la clave se usan
//!   los sugeridos de [`PROVIDERS`]; una lista de cadenas es el formato antiguo («modelos
//!   extra») y se suma a los sugeridos.
//! - `baseUrl`: solo en proveedores con URL configurable (LM Studio).
//!
//! El modelo **predeterminado** es `ai.model` de `config/settings.json` (`proveedor/modelo`).
//!
//! Las claves también se aceptan por variable de entorno (`OPENROUTER_API_KEY`, …). La tabla
//! [`PROVIDERS`] replica la de `python/lsx_agent/providers.py`; un test comprueba que no
//! divergen, y otro que las dos leen igual los modelos activos.
//!
//! La UI **nunca** bloquea el hilo de GPUI: [`AiSetup::probe`] y [`AiSetup::list_models`]
//! lanzan un proceso y el llamador las ejecuta dentro de `cx.background_spawn`. El resto de
//! operaciones solo leen o escriben un JSON pequeño.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use lsx_core::root::PortableRoot;
use serde_json::{Map, Value, json};

/// Presupuesto del botón «Probar» (una petición mínima al modelo).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(90);
/// Presupuesto de la consulta del catálogo de un proveedor (el agente corta a los 12 s).
pub const LIST_TIMEOUT: Duration = Duration::from_secs(30);
/// Máximo de modelos activos por proveedor (protege el selector del chat).
pub const MAX_ACTIVE_MODELS: usize = 500;

/// Familia de un proveedor: cada una tiene su sección en Ajustes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    /// Servicios que se pagan por uso con una clave de API.
    Api,
    /// Planes personales (OpenCode Go, Claude…).
    Subscription,
}

impl ProviderKind {
    /// Valor de `kind` en el catálogo del agente.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Subscription => "subscription",
        }
    }
}

/// Cómo se conecta un proveedor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFlow {
    /// Con una clave de API (opcional si `needs_key` es `false`).
    Key,
    /// Con la sesión del CLI `claude` (`claude auth login`).
    Cli,
}

/// Proveedor de IA que el agente sabe usar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderSpec {
    /// Id de `ai.model` (`<id>/<modelo>`).
    pub id: &'static str,
    /// Nombre visible.
    pub name: &'static str,
    /// Sección de Ajustes donde aparece.
    pub kind: ProviderKind,
    /// Cómo se vincula.
    pub link: LinkFlow,
    /// Variable de entorno con la clave (`None`: sin clave, p. ej. `claude-code`).
    pub env_key: Option<&'static str>,
    /// La clave es obligatoria.
    pub needs_key: bool,
    /// Modelos sugeridos `(id, nombre)`: los activos hasta que la persona elija.
    pub models: &'static [(&'static str, &'static str)],
    /// Ayuda corta para la tarjeta.
    pub help: &'static str,
    /// Pista del campo de clave.
    pub placeholder: &'static str,
    /// URL base propuesta, si la persona puede cambiarla (`None`: fija).
    pub custom_base_url: Option<&'static str>,
    /// Se puede pedir su catálogo de modelos.
    pub can_list: bool,
}

/// Proveedores soportados (mismo orden, ids y modelos que `providers.py`).
pub const PROVIDERS: [ProviderSpec; 8] = [
    // --- Claves de API --------------------------------------------------------------
    ProviderSpec {
        id: "openrouter",
        name: "OpenRouter",
        kind: ProviderKind::Api,
        link: LinkFlow::Key,
        env_key: Some("OPENROUTER_API_KEY"),
        needs_key: true,
        models: &[("openrouter/auto", "Auto (OpenRouter elige)")],
        help: "Clave de openrouter.ai; da acceso a modelos de muchos proveedores.",
        placeholder: "sk-or-v1-…",
        custom_base_url: None,
        can_list: true,
    },
    ProviderSpec {
        id: "openai",
        name: "OpenAI",
        kind: ProviderKind::Api,
        link: LinkFlow::Key,
        env_key: Some("OPENAI_API_KEY"),
        needs_key: true,
        models: &[("gpt-5.5", "GPT-5.5")],
        help: "Clave de platform.openai.com (facturación por uso).",
        placeholder: "sk-…",
        custom_base_url: None,
        can_list: true,
    },
    ProviderSpec {
        id: "anthropic",
        name: "Anthropic (API)",
        kind: ProviderKind::Api,
        link: LinkFlow::Key,
        env_key: Some("ANTHROPIC_API_KEY"),
        needs_key: true,
        models: &[
            ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
            ("claude-opus-5-5", "Claude Opus 5.5"),
            ("claude-haiku-5-5", "Claude Haiku 5.5"),
        ],
        help: "Clave de la API de Anthropic (facturación por uso).",
        placeholder: "sk-ant-…",
        custom_base_url: None,
        can_list: true,
    },
    ProviderSpec {
        id: "gemini",
        name: "Google Gemini",
        kind: ProviderKind::Api,
        link: LinkFlow::Key,
        env_key: Some("GEMINI_API_KEY"),
        needs_key: true,
        models: &[("gemini-3-flash", "Gemini 3 Flash")],
        help: "Clave de Google AI Studio.",
        placeholder: "AIza…",
        custom_base_url: None,
        can_list: true,
    },
    ProviderSpec {
        id: "deepseek",
        name: "DeepSeek",
        kind: ProviderKind::Api,
        link: LinkFlow::Key,
        env_key: Some("DEEPSEEK_API_KEY"),
        needs_key: true,
        models: &[
            ("deepseek-chat", "DeepSeek Chat"),
            ("deepseek-reasoner", "DeepSeek Reasoner"),
        ],
        help: "Clave de platform.deepseek.com.",
        placeholder: "sk-…",
        custom_base_url: None,
        can_list: true,
    },
    ProviderSpec {
        id: "lmstudio",
        name: "LM Studio / OpenAI-compatible local",
        kind: ProviderKind::Api,
        link: LinkFlow::Key,
        env_key: Some("LMSTUDIO_API_KEY"),
        needs_key: false,
        models: &[("local-model", "Modelo cargado en el servidor")],
        help: "Servidor local compatible con OpenAI (LM Studio, Ollama…). La clave es opcional.",
        placeholder: "opcional",
        custom_base_url: Some("http://localhost:1234/v1"),
        can_list: true,
    },
    // --- Suscripciones -------------------------------------------------------------
    ProviderSpec {
        id: "opencode",
        name: "OpenCode Go",
        kind: ProviderKind::Subscription,
        link: LinkFlow::Key,
        env_key: Some("OPENCODE_API_KEY"),
        needs_key: true,
        models: &[
            ("deepseek-v4.1-flash", "DeepSeek V4.1 Flash"),
            ("glm-5.3-flash", "GLM 5.3 Flash"),
            ("qwen3.8-flash", "Qwen 3.8 Flash"),
        ],
        help: "Tu suscripción de OpenCode Go, con la clave de opencode.ai/console/go.",
        placeholder: "clave de opencode.ai/console/go",
        custom_base_url: None,
        can_list: true,
    },
    ProviderSpec {
        id: "claude-code",
        name: "Claude (suscripción)",
        kind: ProviderKind::Subscription,
        link: LinkFlow::Cli,
        env_key: None,
        needs_key: false,
        models: &[
            ("default", "Modelo por defecto del CLI"),
            ("sonnet", "Sonnet"),
            ("opus", "Opus"),
            ("haiku", "Haiku"),
        ],
        help: "Tu plan de Claude (Pro o Max) a través del CLI `claude`. Uso personal y local.",
        placeholder: "",
        custom_base_url: None,
        can_list: true,
    },
];

/// Busca un proveedor por id.
pub fn provider(id: &str) -> Option<&'static ProviderSpec> {
    PROVIDERS.iter().find(|spec| spec.id == id)
}

/// Proveedores de una familia, en el orden de la tabla.
pub fn providers_of(kind: ProviderKind) -> impl Iterator<Item = &'static ProviderSpec> {
    PROVIDERS.iter().filter(move |spec| spec.kind == kind)
}

/// Modelo activo de un proveedor (id que se envía + nombre visible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
}

impl ModelChoice {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let name = name.into();
        let name = if name.trim().is_empty() {
            id.clone()
        } else {
            name
        };
        Self { id, name }
    }
}

/// Estado de un proveedor para pintar en Ajustes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
    pub spec: &'static ProviderSpec,
    /// Listo para el chat: con credencial, activado y con algún modelo activo.
    pub configured: bool,
    /// Hay credencial (clave guardada, variable de entorno, CLI presente o, sin clave
    /// obligatoria, el proveedor añadido).
    pub has_credential: bool,
    /// Interruptor de la tarjeta.
    pub enabled: bool,
    /// La persona lo añadió (hay entrada en `ai.json`) o tiene credencial: se muestra su
    /// tarjeta en «Claves de API».
    pub added: bool,
    /// De dónde sale la credencial, para explicarlo en la UI.
    pub source: CredentialSource,
    /// Modelos activos.
    pub models: Vec<ModelChoice>,
    /// URL base efectiva (solo proveedores con URL configurable).
    pub base_url: Option<String>,
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
    #[error("la operación tardó más de {0} s y se canceló")]
    Timeout(u64),
    /// El proceso terminó con error.
    #[error("la operación falló (código {code:?}): {message}")]
    Command {
        /// Código de salida (puede faltar si lo mató una señal).
        code: Option<i32>,
        /// Últimas líneas de stderr/stdout, ya recortadas.
        message: String,
    },
    /// El proveedor rechazó la petición (clave inválida, sin red…).
    #[error("{0}")]
    Provider(String),
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

/// Configuración de IA de una instalación: intérprete del agente y proveedores.
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

    /// Estado de todos los proveedores (lectura local, sin red).
    pub fn status(&self) -> Vec<ProviderStatus> {
        let secrets = self.load_secrets();
        PROVIDERS
            .iter()
            .map(|spec| provider_status(spec, &secrets))
            .collect()
    }

    /// Modelos `proveedor/modelo` activos de los proveedores listos para el chat.
    pub fn models(&self) -> Vec<String> {
        self.status()
            .into_iter()
            .filter(|status| status.configured)
            .flat_map(|status| {
                status
                    .models
                    .into_iter()
                    .map(move |model| format!("{}/{}", status.spec.id, model.id))
            })
            .collect()
    }

    /// Guarda (o sustituye) la clave de un proveedor con clave. Si es nuevo, nace activado
    /// con sus modelos sugeridos.
    pub fn save_key(&self, provider_id: &str, key: &str) -> Result<(), AiSetupError> {
        let spec = known(provider_id)?;
        if spec.link != LinkFlow::Key {
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
        self.update_entry(spec, |entry| {
            entry.insert("apiKey".to_owned(), Value::String(key.to_owned()));
        })
    }

    /// Borra la clave guardada de un proveedor y conserva el resto (modelos, interruptor).
    /// Las variables de entorno no se tocan.
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

    /// Añade un proveedor sin clave obligatoria (LM Studio) o deja constancia de uno con
    /// interruptor propio. No toca lo que ya hubiera.
    pub fn add_provider(&self, provider_id: &str) -> Result<(), AiSetupError> {
        let spec = known(provider_id)?;
        self.update_entry(spec, |_| {})
    }

    /// Quita un proveedor por completo: clave, modelos e interruptor.
    pub fn remove_provider(&self, provider_id: &str) -> Result<(), AiSetupError> {
        let mut secrets = self.load_secrets();
        if secrets.remove(provider_id).is_some() {
            self.store_secrets(&secrets)?;
        }
        Ok(())
    }

    /// Activa o desactiva un proveedor sin borrar su credencial.
    pub fn set_enabled(&self, provider_id: &str, enabled: bool) -> Result<(), AiSetupError> {
        let spec = known(provider_id)?;
        self.update_entry(spec, |entry| {
            entry.insert("enabled".to_owned(), Value::Bool(enabled));
        })
    }

    /// Fija los modelos activos (orden incluido). Debe quedar al menos uno.
    pub fn set_models(
        &self,
        provider_id: &str,
        models: &[ModelChoice],
    ) -> Result<Vec<ModelChoice>, AiSetupError> {
        let spec = known(provider_id)?;
        let models = normalize_models(models);
        if models.is_empty() {
            return Err(AiSetupError::Invalid(
                "elige al menos un modelo activo".to_owned(),
            ));
        }
        if models.len() > MAX_ACTIVE_MODELS {
            return Err(AiSetupError::Invalid(format!(
                "como mucho {MAX_ACTIVE_MODELS} modelos activos por proveedor"
            )));
        }
        let list: Vec<Value> = models
            .iter()
            .map(|model| json!({"id": model.id, "name": model.name}))
            .collect();
        self.update_entry(spec, |entry| {
            entry.insert("models".to_owned(), Value::Array(list));
        })?;
        Ok(models)
    }

    /// Cambia la URL base de un proveedor que la admite (`http(s)://…`). Vacía: la propuesta.
    pub fn set_base_url(&self, provider_id: &str, url: &str) -> Result<(), AiSetupError> {
        let spec = known(provider_id)?;
        if spec.custom_base_url.is_none() {
            return Err(AiSetupError::Invalid(format!(
                "{} usa una URL fija",
                spec.name
            )));
        }
        let url = url.trim().trim_end_matches('/');
        if !url.is_empty()
            && (!(url.starts_with("http://") || url.starts_with("https://"))
                || url.chars().any(char::is_whitespace)
                || url.len() > 300)
        {
            return Err(AiSetupError::Invalid(
                "la URL base debe empezar por http:// o https://".to_owned(),
            ));
        }
        self.update_entry(spec, |entry| {
            if url.is_empty() {
                entry.remove("baseUrl");
            } else {
                entry.insert("baseUrl".to_owned(), Value::String(url.to_owned()));
            }
        })
    }

    /// Catálogo completo del proveedor (con la credencial guardada) para elegir los modelos
    /// activos. Bloquea: ejecútalo en segundo plano.
    pub fn list_models(&self, provider_id: &str) -> Result<Vec<ModelChoice>, AiSetupError> {
        let spec = known(provider_id)?;
        if !spec.can_list {
            return Err(AiSetupError::Invalid(
                "este proveedor no permite listar modelos: escribe su nombre".to_owned(),
            ));
        }
        let reply = self.run_agent(&["--list-models", spec.id], LIST_TIMEOUT)?;
        if reply.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(AiSetupError::Provider(
                reply
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("respuesta vacía")
                    .to_owned(),
            ));
        }
        Ok(reply
            .get("models")
            .and_then(Value::as_array)
            .map(|items| parse_models(items))
            .unwrap_or_default())
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
        let target = format!("{provider_id}/{model_id}");
        let reply = self.run_agent(&["--probe", &target], PROBE_TIMEOUT)?;
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

    /// Abre una consola con `claude auth login` para vincular la suscripción de Claude. El
    /// inicio de sesión lo hace la persona en el navegador; la app no ve sus credenciales.
    pub fn login_claude(&self) -> Result<(), AiSetupError> {
        let bin = std::env::var("CLAUDE_CODE_BIN").unwrap_or_else(|_| "claude".to_owned());
        let Some(cli) = find_on_path(&bin) else {
            return Err(AiSetupError::Invalid(
                "no se encontró el CLI `claude`: instálalo desde claude.com/claude-code".to_owned(),
            ));
        };
        open_console(&cli, &["auth", "login"])
    }

    /// Ejecuta `python -m lsx_agent <args>` y devuelve su JSON de una línea.
    fn run_agent(&self, args: &[&str], timeout: Duration) -> Result<Value, AiSetupError> {
        if !self.agent_exists() {
            return Err(AiSetupError::MissingExe(self.python.display().to_string()));
        }
        let mut command = Command::new(&self.python);
        command
            .args(["-P", "-s", "-m", "lsx_agent"])
            .args(args)
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
        let output = run_with_timeout(command, timeout)?;
        serde_json::from_str(output.stdout.trim()).map_err(|_| AiSetupError::Command {
            code: None,
            message: summarize_failure("", &output.stdout),
        })
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

    /// Modifica (o crea) la entrada de un proveedor y la guarda de forma atómica.
    fn update_entry(
        &self,
        spec: &ProviderSpec,
        change: impl FnOnce(&mut Map<String, Value>),
    ) -> Result<(), AiSetupError> {
        let mut secrets = self.load_secrets();
        let entry = secrets
            .entry(spec.id.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        if let Some(object) = entry.as_object_mut() {
            change(object);
        }
        self.store_secrets(&secrets)
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

fn known(provider_id: &str) -> Result<&'static ProviderSpec, AiSetupError> {
    provider(provider_id)
        .ok_or_else(|| AiSetupError::Invalid(format!("proveedor desconocido: {provider_id}")))
}

fn provider_status(spec: &'static ProviderSpec, secrets: &Map<String, Value>) -> ProviderStatus {
    let entry = secrets.get(spec.id).and_then(Value::as_object);
    let source = credential_source(spec, entry);
    let has_credential = match spec.link {
        LinkFlow::Cli => source != CredentialSource::None,
        LinkFlow::Key if !spec.needs_key => entry.is_some() || source != CredentialSource::None,
        LinkFlow::Key => source != CredentialSource::None,
    };
    let enabled = entry
        .and_then(|entry| entry.get("enabled"))
        .and_then(Value::as_bool)
        != Some(false);
    let models = active_models(spec, entry);
    let base_url = spec.custom_base_url.map(|default| {
        entry
            .and_then(|entry| entry.get("baseUrl"))
            .and_then(Value::as_str)
            .map(|url| url.trim().trim_end_matches('/'))
            .filter(|url| !url.is_empty())
            .unwrap_or(default)
            .to_owned()
    });
    ProviderStatus {
        spec,
        configured: has_credential && enabled && !models.is_empty(),
        has_credential,
        enabled,
        added: entry.is_some() || has_credential,
        source,
        models,
        base_url,
    }
}

fn credential_source(spec: &ProviderSpec, entry: Option<&Map<String, Value>>) -> CredentialSource {
    let Some(env_key) = spec.env_key else {
        let bin = std::env::var("CLAUDE_CODE_BIN").unwrap_or_else(|_| "claude".to_owned());
        return find_on_path(&bin).map_or(CredentialSource::None, CredentialSource::Cli);
    };
    if let Some(key) = entry
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

/// Modelos activos, con la misma regla que `Secrets.active_models` de `providers.py`.
fn active_models(spec: &ProviderSpec, entry: Option<&Map<String, Value>>) -> Vec<ModelChoice> {
    let suggested = || {
        spec.models
            .iter()
            .map(|(id, name)| ModelChoice::new(*id, *name))
            .collect::<Vec<_>>()
    };
    let Some(raw) = entry
        .and_then(|entry| entry.get("models"))
        .and_then(Value::as_array)
    else {
        return suggested();
    };
    let legacy = raw.iter().all(Value::is_string);
    let mut out = if legacy { suggested() } else { Vec::new() };
    out.extend(parse_models(raw));
    normalize_models(&out)
}

/// `[{"id", "name"}]` o `["id"]` → modelos (sin validar duplicados).
fn parse_models(items: &[Value]) -> Vec<ModelChoice> {
    items
        .iter()
        .filter_map(|item| match item {
            Value::String(id) => Some(ModelChoice::new(id.trim(), id.trim())),
            Value::Object(object) => {
                let id = object.get("id").and_then(Value::as_str)?.trim();
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or_default();
                Some(ModelChoice::new(id, name))
            }
            _ => None,
        })
        .collect()
}

/// Quita vacíos y duplicados conservando el orden.
fn normalize_models(models: &[ModelChoice]) -> Vec<ModelChoice> {
    let mut seen = std::collections::HashSet::new();
    models
        .iter()
        .filter_map(|model| {
            let id = model.id.trim();
            (!id.is_empty() && seen.insert(id.to_owned()))
                .then(|| ModelChoice::new(id, model.name.trim()))
        })
        .collect()
}

/// Abre una consola visible que ejecuta `program args…` (inicio de sesión interactivo).
fn open_console(program: &Path, args: &[&str]) -> Result<(), AiSetupError> {
    let io = |error: std::io::Error| AiSetupError::Io {
        path: program.display().to_string(),
        error: error.to_string(),
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        // `cmd /k` deja la ventana abierta para leer el resultado; los `.cmd` de npm
        // necesitan pasar por `cmd` de todos modos.
        Command::new("cmd")
            .arg("/k")
            .arg(program)
            .args(args)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map(|_| ())
            .map_err(io)
    }
    #[cfg(not(windows))]
    {
        Command::new(program)
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(io)
    }
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
                    "no se pudo esperar al asistente: {error}"
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

    fn status_of(setup: &AiSetup, id: &str) -> ProviderStatus {
        setup
            .status()
            .into_iter()
            .find(|s| s.spec.id == id)
            .expect("proveedor")
    }

    #[test]
    fn la_tabla_de_proveedores_coincide_con_la_del_agente() {
        let python =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/lsx_agent/providers.py");
        let source = std::fs::read_to_string(python).expect("providers.py");
        let mut last = 0;
        for spec in &PROVIDERS {
            let marker = format!("id=\"{}\"", spec.id);
            let at = source
                .find(&marker)
                .unwrap_or_else(|| panic!("{} falta en providers.py", spec.id));
            assert!(at > last, "{} está en otro orden en providers.py", spec.id);
            last = at;
            let block = &source[at..];
            let block = &block[..block.find("ProviderSpec(").unwrap_or(block.len())];
            assert!(
                block.contains(&format!("kind=\"{}\"", spec.kind.as_str())),
                "{}: familia distinta en providers.py",
                spec.id
            );
            for (model, _) in spec.models {
                assert!(
                    block.contains(&format!("(\"{model}\"")),
                    "{}/{model} falta en providers.py",
                    spec.id
                );
            }
            if let Some(env) = spec.env_key {
                assert!(block.contains(&format!("env_key=\"{env}\"")), "{env}");
            }
            assert_eq!(
                block.contains("needs_key=False"),
                !spec.needs_key,
                "{}: needs_key",
                spec.id
            );
            assert_eq!(
                block.contains("custom_base_url=True"),
                spec.custom_base_url.is_some(),
                "{}: custom_base_url",
                spec.id
            );
        }
    }

    #[test]
    fn cada_familia_tiene_sus_proveedores() {
        let api: Vec<_> = providers_of(ProviderKind::Api).map(|s| s.id).collect();
        let subs: Vec<_> = providers_of(ProviderKind::Subscription)
            .map(|s| s.id)
            .collect();
        assert!(api.contains(&"openrouter") && api.contains(&"anthropic"));
        assert_eq!(subs, ["opencode", "claude-code"]);
        assert_eq!(api.len() + subs.len(), PROVIDERS.len());
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
        let status = status_of(&setup, "openrouter");
        assert!(status.configured);
        assert_eq!(
            status.source,
            CredentialSource::Saved {
                last4: "1234".to_owned()
            }
        );
        // Formato antiguo: los «modelos extra» se suman a los sugeridos.
        assert!(
            setup
                .models()
                .contains(&"openrouter/openrouter/auto".to_owned())
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
    fn varios_proveedores_activos_y_el_interruptor_no_borra_la_clave() {
        let (root, dir) = temp_root("enabled");
        let setup = AiSetup::from_root(root);
        setup.save_key("anthropic", "sk-ant-12345678").unwrap();
        setup.save_key("opencode", "oc-12345678").unwrap();
        let models = setup.models();
        assert!(models.contains(&"anthropic/claude-opus-5-5".to_owned()));
        assert!(models.contains(&"opencode/glm-5.3-flash".to_owned()));

        setup.set_enabled("anthropic", false).unwrap();
        let status = status_of(&setup, "anthropic");
        assert!(!status.configured && !status.enabled && status.has_credential);
        assert!(setup.models().iter().all(|m| !m.starts_with("anthropic/")));

        setup.set_enabled("anthropic", true).unwrap();
        assert!(status_of(&setup, "anthropic").configured);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn los_modelos_activos_sustituyen_a_los_sugeridos() {
        let (root, dir) = temp_root("models");
        let setup = AiSetup::from_root(root);
        setup.save_key("openrouter", "sk-or-12345678").unwrap();
        let saved = setup
            .set_models(
                "openrouter",
                &[
                    ModelChoice::new("anthropic/claude-opus-5-5", "Opus"),
                    ModelChoice::new("anthropic/claude-opus-5-5", "duplicado"),
                    ModelChoice::new("  ", ""),
                    ModelChoice::new("openai/gpt-5.5", ""),
                ],
            )
            .unwrap();
        assert_eq!(
            saved,
            [
                ModelChoice::new("anthropic/claude-opus-5-5", "Opus"),
                ModelChoice::new("openai/gpt-5.5", "openai/gpt-5.5"),
            ]
        );
        // Filtra por proveedor: el CLI `claude` puede estar instalado en la máquina.
        let openrouter: Vec<_> = setup
            .models()
            .into_iter()
            .filter(|m| m.starts_with("openrouter/"))
            .collect();
        assert_eq!(
            openrouter,
            [
                "openrouter/anthropic/claude-opus-5-5",
                "openrouter/openai/gpt-5.5"
            ]
        );
        assert!(matches!(
            setup.set_models("openrouter", &[]),
            Err(AiSetupError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn un_servidor_local_se_anade_sin_clave_y_valida_su_url() {
        let (root, dir) = temp_root("local");
        let setup = AiSetup::from_root(root);
        let before = status_of(&setup, "lmstudio");
        assert!(!before.added && !before.configured);
        assert_eq!(before.base_url.as_deref(), Some("http://localhost:1234/v1"));

        setup.add_provider("lmstudio").unwrap();
        assert!(status_of(&setup, "lmstudio").configured);
        setup
            .set_base_url("lmstudio", "http://127.0.0.1:9999/v1/")
            .unwrap();
        assert_eq!(
            status_of(&setup, "lmstudio").base_url.as_deref(),
            Some("http://127.0.0.1:9999/v1")
        );
        assert!(setup.set_base_url("lmstudio", "ftp://x").is_err());
        assert!(setup.set_base_url("openai", "http://evil.test").is_err());

        setup.remove_provider("lmstudio").unwrap();
        assert!(!status_of(&setup, "lmstudio").added);
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
        assert!(matches!(
            setup.list_models("desconocido"),
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
        let error = setup.list_models("openrouter").unwrap_err();
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
