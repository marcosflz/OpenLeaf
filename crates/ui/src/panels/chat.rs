//! Panel de chat IA (M1-E3..E6) — puerto de la UI aprobada en `spike-chat`.
//!
//! Lenguaje visual congelado en M0.5 (GitHub Copilot Chat): sin burbujas
//! grandes, tool cards finas plegables, razonamiento plegado, composer con
//! chip de contexto y estado del server con dot. El look no se cambia: los
//! hex viven solo aquí y el panel no toca el tema global del shell.
//!
//! # Arquitectura (skill `lsx-gpui-patterns`)
//!
//! - El panel es una entidad pura: el estado de la conversación vive en
//!   [`lsx_chat::conversation::Conversation`] y se reduce con eventos del
//!   servicio ([`lsx_chat::service`]).
//! - El servicio corre en su propio hilo tokio; el puente es `async_channel` +
//!   `cx.spawn` (el hilo UI nunca bloquea).
//! - **Modo replay determinista** (`LSX_CHAT_REPLAY` o su alias
//!   `LSX_SPIKE_REPLAY`): carga una fixture de `fixtures/opencode/events/` y
//!   pinta la conversación sin server ni tokens (tests/capturas). Con
//!   `LSX_CHAT_EXPAND=1` se despliegan razonamiento y tool cards.
//! - `ai.autoStart`: si está activo y `agent/opencode.exe` existe, el
//!   supervisor se lanza **al crear la app** (creación del panel); sin
//!   autoStart se arranca perezosamente en la primera acción
//!   («Nueva sesión», enviar, «Arreglar errores»).
//!
//! # Flujos
//!
//! - **Permisos (M1-E4)**: al llegar `permission.asked` se abre
//!   [`PermissionCard::show`] (API del stream A) con el diff parseado por
//!   `DiffSummary::parse_unified`; la decisión viaja como `ChatCmd::Permission`.
//! - **Contexto (M1-E5)**: chips de archivo activo (del [`EditorPanel`]) y de
//!   selección cuando el shell la aporta (`set_editor_context`).
//! - **«Arreglar errores con IA» (M1-E5)**: compone el prompt con los
//!   diagnósticos del [`ProblemsPanel`] + la cola del log en disco y lo envía
//!   con el agente `latex-fix` (`lsx_chat::prompt::compose_fix_prompt`).
//! - **Pickers (M1-E6)**: modelo (`/config/providers`), agente (`/agent`),
//!   sesiones (cambiar/borrar/nueva), abort; el dot IA del status bar se
//!   actualiza vía [`StatusHandle`].

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::dialog::{DialogDescription, DialogFooter};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::{TextView, TextViewStyle};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

// Traits de interacción de `gpui-pre` que no reexporta el glob de `gpui_kit`.

use lsx_chat::conversation::{
    Conversation, Part, Role, ToolCall, TurnStatus, diff_stats, file_label,
};
use lsx_chat::prompt::{
    FixDiagnosticLine, FixPromptInput, compose_fix_prompt, compose_prompt_with_context,
    read_log_tail,
};
use lsx_chat::protocol::{AgentInfo, ModelEntry, ModelRef, PermissionReply};
use lsx_chat::service::{ChatCmd, ChatConfig, ChatEvent, ChatService};
use lsx_chat::supervisor::ServerState;
use lsx_core::config::AiSettings;
use lsx_core::root::PortableRoot;
use lsx_latex::DiagnosticSeverity;

use crate::panels::editor::EditorPanel;
use crate::panels::problems::ProblemsPanel;
use crate::permission::{DiffSummary, PermissionCard, PermissionRequest};
use crate::status::{AiState, StatusHandle, StatusMessage};

/// Escala tipográfica y de radios del panel (auditada en el spike M0-S4).
mod tok {
    /// Texto base de la conversación.
    pub const TEXT: f32 = 13.;
    /// Texto secundario (etiquetas, chips, estados).
    pub const TEXT_SM: f32 = 12.;
    /// Texto micro (paths, argumentos).
    pub const TEXT_XS: f32 = 11.;
    /// Salida de tools y bloques mono.
    pub const MONO_SM: f32 = 11.;
    /// Radio de chips.
    pub const R_SM: f32 = 4.;
    /// Radio de superficies (bloques, composer, cards).
    pub const R_MD: f32 = 6.;
}

/// Eventos del panel de chat hacia el shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatPanelEvent {
    /// El usuario cambió el preset de permisos: el shell lo persiste en
    /// `ai.permissionPreset`.
    PermissionPresetChanged(String),
}

impl EventEmitter<ChatPanelEvent> for ChatPanel {}

/// Paleta oscura «VS Code · Copilot Chat». Copiada del spike aprobado (M0.5):
/// único punto del panel con hex; no toca el tema global del shell. Los tonos
/// de texto secundario y bordes se leen del tema (`Ui`), por eso no están aquí.
mod color {
    /// Fondo del panel.
    pub const PANEL: u32 = 0x151A21;
    /// Texto principal (inline code del markdown).
    pub const FG: u32 = 0xE6EAF2;
    /// Superficie sutil (bloques de usuario, chips, hover).
    pub const SURFACE: u32 = 0x1E2530;
    /// Fondo de bloques de código (más oscuro que el panel).
    pub const CODE_BG: u32 = 0x0E1218;
}

/// Política de permisos del agente (botón «Permisos» del header).
///
/// Se persiste en `ai.permissionPreset`. Con cualquier preset distinto de
/// [`PermissionPreset::Ask`] el panel responde a `permission.asked` sin abrir
/// el modal (con `always`, de modo que opencode deja de preguntar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionPreset {
    /// Preguntar siempre (modal con diff por petición).
    #[default]
    Ask,
    /// Aprobar las ediciones dentro del proyecto; el resto (bash, etc.) pregunta.
    ProjectEdits,
    /// Aprobar todo automáticamente.
    All,
}

impl PermissionPreset {
    /// Valor de `ai.permissionPreset` (`ask` | `project` | `all`).
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::ProjectEdits => "project",
            Self::All => "all",
        }
    }

    /// Acepta también los valores antiguos (`auto`) y sinónimos.
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "project" | "project-edits" | "edits" => Self::ProjectEdits,
            "all" | "auto" | "yolo" => Self::All,
            _ => Self::Ask,
        }
    }

    /// Etiqueta del menú.
    fn label(self) -> &'static str {
        match self {
            Self::Ask => "Preguntar siempre",
            Self::ProjectEdits => "Aprobar ediciones del proyecto",
            Self::All => "Aprobar todo",
        }
    }

    /// Descripción para el tooltip.
    fn hint(self) -> &'static str {
        match self {
            Self::Ask => "Permisos: cada edición o comando pide confirmación con su diff",
            Self::ProjectEdits => {
                "Permisos: las ediciones dentro del proyecto se aprueban solas (bash sigue preguntando)"
            }
            Self::All => "Permisos: todo se aprueba automáticamente (sin modales)",
        }
    }

    /// Respuesta automática a una petición, si este preset la cubre.
    pub fn auto_reply(
        self,
        permission: &str,
        filepath: Option<&str>,
        project_root: &Path,
    ) -> Option<PermissionReply> {
        match self {
            Self::Ask => None,
            Self::All => Some(PermissionReply::Always),
            Self::ProjectEdits => {
                let is_edit = permission.eq_ignore_ascii_case("edit");
                let inside = filepath.is_some_and(|path| {
                    let path = Path::new(path);
                    path.starts_with(project_root)
                });
                (is_edit && inside).then_some(PermissionReply::Always)
            }
        }
    }
}

/// Configuración de creación del panel (la construye el shell).
pub struct ChatPanelConfig {
    /// `agent/opencode.exe`.
    pub exe: PathBuf,
    /// `agent/opencode.json`.
    pub config_json: PathBuf,
    /// Raíz del proyecto activo (cwd del server, `?directory=` de sesiones).
    pub project_root: PathBuf,
    /// Nombre visible del proyecto (título de sesiones nuevas).
    pub project_name: String,
    /// `ai.autoStart`.
    pub auto_start: bool,
    /// `ai.port` (None → puerto libre).
    pub port: Option<u16>,
    /// `ai.model` (`provider/model`).
    pub model: Option<String>,
    /// `ai.agent`.
    pub agent: String,
    /// `ai.showReasoning`.
    pub show_reasoning: bool,
    /// `ai.permissionPreset`.
    pub permission_preset: PermissionPreset,
    /// Config efectiva de opencode (`config/opencode.runtime.json`).
    pub runtime_config: PathBuf,
    /// Skills del bundle (`assets/skills`) para `skills.paths`.
    pub skills_dir: PathBuf,
    /// `tex/bin/windows` + `agent` para el PATH del server.
    pub path_prepend: Vec<PathBuf>,
    /// Handle de estado compartido (dot IA de la status bar).
    pub status: StatusHandle,
    /// Editor del shell (chips de contexto; opcional en tests).
    pub editor: Option<Entity<EditorPanel>>,
    /// Panel de problemas (prompt «Arreglar errores»; opcional en tests).
    pub problems: Option<Entity<ProblemsPanel>>,
    /// `main.tex` detectado, si lo hay (log y prompt de reparación).
    pub main_file: Option<String>,
}

