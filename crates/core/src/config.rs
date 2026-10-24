//! `config/settings.json` — esquema v1 (M1-A0, `INTERFACES.md` §7).
//!
//! Reglas de oro del cargador:
//!
//! - **Nunca falla**: un fichero corrupto, un campo con tipo equivocado o un
//!   valor fuera de rango producen *defaults* + [`ConfigWarning`]s, jamás un
//!   error que impida arrancar.
//! - **Forward-compatible**: los campos desconocidos se preservan en
//!   `extra` por sección y se vuelven a escribir tal cual al guardar.
//! - **Escritura atómica**: temp + `sync_all` + rename (mismo volumen).
//!
//! `ui.dock` es un [`serde_json::Value`] **opaco**: el layout del dock es
//! propiedad de `lsx-ui` y aquí solo se transporta sin interpretarlo.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use crate::root::PortableRoot;

/// Versión del esquema que este build escribe en `$schemaVersion`.
pub const SCHEMA_VERSION: u32 = 1;

/// Nombre del fichero dentro de `<root>/config`.
pub const SETTINGS_FILE: &str = "settings.json";

/// Campos desconocidos de una sección, preservados en orden estable.
pub type ExtraFields = BTreeMap<String, Value>;

fn default_true() -> bool {
    true
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

/// Aviso no fatal producido al cargar/validar la configuración.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWarning {
    /// Clave (en notación JSON) a la que se refiere el aviso.
    pub key: String,
    /// Mensaje orientado al usuario (español).
    pub message: String,
}

impl ConfigWarning {
    /// Crea un aviso.
    pub fn new(key: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.key.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.key, self.message)
        }
    }
}

/// Configuración completa de la aplicación.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Versión del esquema (clave `$schemaVersion`).
    #[serde(rename = "$schemaVersion", default = "default_schema_version")]
    pub schema_version: u32,
    /// Último proyecto y recientes.
    #[serde(default)]
    pub project: ProjectSettings,
    /// Preferencias del editor.
    #[serde(default)]
    pub editor: EditorSettings,
    /// Motor LaTeX y compilación.
    #[serde(default)]
    pub latex: LatexSettings,
    /// Visor PDF.
    #[serde(default)]
    pub pdf: PdfSettings,
    /// Asistente IA (opencode).
    #[serde(default)]
    pub ai: AiSettings,
    /// Shell: tema, idioma, layout del dock.
    #[serde(default)]
    pub ui: UiSettings,
    /// Opciones avanzadas.
    #[serde(default)]
    pub advanced: AdvancedSettings,
    /// Campos de nivel raíz no reconocidos (forward-compat).
    #[serde(flatten)]
    pub extra: ExtraFields,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            project: ProjectSettings::default(),
            editor: EditorSettings::default(),
            latex: LatexSettings::default(),
            pdf: PdfSettings::default(),
            ai: AiSettings::default(),
            ui: UiSettings::default(),
            advanced: AdvancedSettings::default(),
            extra: ExtraFields::new(),
        }
    }
}

/// Proyectos: último abierto y recientes (rutas relativas a la raíz, `/`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettings {
    /// Último proyecto abierto, p. ej. `workspace/cv`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    /// Recientes, más nuevo primero.
    #[serde(default)]
    pub recent: Vec<String>,
    /// Campos desconocidos.
    #[serde(flatten)]
    pub extra: ExtraFields,
}

/// Preferencias del editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSettings {
    /// Familia tipográfica; `None` = la del sistema (M1 no bundlea fuentes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// Tamaño en px.
    #[serde(default = "editor_font_size")]
    pub font_size: f32,
    /// Tamaño de tabulación en espacios.
    #[serde(default = "editor_tab_size")]
    pub tab_size: u32,
    /// Ajuste de línea.
    #[serde(default)]
    pub soft_wrap: bool,
    /// Autoguardado.
    #[serde(default)]
    pub autosave: AutosaveSettings,
    /// Mostrar espacios en blanco.
    #[serde(default)]
    pub show_whitespace: bool,
    /// Campos desconocidos.
    #[serde(flatten)]
    pub extra: ExtraFields,
}

fn editor_font_size() -> f32 {
    14.0
}

