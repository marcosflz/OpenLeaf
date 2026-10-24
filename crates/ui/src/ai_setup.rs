//! Conexión de IA desde Ajustes (P2-AI-1).
//!
//! Este módulo encapsula el binario de opencode que viaja en la distro
//! (`agent/opencode.exe`). La UI **nunca** bloquea el hilo de GPUI: los
//! comandos [`AiSetup::auth_list`] y [`AiSetup::models`] son síncronos a
//! propósito y el llamador los ejecuta dentro de `cx.background_spawn`.
//!
//! Comandos cubiertos:
//! - `opencode auth list` → proveedores conectados.
//! - `opencode models` → modelos `provider/model` disponibles.
//! - `opencode auth login` → se abre en una consola aparte para que el
//!   usuario complete la suscripción o pegue su API key.
//! - abrir `agent/opencode.json` o la carpeta de configuración con el
//!   gestor de archivos del sistema.
//!
//! La configuración del producto se pasa por `OPENCODE_CONFIG`, igual que
//! hace el supervisor del chat, para que `auth`/`models` vean los mismos
//! proveedores que verá el servidor.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use lsx_core::root::PortableRoot;

/// Presupuesto de `auth list` (no debería tardar: lee credenciales locales).
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// Presupuesto de `models` (puede refrescar catálogo y tocar red).
pub const MODELS_TIMEOUT: Duration = Duration::from_secs(25);
/// Presupuesto del botón «Probar».
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// Error accionable de la capa de conexión.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiSetupError {
    /// No se pudo localizar la raíz portable.
    #[error("no se encontró la carpeta de instalación: {0}")]
    Root(String),
    /// Falta `agent/opencode.exe`.
    #[error(
        "no se encontró agent/opencode.exe en {0}. Restaura el binario o reinstala LaTeX Studio"
    )]
    MissingExe(String),
    /// Falta una ruta que la acción necesita (config, carpeta del agente…).
    #[error("no existe {0}")]
    MissingPath(String),
    /// No se pudo lanzar el proceso.
    #[error("no se pudo ejecutar opencode: {0}")]
    Spawn(String),
    /// El proceso no terminó dentro del presupuesto.
    #[error("opencode tardó más de {0} s y se canceló")]
    Timeout(u64),
    /// El proceso terminó con error.
    #[error("opencode falló con código {code:?}: {message}")]
    Command {
        /// Código de salida (puede faltar si lo mató una señal).
        code: Option<i32>,
        /// Últimas líneas de stderr/stdout, ya recortadas.
        message: String,
    },
    /// Error de E/S abriendo una ruta.
    #[error("no se pudo abrir {path}: {error}")]
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

/// Localizador de opencode y ejecutor de sus comandos de configuración.
#[derive(Debug, Clone)]
pub struct AiSetup {
    root: PortableRoot,
    exe: PathBuf,
    config: PathBuf,
}

impl AiSetup {
    /// Detecta la raíz portable y deriva las rutas de opencode.
    pub fn detect() -> Result<Self, AiSetupError> {
        PortableRoot::detect()
            .map(Self::from_root)
            .map_err(|error| AiSetupError::Root(error.to_string()))
    }

    /// Construye el localizador sobre una raíz conocida (tests y shell).
    pub fn from_root(root: PortableRoot) -> Self {
        let exe = root.opencode_exe();
        let config = root.opencode_config();
        Self { root, exe, config }
    }

    /// Ruta esperada de `agent/opencode.exe`.
    pub fn exe_path(&self) -> &Path {
        &self.exe
    }

    /// Ruta esperada de `agent/opencode.json`.
    pub fn config_path(&self) -> &Path {
        &self.config
    }

    /// Raíz portable detectada.
    pub fn root(&self) -> &PortableRoot {
        &self.root
    }

    /// ¿Está el binario en su sitio?
    pub fn exe_exists(&self) -> bool {
        self.exe.is_file()
    }

    /// Proveedores autenticados según `opencode auth list`.
    pub fn auth_list(&self) -> Result<Vec<String>, AiSetupError> {
        self.ensure_exe()?;
        let output = self.run(&["auth", "list"], AUTH_TIMEOUT)?;
        Ok(parse_auth_list(&output.stdout))
    }