impl ChatPanelConfig {
    /// Arma la configuración desde la raíz portable y los ajustes `ai`.
    pub fn from_root(
        root: &PortableRoot,
        project_root: &Path,
        project_name: String,
        ai: &AiSettings,
        status: StatusHandle,
        editor: Option<Entity<EditorPanel>>,
        problems: Option<Entity<ProblemsPanel>>,
    ) -> Self {
        let main_file = detect_main_file(project_root);
        Self {
            exe: root.opencode_exe(),
            config_json: root.opencode_config(),
            project_root: project_root.to_path_buf(),
            project_name,
            auto_start: ai.auto_start,
            port: ai.port,
            model: ai.model.clone(),
            agent: ai.agent.clone(),
            show_reasoning: ai.show_reasoning,
            permission_preset: PermissionPreset::from_setting(&ai.permission_preset),
            runtime_config: root.config_dir().join("opencode.runtime.json"),
            skills_dir: root.path().join("assets").join("skills"),
            path_prepend: vec![root.tex_dir().join("bin").join("windows"), root.agent_dir()],
            status,
            editor,
            problems,
            main_file,
        }
    }

    fn service_config(&self) -> ChatConfig {
        ChatConfig {
            exe: self.exe.clone(),
            config_json: self.config_json.clone(),
            runtime_config: Some(self.runtime_config.clone()),
            skills_dir: Some(self.skills_dir.clone()),
            project_cwd: self.project_root.clone(),
            project_name: self.project_name.clone(),
            port: self.port,
            // Password aleatorio por sesión (basic auth verificado live).
            password: Some(lsx_chat::supervisor::random_password()),
            model: self.model.clone(),
            agent: self.agent.clone(),
            path_prepend: self.path_prepend.clone(),
            ..ChatConfig::default()
        }
    }
}

/// Fase del server que pinta el header (proyección de [`ServerState`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ServerPhase {
    Idle,
    Starting,
    Ready,
    Error,
}

/// Colores resueltos del tema, copiados para no mantener el préstamo de `cx`.
struct Ui {
    fg: Hsla,
    fg_muted: Hsla,
    surface: Hsla,
    border: Hsla,
    accent: Hsla,
    success: Hsla,
    danger: Hsla,
    warning: Hsla,
    mono: SharedString,
}

impl Ui {
    fn new(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            fg: theme.foreground,
            fg_muted: theme.muted_foreground,
            surface: theme.muted,
            border: theme.border,
            accent: theme.primary,
            success: theme.success,
            danger: theme.danger,
            warning: theme.warning,
            mono: theme.mono_font_family.clone(),
        }
    }
}

/// Estilo de los markdown de la conversación (copiado del spike aprobado).
fn rich_style() -> TextViewStyle {
    let code_block = StyleRefinement::default()
        .bg(rgb(color::CODE_BG))
        .text_size(px(12.));
    TextViewStyle::default()
        .heading_font_size(|level, _base| match level {
            1 => px(15.),
            2 => px(14.),
            _ => px(13.),
        })
        .code_block(code_block)
        .inline_code(HighlightStyle {
            background_color: Some(rgb(color::SURFACE).into()),
            color: Some(rgb(color::FG).into()),
            ..Default::default()
        })
}

/// Estilo compacto del razonamiento (markdown secundario).
fn reasoning_style() -> TextViewStyle {
    rich_style().paragraph_gap(rems(0.5))
}

/// Panel de chat IA del shell.
pub struct ChatPanel {
    config: ChatPanelConfig,
    conversation: Conversation,
    service: Option<ChatService>,
    pump: Option<Task<()>>,
    server: ServerPhase,
    port: Option<u16>,
    /// Nombre de la fixture en modo replay.
    replay: Option<String>,
    textarea: Entity<TextareaState>,
    scroll: ScrollHandle,
    focus: FocusHandle,
    expanded_tools: HashMap<String, bool>,
    expanded_reasoning: HashMap<String, bool>,
    models: Vec<ModelEntry>,
    agents: Vec<AgentInfo>,
    /// Errores del servicio mostrables (cap 3).
    error_rows: Vec<String>,
    /// Chips de contexto (el shell también puede fijarlos por API).
    context_file: Option<(String, u32)>,
    context_selection: Option<String>,
    /// Diagnósticos y log para «Arreglar errores con IA» (se refrescan al pulsar).
    fix_diagnostics: Vec<FixDiagnosticLine>,
    fix_log_tail: Option<String>,
    show_reasoning: bool,
    /// Preset de permisos vigente (botón «Permisos»).
    permission_preset: PermissionPreset,
    /// Mensaje hasta el que se revirtió la sesión (banner «Deshacer»).
    reverted_to: Option<String>,
    /// Permisos ya mostrados como modal (no repetir).
    permission_shown: BTreeSet<String>,
}