fn editor_tab_size() -> u32 {
    2
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: editor_font_size(),
            tab_size: editor_tab_size(),
            soft_wrap: false,
            autosave: AutosaveSettings::default(),
            show_whitespace: false,
            extra: ExtraFields::new(),
        }
    }
}

/// Autoguardado del editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutosaveSettings {
    /// Si está activo.
    #[serde(default)]
    pub enabled: bool,
    /// Intervalo en segundos.
    #[serde(default = "autosave_interval")]
    pub interval_secs: u32,
    /// Campos desconocidos.
    #[serde(flatten)]
    pub extra: ExtraFields,
}

fn autosave_interval() -> u32 {
    30
}

impl Default for AutosaveSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: autosave_interval(),
            extra: ExtraFields::new(),
        }
    }
}

/// Motor LaTeX y política de compilación.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatexSettings {
    /// `texlive` (v1) o `tectonic` (M4).
    #[serde(default = "latex_engine")]
    pub engine: String,
    /// Fichero principal, relativo al proyecto; `None` = autodetectar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_file: Option<String>,
    /// Compilar al guardar.
    #[serde(default = "default_true")]
    pub auto_compile: bool,
    /// Debounce de recompilación en ms.
    #[serde(default = "latex_debounce")]
    pub debounce_ms: u32,
    /// Auto-instalación de paquetes con tlmgr.
    #[serde(default = "default_true")]
    pub auto_install_packages: bool,
    /// Argumentos extra para latexmk.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Timeout de compilación en segundos.
    #[serde(default = "latex_timeout")]
    pub timeout_secs: u64,
    /// Limpiar auxiliares tras compilar con éxito.
    #[serde(default)]
    pub clean_on_success: bool,
    /// Sincronización de scroll `.tex` ↔ PDF (SyncTeX automático).
    /// Con `true`, al mover el cursor el visor se desplaza solo, y al
    /// desplazar el visor el editor se desplaza (sin mover el cursor).
    #[serde(default = "default_true")]
    pub sync_scroll: bool,
    /// Campos desconocidos.
    #[serde(flatten)]
    pub extra: ExtraFields,
}

fn latex_engine() -> String {
    "texlive".to_owned()
}

fn latex_debounce() -> u32 {
    400
}

fn latex_timeout() -> u64 {
    600
}

impl Default for LatexSettings {
    fn default() -> Self {
        Self {
            engine: latex_engine(),
            main_file: None,
            auto_compile: true,
            debounce_ms: latex_debounce(),
            auto_install_packages: true,
            extra_args: Vec::new(),
            timeout_secs: latex_timeout(),
            clean_on_success: false,
            sync_scroll: true,
            extra: ExtraFields::new(),
        }
    }
}

/// Visor PDF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfSettings {
    /// `fit-width`, `fit-page`, `actual` o `custom`.
    #[serde(default = "pdf_zoom_mode")]
    pub zoom_mode: String,
    /// Zoom cuando `zoomMode = custom`.
    #[serde(default = "pdf_zoom")]
    pub zoom: f32,
    /// Presupuesto de caché en MB.
    #[serde(default = "pdf_cache", rename = "maxCacheMB")]
    pub max_cache_mb: u32,
    /// Doble clic abre el PDF en el visor externo.
    #[serde(default = "default_true")]
    pub open_external_on_double_click: bool,
    /// Campos desconocidos.
    #[serde(flatten)]
    pub extra: ExtraFields,
}

fn pdf_zoom_mode() -> String {
    "fit-width".to_owned()
}

fn pdf_zoom() -> f32 {
    1.0
}

fn pdf_cache() -> u32 {
    256
}

impl Default for PdfSettings {
    fn default() -> Self {
        Self {
            zoom_mode: pdf_zoom_mode(),
            zoom: pdf_zoom(),
            max_cache_mb: pdf_cache(),
            open_external_on_double_click: true,
            extra: ExtraFields::new(),
        }
    }
}

/// Asistente IA (opencode).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    /// Arrancar el servidor al abrir la app.
    #[serde(default = "default_true")]
    pub auto_start: bool,
    /// Puerto fijo; `None` = puerto libre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Modelo preferido `provider/model`.
    #[serde(default = "ai_model", skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Agente de opencode.
    #[serde(default = "ai_agent")]
    pub agent: String,
    /// `ask` | `auto`.
    #[serde(default = "ai_permission_preset")]
    pub permission_preset: String,
    /// Mostrar el razonamiento plegado.
    #[serde(default = "default_true")]
    pub show_reasoning: bool,
    /// Carpeta de plantillas de prompt.
    #[serde(default = "ai_prompts_dir")]
    pub prompt_templates_dir: String,
    /// Campos desconocidos.
    #[serde(flatten)]
    pub extra: ExtraFields,
}