    /// Modelos `provider/model` según `opencode models`.
    pub fn models(&self) -> Result<Vec<String>, AiSetupError> {
        self.ensure_exe()?;
        let output = self.run(&["models"], MODELS_TIMEOUT)?;
        Ok(parse_models(&output.stdout))
    }

    /// Prueba de conexión: el mismo comando que usa el catálogo, con menos
    /// presupuesto para que el botón responda rápido.
    pub fn probe(&self) -> Result<Vec<String>, AiSetupError> {
        self.ensure_exe()?;
        let output = self.run(&["models"], PROBE_TIMEOUT)?;
        Ok(parse_models(&output.stdout))
    }

    /// Abre una consola con `opencode auth login`.
    pub fn auth_login(&self) -> Result<(), AiSetupError> {
        self.ensure_exe()?;

        #[cfg(windows)]
        {
            // `cmd /C start "" cmd /K "…\opencode.exe" auth login` abre una
            // consola nueva y la deja abierta para que el usuario vea el
            // resultado o el error. El entorno (OPENCODE_CONFIG y la raíz del
            // proyecto) se hereda del proceso que lanza `start`.
            let login = format!("\"{}\" auth login", self.exe.display());
            let mut command = Command::new("cmd");
            command
                .arg("/C")
                .arg("start")
                .arg("")
                .arg("cmd")
                .arg("/K")
                .arg(login)
                .current_dir(self.workdir())
                .env("OPENCODE_CONFIG", &self.config)
                .env("OPENCODE_DISABLE_AUTOUPDATE", "1");
            command.spawn().map(|_| ()).map_err(|error| {
                AiSetupError::Spawn(format!("no se pudo abrir la consola: {error}"))
            })
        }

        #[cfg(not(windows))]
        {
            // En el resto de plataformas no hay una consola portable garantizada;
            // se lanza el comando interactivo y el usuario usa su terminal si
            // el proceso necesita TTY.
            let mut command = Command::new(&self.exe);
            command
                .args(["auth", "login"])
                .current_dir(self.workdir())
                .env("OPENCODE_CONFIG", &self.config)
                .env("OPENCODE_DISABLE_AUTOUPDATE", "1");
            command.spawn().map(|_| ()).map_err(|error| {
                AiSetupError::Spawn(format!("no se pudo abrir opencode auth login: {error}"))
            })
        }
    }

    /// Abre la carpeta `agent/` (creándola si la distribución viene incompleta).
    pub fn open_agent_dir(&self) -> Result<(), AiSetupError> {
        let dir = self.root.agent_dir();
        if !dir.is_dir() {
            std::fs::create_dir_all(&dir).map_err(|error| AiSetupError::Io {
                path: dir.display().to_string(),
                error: error.to_string(),
            })?;
        }
        open_folder(&dir)
    }

    /// Abre `agent/opencode.json` con el editor asociado del sistema.
    pub fn open_config(&self) -> Result<(), AiSetupError> {
        if !self.config.is_file() {
            return Err(AiSetupError::MissingPath(self.config.display().to_string()));
        }
        open_file(&self.config)
    }

    /// Abre la carpeta `config/` (settings.json y backups).
    pub fn open_config_dir(&self) -> Result<(), AiSetupError> {
        let dir = self.root.config_dir();
        if !dir.is_dir() {
            std::fs::create_dir_all(&dir).map_err(|error| AiSetupError::Io {
                path: dir.display().to_string(),
                error: error.to_string(),
            })?;
        }
        open_folder(&dir)
    }

    fn ensure_exe(&self) -> Result<(), AiSetupError> {
        if self.exe.is_file() {
            Ok(())
        } else {
            Err(AiSetupError::MissingExe(self.exe.display().to_string()))
        }
    }

    fn workdir(&self) -> PathBuf {
        let workspace = self.root.default_workspace();
        if workspace.is_dir() {
            workspace
        } else {
            self.root.path().to_path_buf()
        }
    }