impl ChatPanel {
    /// Crea el panel; con autoStart arranca el supervisor ya (documentado en el
    /// módulo). En modo replay no hay servicio.
    pub fn new(window: &mut Window, cx: &mut Context<Self>, config: ChatPanelConfig) -> Self {
        let textarea = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Pregunta lo que quieras…")
                .auto_grow(1, 6)
        });
        let replay_path = std::env::var_os("LSX_CHAT_REPLAY")
            .or_else(|| std::env::var_os("LSX_SPIKE_REPLAY"))
            .map(PathBuf::from);

        let mut panel = Self {
            show_reasoning: config.show_reasoning,
            permission_preset: config.permission_preset,
            reverted_to: None,
            config,
            conversation: Conversation::default(),
            service: None,
            pump: None,
            server: ServerPhase::Idle,
            port: None,
            replay: None,
            textarea,
            scroll: ScrollHandle::default(),
            focus: cx.focus_handle(),
            expanded_tools: HashMap::new(),
            expanded_reasoning: HashMap::new(),
            models: Vec::new(),
            agents: Vec::new(),
            error_rows: Vec::new(),
            context_file: None,
            context_selection: None,
            fix_diagnostics: Vec::new(),
            fix_log_tail: None,
            permission_shown: BTreeSet::new(),
        };

        if let Some(path) = replay_path {
            panel.load_replay(&path);
            return panel;
        }

        // Servicio (hilo propio; no bloquea la creación).
        let service = lsx_chat::service::spawn(panel.config.service_config());
        let rx = service.events.clone();
        panel.pump = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                let ok = this.update_in(cx, |panel, window, cx| {
                    panel.apply(event, window, cx);
                });
                if ok.is_err() {
                    break;
                }
            }
        }));
        panel.service = Some(service);

        if panel.config.auto_start && panel.config.exe.is_file() {
            panel.send_command(ChatCmd::Start, cx);
            panel.server = ServerPhase::Starting;
        }
        panel
    }

    /// Carga una fixture y deja el panel en modo replay estático.
    fn load_replay(&mut self, path: &Path) {
        let resolved = resolve_fixture(path);
        match lsx_chat::replay::replay_file(&resolved) {
            Ok(events) => {
                self.conversation = Conversation::default();
                self.conversation.apply_streams(&events);
                let name = resolved
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("fixture.jsonl")
                    .to_owned();
                self.replay = Some(name);
                if std::env::var_os("LSX_CHAT_EXPAND").is_some()
                    || std::env::var_os("LSX_SPIKE_EXPAND").is_some()
                {
                    for message in &self.conversation.messages {
                        for part in &message.parts {
                            match part {
                                Part::Reasoning { part_id, .. } => {
                                    self.expanded_reasoning.insert(part_id.clone(), true);
                                }
                                Part::Tool(call) => {
                                    self.expanded_tools.insert(call.part_id.clone(), true);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                tracing::info!(fixture = %resolved.display(), events = events.len(), "chat en modo replay");
            }
            Err(error) => {
                tracing::error!(fixture = %resolved.display(), %error, "la fixture de replay no carga");
                self.error_rows.push(format!(
                    "Replay: {} no se pudo cargar ({error})",
                    resolved.display()
                ));
            }
        }
    }

    // --- API para el shell ------------------------------------------------------

    /// Estado de la conversación (tests/QA).
    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    /// `true` si el panel está en modo replay (sin server).
    pub fn is_replay(&self) -> bool {
        self.replay.is_some()
    }
    /// Arranca el supervisor si aún no lo hizo (arranque perezoso).
    pub fn start_server(&mut self, cx: &mut Context<Self>) {
        if self.service.is_some() && self.server == ServerPhase::Idle {
            self.server = ServerPhase::Starting;
            self.send_command(ChatCmd::Start, cx);
        }
    }

    /// Fija el contexto del editor (chips de archivo/selección). El shell puede
    /// llamarlo al cambiar de documento; el panel también lo refresca solo al
    /// adjuntar o al pulsar «Arreglar errores».
    pub fn set_editor_context(
        &mut self,
        file: Option<(String, u32)>,
        selection: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.context_file = file;
        self.context_selection = selection;
        cx.notify();
    }

    /// Fija los diagnósticos y la cola del log para «Arreglar errores».
    pub fn set_fix_context(
        &mut self,
        diagnostics: Vec<FixDiagnosticLine>,
        log_tail: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.fix_diagnostics = diagnostics;
        self.fix_log_tail = log_tail;
        cx.notify();
    }

    /// Acción «Arreglar errores con IA» (la dispara el shell o el botón del
    /// composer): compone el prompt y lo envía con el agente `latex-fix`.
    pub fn request_fix(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_fix_context(cx);
        let input = FixPromptInput {
            project: Some(self.config.project_name.clone()),
            main_file: self.config.main_file.clone(),
            active_file: self.context_file.as_ref().map(|(file, _)| file.clone()),
            diagnostics: self.fix_diagnostics.clone(),
            log_tail: self.fix_log_tail.clone(),
        };
        let prompt = compose_fix_prompt(&input);
        self.send_prompt(prompt, Some("latex-fix".to_owned()), cx);
    }

    // --- reducer ----------------------------------------------------------------

    fn apply(&mut self, event: ChatEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            ChatEvent::Server(state) => {
                let (phase, ai) = match state {
                    ServerState::Stopped => (ServerPhase::Idle, AiState::Stopped),
                    ServerState::Starting | ServerState::Restarting { .. } => {
                        (ServerPhase::Starting, AiState::Starting)
                    }
                    ServerState::Ready { port, .. } => {
                        self.port = Some(port);
                        (ServerPhase::Ready, AiState::Ready)
                    }
                    ServerState::Error(message) => {
                        self.error_rows.push(message);
                        (ServerPhase::Error, AiState::Error)
                    }
                };
                self.server = phase;
                self.config.status.send(StatusMessage::Ai(ai));
            }
            ChatEvent::Ready {
                port, model, agent, ..
            } => {
                self.port = Some(port);
                self.server = ServerPhase::Ready;
                self.config.status.send(StatusMessage::Ai(AiState::Ready));
                if model.is_some() {
                    self.conversation.set_model(model);
                }
                self.conversation.set_agent(agent);
            }
            ChatEvent::SessionActivated { session } => {
                self.conversation.set_active_session(Some(&session));
                self.permission_shown.clear();
                self.error_rows.clear();
                self.reverted_to = None;
                self.scroll.scroll_to_bottom();
            }
            ChatEvent::SessionCleared => {
                self.conversation.set_active_session(None);
                self.permission_shown.clear();
                self.reverted_to = None;
            }
            ChatEvent::Stream(event) => {
                // Los permisos abren modal (o se responden solos según el
                // preset); el resto se reduce sin más.
                if let lsx_chat::events::SpikeEvent::PermissionAsked { request_id, .. } =
                    event.as_ref()
                {
                    let request_id = request_id.clone();
                    self.conversation.apply_stream(&event);
                    self.answer_permission(&request_id, window, cx);
                } else {
                    self.conversation.apply_stream(&event);
                }
                self.scroll.scroll_to_bottom();
            }
            ChatEvent::History { session_id, events } => {
                self.conversation.apply_history(&session_id, &events);
                self.scroll.scroll_to_bottom();
            }
            ChatEvent::Sessions(sessions) => {
                self.conversation.apply_sessions(sessions);
            }
            ChatEvent::Providers(catalog) => {
                self.models = catalog.model_entries();
            }
            ChatEvent::Agents(agents) => {
                self.agents = agents;
            }
            ChatEvent::SelectionChanged { model, agent } => {
                if model.is_some() {
                    self.conversation.set_model(model);
                }
                self.conversation.set_agent(agent);
            }
            ChatEvent::Error { message, .. } => {
                self.error_rows.push(message);
                if self.error_rows.len() > 3 {
                    self.error_rows.remove(0);
                }
            }
            ChatEvent::Reverted { message_id } => {
                self.reverted_to = Some(message_id);
            }
            ChatEvent::Unreverted => {
                self.reverted_to = None;
            }
            ChatEvent::Stopped => {
                self.server = ServerPhase::Idle;
                self.config.status.send(StatusMessage::Ai(AiState::Stopped));
            }
        }
        cx.notify();
    }

    /// Convierte `permission.asked` en el modal de [`PermissionCard`] y manda
    /// Atiende un `permission.asked`: con el preset en «preguntar» abre el
    /// modal; con los presets automáticos responde sin molestar al usuario.
    fn answer_permission(&mut self, request_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.replay.is_some() {
            return;
        }
        if let Some(entry) = self.conversation.permission(request_id)
            && let Some(reply) = self.permission_preset.auto_reply(
                &entry.permission,
                entry.filepath.as_deref(),
                &self.config.project_root,
            )
        {
            let request_id = request_id.to_owned();
            tracing::info!(
                preset = self.permission_preset.as_setting(),
                permission = %entry.permission,
                "permiso respondido automáticamente por el preset"
            );
            self.conversation.note_reply(&request_id, reply.as_str());
            self.send_command(ChatCmd::Permission { request_id, reply }, cx);
            return;
        }
        self.open_permission_modal(request_id, window, cx);
    }

    /// la decisión al servicio (M1-E4).
    fn open_permission_modal(
        &mut self,
        request_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.replay.is_some() || !self.permission_shown.insert(request_id.to_owned()) {
            return;
        }
        let Some(entry) = self.conversation.permission(request_id) else {
            return;
        };
        let file = entry.file();
        let request = PermissionRequest {
            id: entry.request_id.clone(),
            title: format!("Permiso de {} · opencode", entry.permission),
            detail: format!("El agente quiere modificar {file}. Revisa el diff y decide."),
            diff: entry
                .diff
                .as_deref()
                .map(|diff| DiffSummary::parse_unified(file.clone(), diff)),
        };
        let receiver = PermissionCard::show(window, cx, request);
        let request_id = request_id.to_owned();
        cx.spawn(async move |this, cx| {
            match receiver.recv().await {
                Ok(decision) => {
                    let reply = match decision {
                        crate::permission::PermissionDecision::Once => PermissionReply::Once,
                        crate::permission::PermissionDecision::Always => PermissionReply::Always,
                        crate::permission::PermissionDecision::Reject => PermissionReply::Reject,
                    };
                    this.update(cx, |panel, cx| {
                        panel.conversation.note_reply(&request_id, reply.as_str());
                        panel.send_command(ChatCmd::Permission { request_id, reply }, cx);
                        cx.notify();
                    })
                    .ok();
                }
                Err(_) => {
                    // Diálogo cerrado sin decisión: permitir reabrirlo si el
                    // server vuelve a pedir el mismo permiso.
                    this.update(cx, |panel, cx| {
                        panel.permission_shown.remove(&request_id);
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Refresca diagnósticos, archivo activo y cola del log justo antes de
    /// componer el prompt de reparación.
    fn refresh_fix_context(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = &self.config.editor
            && let Some(path) = editor.read(cx).active_path()
        {
            let line = editor
                .read(cx)
                .active_cursor(cx)
                .map(|(line, _)| line)
                .unwrap_or(1);
            self.context_file = Some((path, line));
        }
        if let Some(problems) = &self.config.problems {
            let mut diagnostics: Vec<FixDiagnosticLine> = problems
                .read(cx)
                .diagnostics()
                .iter()
                .map(|diagnostic| FixDiagnosticLine {
                    file: diagnostic.file.as_ref().map(|file| file.to_string()),
                    line: diagnostic.line,
                    severity: severity_label(diagnostic.severity).to_owned(),
                    message: diagnostic.message.clone(),
                })
                .collect();
            // Errores primero: son los que el agente debe arreglar.
            diagnostics.sort_by_key(|line| match line.severity.as_str() {
                "error" => 0,
                "warning" => 1,
                _ => 2,
            });
            self.fix_diagnostics = diagnostics;
        }
        self.fix_log_tail =
            read_log_tail(&self.config.project_root, self.config.main_file.as_deref());
    }

    // --- acciones ----------------------------------------------------------------

    fn send_command(&self, command: ChatCmd, cx: &mut Context<Self>) {
        if let Some(service) = &self.service {
            let _ = service.cmd.try_send(command);
        } else if self.replay.is_none() {
            tracing::warn!("comando de chat sin servicio");
        }
        cx.notify();
    }

    fn on_new_session(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.new_session(cx);
    }

    fn new_session(&mut self, cx: &mut Context<Self>) {
        if self.replay.is_some() {
            return;
        }
        self.start_server(cx);
        self.send_command(ChatCmd::NewSession, cx);
    }

    fn delete_session(&mut self, session_id: String, cx: &mut Context<Self>) {
        self.send_command(ChatCmd::DeleteSession { id: session_id }, cx);
    }

    fn switch_session(&mut self, session_id: String, cx: &mut Context<Self>) {
        self.send_command(ChatCmd::SwitchSession { id: session_id }, cx);
    }

    fn on_send(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.send_current(window, cx);
    }

    /// Envía lo escrito en el composer (botón, o `Enter` sin modificadores).
    fn send_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.replay.is_some()
            || self.server != ServerPhase::Ready
            || self.conversation.streaming()
        {
            return;
        }
        let text = self.textarea.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.textarea
            .update(cx, |state, cx| state.set_value("", window, cx));
        let file = self
            .context_file
            .as_ref()
            .map(|(file, line)| format!("{file}:{line}"));
        let prompt =
            compose_prompt_with_context(&text, file.as_deref(), self.context_selection.as_deref());
        self.send_prompt(prompt, None, cx);
    }

    /// Envía un prompt (opcionalmente con agente distinto al vigente).
    fn send_prompt(&mut self, text: String, agent: Option<String>, cx: &mut Context<Self>) {
        if self.replay.is_some() {
            return;
        }
        self.start_server(cx);
        self.error_rows.clear();
        self.conversation.set_status(TurnStatus::Busy);
        self.send_command(ChatCmd::Prompt { text, agent }, cx);
    }

    fn on_abort(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.replay.is_some() {
            return;
        }
        self.send_command(ChatCmd::Abort, cx);
    }

    fn select_model(&mut self, entry: &ModelEntry, cx: &mut Context<Self>) {
        self.conversation.set_model(Some(ModelRef::new(
            entry.provider_id.clone(),
            entry.model_id.clone(),
        )));
        self.send_command(
            ChatCmd::SetModel {
                provider: entry.provider_id.clone(),
                model: entry.model_id.clone(),
            },
            cx,
        );
    }

    fn select_agent(&mut self, agent: String, cx: &mut Context<Self>) {
        self.conversation.set_agent(agent.clone());
        self.send_command(ChatCmd::SetAgent { name: agent }, cx);
    }

    fn on_toggle_tool(&mut self, part_id: String, cx: &mut Context<Self>) {
        let entry = self.expanded_tools.entry(part_id).or_insert(false);
        *entry = !*entry;
        cx.notify();
    }

    fn on_toggle_reasoning(&mut self, part_id: String, cx: &mut Context<Self>) {
        let entry = self.expanded_reasoning.entry(part_id).or_insert(false);
        *entry = !*entry;
        cx.notify();
    }

    fn on_attach_file(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = &self.config.editor
            && let Some(path) = editor.read(cx).active_path()
        {
            let line = editor
                .read(cx)
                .active_cursor(cx)
                .map(|(line, _)| line)
                .unwrap_or(1);
            self.context_file = Some((path, line));
            cx.notify();
        }
    }

    fn on_clear_context_file(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.context_file = None;
        cx.notify();
    }

    fn on_clear_context_selection(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_selection = None;
        cx.notify();
    }

    fn on_fix_errors(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.request_fix(window, cx);
    }

    // --- helpers --------------------------------------------------------------

    fn streaming(&self) -> bool {
        self.conversation.streaming()
    }

    fn model_full(&self) -> Option<String> {
        self.conversation.model_full()
    }

    fn model_short(&self) -> String {
        self.conversation
            .model
            .clone()
            .unwrap_or_else(|| "sin modelo".to_owned())
    }

    fn agent_label(&self) -> String {
        self.conversation
            .agent
            .clone()
            .unwrap_or_else(|| self.config.agent.clone())
    }

    /// Dot + texto del estado del server (color + palabra, nunca color solo).
    fn server_status(&self, ui: &Ui) -> (Hsla, String) {
        if self.replay.is_some() {
            return (ui.fg_muted, "replay estático".to_owned());
        }
        match self.server {
            ServerPhase::Idle => (ui.fg_muted, "sin sesión".to_owned()),
            ServerPhase::Starting => (ui.warning, "arrancando…".to_owned()),
            ServerPhase::Ready => {
                if self.streaming() {
                    (ui.accent, "trabajando".to_owned())
                } else {
                    (ui.success, "en línea".to_owned())
                }
            }
            ServerPhase::Error => (ui.danger, "error".to_owned()),
        }
    }

    fn chip(&self, text: impl Into<SharedString>, color: Hsla) -> Div {
        div()
            .rounded(px(tok::R_SM))
            .bg(color.opacity(0.14))
            .px(px(5.))
            .py(px(1.))
            .text_size(px(tok::TEXT_XS))
            .line_height(relative(1.4))
            .text_color(color)
            .whitespace_nowrap()
            .child(text.into())
    }

    /// Chip removible (contexto del composer).
    fn chip_removable(
        &self,
        ui: &Ui,
        id: &'static str,
        text: String,
        on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        h_flex()
            .id(id)
            .items_center()
            .gap(px(4.))
            .rounded(px(tok::R_SM))
            .bg(ui.surface.opacity(0.7))
            .px(px(6.))
            .py(px(1.))
            .text_size(px(tok::TEXT_XS))
            .text_color(ui.fg_muted)
            .cursor_pointer()
            .hover(|style| style.text_color(ui.fg))
            .child(SharedString::from(text))
            .child(Icon::new(IconName::X).size(px(10.)))
            .on_click(on_close)
    }

    // --- render -----------------------------------------------------------------

    fn render_header(&self, ui: &Ui, cx: &mut Context<Self>) -> impl IntoElement {
        let (dot, status) = self.server_status(ui);
        let model_full = self.model_full();
        h_flex()
            .flex_shrink_0()
            .h(px(46.))
            .w_full()
            .items_center()
            .gap(px(8.))
            .px(px(14.))
            .border_b_1()
            .border_color(ui.border)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Icon::new(IconName::MessagesSquare)
                            .flex_shrink_0()
                            .size(px(15.))
                            .text_color(ui.fg_muted),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(tok::TEXT))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(self.conversation.title())),
                    ),
            )
            .when_some(self.replay.clone(), |this, _fixture| {
                this.child(self.chip("replay", ui.warning))
            })
            .child(self.render_agent_menu(ui, cx))
            .child(self.render_permission_menu(cx))
            .child(self.render_model_menu(ui, model_full, cx))
            .child(self.render_sessions_menu(ui, cx))
            .child(
                Button::new("chat-new-session")
                    .small()
                    .ghost()
                    .compact()
                    .icon(IconName::SquarePen)
                    .tooltip("Nueva sesión")
                    .disabled(self.replay.is_some())
                    .on_click(cx.listener(Self::on_new_session)),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap(px(6.))
                    .items_center()
                    .child(div().size(px(7.)).rounded_full().bg(dot))
                    .child(
                        div()
                            .text_size(px(tok::TEXT_SM))
                            .text_color(ui.fg_muted)
                            .child(SharedString::from(status)),
                    ),
            )
    }

    /// Selector de modelo: etiqueta + caret que abre el menú del catálogo.
    fn render_model_menu(
        &self,
        _ui: &Ui,
        model_full: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = self.model_short();
        let weak = cx.weak_entity();
        let current = model_full.unwrap_or_else(|| label.clone());
        let models = self.models.clone();
        Button::new("chat-model")
            .small()
            .ghost()
            .compact()
            .label(label)
            .dropdown_caret(true)
            .tooltip("Modelo de la sesión")
            .disabled(self.replay.is_some())
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, _cx| {
                let mut menu = menu.label("Modelo");
                if models.is_empty() {
                    return menu.item(
                        PopupMenuItem::new("Sin catálogo (arranca el server)").disabled(true),
                    );
                }
                let mut provider = String::new();
                for entry in &models {
                    if entry.provider_id != provider {
                        provider = entry.provider_id.clone();
                        menu = menu.separator().item(
                            PopupMenuItem::new(
                                entry
                                    .provider_name
                                    .clone()
                                    .unwrap_or_else(|| provider.clone()),
                            )
                            .disabled(true),
                        );
                    }
                    let id = entry.id();
                    let checked = id == current;
                    let weak = weak.clone();
                    let entry = entry.clone();
                    let label = entry.label();
                    menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                        move |_, _window, cx| {
                            let entry = entry.clone();
                            let _ = weak.update(cx, |panel, cx| {
                                panel.select_model(&entry, cx);
                            });
                        },
                    ));
                }
                menu
            })
    }

    /// Pide confirmación y revierte la sesión hasta `message_id` (M2): el
    /// revert nativo de opencode deshace también los archivos que tocó el
    /// agente desde ese punto.
    fn confirm_revert(&mut self, message_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.replay.is_some() {
            return;
        }
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let weak = weak.clone();
            let id = message_id.clone();
            dialog
                .title("Revertir hasta este mensaje")
                .width(px(520.))
                .child(DialogDescription::new().child(
                    "Los archivos vuelven al estado anterior a este mensaje y el historial se \
                     recorta aquí. Mientras no envíes otro mensaje podrás deshacerlo.",
                ))
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("chat-revert-cancel")
                                .label("Cancelar")
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("chat-revert-ok")
                                .label("Revertir")
                                .danger()
                                .on_click(move |_, window, cx| {
                                    let id = id.clone();
                                    let _ = weak.update(cx, |panel, cx| {
                                        panel.reverted_to = Some(id.clone());
                                        panel.send_command(
                                            ChatCmd::Revert {
                                                message_id: id.clone(),
                                            },
                                            cx,
                                        );
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }

    /// Deshace el último revert (`unrevert` de opencode).
    fn on_unrevert(&mut self, _: &ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.replay.is_some() {
            return;
        }
        self.reverted_to = None;
        self.send_command(ChatCmd::Unrevert, cx);
    }

    /// Aviso de sesión revertida con botón «Deshacer».
    fn render_reverted_banner(&self, ui: &Ui, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.reverted_to.as_ref()?;
        Some(
            h_flex()
                .w_full()
                .items_center()
                .gap(px(8.))
                .rounded(px(tok::R_MD))
                .bg(ui.warning.opacity(0.12))
                .px(px(10.))
                .py(px(6.))
                .child(
                    Icon::new(IconName::Undo2)
                        .size(px(13.))
                        .text_color(ui.warning),
                )
                .child(
                    div()
                        .text_size(px(tok::TEXT_SM))
                        .text_color(ui.fg)
                        .child("Cambios revertidos: los archivos volvieron al estado anterior"),
                )
                .child(div().flex_1())
                .child(
                    Button::new("chat-unrevert")
                        .small()
                        .ghost()
                        .label("Deshacer")
                        .on_click(cx.listener(Self::on_unrevert)),
                )
                .into_any_element(),
        )
    }

    /// Cambia el preset de permisos y avisa al shell para persistirlo.
    fn select_permission_preset(&mut self, preset: PermissionPreset, cx: &mut Context<Self>) {
        if self.permission_preset == preset {
            return;
        }
        self.permission_preset = preset;
        cx.emit(ChatPanelEvent::PermissionPresetChanged(
            preset.as_setting().to_owned(),
        ));
        cx.notify();
    }

    /// Menú «Permisos» del header (M1-E4): preguntar / ediciones / todo.
    fn render_permission_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.permission_preset;
        let weak = cx.weak_entity();
        Button::new("chat-permissions")
            .small()
            .ghost()
            .compact()
            .icon(IconName::ShieldCheck)
            .dropdown_caret(true)
            .selected(current != PermissionPreset::Ask)
            .tooltip(current.hint())
            .disabled(self.replay.is_some())
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, _cx| {
                let mut menu = menu.label("Permisos de la IA");
                for preset in [
                    PermissionPreset::Ask,
                    PermissionPreset::ProjectEdits,
                    PermissionPreset::All,
                ] {
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(preset.label())
                            .checked(preset == current)
                            .on_click(move |_, _window, cx| {
                                let _ = weak.update(cx, |panel, cx| {
                                    panel.select_permission_preset(preset, cx);
                                });
                            }),
                    );
                }
                menu.separator().item(
                    PopupMenuItem::new(
                        "«Aprobar todo» no pide confirmación: úsalo si tienes copias",
                    )
                    .disabled(true),
                )
            })
    }

    /// Selector de agente (`/agent`): build, plan, latex-fix…
    fn render_agent_menu(&self, _ui: &Ui, cx: &mut Context<Self>) -> impl IntoElement {
        let label = self.agent_label();
        let current = label.clone();
        let agents = self.agents.clone();
        let weak = cx.weak_entity();
        Button::new("chat-agent")
            .small()
            .ghost()
            .compact()
            .label(label)
            .dropdown_caret(true)
            .tooltip("Agente de opencode")
            .disabled(self.replay.is_some())
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, _cx| {
                let mut menu = menu.label("Agente");
                if agents.is_empty() {
                    return menu.item(
                        PopupMenuItem::new("Sin agentes (arranca el server)").disabled(true),
                    );
                }
                for agent in &agents {
                    let name = agent.name.clone();
                    let checked = name == current;
                    let weak = weak.clone();
                    menu = menu.item(PopupMenuItem::new(agent.label()).checked(checked).on_click(
                        move |_, _window, cx| {
                            let name = name.clone();
                            let _ = weak.update(cx, |panel, cx| {
                                panel.select_agent(name, cx);
                            });
                        },
                    ));
                }
                menu
            })
    }

    /// Menú de sesiones: cambiar, borrar y nueva.
    fn render_sessions_menu(&self, _ui: &Ui, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.weak_entity();
        let current = self.conversation.session_id.clone().unwrap_or_default();
        let sessions = self.conversation.sessions.clone();
        Button::new("chat-sessions")
            .small()
            .ghost()
            .compact()
            .icon(IconName::Clock)
            .tooltip("Sesiones del proyecto")
            .disabled(self.replay.is_some())
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, _cx| {
                let mut menu = menu.label("Sesiones");
                if sessions.is_empty() {
                    menu = menu.item(PopupMenuItem::new("Sin sesiones todavía").disabled(true));
                }
                for session in &sessions {
                    let id = session.id.clone();
                    let title = truncate_ui(&session.title, 48);
                    let weak_switch = weak.clone();
                    let switch_id = id.clone();
                    menu = menu.item(PopupMenuItem::new(title).checked(id == current).on_click(
                        move |_, _window, cx| {
                            let id = switch_id.clone();
                            let _ = weak_switch.update(cx, |panel, cx| {
                                panel.switch_session(id, cx);
                            });
                        },
                    ));
                    let weak_delete = weak.clone();
                    let delete_id = id.clone();
                    menu = menu.item(
                        PopupMenuItem::new("Borrar sesión")
                            .icon(IconName::Trash)
                            .on_click(move |_, _window, cx| {
                                let id = delete_id.clone();
                                let _ = weak_delete.update(cx, |panel, cx| {
                                    panel.delete_session(id, cx);
                                });
                            }),
                    );
                    menu = menu.separator();
                }
                let weak_new = weak.clone();
                menu.item(
                    PopupMenuItem::new("Nueva sesión")
                        .icon(IconName::SquarePen)
                        .on_click(move |_, _window, cx| {
                            let _ = weak_new.update(cx, |panel, cx| {
                                panel.new_session(cx);
                            });
                        }),
                )
            })
    }

    fn render_transcript(&self, ui: &Ui, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("chat-transcript")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .map(|this| {
                if self.conversation.messages.is_empty() && self.error_rows.is_empty() {
                    this.child(self.render_empty_state(ui))
                } else {
                    this.child(
                        v_flex()
                            .w_full()
                            .max_w(px(700.))
                            .mx_auto()
                            .px(px(20.))
                            .py(px(16.))
                            .gap(px(16.))
                            .children(self.conversation.messages.iter().enumerate().filter_map(
                                |(ix, message)| self.render_message(ui, ix, message, cx),
                            ))
                            .children(
                                self.error_rows
                                    .iter()
                                    .map(|error| self.render_error_row(ui, error.clone())),
                            )
                            .child(self.render_tail_status(ui)),
                    )
                }
            })
    }

    fn render_empty_state(&self, ui: &Ui) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(
                Icon::new(IconName::MessagesSquare)
                    .size(px(28.))
                    .text_color(ui.fg_muted.opacity(0.6)),
            )
            .child(
                div()
                    .text_size(px(tok::TEXT))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Aún no hay mensajes"),
            )
            .child(
                div()
                    .text_size(px(tok::TEXT_SM))
                    .text_color(ui.fg_muted)
                    .child("Pulsa «Nueva sesión» y escribe abajo para empezar."),
            )
    }

    /// Renderiza un mensaje; `None` si no tiene nada visible.
    fn render_message(
        &self,
        ui: &Ui,
        ix: usize,
        message: &lsx_chat::conversation::Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mut column = v_flex()
            .w_full()
            .gap(px(6.))
            .when_some(message.error.clone(), |this, error| {
                this.child(self.render_error_row(ui, error))
            });
        match message.role {
            Role::User => {
                let texts: Vec<String> = message
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        Part::Text { text, .. } if !text.is_empty() => Some(text.clone()),
                        _ => None,
                    })
                    .collect();
                if texts.is_empty() && message.error.is_none() {
                    return None;
                }
                let can_revert = self.replay.is_none()
                    && !self.conversation.streaming()
                    && message.id.starts_with("msg");
                let message_id = message.id.clone();
                column = column
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(tok::TEXT_XS))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(ui.fg_muted)
                                    .child("Tú"),
                            )
                            .child(div().flex_1())
                            .when(can_revert, |this| {
                                this.child(
                                    Button::new(SharedString::from(format!("chat-revert-{ix}")))
                                        .xsmall()
                                        .ghost()
                                        .compact()
                                        .icon(IconName::Undo2)
                                        .tooltip(
                                            "Revertir hasta este mensaje: deshace los cambios que \
                                             hizo el agente después",
                                        )
                                        .on_click(cx.listener(
                                            move |panel, _: &ClickEvent, window, cx| {
                                                panel.confirm_revert(
                                                    message_id.clone(),
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )),
                                )
                            }),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .rounded(px(tok::R_MD))
                            .bg(ui.surface.opacity(0.5))
                            .px(px(10.))
                            .py(px(8.))
                            .children(texts.into_iter().enumerate().map(|(iy, text)| {
                                TextView::markdown(
                                    SharedString::from(format!("user-text-{ix}-{iy}")),
                                    text,
                                )
                                .selectable(true)
                                .style(rich_style())
                            })),
                    );
            }
            Role::Assistant => {
                let items: Vec<AnyElement> = message
                    .parts
                    .iter()
                    .filter_map(|part| self.render_part(ui, ix, part, cx))
                    .collect();
                if items.is_empty() && message.error.is_none() {
                    return None;
                }
                column = column
                    .child(self.render_assistant_header(ui, message))
                    .children(items);
            }
        }
        Some(column.into_any_element())
    }

    fn render_assistant_header(
        &self,
        ui: &Ui,
        message: &lsx_chat::conversation::Message,
    ) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap(px(6.))
            .child(
                Icon::new(IconName::Sparkles)
                    .size(px(13.))
                    .text_color(ui.accent),
            )
            .child(
                div()
                    .text_size(px(tok::TEXT_XS))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ui.fg_muted)
                    .child("opencode"),
            )
            .when_some(message.model.clone(), |this, model| {
                this.child(self.chip(model, ui.fg_muted))
            })
    }

    fn render_error_row(&self, ui: &Ui, message: String) -> impl IntoElement {
        h_flex()
            .items_start()
            .gap(px(6.))
            .child(
                Icon::new(IconName::TriangleAlert)
                    .flex_shrink_0()
                    .size(px(14.))
                    .text_color(ui.danger),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(tok::TEXT_SM))
                    .text_color(ui.danger)
                    .child(SharedString::from(message)),
            )
    }

    fn render_part(
        &self,
        ui: &Ui,
        ix: usize,
        part: &Part,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match part {
            Part::Text { part_id, text, .. } if text.is_empty() => {
                let _ = part_id;
                None
            }
            Part::Text { part_id, text, .. } => Some(
                TextView::markdown(
                    SharedString::from(format!("asst-text-{ix}-{part_id}")),
                    text.clone(),
                )
                .selectable(true)
                .style(rich_style())
                .code_block_actions(|block, _window, cx| {
                    let theme = cx.theme();
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(
                            div()
                                .text_size(px(tok::TEXT_XS))
                                .text_color(theme.muted_foreground)
                                .child(block.lang().unwrap_or_else(|| SharedString::from("texto"))),
                        )
                        .child(
                            Clipboard::new("code-copy")
                                .xsmall()
                                .value(block.code())
                                .tooltip("Copiar código"),
                        )
                })
                .into_any_element(),
            ),
            Part::Reasoning {
                part_id,
                text,
                done,
            } => {
                if !self.show_reasoning && *done {
                    return None;
                }
                let expanded = self
                    .expanded_reasoning
                    .get(part_id)
                    .copied()
                    .unwrap_or(false);
                let done = *done;
                let id = part_id.clone();
                let trigger = h_flex()
                    .id(SharedString::from(format!("reasoning-{part_id}")))
                    .w_full()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .child(
                        Icon::new(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(px(12.))
                        .text_color(ui.fg_muted),
                    )
                    .child(
                        Icon::new(IconName::Brain)
                            .size(px(13.))
                            .text_color(ui.fg_muted),
                    )
                    .child(
                        div()
                            .text_size(px(tok::TEXT_SM))
                            .text_color(ui.fg_muted)
                            .child("Razonamiento"),
                    )
                    .when(!done, |this| {
                        this.child(Spinner::new().with_size(px(11.)).color(ui.fg_muted))
                    })
                    .on_click(
                        cx.listener(move |panel, _: &ClickEvent, _: &mut Window, cx| {
                            panel.on_toggle_reasoning(id.clone(), cx);
                        }),
                    );
                let content = div()
                    .pl(px(24.))
                    .pt(px(2.))
                    .text_size(px(tok::TEXT_SM))
                    .text_color(ui.fg_muted)
                    .child(
                        TextView::markdown(
                            SharedString::from(format!("reasoning-text-{part_id}")),
                            truncate_ui(text, 8000),
                        )
                        .style(reasoning_style()),
                    );
                Some(
                    Collapsible::new()
                        .open(expanded)
                        .child(trigger)
                        .content(content)
                        .into_any_element(),
                )
            }
            Part::Tool(call) => Some(self.render_tool(ui, call, cx)),
            Part::Permission { request_id } => self.render_permission(ui, request_id),
            Part::File { .. } | Part::Step { .. } => None,
        }
    }

    /// Tool card: una fila (chevron + icono + `nombre · resumen` + estado) y
    /// salida plegada (mono 11px). El look viene del spike aprobado.
    fn render_tool(&self, ui: &Ui, card: &ToolCall, cx: &mut Context<Self>) -> AnyElement {
        let expanded = self
            .expanded_tools
            .get(&card.part_id)
            .copied()
            .unwrap_or(false);
        let (status_color, status_label, status_icon) = match card.state {
            lsx_chat::events::SpikeToolState::Pending => {
                (ui.fg_muted, "pendiente", Some(IconName::Circle))
            }
            lsx_chat::events::SpikeToolState::Running => (ui.accent, "ejecutando", None),
            lsx_chat::events::SpikeToolState::Completed => {
                (ui.success, "completada", Some(IconName::CircleCheck))
            }
            lsx_chat::events::SpikeToolState::Error => {
                (ui.danger, "error", Some(IconName::CircleX))
            }
        };
        let id = card.part_id.clone();
        let trigger = h_flex()
            .id(SharedString::from(format!("tool-{}", card.part_id)))
            .w_full()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(4.))
            .cursor_pointer()
            .child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .flex_shrink_0()
                .size(px(12.))
                .text_color(ui.fg_muted),
            )
            .child(
                Icon::new(tool_icon(&card.name))
                    .flex_shrink_0()
                    .size(px(13.))
                    .text_color(ui.fg_muted),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(tok::TEXT_SM))
                    .font_weight(FontWeight::MEDIUM)
                    .child(SharedString::from(card.name.clone())),
            )
            .when(!card.summary.is_empty(), |this| {
                this.child(
                    div()
                        .text_size(px(tok::TEXT_SM))
                        .text_color(ui.fg_muted)
                        .child("·"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(tok::TEXT_SM))
                        .text_color(ui.fg_muted)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(SharedString::from(card.summary.clone())),
                )
            })
            .when(card.summary.is_empty(), |this| this.child(div().flex_1()))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(4.))
                    .when_some(status_icon, |this, icon| {
                        this.child(Icon::new(icon).size(px(12.)).text_color(status_color))
                    })
                    .when(
                        card.state == lsx_chat::events::SpikeToolState::Running,
                        |this| this.child(Spinner::new().with_size(px(11.)).color(status_color)),
                    )
                    .child(
                        div()
                            .text_size(px(tok::TEXT_XS))
                            .text_color(status_color)
                            .child(status_label),
                    ),
            )
            .on_click(
                cx.listener(move |panel, _: &ClickEvent, _: &mut Window, cx| {
                    panel.on_toggle_tool(id.clone(), cx);
                }),
            );

        let mut body = v_flex().gap(px(6.)).px(px(8.)).pb(px(8.)).pt(px(2.));
        let mut has_detail = false;
        if !card.args.is_empty() {
            body = body.child(self.mono_section(
                ui,
                SharedString::from(format!("mono-args-{}", card.part_id)),
                "argumentos",
                &card.args,
                ui.fg_muted,
            ));
            has_detail = true;
        }
        if let Some(error) = &card.error {
            body = body.child(self.mono_section(
                ui,
                SharedString::from(format!("mono-error-{}", card.part_id)),
                "error",
                error,
                ui.danger,
            ));
            has_detail = true;
        }
        if let Some(output) = &card.output {
            body = body.child(self.mono_section(
                ui,
                SharedString::from(format!("mono-output-{}", card.part_id)),
                "salida",
                output,
                ui.fg_muted,
            ));
            has_detail = true;
        }
        if let Some(diff) = &card.diff {
            let (additions, deletions) = diff_stats(Some(diff));
            body = body.child(
                h_flex()
                    .gap(px(6.))
                    .text_size(px(tok::TEXT_XS))
                    .child(div().flex_shrink_0().text_color(ui.fg_muted).child("diff"))
                    .child(div().text_color(ui.success).child(format!("+{additions}")))
                    .child(div().text_color(ui.danger).child(format!("−{deletions}"))),
            );
            has_detail = true;
        }
        if !has_detail {
            body = body.child(
                div()
                    .text_size(px(tok::TEXT_XS))
                    .text_color(ui.fg_muted)
                    .child("(sin salida)"),
            );
        }

        Collapsible::new()
            .open(expanded)
            .w_full()
            .rounded(px(tok::R_MD))
            .bg(ui.surface.opacity(if expanded { 0.45 } else { 0.3 }))
            .when(expanded, |this| this.border_1().border_color(ui.border))
            .child(trigger)
            .content(body)
            .into_any_element()
    }

    /// Bloque mono compacto (argumentos/salida de tools): borde 1px, fondo más
    /// oscuro que el panel.
    fn mono_section(
        &self,
        ui: &Ui,
        id: SharedString,
        label: &'static str,
        text: &str,
        text_color: Hsla,
    ) -> impl IntoElement {
        v_flex()
            .gap(px(2.))
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(ui.fg_muted)
                    .child(label),
            )
            .child(
                div()
                    .id(id)
                    .w_full()
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .rounded(px(tok::R_SM))
                    .border_1()
                    .border_color(ui.border)
                    .bg(rgb(color::CODE_BG))
                    .px(px(8.))
                    .py(px(5.))
                    .font_family(ui.mono.clone())
                    .text_size(px(tok::MONO_SM))
                    .line_height(relative(1.5))
                    .text_color(text_color)
                    .child(SharedString::from(truncate_ui(text, 8000))),
            )
    }

    /// Card de permiso inline: archivo + stats del diff + decisión. Las
    /// acciones viven en el modal ([`PermissionCard`]).
    fn render_permission(&self, ui: &Ui, request_id: &str) -> Option<AnyElement> {
        let entry = self.conversation.permission(request_id)?;
        let (reply_label, reply_color) = match entry.reply.as_deref() {
            Some("reject") => (Some("rechazado".to_owned()), ui.danger),
            Some("always") => (Some("siempre permitido".to_owned()), ui.accent),
            Some("once") => (Some("permitido".to_owned()), ui.success),
            Some(other) => (Some(truncate_ui(other, 16)), ui.fg_muted),
            None => (Some("esperando aprobación…".to_owned()), ui.warning),
        };
        let (additions, deletions) = diff_stats(entry.diff.as_deref());
        let file = entry.file();

        let mut file_row = h_flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(tok::TEXT_SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(file_label(&file))),
            )
            .child(self.chip(entry.permission.clone(), ui.warning))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap(px(3.))
                    .font_family(ui.mono.clone())
                    .text_size(px(tok::TEXT_XS))
                    .child(div().text_color(ui.success).child(format!("+{additions}")))
                    .child(div().text_color(ui.danger).child(format!("−{deletions}"))),
            );
        if let Some(label) = reply_label {
            file_row = file_row.child(self.chip(label, reply_color));
        }

        Some(
            v_flex()
                .w_full()
                .rounded(px(tok::R_MD))
                .border_1()
                .border_color(ui.warning.opacity(0.35))
                .bg(ui.surface.opacity(0.35))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(8.))
                        .child(
                            Icon::new(IconName::ShieldAlert)
                                .flex_shrink_0()
                                .size(px(15.))
                                .text_color(ui.warning),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(px(2.))
                                .child(file_row)
                                .child(
                                    div()
                                        .min_w_0()
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .font_family(ui.mono.clone())
                                        .text_size(px(tok::TEXT_XS))
                                        .text_color(ui.fg_muted)
                                        .child(SharedString::from(file)),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Estado del último turno (fin determinista del transcript).
    fn render_tail_status(&self, ui: &Ui) -> AnyElement {
        match self.conversation.status {
            TurnStatus::Busy | TurnStatus::Retry => h_flex()
                .items_center()
                .gap(px(6.))
                .child(Spinner::new().with_size(px(12.)).color(ui.accent))
                .child(
                    div()
                        .text_size(px(tok::TEXT_SM))
                        .text_color(ui.fg_muted)
                        .child("Trabajando…"),
                )
                .into_any_element(),
            TurnStatus::Idle if !self.conversation.messages.is_empty() => h_flex()
                .items_center()
                .gap(px(6.))
                .child(
                    Icon::new(IconName::CircleCheck)
                        .size(px(12.))
                        .text_color(ui.success),
                )
                .child(
                    div()
                        .text_size(px(tok::TEXT_SM))
                        .text_color(ui.fg_muted)
                        .child("Turno completado"),
                )
                .into_any_element(),
            _ => div().into_any_element(),
        }
    }

    fn render_composer(&self, ui: &Ui, cx: &mut Context<Self>) -> impl IntoElement {
        let streaming = self.streaming();
        let ready = self.server == ServerPhase::Ready;
        let replay = self.replay.is_some();
        let has_errors = self
            .fix_diagnostics
            .iter()
            .any(|line| line.severity == "error");
        v_flex()
            .flex_shrink_0()
            .w_full()
            .px(px(20.))
            .pt(px(6.))
            .pb(px(14.))
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(700.))
                    .mx_auto()
                    .gap(px(6.))
                    .when_some(self.render_reverted_banner(ui, cx), |this, banner| {
                        this.child(banner)
                    })
                    .when(
                        self.context_file.is_some()
                            || self.context_selection.is_some()
                            || has_errors,
                        |this| {
                            this.child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap(px(4.))
                                    .when_some(self.context_file.clone(), |row, (file, line)| {
                                        row.child(self.chip_removable(
                                            ui,
                                            "chat-context-file",
                                            format!("{}:{line}", truncate_ui(&file, 40)),
                                            cx.listener(Self::on_clear_context_file),
                                        ))
                                    })
                                    .when_some(self.context_selection.clone(), |row, selection| {
                                        row.child(self.chip_removable(
                                            ui,
                                            "chat-context-selection",
                                            format!(
                                                "selección · {} car.",
                                                selection.chars().count()
                                            ),
                                            cx.listener(Self::on_clear_context_selection),
                                        ))
                                    })
                                    .when(has_errors, |row| {
                                        row.child(self.chip(
                                            format!(
                                                    "{} errores",
                                                    self.fix_diagnostics
                                                        .iter()
                                                        .filter(|line| line.severity == "error")
                                                        .count()
                                                ),
                                            ui.danger,
                                        ))
                                    }),
                            )
                        },
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap(px(2.))
                            .rounded(px(tok::R_MD))
                            .border_1()
                            .border_color(ui.border)
                            .bg(ui.surface.opacity(0.35))
                            .px(px(8.))
                            .pt(px(6.))
                            .pb(px(6.))
                            // `Enter` envía; `Shift+Enter` mete salto de línea.
                            // Va en captura (antes del textarea) para que el
                            // salto no llegue a insertarse.
                            .capture_key_down(cx.listener(
                                |panel, event: &KeyDownEvent, window, cx| {
                                    let enter =
                                        matches!(event.keystroke.key.as_str(), "enter" | "return");
                                    if enter && !event.keystroke.modifiers.shift {
                                        cx.stop_propagation();
                                        panel.send_current(window, cx);
                                    }
                                },
                            ))
                            .child(
                                Textarea::new(&self.textarea)
                                    .appearance(false)
                                    .bordered(false)
                                    .w_full()
                                    .max_h(px(140.))
                                    .text_size(px(tok::TEXT))
                                    .readonly(replay),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(
                                        Button::new("chat-fix")
                                            .small()
                                            .ghost()
                                            .compact()
                                            .icon(IconName::Wrench)
                                            .tooltip("Arreglar errores con IA (agente latex-fix)")
                                            .disabled(replay)
                                            .on_click(cx.listener(Self::on_fix_errors)),
                                    )
                                    .child(
                                        Button::new("chat-attach")
                                            .small()
                                            .ghost()
                                            .compact()
                                            .icon(IconName::Plus)
                                            .tooltip("Añadir contexto: archivo activo")
                                            .disabled(replay || self.config.editor.is_none())
                                            .on_click(cx.listener(Self::on_attach_file)),
                                    )
                                    .child(div().flex_1())
                                    .when(streaming, |this| {
                                        this.child(
                                            Button::new("chat-abort")
                                                .ghost()
                                                .icon(IconName::Square)
                                                .tooltip("Detener")
                                                .on_click(cx.listener(Self::on_abort)),
                                        )
                                    })
                                    .child(
                                        Button::new("chat-send")
                                            .primary()
                                            .icon(IconName::ArrowUp)
                                            .tooltip("Enviar")
                                            .disabled(!ready || streaming)
                                            .on_click(cx.listener(Self::on_send)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(tok::TEXT_XS))
                            .text_color(ui.fg_muted.opacity(0.8))
                            .child(SharedString::from(self.composer_hint())),
                    ),
            )
    }

    /// Pista bajo el composer: modo replay, sesión viva o estado inicial.
    fn composer_hint(&self) -> String {
        if let Some(fixture) = &self.replay {
            return format!("Replay estático · {fixture} — sin server ni tokens.");
        }
        match (&self.conversation.session_id, self.port) {
            (Some(session_id), Some(port)) => {
                let tail: String = session_id
                    .chars()
                    .rev()
                    .take(6)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                format!(
                    "Sesión …{tail} · 127.0.0.1:{port} — envía con el botón; Stop aborta el turno."
                )
            }
            _ => match self.server {
                ServerPhase::Starting => "Arrancando opencode…".to_owned(),
                ServerPhase::Error => {
                    "opencode no está disponible; revisa agent\\opencode.exe.".to_owned()
                }
                _ => {
                    "Pulsa «Nueva sesión» para arrancar opencode; escribe abajo y envía.".to_owned()
                }
            },
        }
    }
}