fn ai_agent() -> String {
    "build".to_owned()
}

/// Modelo por defecto del chat cuando el usuario no elige otro en Ajustes.
///
/// La app lo manda explícitamente en cada prompt: sin esto, opencode resuelve
/// el modelo de su configuración global (el del usuario) y el chat dejaría de
/// usar el que documenta el producto.
pub const DEFAULT_AI_MODEL: &str = "opencode-go/deepseek-v4.1-flash";

fn ai_model() -> Option<String> {
    Some(DEFAULT_AI_MODEL.to_owned())
}

fn ai_permission_preset() -> String {
    "ask".to_owned()
}

fn ai_prompts_dir() -> String {
    "assets/prompts".to_owned()
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            auto_start: true,
            port: None,
            model: ai_model(),
            agent: ai_agent(),
            permission_preset: ai_permission_preset(),
            show_reasoning: true,
            prompt_templates_dir: ai_prompts_dir(),
            extra: ExtraFields::new(),
        }
    }
}

/// Shell: tema, idioma y layout del dock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSettings {
    /// `dark` | `light` | `system`.
    #[serde(default = "ui_theme")]
    pub theme: String,
    /// Acento `#RRGGBB`.
    #[serde(default = "ui_accent")]
    pub accent: String,
    /// Idioma de la UI (v1: `es`).
    #[serde(default = "ui_language")]
    pub language: String,
    /// Layout serializado del dock (opaco para `lsx-core`).
    #[serde(default = "opaque_dock")]
    pub dock: Value,
    /// Última geometría de la ventana principal.
    #[serde(default)]
    pub window: WindowSettings,
    /// Menos animaciones.
    #[serde(default)]
    pub reduced_motion: bool,
    /// Campos desconocidos.
    #[serde(flatten)]
    pub extra: ExtraFields,
}

/// Geometría persistida de la ventana principal.
///
/// `x`/`y` son opcionales para distinguir "primera vez" de "posición 0,0".
/// `maximized` conserva el estado; `width`/`height` son el tamaño de restaurar
/// cuando la ventana no está maximizada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSettings {
    /// Origen X en píxeles lógicos; `None` = centrar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    /// Origen Y en píxeles lógicos; `None` = centrar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    /// Ancho de restaurar en píxeles lógicos.
    #[serde(default = "window_width")]
    pub width: f32,
    /// Alto de restaurar en píxeles lógicos.
    #[serde(default = "window_height")]
    pub height: f32,
    /// Si la ventana estaba maximizada.
    #[serde(default = "default_true")]
    pub maximized: bool,
    /// Campos desconocidos.
    #[serde(flatten)]
    pub extra: ExtraFields,
}

fn window_width() -> f32 {
    1280.0
}

fn window_height() -> f32 {
    800.0
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            width: window_width(),
            height: window_height(),
            maximized: true,
            extra: ExtraFields::new(),
        }
    }
}

fn ui_theme() -> String {
    "dark".to_owned()
}

fn ui_accent() -> String {
    "#4C8DFF".to_owned()
}

fn ui_language() -> String {
    "es".to_owned()
}

fn opaque_dock() -> Value {
    Value::Object(Map::new())
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme: ui_theme(),
            accent: ui_accent(),
            language: ui_language(),
            dock: opaque_dock(),
            window: WindowSettings::default(),
            reduced_motion: false,
            extra: ExtraFields::new(),
        }
    }
}

/// Opciones avanzadas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedSettings {
    /// `trace` | `debug` | `info` | `warn` | `error`.
    #[serde(default = "advanced_log_level")]
    pub log_level: String,
    /// Override de `tex/bin` (rutas raras).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tex_bin_override: Option<String>,
    /// Override del ejecutable de opencode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode_exe_override: Option<String>,
    /// Campos desconocidos.
    #[serde(flatten)]
    pub extra: ExtraFields,
}

fn advanced_log_level() -> String {
    "info".to_owned()
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            log_level: advanced_log_level(),
            tex_bin_override: None,
            opencode_exe_override: None,
            extra: ExtraFields::new(),
        }
    }
}