    fn run(&self, args: &[&str], timeout: Duration) -> Result<CommandOutput, AiSetupError> {
        let mut command = Command::new(&self.exe);
        command
            .args(args)
            .current_dir(self.workdir())
            .env("OPENCODE_CONFIG", &self.config)
            .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        run_with_timeout(command, timeout)
    }
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

/// Extrae los modelos `provider/model` de la salida de `opencode models`.
///
/// Acepta líneas con secuencias ANSI, viñetas o decoración de caja. Descarta
/// cabeceras, líneas de metadatos (`--verbose`) y URLs.
pub fn parse_models(output: &str) -> Vec<String> {
    let cleaned = strip_ansi(output);
    let mut models = Vec::new();

    for raw in cleaned.lines() {
        let line = trim_decoration(raw);
        if line.is_empty() || line.starts_with("http://") || line.starts_with("https://") {
            continue;
        }
        let Some((provider, model)) = line.split_once('/') else {
            continue;
        };
        if provider.is_empty()
            || model.is_empty()
            || model.starts_with('/')
            || model.chars().any(char::is_whitespace)
            || !provider
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        {
            continue;
        }
        // Los metadatos de `--verbose` añaden espacios y precios; se descartan.
        if line.chars().any(char::is_whitespace) {
            continue;
        }
        models.push(line.to_owned());
    }

    models.sort();
    models.dedup();
    models
}

/// Extrae los proveedores de la salida de `opencode auth list`.
///
/// El CLI cambia de formato entre versiones (lista con viñetas, tabla o
/// líneas `proveedor: método`), así que se aceptan las tres formas y se
/// descartan cabeceras y resúmenes (`1 credentials`, `No credentials found`).
pub fn parse_auth_list(output: &str) -> Vec<String> {
    let cleaned = strip_ansi(output);
    let mut providers = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for raw in cleaned.lines() {
        let mut line = raw.trim();
        if line.is_empty() {
            continue;
        }

        // Tabla: │ provider │ auth │
        if line.contains('│') || line.contains('|') {
            let separator = if line.contains('│') { '│' } else { '|' };
            line = line
                .split(separator)
                .map(str::trim)
                .find(|cell| !cell.is_empty() && !is_decoration_only(cell))
                .unwrap_or("");
        }

        line = trim_decoration(line);
        if line.is_empty() {
            continue;
        }

        // `proveedor: método` o `proveedor\tapi key`.
        let name = line
            .split([':', '\t'])
            .next()
            .unwrap_or(line)
            .trim()
            .split("  ")
            .next()
            .unwrap_or(line)
            .trim();
        // `Anthropic (OAuth)` → `Anthropic`.
        let name = name.split(" (").next().unwrap_or(name).trim();
        // `anthropic (1)` / `anthropic 1`.
        let name = name
            .trim_end_matches(|ch: char| ch.is_ascii_digit() || ch == '(' || ch == ')')
            .trim();

        if name.is_empty() || name.len() > 60 || is_decoration_only(name) {
            continue;
        }

        let lower = name.to_ascii_lowercase();
        if lower.starts_with("credential")
            || lower.starts_with("provider")
            || lower.starts_with("auth")
            || lower.starts_with("no ")
            || lower.contains("credential")
            || lower.contains("provider")
            || lower.contains("found")
            || lower.contains("logged in")
            || lower.starts_with("total")
        {
            continue;
        }
        if name.chars().all(|ch| ch.is_ascii_punctuation()) {
            continue;
        }

        if seen.insert(lower) {
            providers.push(name.to_owned());
        }
    }

    providers
}

/// Elimina secuencias ANSI (CSI y OSC) de la salida de un CLI.
fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            output.push(ch);
            continue;
        }

        match chars.peek().copied() {
            // CSI: ESC [ … letra final (@..~).
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: ESC ] … BEL o ESC \.
            Some(']') => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if next == '\u{7}' || (previous == '\u{1b}' && next == '\\') {
                        break;
                    }
                    previous = next;
                }
            }
            // Otras secuencias de dos caracteres.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }

    output
}

fn is_decoration_only(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_whitespace() || is_decoration(ch))
}

fn is_decoration(ch: char) -> bool {
    matches!(
        ch,
        '─' | '━'
            | '│'
            | '┃'
            | '┌'
            | '┐'
            | '└'
            | '┘'
            | '├'
            | '┤'
            | '┬'
            | '┴'
            | '┼'
            | '╭'
            | '╮'
            | '╰'
            | '╯'
            | '╔'
            | '╗'
            | '╚'
            | '╝'
            | '║'
            | '═'
            | '╠'
            | '╣'
            | '╦'
            | '╩'
            | '╬'
            | '▀'
            | '▄'
            | '█'
            | '▌'
            | '▐'
            | '░'
            | '▒'
            | '▓'
    )
}