impl Render for ChatPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = Ui::new(cx);
        v_flex()
            .size_full()
            .bg(rgb(color::PANEL))
            .text_color(ui.fg)
            .track_focus(&self.focus)
            .child(self.render_header(&ui, cx))
            .child(self.render_transcript(&ui, cx))
            .child(self.render_composer(&ui, cx))
    }
}

/// Icono lucide por nombre de tool (fallback: llave inglesa).
fn tool_icon(name: &str) -> IconName {
    match name {
        "bash" | "shell" | "terminal" => IconName::Terminal,
        "read" => IconName::FileText,
        "edit" | "write" | "patch" | "apply_patch" | "multiedit" => IconName::FilePen,
        "glob" | "grep" | "search" | "list" | "ls" => IconName::Search,
        "webfetch" | "websearch" | "fetch" => IconName::Globe,
        "task" | "todowrite" | "todoread" => IconName::ListTodo,
        _ => IconName::Wrench,
    }
}

/// Etiqueta de severidad para el prompt de reparación.
fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Info => "info",
    }
}

/// Acota un texto para chips/títulos de menú.
fn truncate_ui(text: &str, max: usize) -> String {
    lsx_chat::conversation::truncate_chars(text, max)
}

/// Resuelve una ruta de fixture relativa (cwd o raíz del repo de desarrollo).
fn resolve_fixture(path: &Path) -> PathBuf {
    if path.is_file() {
        return path.to_path_buf();
    }
    if let Some(repo) = lsx_core::root::repo_root() {
        let candidate = repo.join(path);
        if candidate.is_file() {
            return candidate;
        }
    }
    path.to_path_buf()
}