/// Error de guardado (la lectura nunca falla: ver [`Settings::load`]).
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Error de E/S escribiendo el fichero o el temporal.
    #[error("no se pudo escribir la configuración: {0}")]
    Io(#[from] std::io::Error),
    /// La configuración no se pudo serializar (no debería ocurrir: son datos planos).
    #[error("no se pudo serializar la configuración: {0}")]
    Json(#[from] serde_json::Error),
}

impl Settings {
    /// Ruta del fichero de configuración: `<root>/config/settings.json`.
    pub fn path(root: &PortableRoot) -> PathBuf {
        root.config_dir().join(SETTINGS_FILE)
    }

    /// Carga la configuración de la raíz.
    ///
    /// Sin fichero → defaults sin avisos (primer arranque). Con fichero
    /// ilegible/corrupto/parcial → defaults + avisos por cada campo
    /// descartado o corregido. Nunca devuelve `Err`.
    pub fn load(root: &PortableRoot) -> (Self, Vec<ConfigWarning>) {
        let path = Self::path(root);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), Vec::new());
            }
            Err(err) => {
                return (
                    Self::default(),
                    vec![ConfigWarning::new(
                        "",
                        format!("no se pudo leer {}: {err}", path.display()),
                    )],
                );
            }
        };
        let mut warnings = Vec::new();
        let value: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(err) => {
                warnings.push(ConfigWarning::new(
                    "",
                    format!(
                        "{} no es JSON válido ({err}); se usan los valores por defecto",
                        path.display()
                    ),
                ));
                return (Self::default(), warnings);
            }
        };
        let mut settings = Self::from_value_lenient(value, &mut warnings);
        warnings.extend(settings.validate());
        (settings, warnings)
    }

    /// Guarda la configuración de forma atómica (temp + rename).
    pub fn save(&self, root: &PortableRoot) -> Result<(), ConfigError> {
        let path = Self::path(root);
        if let Some(dir) = path.parent()
            && !dir.exists()
        {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        write_file_atomic(&path, json.as_bytes())?;
        Ok(())
    }

    /// Reconstruye sección a sección sin abortar por un campo mal tipado.
    fn from_value_lenient(value: Value, warnings: &mut Vec<ConfigWarning>) -> Self {
        let Value::Object(mut map) = value else {
            warnings.push(ConfigWarning::new(
                "",
                "el contenido de settings.json no es un objeto JSON; se usan los valores por defecto",
            ));
            return Self::default();
        };

        let schema_version = match map.remove("$schemaVersion") {
            None => SCHEMA_VERSION,
            Some(value) => match serde_json::from_value::<u32>(value) {
                Ok(version) => version,
                Err(err) => {
                    warnings.push(ConfigWarning::new(
                        "$schemaVersion",
                        format!(
                            "versión de esquema ilegible ({err}); se asume la {SCHEMA_VERSION}"
                        ),
                    ));
                    SCHEMA_VERSION
                }
            },
        };
        if schema_version > SCHEMA_VERSION {
            warnings.push(ConfigWarning::new(
                "$schemaVersion",
                format!(
                    "settings.json lo escribió una versión más nueva ({schema_version} > {SCHEMA_VERSION}); se cargan los campos conocidos"
                ),
            ));
        }

        Self {
            schema_version,
            project: take_section(&mut map, "project", warnings),
            editor: take_section(&mut map, "editor", warnings),
            latex: take_section(&mut map, "latex", warnings),
            pdf: take_section(&mut map, "pdf", warnings),
            ai: take_section(&mut map, "ai", warnings),
            ui: take_section(&mut map, "ui", warnings),
            advanced: take_section(&mut map, "advanced", warnings),
            extra: map.into_iter().collect(),
        }
    }

    /// Valida rangos y enums, corrigiendo a defaults/clamps y devolviendo avisos.
    ///
    /// Se usa al cargar y, de nuevo, al guardar desde Ajustes: la UI no puede
    /// colar un valor inválido.
    pub fn validate(&mut self) -> Vec<ConfigWarning> {
        let mut warnings = Vec::new();

        clamp_f32(
            &mut self.editor.font_size,
            6.0,
            72.0,
            "editor.fontSize",
            &mut warnings,
        );
        clamp_u32(
            &mut self.editor.tab_size,
            1,
            16,
            "editor.tabSize",
            &mut warnings,
        );
        clamp_u32(
            &mut self.editor.autosave.interval_secs,
            5,
            3600,
            "editor.autosave.intervalSecs",
            &mut warnings,
        );
        clamp_u32(
            &mut self.latex.debounce_ms,
            0,
            10_000,
            "latex.debounceMs",
            &mut warnings,
        );
        clamp_u64(
            &mut self.latex.timeout_secs,
            5,
            7200,
            "latex.timeoutSecs",
            &mut warnings,
        );
        clamp_f32(&mut self.pdf.zoom, 0.1, 8.0, "pdf.zoom", &mut warnings);
        clamp_u32(
            &mut self.pdf.max_cache_mb,
            16,
            4096,
            "pdf.maxCacheMB",
            &mut warnings,
        );

        if let Some(port) = self.ai.port
            && port < 1024
        {
            warnings.push(ConfigWarning::new(
                "ai.port",
                format!("el puerto {port} es privilegiado; se usará un puerto libre"),
            ));
            self.ai.port = None;
        }

        let engine = self.latex.engine.to_ascii_lowercase();
        if !matches!(engine.as_str(), "texlive" | "tectonic") {
            warnings.push(ConfigWarning::new(
                "latex.engine",
                format!("motor «{engine}» desconocido; se usa texlive"),
            ));
            self.latex.engine = latex_engine();
        }

        let zoom_mode = self.pdf.zoom_mode.to_ascii_lowercase();
        if !matches!(
            zoom_mode.as_str(),
            "fit-width" | "fit-page" | "actual" | "custom"
        ) {
            warnings.push(ConfigWarning::new(
                "pdf.zoomMode",
                format!("modo de zoom «{zoom_mode}» desconocido; se usa fit-width"),
            ));
            self.pdf.zoom_mode = pdf_zoom_mode();
        }

        let theme = self.ui.theme.to_ascii_lowercase();
        if !matches!(theme.as_str(), "dark" | "light" | "system") {
            warnings.push(ConfigWarning::new(
                "ui.theme",
                format!("tema «{theme}» desconocido; se usa dark"),
            ));
            self.ui.theme = ui_theme();
        }

        if parse_accent(&self.ui.accent).is_none() {
            warnings.push(ConfigWarning::new(
                "ui.accent",
                format!(
                    "acento «{}» no es #RRGGBB; se usa {}",
                    self.ui.accent,
                    ui_accent()
                ),
            ));
            self.ui.accent = ui_accent();
        }

        // Ventana: tamaños absurdos o NaN vuelven a los defaults; una posición
        // no finita se descarta para que el shell centre la ventana.
        if !self.ui.window.width.is_finite() || !(640.0..=16_384.0).contains(&self.ui.window.width)
        {
            warnings.push(ConfigWarning::new(
                "ui.window.width",
                "ancho de ventana inválido; se usa 1280",
            ));
            self.ui.window.width = window_width();
        }
        if !self.ui.window.height.is_finite()
            || !(480.0..=16_384.0).contains(&self.ui.window.height)
        {
            warnings.push(ConfigWarning::new(
                "ui.window.height",
                "alto de ventana inválido; se usa 800",
            ));
            self.ui.window.height = window_height();
        }
        if self.ui.window.x.is_some_and(|x| !x.is_finite()) {
            warnings.push(ConfigWarning::new(
                "ui.window.x",
                "posición X inválida; se centrará la ventana",
            ));
            self.ui.window.x = None;
        }
        if self.ui.window.y.is_some_and(|y| !y.is_finite()) {
            warnings.push(ConfigWarning::new(
                "ui.window.y",
                "posición Y inválida; se centrará la ventana",
            ));
            self.ui.window.y = None;
        }

        let language = self.ui.language.to_ascii_lowercase();
        if !matches!(language.as_str(), "es" | "en") {
            warnings.push(ConfigWarning::new(
                "ui.language",
                format!("idioma «{language}» desconocido; se usa es"),
            ));
            self.ui.language = ui_language();
        }

        let log_level = self.advanced.log_level.to_ascii_lowercase();
        if !matches!(
            log_level.as_str(),
            "trace" | "debug" | "info" | "warn" | "error"
        ) {
            warnings.push(ConfigWarning::new(
                "advanced.logLevel",
                format!("nivel de log «{log_level}» desconocido; se usa info"),
            ));
            self.advanced.log_level = advanced_log_level();
        }

        let preset = self.ai.permission_preset.to_ascii_lowercase();
        // `project`/`all` son los presets que escribe el panel de chat;
        // `auto` se acepta como alias histórico de `all`.
        if !matches!(preset.as_str(), "ask" | "project" | "all" | "auto") {
            warnings.push(ConfigWarning::new(
                "ai.permissionPreset",
                format!("preset de permisos «{preset}» desconocido; se usa ask"),
            ));
            self.ai.permission_preset = ai_permission_preset();
        } else if preset == "auto" {
            // Alias histórico del preset que el panel llama `all`.
            self.ai.permission_preset = "all".to_owned();
        }

        warnings
    }
}