fn trim_decoration(value: &str) -> &str {
    value
        .trim()
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || is_decoration(ch)
                || matches!(
                    ch,
                    '●' | '○' | '◆' | '◇' | '•' | '·' | '▪' | '▫' | '◦' | '✓'
                )
                || ch.is_ascii_punctuation() && !matches!(ch, '/' | ':' | '.' | '-' | '_')
        })
        .trim()
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
    use std::path::PathBuf;

    #[test]
    fn models_parser_accepts_plain_ansi_and_rejects_noise() {
        let output = concat!(
            "\u{1b}[32mopencode-go/deepseek-v4.1-flash\u{1b}[0m\n",
            "opencode-go/gpt-5.6-luna\n",
            "anthropic/claude-sonnet-4-5\n",
            "ollama/qwen2.5-coder:7b\n",
            "\n",
            "Models\n",
            "not a model\n",
            "provider/\n",
            "/onlymodel\n",
            "http://example.com/model\n",
        );

        assert_eq!(
            parse_models(output),
            vec![
                "anthropic/claude-sonnet-4-5",
                "ollama/qwen2.5-coder:7b",
                "opencode-go/deepseek-v4.1-flash",
                "opencode-go/gpt-5.6-luna",
            ]
        );
    }

    #[test]
    fn models_parser_deduplicates_and_ignores_verbose_rows() {
        let output = concat!(
            "opencode-go/deepseek-v4.1-flash\n",
            "opencode-go/deepseek-v4.1-flash\n",
            "opencode-go/gpt-5.6-luna  $3.00 / 1M tokens\n",
        );
        assert_eq!(
            parse_models(output),
            vec!["opencode-go/deepseek-v4.1-flash"]
        );
    }

    #[test]
    fn auth_parser_accepts_bullets_and_discards_the_summary() {
        let output = concat!(
            "┌  Credentials\n",
            "│\n",
            "◆  Anthropic\n",
            "│\n",
            "◆  opencode\n",
            "│\n",
            "└  2 credentials\n",
        );
        assert_eq!(parse_auth_list(output), vec!["Anthropic", "opencode"]);
    }

    #[test]
    fn auth_parser_accepts_a_table() {
        let output = concat!(
            "┌────────────┬──────────┐\n",
            "│ provider   │ auth     │\n",
            "├────────────┼──────────┤\n",
            "│ anthropic  │ api key  │\n",
            "│ opencode   │ oauth    │\n",
            "└────────────┴──────────┘\n",
        );
        assert_eq!(parse_auth_list(output), vec!["anthropic", "opencode"]);
    }

    #[test]
    fn auth_parser_accepts_colon_lines_and_no_credentials() {
        assert_eq!(
            parse_auth_list("anthropic: api key\nopencode: oauth\n"),
            vec!["anthropic", "opencode"]
        );
        assert!(parse_auth_list("No credentials found\n").is_empty());
        assert!(parse_auth_list("").is_empty());
    }

    #[test]
    fn ansi_and_osc_sequences_are_removed() {
        assert_eq!(strip_ansi("\u{1b}[31mrojo\u{1b}[0m"), "rojo");
        assert_eq!(strip_ansi("\u{1b}]0;título\u{7}texto"), "texto");
        assert_eq!(strip_ansi("normal"), "normal");
    }

    #[test]
    fn setup_derives_the_portable_paths() {
        let setup = AiSetup::from_root(PortableRoot::from_dir(PathBuf::from("C:/lsx")));
        assert_eq!(setup.exe_path(), Path::new("C:/lsx/agent/opencode.exe"));
        assert_eq!(setup.config_path(), Path::new("C:/lsx/agent/opencode.json"));
    }

    #[test]
    fn missing_binary_is_reported_with_the_expected_path() {
        let setup = AiSetup::from_root(PortableRoot::from_dir(PathBuf::from("C:/lsx-inexistente")));
        let error = setup.models().expect_err("no hay binario");
        assert!(matches!(error, AiSetupError::MissingExe(_)));
        assert!(error.to_string().contains("agent/opencode.exe"));
    }
}