/// Detecta si un proyecto tiene un `main.tex` (ruta relativa con `/`). Helper
/// del panel (chips, log y prompt de reparación).
pub fn detect_main_file(project_root: &Path) -> Option<String> {
    let root = camino::Utf8Path::from_path(project_root)?;
    let main = lsx_latex::detect_main(root, None, None).ok()??;
    main.strip_prefix(root).ok().map(|rel| rel.to_string())
}

#[cfg(test)]
mod tests {
    // Nota (gpui-kit 0.7.3, `lib.rs` líneas 97-99): con `test-support` el glob
    // `gpui_kit::*` trae la macro `test` de GPUI y tapa la de Rust, así que
    // este módulo importa los tipos explícitamente (sin `use super::*`).
    use std::path::PathBuf;

    use gpui_kit::component::input::TextareaState;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, MouseButton,
        ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window,
    };

    use super::{ChatPanel, ChatPanelConfig, PermissionPreset};
    use crate::layout::SlotKind;
    use crate::panels::pane::{PaneState, pane_container};
    use crate::status::channel;

    /// Chrome mínimo de una hoja del shell: igual que `ChatSlot` (M1-A2), el
    /// contenedor de la hoja reclama el foco al hacer clic dentro.
    struct Host {
        pane: PaneState,
        chat: Entity<ChatPanel>,
    }

    impl Render for Host {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            pane_container(SlotKind::Chat, cx)
                .size_full()
                .track_focus(self.pane.focus_handle())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        // Mismo camino que `ChatSlot`: `Pane::activate_by_user`.
                        this.pane.focus_if_outside(window, cx);
                    }),
                )
                .child(self.chat.clone())
        }
    }

    /// Panel de chat en una ventana de test, dentro del chrome de hoja.
    ///
    /// `auto_start` en `false` y un `opencode.exe` inexistente: el panel no
    /// arranca server ni gasta tokens.
    fn with_chat<R>(
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut VisualTestContext, Entity<ChatPanel>, Entity<TextareaState>) -> R,
    ) -> R {
        cx.update(gpui_kit::init);
        let mut chat = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let (status, _rx) = channel();
            let config = ChatPanelConfig {
                exe: PathBuf::from("agent/opencode-no-existe.exe"),
                config_json: PathBuf::from("agent/opencode.json"),
                project_root: PathBuf::from("."),
                project_name: "test".to_owned(),
                auto_start: false,
                port: None,
                model: None,
                agent: "build".to_owned(),
                show_reasoning: false,
                permission_preset: PermissionPreset::Ask,
                runtime_config: PathBuf::from("config/opencode.runtime.json"),
                skills_dir: PathBuf::from("assets/skills"),
                path_prepend: Vec::new(),
                status,
                editor: None,
                problems: None,
                main_file: None,
            };
            let panel = cx.new(|cx| ChatPanel::new(window, cx, config));
            chat = Some(panel.clone());
            Host {
                pane: PaneState::new(SlotKind::Chat, None, cx),
                chat: panel,
            }
        });
        let chat = chat.unwrap();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let textarea = cx.read(|cx| chat.read(cx).textarea.clone());
        f(cx, chat, textarea)
    }

    /// Los presets de permisos deciden cuándo se puede prescindir del modal.
    #[test]
    fn los_presets_de_permisos_deciden_sin_preguntar() {
        use lsx_chat::protocol::PermissionReply;
        use std::path::Path;

        let root = Path::new("C:/proyecto");
        let inside = Some("C:/proyecto/main.tex");
        let outside = Some("C:/otro/main.tex");

        // Preguntar: nunca responde solo.
        assert_eq!(PermissionPreset::Ask.auto_reply("edit", inside, root), None);

        // Ediciones del proyecto: aprueba dentro; fuera y bash preguntan.
        assert_eq!(
            PermissionPreset::ProjectEdits.auto_reply("edit", inside, root),
            Some(PermissionReply::Always)
        );
        assert_eq!(
            PermissionPreset::ProjectEdits.auto_reply("edit", outside, root),
            None
        );
        assert_eq!(
            PermissionPreset::ProjectEdits.auto_reply("bash", inside, root),
            None
        );
        assert_eq!(
            PermissionPreset::ProjectEdits.auto_reply("edit", None, root),
            None
        );

        // Todo: aprueba cualquier petición, incluso sin ruta.
        assert_eq!(
            PermissionPreset::All.auto_reply("bash", None, root),
            Some(PermissionReply::Always)
        );

        // Ida y vuelta de lo que se persiste en `ai.permissionPreset`.
        for preset in [
            PermissionPreset::Ask,
            PermissionPreset::ProjectEdits,
            PermissionPreset::All,
        ] {
            assert_eq!(PermissionPreset::from_setting(preset.as_setting()), preset);
        }
        assert_eq!(
            PermissionPreset::from_setting("auto"),
            PermissionPreset::All
        );
        assert_eq!(
            PermissionPreset::from_setting("project-edits"),
            PermissionPreset::ProjectEdits
        );
    }

    /// Regresión (QA 2026-10-18): el clic en el composer debe dejar el foco en
    /// el textarea. Si el chrome de la hoja se lo roba, el usuario no puede
    /// escribir nada en el chat.
    #[test]
    fn clic_en_el_composer_permite_escribir() {
        if std::env::var_os("LSX_CHAT_REPLAY").is_some()
            || std::env::var_os("LSX_SPIKE_REPLAY").is_some()
        {
            eprintln!("replay activo: el composer es de solo lectura, test omitido");
            return;
        }
        with_chat(&mut TestAppContext::single(), |cx, _chat, textarea| {
            // Identificador del `Input` interno (gpui-component lo registra
            // como `("input", entity_id)` cuando el llamador no fija uno).
            let input_id = ("input", textarea.entity_id());
            cx.update(|window, cx| window.click(input_id, cx));
            cx.update(|window, cx| window.input("hola", cx));
            let text = cx.read(|cx| textarea.read(cx).value().to_string());
            assert_eq!(text, "hola", "el composer debe recibir lo tecleado");
        });
    }
}