/// Extrae y deserializa una sección; en error, default + aviso.
fn take_section<T: DeserializeOwned + Default>(
    map: &mut Map<String, Value>,
    key: &str,
    warnings: &mut Vec<ConfigWarning>,
) -> T {
    match map.remove(key) {
        None => T::default(),
        Some(value) => match serde_json::from_value::<T>(value) {
            Ok(section) => section,
            Err(err) => {
                warnings.push(ConfigWarning::new(
                    key,
                    format!("sección ilegible ({err}); se usan sus valores por defecto"),
                ));
                T::default()
            }
        },
    }
}

fn clamp_f32(value: &mut f32, min: f32, max: f32, key: &str, warnings: &mut Vec<ConfigWarning>) {
    if !value.is_finite() || *value < min || *value > max {
        warnings.push(ConfigWarning::new(
            key,
            format!("valor {value} fuera de [{min}, {max}]; se ajusta"),
        ));
        *value = value.clamp(min, max);
        if !value.is_finite() {
            *value = min;
        }
    }
}

fn clamp_u32(value: &mut u32, min: u32, max: u32, key: &str, warnings: &mut Vec<ConfigWarning>) {
    if *value < min || *value > max {
        warnings.push(ConfigWarning::new(
            key,
            format!("valor {value} fuera de [{min}, {max}]; se ajusta"),
        ));
        *value = (*value).clamp(min, max);
    }
}

fn clamp_u64(value: &mut u64, min: u64, max: u64, key: &str, warnings: &mut Vec<ConfigWarning>) {
    if *value < min || *value > max {
        warnings.push(ConfigWarning::new(
            key,
            format!("valor {value} fuera de [{min}, {max}]; se ajusta"),
        ));
        *value = (*value).clamp(min, max);
    }
}

/// Parsea un acento `#RRGGBB` a un entero `0x00RRGGBB` (sin depender de GPUI).
pub fn parse_accent(value: &str) -> Option<u32> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// Escritura atómica: fichero temporal en el mismo directorio, `sync_all`,
/// rename sobre el destino (reemplaza en Windows con `MoveFileEx`).
fn write_file_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "settings.json".to_owned()),
        std::process::id()
    ));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_root(name: &str) -> (PortableRoot, PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "lsx-core-config-{}-{}-{n}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("crear raíz temporal");
        (PortableRoot::from_dir(dir.clone()), dir)
    }

    fn write_settings(dir: &Path, json: &str) {
        std::fs::create_dir_all(dir.join("config")).expect("crear config/");
        std::fs::write(dir.join("config").join(SETTINGS_FILE), json).expect("escribir settings");
    }

    #[test]
    fn defaults_round_trip_through_save_and_load() {
        let (root, dir) = temp_root("roundtrip");
        let settings = Settings::default();
        settings.save(&root).expect("guardar");

        let (loaded, warnings) = Settings::load(&root);
        assert!(warnings.is_empty(), "avisos inesperados: {warnings:?}");
        assert_eq!(loaded, settings);

        let text = std::fs::read_to_string(Settings::path(&root)).expect("leer");
        let value: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["$schemaVersion"], Value::from(1));
        assert_eq!(value["ui"]["accent"], Value::from("#4C8DFF"));
        assert_eq!(value["ui"]["dock"], Value::from(Map::new()));
        assert_eq!(value["latex"]["autoCompile"], Value::from(true));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// El chat arranca con el modelo documentado, incluso si el `settings.json`
    /// del usuario es anterior y no trae `model`; si el usuario elige otro, manda
    /// su elección.
    #[test]
    fn el_chat_usa_el_modelo_documentado_salvo_eleccion_explicita() {
        let (root, dir) = temp_root("modelo");
        write_settings(
            &dir,
            r#"{ "$schemaVersion": 1, "ai": { "agent": "build" } }"#,
        );
        let (loaded, _) = Settings::load(&root);
        assert_eq!(loaded.ai.model.as_deref(), Some(DEFAULT_AI_MODEL));

        write_settings(
            &dir,
            r#"{ "$schemaVersion": 1, "ai": { "model": "ollama/qwen2.5-coder:7b" } }"#,
        );
        let (loaded, warnings) = Settings::load(&root);
        assert_eq!(loaded.ai.model.as_deref(), Some("ollama/qwen2.5-coder:7b"));
        assert!(warnings.is_empty(), "avisos inesperados: {warnings:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_defaults_without_warnings() {
        let (root, dir) = temp_root("missing");
        let (loaded, warnings) = Settings::load(&root);
        assert_eq!(loaded, Settings::default());
        assert!(warnings.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_falls_back_to_defaults_with_a_warning() {
        let (root, dir) = temp_root("corrupt");
        write_settings(&dir, "{ esto no es json }");
        let (loaded, warnings) = Settings::load(&root);
        assert_eq!(loaded, Settings::default());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("JSON válido"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_section_keeps_the_other_sections_and_warns() {
        let (root, dir) = temp_root("section");
        write_settings(
            &dir,
            r#"{
                "$schemaVersion": 1,
                "latex": "esto no es un objeto",
                "pdf": { "zoom": 1.5 }
            }"#,
        );
        let (loaded, warnings) = Settings::load(&root);
        assert_eq!(loaded.latex, LatexSettings::default());
        assert_eq!(loaded.pdf.zoom, 1.5);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].key, "latex");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_fields_survive_a_load_save_round_trip() {
        let (root, dir) = temp_root("forward");
        write_settings(
            &dir,
            r##"{
                "$schemaVersion": 1,
                "experimental": { "flags": ["a", "b"] },
                "ui": { "theme": "dark", "accent": "#4C8DFF", "language": "es", "dock": { "version": 1 }, "futuro": 42 },
                "editor": { "fontSize": 15, "otraCosa": true }
            }"##,
        );
        let (loaded, _) = Settings::load(&root);
        assert_eq!(loaded.editor.font_size, 15.0);
        assert_eq!(loaded.ui.extra.get("futuro"), Some(&Value::from(42)));
        loaded.save(&root).expect("guardar");

        let text = std::fs::read_to_string(Settings::path(&root)).expect("leer");
        let value: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(
            value["experimental"]["flags"],
            serde_json::json!(["a", "b"])
        );
        assert_eq!(value["ui"]["futuro"], Value::from(42));
        assert_eq!(value["editor"]["otraCosa"], Value::from(true));
        assert_eq!(value["ui"]["dock"], serde_json::json!({ "version": 1 }));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn out_of_range_values_are_clamped_with_warnings() {
        let (root, dir) = temp_root("clamps");
        write_settings(
            &dir,
            r#"{
                "$schemaVersion": 1,
                "editor": { "fontSize": 999.0, "tabSize": 0 },
                "latex": { "debounceMs": 90000 },
                "pdf": { "zoom": 100.0, "maxCacheMB": 1 },
                "ai": { "port": 80 },
                "advanced": { "logLevel": "verbose" }
            }"#,
        );
        let (loaded, warnings) = Settings::load(&root);
        assert_eq!(loaded.editor.font_size, 72.0);
        assert_eq!(loaded.editor.tab_size, 1);
        assert_eq!(loaded.latex.debounce_ms, 10_000);
        assert_eq!(loaded.pdf.zoom, 8.0);
        assert_eq!(loaded.pdf.max_cache_mb, 16);
        assert_eq!(loaded.ai.port, None);
        assert_eq!(loaded.advanced.log_level, "info");
        assert_eq!(warnings.len(), 7, "avisos: {warnings:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_schema_version_is_loaded_with_a_warning() {
        let (root, dir) = temp_root("version");
        write_settings(
            &dir,
            r#"{ "$schemaVersion": 99, "ui": { "theme": "light" } }"#,
        );
        let (loaded, warnings) = Settings::load(&root);
        assert_eq!(loaded.schema_version, 99);
        assert_eq!(loaded.ui.theme, "light");
        assert!(warnings.iter().any(|w| w.key == "$schemaVersion"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validation_keeps_good_values_and_fixes_bad_enums() {
        let mut settings = Settings::default();
        settings.ui.theme = "claro".into();
        settings.ui.accent = "azul".into();
        settings.latex.engine = "pdftex".into();
        settings.pdf.zoom_mode = "gigante".into();
        settings.ui.language = "de".into();
        settings.ai.permission_preset = "siempre".into();
        let warnings = settings.validate();
        assert_eq!(settings.ui.theme, "dark");
        assert_eq!(settings.ui.accent, "#4C8DFF");
        assert_eq!(settings.latex.engine, "texlive");
        assert_eq!(settings.pdf.zoom_mode, "fit-width");
        assert_eq!(settings.ui.language, "es");
        assert_eq!(settings.ai.permission_preset, "ask");
        assert_eq!(warnings.len(), 6);
    }

    #[test]
    fn los_presets_de_permisos_del_chat_sobreviven_a_validate() {
        for (preset, expected) in [
            ("ask", "ask"),
            ("project", "project"),
            ("all", "all"),
            ("auto", "all"),
        ] {
            let mut settings = Settings::default();
            settings.ai.permission_preset = preset.to_owned();
            let warnings = settings.validate();
            assert_eq!(settings.ai.permission_preset, expected);
            assert!(
                !warnings.iter().any(|w| w.key == "ai.permissionPreset"),
                "«{preset}» no debe generar aviso"
            );
        }
    }

    #[test]
    fn window_state_defaults_to_a_restorable_maximized_window() {
        let settings = Settings::default();
        assert_eq!(settings.ui.window.width, 1280.0);
        assert_eq!(settings.ui.window.height, 800.0);
        assert!(settings.ui.window.maximized);
        assert!(settings.ui.window.x.is_none());
        assert!(settings.ui.window.y.is_none());
    }

    #[test]
    fn invalid_window_state_falls_back_to_defaults() {
        let mut settings = Settings::default();
        settings.ui.window.width = f32::NAN;
        settings.ui.window.height = 10.0;
        settings.ui.window.x = Some(f32::INFINITY);
        settings.ui.window.y = Some(f32::NEG_INFINITY);
        let warnings = settings.validate();
        assert_eq!(settings.ui.window.width, 1280.0);
        assert_eq!(settings.ui.window.height, 800.0);
        assert!(settings.ui.window.x.is_none());
        assert!(settings.ui.window.y.is_none());
        assert!(warnings.iter().any(|w| w.key == "ui.window.width"));
        assert!(warnings.iter().any(|w| w.key == "ui.window.height"));
        assert!(warnings.iter().any(|w| w.key == "ui.window.x"));
        assert!(warnings.iter().any(|w| w.key == "ui.window.y"));
    }

    #[test]
    fn accent_parsing_accepts_only_hex_rgb() {
        assert_eq!(parse_accent("#4C8DFF"), Some(0x004C_8DFF));
        assert_eq!(parse_accent("#000000"), Some(0));
        assert_eq!(parse_accent("4C8DFF"), None);
        assert_eq!(parse_accent("#GGGGGG"), None);
        assert_eq!(parse_accent("#12345"), None);
    }

    #[test]
    fn saving_replaces_atomically_and_leaves_no_temp_file() {
        let (root, dir) = temp_root("atomic");
        let mut first = Settings::default();
        first.editor.font_size = 13.0;
        first.save(&root).expect("guardar primera");

        let mut second = Settings::default();
        second.editor.font_size = 17.0;
        second.save(&root).expect("guardar segunda");

        let (loaded, warnings) = Settings::load(&root);
        assert!(warnings.is_empty());
        assert_eq!(loaded.editor.font_size, 17.0);

        let leftovers: Vec<String> = std::fs::read_dir(dir.join("config"))
            .expect("leer config/")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temporales olvidados: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_creates_the_config_directory_when_missing() {
        let (root, dir) = temp_root("mkdir");
        assert!(!root.config_dir().exists());
        Settings::default().save(&root).expect("guardar");
        assert!(Settings::path(&root).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
