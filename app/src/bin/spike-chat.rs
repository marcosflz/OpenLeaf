#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(linker_messages)]
//! Spike M0-S4 — demo UI del pipeline **UI → supervisor → REST/SSE** con el
//! lenguaje visual del panel de **GitHub Copilot Chat** (compacto, sobrio, sin
//! burbujas grandes).
//!
//! Modos de ejecución:
//! - **Live** (`cargo run -p latex-studio --bin spike-chat`): «Nueva sesión»
//!   arranca `opencode serve` vía `lsx-chat`; el composer envía el prompt y los
//!   eventos SSE se reducen a la conversación en streaming.
//! - **Autorun** (`LSX_SPIKE_AUTORUN=1`): arranca, envía `PROMPT` y cierra al
//!   ver `session.idle`; imprime `SPIKE-AUTORUN-OK` (smoke E2E).
//! - **Replay** (`LSX_SPIKE_REPLAY=<fixture.jsonl>`): carga la fixture con
//!   `lsx_chat::replay::replay_file` y renderiza la conversación de forma
//!   estática y determinista — texto, tool cards con salida, card de permiso y
//!   estado del último turno — sin arrancar opencode y sin gastar tokens. La
//!   ventana no se cierra sola.
//!
//! Arquitectura del demo (patrón del skill `lsx-gpui-patterns`):
//! - runtime tokio multi-hilo en hilo propio; nunca se bloquea el hilo UI;
//! - UI → servicios: `UiToChat` por `async_channel`;
//! - servicios → UI: `ChatToUi` por `async_channel` + `cx.spawn` que reduce y
//!   llama `cx.notify`.
//!
//! El tema (acento `#4C8DFF`, superficies y radios) se fija una vez con
//! `Theme::update`; el resto del render lee tokens (`cx.theme()`), nunca hex.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::assets::{AllAssets, IconName};
use gpui_kit::component::button::*;
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::{TextView, TextViewStyle};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use lsx_chat::events::{SpikeEvent, SpikePartUpdate, SpikeToolState};
use lsx_chat::protocol::PermissionReply;
use lsx_chat::spike::{SpikeChat, SpikeConfig};

/// Prompt del spike (idéntico al del grabador de fixtures).
const PROMPT: &str = "Responde exactamente: hola";

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

/// Paleta oscura «VS Code · Copilot Chat». Único punto del binario con hex:
/// el resto lee tokens del tema.
mod color {
    /// Fondo del panel.
    pub const PANEL: u32 = 0x151A21;
    /// Texto principal.
    pub const FG: u32 = 0xE6EAF2;
    /// Texto secundario/muted.
    pub const FG_MUTED: u32 = 0x96A2B4;
    /// Superficie sutil (bloques de usuario, chips, hover).
    pub const SURFACE: u32 = 0x1E2530;
    /// Hairlines y bordes.
    pub const BORDER: u32 = 0x2A323E;
    /// Fondo del composer.
    pub const INPUT: u32 = 0x1A212B;
    /// Superficie de popovers (menú de modelo).
    pub const POPOVER: u32 = 0x1B222C;
    /// Acento pedido (#4C8DFF).
    pub const ACCENT: u32 = 0x4C8DFF;
    pub const ACCENT_HOVER: u32 = 0x5E9AFF;
    pub const ACCENT_ACTIVE: u32 = 0x3F80F0;
    /// Fondo de bloques de código (más oscuro que el panel).
    pub const CODE_BG: u32 = 0x0E1218;
}

/// UI → worker tokio.
enum UiToChat {
    Start,
    Prompt(String),
    /// `POST abort`: parar el streaming sin matar el server.
    Abort,
    /// Respuesta a una tarjeta de permiso de la conversación.
    Permission {
        request_id: String,
        reply: PermissionReply,
    },
    Stop,
}

/// Worker tokio → UI.
enum ChatToUi {
    Starting,
    Ready {
        port: u16,
        session_id: String,
        model: String,
    },
    /// Boxeado: `SpikeEvent` es grande y viaja por un canal sin beneficio de
    /// copiarlo inline (clippy::large_enum_variant).
    Event(Box<SpikeEvent>),
    Error(String),
    Stopped,
}

// ---------------------------------------------------------------------------
// Modelo de la conversación (reducer evento → estado)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolPhase {
    Pending,
    Running,
    Completed,
    Error,
}

/// Tipo de parte correlacionado desde `message.part.updated` (los deltas no
/// traen tipo; ver `lsx_chat::events`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum PartKind {
    Text,
    Reasoning,
    Other,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ServerPhase {
    #[default]
    Idle,
    Starting,
    Ready,
    Error,
}

/// Tool card: fila fina plegable (plegada por defecto).
struct ToolCard {
    name: String,
    /// `nombre · resumen`: título del server o derivado de los argumentos.
    summary: String,
    phase: ToolPhase,
    /// Argumentos legibles (patch crudo o JSON compacto, acotado).
    args: String,
    output: Option<String>,
    error: Option<String>,
    diff: Option<String>,
    expanded: bool,
}

/// Tarjeta de permiso: archivo + stats del diff + decisión.
struct PermissionCard {
    request_id: String,
    kind: String,
    file: String,
    additions: usize,
    deletions: usize,
    reply: Option<String>,
}

enum Item {
    Text {
        markdown: String,
        streaming: bool,
    },
    Reasoning {
        text: String,
        done: bool,
        expanded: bool,
    },
    Tool(ToolCard),
    Permission(PermissionCard),
}

struct Turn {
    role: Role,
    /// `modelID` del mensaje (chip de la etiqueta del asistente).
    model: Option<String>,
    items: Vec<Item>,
    completed: bool,
    error: Option<String>,
}

/// Estado de la conversación + de la sesión (semilla del reducer de M1-E2).
#[derive(Default)]
struct TranscriptState {
    turns: Vec<Turn>,
    message_ix: HashMap<String, usize>,
    part_ix: HashMap<String, (usize, usize)>,
    part_kind: HashMap<String, PartKind>,
    permission_ix: HashMap<String, (usize, usize)>,
    provider_id: Option<String>,
    model_id: Option<String>,
    session_title: Option<String>,
    /// Sesión viva (`ses_…`) y puerto del server (modo live).
    session_id: Option<String>,
    port: Option<u16>,
    session_status: Option<String>,
    server: ServerPhase,
    diff_files: usize,
    /// Modo de verificación (`LSX_SPIKE_AUTORUN=1`).
    autorun: bool,
    autorun_prompt_sent: bool,
    autorun_done: bool,
}

impl TranscriptState {
    fn streaming(&self) -> bool {
        matches!(self.session_status.as_deref(), Some("busy") | Some("retry"))
    }

    fn ensure_turn(&mut self, message_id: &str, default_role: Role) -> usize {
        if let Some(ix) = self.message_ix.get(message_id) {
            return *ix;
        }
        self.turns.push(Turn {
            role: default_role,
            model: None,
            items: Vec::new(),
            completed: false,
            error: None,
        });
        let ix = self.turns.len() - 1;
        self.message_ix.insert(message_id.to_owned(), ix);
        ix
    }

    fn push_item(&mut self, turn_ix: usize, item: Item) -> (usize, usize) {
        let iy = self.turns[turn_ix].items.len();
        self.turns[turn_ix].items.push(item);
        (turn_ix, iy)
    }

    fn item_mut(&mut self, (ix, iy): (usize, usize)) -> Option<&mut Item> {
        self.turns.get_mut(ix)?.items.get_mut(iy)
    }

    /// Reduce un evento de dominio (semilla del reducer de M1-E2).
    fn apply(&mut self, event: SpikeEvent) {
        match event {
            SpikeEvent::SessionCreated { title, .. } | SpikeEvent::SessionUpdated { title, .. } => {
                if !title.is_empty() {
                    self.session_title = Some(title);
                }
            }
            SpikeEvent::MessageUpdated {
                message_id,
                role,
                completed,
                provider_id,
                model_id,
                error,
                ..
            } => {
                let role = if role == "user" {
                    Role::User
                } else {
                    Role::Assistant
                };
                let ix = self.ensure_turn(&message_id, role);
                if completed {
                    self.turns[ix].completed = true;
                }
                if error.is_some() {
                    self.turns[ix].error = error;
                }
                if role == Role::Assistant {
                    if let Some(provider_id) = provider_id {
                        self.provider_id = Some(provider_id);
                    }
                    if let Some(model_id) = model_id {
                        self.turns[ix].model = Some(model_id.clone());
                        self.model_id = Some(model_id);
                    }
                }
            }
            SpikeEvent::PartUpdated { part, .. } => self.apply_part(part),
            SpikeEvent::PartDelta {
                message_id,
                part_id,
                field,
                delta,
                ..
            } if field == "text" => {
                if let Some((ix, iy)) = self.part_ix.get(&part_id).copied() {
                    match self.item_mut((ix, iy)) {
                        Some(Item::Text { markdown, .. }) => markdown.push_str(&delta),
                        Some(Item::Reasoning { text, .. }) => text.push_str(&delta),
                        _ => {}
                    }
                } else {
                    let kind = self
                        .part_kind
                        .get(&part_id)
                        .copied()
                        .unwrap_or(PartKind::Text);
                    let ix = self.ensure_turn(&message_id, Role::Assistant);
                    let item = match kind {
                        PartKind::Reasoning => Item::Reasoning {
                            text: delta,
                            done: false,
                            expanded: false,
                        },
                        _ => Item::Text {
                            markdown: delta,
                            streaming: true,
                        },
                    };
                    let at = self.push_item(ix, item);
                    self.part_ix.insert(part_id, at);
                }
            }
            SpikeEvent::PermissionAsked {
                request_id,
                permission,
                patterns,
                filepath,
                diff,
                ..
            } => {
                let file = patterns
                    .first()
                    .cloned()
                    .or(filepath)
                    .unwrap_or_else(|| "(sin ruta)".to_owned())
                    .replace('\\', "/");
                let (additions, deletions) = diff_stats(diff.as_deref());
                let turn_ix = match self
                    .turns
                    .iter()
                    .rposition(|turn| turn.role == Role::Assistant)
                {
                    Some(ix) => ix,
                    None => {
                        self.turns.push(Turn {
                            role: Role::Assistant,
                            model: None,
                            items: Vec::new(),
                            completed: false,
                            error: None,
                        });
                        self.turns.len() - 1
                    }
                };
                let at = self.push_item(
                    turn_ix,
                    Item::Permission(PermissionCard {
                        request_id: request_id.clone(),
                        kind: permission,
                        file,
                        additions,
                        deletions,
                        reply: None,
                    }),
                );
                self.permission_ix.insert(request_id, at);
            }
            SpikeEvent::PermissionReplied {
                request_id, reply, ..
            } => {
                if let Some(at) = self.permission_ix.get(&request_id).copied()
                    && let Some(Item::Permission(card)) = self.item_mut(at)
                {
                    card.reply = Some(reply);
                }
            }
            SpikeEvent::SessionStatus { status, .. } => {
                self.session_status = Some(status);
            }
            SpikeEvent::SessionIdle { .. } => {
                self.session_status = Some("idle".to_owned());
                // Cierra los snapshots abiertos (los user text nunca reciben
                // `time.end` en opencode 1.18.30).
                for turn in &mut self.turns {
                    for item in &mut turn.items {
                        match item {
                            Item::Text { streaming, .. } => *streaming = false,
                            Item::Reasoning { done, .. } => *done = true,
                            _ => {}
                        }
                    }
                }
                if self.autorun {
                    self.autorun_done = true;
                }
            }
            SpikeEvent::SessionError { message, .. } => {
                self.turns.push(Turn {
                    role: Role::Assistant,
                    model: None,
                    items: Vec::new(),
                    completed: true,
                    error: Some(message),
                });
            }
            SpikeEvent::SessionDiff { files, .. } => {
                self.diff_files = files;
            }
            SpikeEvent::PartDelta { .. }
            | SpikeEvent::ServerConnected
            | SpikeEvent::Heartbeat
            | SpikeEvent::SessionDeleted { .. }
            | SpikeEvent::PartRemoved { .. }
            | SpikeEvent::FileEdited { .. }
            | SpikeEvent::Ignored { .. } => {}
        }
    }

    fn apply_part(&mut self, part: SpikePartUpdate) {
        match part {
            SpikePartUpdate::Text {
                message_id,
                part_id,
                markdown,
                streaming,
            } => {
                self.part_kind.insert(part_id.clone(), PartKind::Text);
                if let Some((ix, iy)) = self.part_ix.get(&part_id).copied() {
                    if let Some(Item::Text {
                        markdown: current,
                        streaming: current_streaming,
                    }) = self.item_mut((ix, iy))
                    {
                        // El snapshot final reconcilia si faltaron deltas.
                        if markdown.len() >= current.len() {
                            *current = markdown;
                        }
                        *current_streaming = streaming && !current.is_empty();
                    }
                } else if !markdown.is_empty() {
                    let ix = self.ensure_turn(&message_id, Role::Assistant);
                    let at = self.push_item(
                        ix,
                        Item::Text {
                            markdown,
                            streaming,
                        },
                    );
                    self.part_ix.insert(part_id, at);
                }
            }
            SpikePartUpdate::Reasoning {
                message_id,
                part_id,
                text,
                done,
            } => {
                self.part_kind.insert(part_id.clone(), PartKind::Reasoning);
                if let Some((ix, iy)) = self.part_ix.get(&part_id).copied() {
                    if let Some(Item::Reasoning {
                        text: current,
                        done: current_done,
                        ..
                    }) = self.item_mut((ix, iy))
                    {
                        if text.len() >= current.len() {
                            *current = text;
                        }
                        *current_done = done;
                    }
                } else if !text.is_empty() {
                    let ix = self.ensure_turn(&message_id, Role::Assistant);
                    let at = self.push_item(
                        ix,
                        Item::Reasoning {
                            text,
                            done,
                            expanded: false,
                        },
                    );
                    self.part_ix.insert(part_id, at);
                }
            }
            SpikePartUpdate::Tool {
                message_id,
                part_id,
                name,
                state,
                input,
                title,
                output,
                error,
                diff,
                ..
            } => {
                self.part_kind.insert(part_id.clone(), PartKind::Other);
                let phase = match state {
                    SpikeToolState::Pending => ToolPhase::Pending,
                    SpikeToolState::Running => ToolPhase::Running,
                    SpikeToolState::Completed => ToolPhase::Completed,
                    SpikeToolState::Error => ToolPhase::Error,
                };
                // Se calcula aquí: `input` es `serde_json::Value`, que el crate
                // `app` no declara como dependencia directa (solo métodos).
                let args = if input.is_null() {
                    String::new()
                } else if let Some(patch) = input.get("patchText").and_then(|value| value.as_str())
                {
                    truncate_chars(patch, 4000)
                } else {
                    truncate_chars(&input.to_string(), 1200)
                };
                let mut summary = title
                    .as_deref()
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .map(|title| truncate_chars(title, 72))
                    .unwrap_or_default();
                if summary.is_empty() {
                    for key in [
                        "filePath", "filepath", "file", "path", "pattern", "url", "query",
                    ] {
                        if let Some(value) = input
                            .get(key)
                            .and_then(|value| value.as_str())
                            .filter(|value| !value.is_empty())
                        {
                            summary = truncate_chars(value, 72);
                            break;
                        }
                    }
                }
                if summary.is_empty()
                    && let Some(command) = input.get("command").and_then(|value| value.as_str())
                {
                    summary = truncate_chars(command.lines().next().unwrap_or_default(), 72);
                }
                if summary.is_empty()
                    && name.contains("patch")
                    && let Some(target) = patch_target(&args)
                {
                    summary = truncate_chars(&target, 72);
                }
                if let Some((ix, iy)) = self.part_ix.get(&part_id).copied() {
                    if let Some(Item::Tool(card)) = self.item_mut((ix, iy)) {
                        card.name = name;
                        card.phase = phase;
                        card.args = args;
                        if !summary.is_empty() {
                            card.summary = summary;
                        }
                        if output.is_some() {
                            card.output = output;
                        }
                        if error.is_some() {
                            card.error = error;
                        }
                        if diff.is_some() {
                            card.diff = diff;
                        }
                    }
                } else {
                    let ix = self.ensure_turn(&message_id, Role::Assistant);
                    let at = self.push_item(
                        ix,
                        Item::Tool(ToolCard {
                            name,
                            summary,
                            phase,
                            args,
                            output,
                            error,
                            diff,
                            expanded: false,
                        }),
                    );
                    self.part_ix.insert(part_id, at);
                }
            }
            SpikePartUpdate::File { .. }
            | SpikePartUpdate::Step { .. }
            | SpikePartUpdate::Other { .. } => {}
        }
    }
}

/// Estadísticas `+N −M` de un unified diff (cabeceras `+++`/`---` excluidas).
fn diff_stats(diff: Option<&str>) -> (usize, usize) {
    let mut additions = 0;
    let mut deletions = 0;
    for line in diff.unwrap_or_default().lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            additions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
    }
    (additions, deletions)
}

/// Primer `*** Add/Update/Delete File: <ruta>` de un patch de apply_patch.
fn patch_target(patch: &str) -> Option<String> {
    for line in patch.lines() {
        for prefix in ["*** Add File: ", "*** Update File: ", "*** Delete File: "] {
            if let Some(rest) = line.trim().strip_prefix(prefix) {
                return Some(rest.trim().to_owned());
            }
        }
    }
    None
}

/// Acota por caracteres, sin partir el string en medio de un carácter.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Nombre de fichero de una ruta (el último segmento, con `/` normalizado).
fn file_label(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .rsplit('/')
        .next()
        .filter(|last| !last.is_empty())
        .unwrap_or(&normalized)
        .to_owned()
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

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

/// Fija paleta, tipografía y radios una sola vez (único punto con hex).
fn apply_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        let accent: Hsla = rgb(color::ACCENT).into();
        let foreground: Hsla = rgb(color::FG).into();
        let surface: Hsla = rgb(color::SURFACE).into();

        theme.colors.background = rgb(color::PANEL).into();
        theme.colors.foreground = foreground;
        theme.colors.muted = surface;
        theme.colors.muted_foreground = rgb(color::FG_MUTED).into();
        theme.colors.border = rgb(color::BORDER).into();
        theme.colors.input = rgb(color::INPUT).into();
        theme.colors.popover = rgb(color::POPOVER).into();
        theme.colors.popover_foreground = foreground;

        theme.colors.primary = accent;
        theme.colors.primary_hover = rgb(color::ACCENT_HOVER).into();
        theme.colors.primary_active = rgb(color::ACCENT_ACTIVE).into();
        theme.colors.primary_foreground = rgb(0xFFFFFF).into();
        theme.colors.ring = accent;
        theme.colors.selection = accent.opacity(0.25);
        theme.colors.caret = accent;
        theme.colors.link = accent;
        theme.colors.link_hover = rgb(color::ACCENT_HOVER).into();
        theme.colors.link_active = rgb(color::ACCENT_ACTIVE).into();
        theme.colors.accent = surface;
        theme.colors.accent_foreground = foreground;

        theme.colors.button_primary = accent;
        theme.colors.button_primary_hover = rgb(color::ACCENT_HOVER).into();
        theme.colors.button_primary_active = rgb(color::ACCENT_ACTIVE).into();
        theme.colors.button_primary_foreground = rgb(0xFFFFFF).into();

        theme.font_size = px(tok::TEXT);
        theme.mono_font_size = px(tok::MONO_SM);
        theme.radius = px(tok::R_MD);
        theme.radius_lg = px(8.);
    });
}

/// Estilo de los markdown de la conversación: bloques de código oscuros con
/// borde, inline code sobre la superficie sutil y titulares contenidos (la
/// escala por defecto rompería la densidad del panel).
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

struct SpikeChatDemo {
    state: TranscriptState,
    /// Live: canal de comandos al worker tokio. `None` en replay.
    cmd_tx: Option<async_channel::Sender<UiToChat>>,
    textarea: Entity<TextareaState>,
    scroll: ScrollHandle,
    /// Nombre de la fixture en modo replay (badge del header).
    replay: Option<String>,
    autorun_reported: bool,
}

impl SpikeChatDemo {
    fn build(
        window: &mut Window,
        cx: &mut Context<Self>,
        state: TranscriptState,
        cmd_tx: Option<async_channel::Sender<UiToChat>>,
        replay: Option<String>,
    ) -> Self {
        let textarea = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Pregunta lo que quieras…")
                .auto_grow(1, 6)
        });
        Self {
            state,
            cmd_tx,
            textarea,
            scroll: ScrollHandle::default(),
            replay,
            autorun_reported: false,
        }
    }

    fn live(
        window: &mut Window,
        cx: &mut Context<Self>,
        cmd_tx: async_channel::Sender<UiToChat>,
        ui_rx: async_channel::Receiver<ChatToUi>,
        autorun: bool,
    ) -> Self {
        // Puente servicios → UI: reduce cada mensaje y repinta.
        cx.spawn(async move |this, cx| {
            while let Ok(message) = ui_rx.recv().await {
                let ok = this.update(cx, |demo, cx| {
                    demo.apply(message);
                    demo.scroll.scroll_to_bottom();
                    cx.notify();
                    if demo.state.autorun_done && !demo.autorun_reported {
                        demo.autorun_reported = true;
                        println!("SPIKE-AUTORUN-OK: streaming + session.idle en la entidad UI");
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                        cx.quit();
                    }
                });
                if ok.is_err() {
                    break;
                }
            }
        })
        .detach();

        let demo = Self::build(
            window,
            cx,
            TranscriptState {
                server: if autorun {
                    ServerPhase::Starting
                } else {
                    ServerPhase::Idle
                },
                autorun,
                ..TranscriptState::default()
            },
            Some(cmd_tx),
            None,
        );
        if autorun && let Some(cmd_tx) = &demo.cmd_tx {
            let _ = cmd_tx.try_send(UiToChat::Start);
        }
        demo
    }

    fn from_replay(
        window: &mut Window,
        cx: &mut Context<Self>,
        events: Vec<SpikeEvent>,
        fixture: String,
    ) -> Self {
        let mut state = TranscriptState::default();
        for event in events {
            state.apply(event);
        }
        // QA visual: `LSX_SPIKE_EXPAND=1` abre razonamiento y tool cards para
        // revisar el detalle plegado sin tocar la fixture (el default es
        // plegado, como en el panel real).
        if std::env::var_os("LSX_SPIKE_EXPAND").is_some() {
            for turn in &mut state.turns {
                for item in &mut turn.items {
                    match item {
                        Item::Reasoning { expanded, .. } => *expanded = true,
                        Item::Tool(card) => card.expanded = true,
                        _ => {}
                    }
                }
            }
        }
        Self::build(window, cx, state, None, Some(fixture))
    }

    // --- acciones ---------------------------------------------------------

    fn on_new_session(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(cmd_tx) = &self.cmd_tx else {
            return;
        };
        if self.state.server == ServerPhase::Starting {
            return;
        }
        self.state.server = ServerPhase::Starting;
        self.state.session_status = None;
        let _ = cmd_tx.try_send(UiToChat::Start);
        cx.notify();
    }

    fn on_send(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cmd_tx) = &self.cmd_tx else {
            return;
        };
        if self.state.server != ServerPhase::Ready || self.state.streaming() {
            return;
        }
        let text = self.textarea.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.textarea
            .update(cx, |state, cx| state.set_value("", window, cx));
        let _ = cmd_tx.try_send(UiToChat::Prompt(text));
        cx.notify();
    }

    fn on_abort(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(cmd_tx) = &self.cmd_tx {
            let _ = cmd_tx.try_send(UiToChat::Abort);
        }
        cx.notify();
    }

    fn on_toggle_item(&mut self, at: (usize, usize), cx: &mut Context<Self>) {
        if let Some(item) = self.state.item_mut(at) {
            match item {
                Item::Tool(card) => card.expanded = !card.expanded,
                Item::Reasoning { expanded, .. } => *expanded = !*expanded,
                _ => {}
            }
        }
        cx.notify();
    }

    fn on_permission(
        &mut self,
        request_id: String,
        reply: PermissionReply,
        cx: &mut Context<Self>,
    ) {
        if let Some(cmd_tx) = &self.cmd_tx {
            let _ = cmd_tx.try_send(UiToChat::Permission { request_id, reply });
        }
        cx.notify();
    }

    fn apply(&mut self, message: ChatToUi) {
        match message {
            ChatToUi::Starting => self.state.server = ServerPhase::Starting,
            ChatToUi::Ready {
                port,
                session_id,
                model,
            } => {
                self.state.server = ServerPhase::Ready;
                self.state.port = Some(port);
                self.state.session_id = Some(session_id);
                if let Some((provider, model)) = model.split_once('/') {
                    self.state.provider_id = Some(provider.to_owned());
                    self.state.model_id = Some(model.to_owned());
                }
                if self.state.autorun && !self.state.autorun_prompt_sent {
                    self.state.autorun_prompt_sent = true;
                    if let Some(cmd_tx) = &self.cmd_tx {
                        let _ = cmd_tx.try_send(UiToChat::Prompt(PROMPT.to_owned()));
                    }
                }
            }
            ChatToUi::Event(event) => self.state.apply(*event),
            ChatToUi::Error(error) => {
                self.state.server = ServerPhase::Error;
                self.state.turns.push(Turn {
                    role: Role::Assistant,
                    model: None,
                    items: Vec::new(),
                    completed: true,
                    error: Some(error),
                });
            }
            ChatToUi::Stopped => self.state.server = ServerPhase::Idle,
        }
    }

    // --- helpers ------------------------------------------------------------

    fn session_title(&self) -> String {
        self.state
            .session_title
            .clone()
            .unwrap_or_else(|| "Sesión de chat".to_owned())
    }

    fn model_full(&self) -> Option<String> {
        let model = self.state.model_id.clone()?;
        Some(match &self.state.provider_id {
            Some(provider) => format!("{provider}/{model}"),
            None => model,
        })
    }

    fn model_short(&self) -> String {
        self.state
            .model_id
            .clone()
            .unwrap_or_else(|| "sin modelo".to_owned())
    }

    /// Dot + texto del estado del server (color + palabra, nunca color solo).
    fn server_status(&self, ui: &Ui) -> (Hsla, String) {
        if self.replay.is_some() {
            return (ui.fg_muted, "replay estático".to_owned());
        }
        match self.state.server {
            ServerPhase::Idle => (ui.fg_muted, "sin sesión".to_owned()),
            ServerPhase::Starting => (ui.warning, "arrancando…".to_owned()),
            ServerPhase::Ready => {
                if self.state.streaming() {
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

    // --- render -------------------------------------------------------------

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
                            .child(SharedString::from(self.session_title())),
                    ),
            )
            .when_some(self.replay.clone(), |this, _fixture| {
                this.child(self.chip("replay", ui.warning))
            })
            .child(self.render_model_menu(model_full))
            .child(
                Button::new("spike-new-session")
                    .small()
                    .ghost()
                    .compact()
                    .icon(IconName::SquarePen)
                    .tooltip("Nueva sesión")
                    .disabled(self.cmd_tx.is_none())
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

    /// Selector de modelo pequeño: etiqueta + caret que abre el menú.
    fn render_model_menu(&self, model_full: Option<String>) -> impl IntoElement {
        let label = self.model_short();
        let current = model_full.unwrap_or_else(|| label.clone());
        Button::new("spike-model")
            .small()
            .ghost()
            .compact()
            .label(label)
            .dropdown_caret(true)
            .tooltip("Modelo de la sesión")
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, _cx| {
                let mut menu = menu.label("Modelo");
                menu = menu.item(PopupMenuItem::new(current.clone()).checked(true));
                menu.separator()
                    .item(PopupMenuItem::new("Cambiar modelo (M1)…").disabled(true))
            })
    }

    fn render_transcript(&self, ui: &Ui, cx: &mut Context<Self>) -> impl IntoElement {
        let tail = self.render_tail_status(ui);
        div()
            .id("spike-transcript")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .map(|this| {
                if self.state.turns.is_empty() {
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
                            .children(
                                self.state
                                    .turns
                                    .iter()
                                    .enumerate()
                                    .map(|(ix, turn)| self.render_turn(ui, ix, turn, cx)),
                            )
                            .when_some(tail, |this, tail| this.child(tail)),
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

    fn render_turn(&self, ui: &Ui, ix: usize, turn: &Turn, cx: &mut Context<Self>) -> AnyElement {
        let mut column = v_flex()
            .w_full()
            .gap(px(6.))
            .when_some(turn.error.clone(), |this, error| {
                this.child(self.render_error_row(ui, error))
            });
        match turn.role {
            Role::User => {
                column = column
                    .child(
                        div()
                            .text_size(px(tok::TEXT_XS))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ui.fg_muted)
                            .child("Tú"),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .rounded(px(tok::R_MD))
                            .bg(ui.surface.opacity(0.5))
                            .px(px(10.))
                            .py(px(8.))
                            .children(turn.items.iter().enumerate().filter_map(|(iy, item)| {
                                match item {
                                    Item::Text { markdown, .. } if !markdown.is_empty() => Some(
                                        TextView::markdown(
                                            SharedString::from(format!("user-text-{ix}-{iy}")),
                                            markdown.clone(),
                                        )
                                        .selectable(true)
                                        .style(rich_style())
                                        .into_any_element(),
                                    ),
                                    _ => None,
                                }
                            })),
                    );
            }
            Role::Assistant => {
                column = column
                    .child(self.render_assistant_header(ui, turn))
                    .children(
                        turn.items
                            .iter()
                            .enumerate()
                            .filter_map(|(iy, item)| self.render_item(ui, ix, iy, item, cx)),
                    );
            }
        }
        column.into_any_element()
    }

    fn render_assistant_header(&self, ui: &Ui, turn: &Turn) -> impl IntoElement {
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
            .when_some(turn.model.clone(), |this, model| {
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

    fn render_item(
        &self,
        ui: &Ui,
        ix: usize,
        iy: usize,
        item: &Item,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match item {
            Item::Text { markdown, .. } if markdown.is_empty() => None,
            Item::Text { markdown, .. } => Some(
                TextView::markdown(
                    SharedString::from(format!("asst-text-{ix}-{iy}")),
                    markdown.clone(),
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
            Item::Reasoning {
                text,
                done,
                expanded,
            } => {
                let expanded = *expanded;
                let done = *done;
                let trigger = h_flex()
                    .id(SharedString::from(format!("reasoning-{ix}-{iy}")))
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
                        cx.listener(move |demo, _: &ClickEvent, _: &mut Window, cx| {
                            demo.on_toggle_item((ix, iy), cx);
                        }),
                    );
                let content = div()
                    .pl(px(24.))
                    .pt(px(2.))
                    .text_size(px(tok::TEXT_SM))
                    .text_color(ui.fg_muted)
                    .child(
                        TextView::markdown(
                            SharedString::from(format!("reasoning-text-{ix}-{iy}")),
                            truncate_chars(text, 8000),
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
            Item::Tool(card) => Some(self.render_tool(ui, ix, iy, card, cx)),
            Item::Permission(card) => Some(self.render_permission(ui, card, cx)),
        }
    }

    /// Tool card: una fila (chevron + icono + `nombre · resumen` + estado) y
    /// salida plegada (mono 11px).
    fn render_tool(
        &self,
        ui: &Ui,
        ix: usize,
        iy: usize,
        card: &ToolCard,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (status_color, status_label) = match card.phase {
            ToolPhase::Pending => (ui.fg_muted, "pendiente"),
            ToolPhase::Running => (ui.accent, "ejecutando"),
            ToolPhase::Completed => (ui.success, "completada"),
            ToolPhase::Error => (ui.danger, "error"),
        };
        let trigger = h_flex()
            .id(SharedString::from(format!("tool-{ix}-{iy}")))
            .w_full()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(4.))
            .cursor_pointer()
            .child(
                Icon::new(if card.expanded {
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
                    .when(card.phase == ToolPhase::Running, |this| {
                        this.child(Spinner::new().with_size(px(11.)).color(status_color))
                    })
                    .when(card.phase != ToolPhase::Running, |this| {
                        this.child(
                            Icon::new(match card.phase {
                                ToolPhase::Error => IconName::CircleX,
                                ToolPhase::Completed => IconName::CircleCheck,
                                _ => IconName::Circle,
                            })
                            .size(px(12.))
                            .text_color(status_color),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(tok::TEXT_XS))
                            .text_color(status_color)
                            .child(status_label),
                    ),
            )
            .on_click(
                cx.listener(move |demo, _: &ClickEvent, _: &mut Window, cx| {
                    demo.on_toggle_item((ix, iy), cx);
                }),
            );

        let mut body = v_flex().gap(px(6.)).px(px(8.)).pb(px(8.)).pt(px(2.));
        let mut has_detail = false;
        if !card.args.is_empty() {
            body = body.child(self.mono_section(
                ui,
                SharedString::from(format!("mono-args-{ix}-{iy}")),
                "argumentos",
                &card.args,
                ui.fg_muted,
            ));
            has_detail = true;
        }
        if let Some(error) = &card.error {
            body = body.child(self.mono_section(
                ui,
                SharedString::from(format!("mono-error-{ix}-{iy}")),
                "error",
                error,
                ui.danger,
            ));
            has_detail = true;
        }
        if let Some(output) = &card.output {
            body = body.child(self.mono_section(
                ui,
                SharedString::from(format!("mono-output-{ix}-{iy}")),
                "salida",
                output,
                ui.fg_muted,
            ));
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
            .open(card.expanded)
            .w_full()
            .rounded(px(tok::R_MD))
            .bg(ui.surface.opacity(if card.expanded { 0.45 } else { 0.3 }))
            .when(card.expanded, |this| {
                this.border_1().border_color(ui.border)
            })
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
                    .child(SharedString::from(truncate_chars(text, 8000))),
            )
    }

    /// Card de permiso: archivo + stats del diff + 3 botones pequeños.
    fn render_permission(
        &self,
        ui: &Ui,
        card: &PermissionCard,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let replied = card.reply.is_some();
        let replay = self.cmd_tx.is_none();
        let (reply_label, reply_color) = match card.reply.as_deref() {
            Some("reject") => (Some("rechazado".to_owned()), ui.danger),
            Some("always") => (Some("siempre permitido".to_owned()), ui.accent),
            Some("once") => (Some("permitido".to_owned()), ui.success),
            Some(other) => (Some(truncate_chars(other, 16)), ui.fg_muted),
            None => (None, ui.fg_muted),
        };

        let mut file_row = h_flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(tok::TEXT_SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(file_label(&card.file))),
            )
            .child(self.chip(card.kind.clone(), ui.warning))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap(px(3.))
                    .font_family(ui.mono.clone())
                    .text_size(px(tok::TEXT_XS))
                    .child(
                        div()
                            .text_color(ui.success)
                            .child(format!("+{}", card.additions)),
                    )
                    .child(
                        div()
                            .text_color(ui.danger)
                            .child(format!("−{}", card.deletions)),
                    ),
            );
        if let Some(label) = reply_label {
            file_row = file_row.child(self.chip(label, reply_color));
        }

        let mut actions = h_flex().flex_shrink_0().gap(px(4.)).items_center();
        let buttons = [
            (
                "permitir",
                "Permitir",
                "Permitir una vez",
                PermissionReply::Once,
                ButtonStyleKind::Primary,
            ),
            (
                "siempre",
                "Siempre",
                "Permitir siempre en esta sesión",
                PermissionReply::Always,
                ButtonStyleKind::Subtle,
            ),
            (
                "rechazar",
                "Rechazar",
                "Rechazar la edición",
                PermissionReply::Reject,
                ButtonStyleKind::DangerGhost,
            ),
        ];
        for (name, label, tooltip, reply, kind) in buttons {
            let mut button = Button::new(SharedString::from(format!(
                "perm-{}-{name}",
                card.request_id
            )))
            .xsmall()
            .label(label)
            .tooltip(tooltip)
            .disabled(replied || replay);
            button = match kind {
                ButtonStyleKind::Primary => button.primary(),
                ButtonStyleKind::Subtle => button.outline(),
                ButtonStyleKind::DangerGhost => button.ghost().text_color(ui.danger),
            };
            if card.reply.as_deref() == Some(reply.as_str()) {
                button = button.selected(true);
            }
            let request_id = card.request_id.clone();
            actions = actions.child(button.on_click(cx.listener(
                move |demo, _: &ClickEvent, _: &mut Window, cx| {
                    demo.on_permission(request_id.clone(), reply, cx);
                },
            )));
        }

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
                                    .child(SharedString::from(card.file.clone())),
                            ),
                    )
                    .child(actions),
            )
            .into_any_element()
    }

    /// Estado del último turno (fin determinista del transcript).
    fn render_tail_status(&self, ui: &Ui) -> Option<AnyElement> {
        match self.state.session_status.as_deref() {
            Some("busy") | Some("retry") => Some(
                h_flex()
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
            ),
            Some("idle") if !self.state.turns.is_empty() => Some(
                h_flex()
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
            ),
            _ => None,
        }
    }

    fn render_composer(&self, ui: &Ui, cx: &mut Context<Self>) -> impl IntoElement {
        let streaming = self.state.streaming();
        let ready = self.state.server == ServerPhase::Ready;
        let replay = self.cmd_tx.is_none();
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
                                        Button::new("spike-attach")
                                            .small()
                                            .ghost()
                                            .compact()
                                            .icon(IconName::Plus)
                                            .tooltip("Añadir contexto (M1)")
                                            .disabled(true),
                                    )
                                    .child(div().flex_1())
                                    .when(streaming, |this| {
                                        this.child(
                                            Button::new("spike-abort")
                                                .ghost()
                                                .icon(IconName::Square)
                                                .tooltip("Detener")
                                                .on_click(cx.listener(Self::on_abort)),
                                        )
                                    })
                                    .child(
                                        Button::new("spike-send")
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
        match (&self.state.session_id, self.state.port) {
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
            _ => "Pulsa «Nueva sesión» para arrancar opencode; escribe abajo y envía.".to_owned(),
        }
    }
}

enum ButtonStyleKind {
    Primary,
    Subtle,
    DangerGhost,
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

impl Render for SpikeChatDemo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = Ui::new(cx);
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(ui.fg)
            .child(self.render_header(&ui, cx))
            .child(self.render_transcript(&ui, cx))
            .child(self.render_composer(&ui, cx))
    }
}

// ---------------------------------------------------------------------------
// Worker tokio + main
// ---------------------------------------------------------------------------

/// Hilo propio con runtime tokio: ejecuta el supervisor y reenvía todo a la UI.
/// `done_tx` avisa a `main` (tras cerrarse la app) de que el server ya paró.
fn spawn_chat_worker(
    cmd_rx: async_channel::Receiver<UiToChat>,
    ui_tx: async_channel::Sender<ChatToUi>,
    done_tx: std::sync::mpsc::Sender<()>,
) {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = ui_tx.send_blocking(ChatToUi::Error(format!(
                    "no se pudo crear el runtime tokio: {error}"
                )));
                let _ = done_tx.send(());
                return;
            }
        };
        runtime.block_on(async move {
            let cfg = SpikeConfig::from_repo_root(&find_repo_root());
            let mut chat: Option<SpikeChat> = None;
            while let Ok(command) = cmd_rx.recv().await {
                match command {
                    UiToChat::Start => {
                        let _ = ui_tx.send(ChatToUi::Starting).await;
                        match SpikeChat::start(&cfg, None).await {
                            Ok((handle, info)) => {
                                let model = info
                                    .model
                                    .as_ref()
                                    .map(|m| m.to_string())
                                    .unwrap_or_else(|| "sin modelo utilizable".to_owned());
                                let _ = ui_tx
                                    .send(ChatToUi::Ready {
                                        port: info.port,
                                        session_id: info.session_id.clone(),
                                        model,
                                    })
                                    .await;
                                // Eventos → UI (un único consumidor del canal).
                                let events = handle.events();
                                let ui_tx_events = ui_tx.clone();
                                tokio::spawn(async move {
                                    while let Ok(event) = events.recv().await {
                                        if ui_tx_events
                                            .send(ChatToUi::Event(Box::new(event)))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                });
                                chat = Some(handle);
                            }
                            Err(error) => {
                                let _ = ui_tx.send(ChatToUi::Error(error.to_string())).await;
                            }
                        }
                    }
                    UiToChat::Prompt(text) => {
                        if let Some(handle) = &chat
                            && let Err(error) = handle.send_prompt(&text).await
                        {
                            let _ = ui_tx.send(ChatToUi::Error(error.to_string())).await;
                        }
                    }
                    UiToChat::Abort => {
                        if let Some(handle) = &chat
                            && let Err(error) = handle.abort().await
                        {
                            let _ = ui_tx.send(ChatToUi::Error(error.to_string())).await;
                        }
                    }
                    UiToChat::Permission { request_id, reply } => {
                        if let Some(handle) = &chat
                            && let Err(error) = handle.respond_permission(&request_id, reply).await
                        {
                            let _ = ui_tx.send(ChatToUi::Error(error.to_string())).await;
                        }
                    }
                    UiToChat::Stop => break,
                }
            }
            if let Some(handle) = chat {
                let _ = handle.stop().await;
            }
            let _ = ui_tx.send(ChatToUi::Stopped).await;
        });
        runtime.shutdown_timeout(Duration::from_secs(2));
        let _ = done_tx.send(());
    });
}

/// Raíz del repo portable: cwd o `target/{debug,release}/..` (según se lance).
fn find_repo_root() -> PathBuf {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(root) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
    {
        candidates.push(root.to_path_buf());
    }
    for candidate in candidates {
        if candidate.join("agent").join("opencode.exe").is_file() {
            return candidate;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Opciones de ventana del spike (≈1000x700, título propio).
fn window_options() -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(120.), px(80.)),
            size(px(1000.), px(700.)),
        ))),
        window_min_size: Some(size(px(720.), px(520.))),
        titlebar: Some(TitlebarOptions {
            title: Some("LaTeX Studio — Chat (spike S4)".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn main() {
    if let Some(path) = std::env::var_os("LSX_SPIKE_REPLAY").map(PathBuf::from) {
        run_replay(path);
        return;
    }
    run_live();
}

/// Modo replay: fixture → conversación estática, sin worker ni opencode.
fn run_replay(path: PathBuf) {
    let events = match lsx_chat::replay::replay_file(&path) {
        Ok(events) => events,
        Err(error) => {
            eprintln!("LSX_SPIKE_REPLAY: {} no replaya: {error}", path.display());
            std::process::exit(2);
        }
    };
    let fixture = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("fixture.jsonl")
        .to_owned();
    let total = events.len();
    gpui_kit::application()
        .with_assets(AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            apply_theme(cx);
            gpui_kit::open_window(window_options(), cx, |window, cx| {
                cx.new(move |cx| SpikeChatDemo::from_replay(window, cx, events, fixture))
            })
            .expect("no se pudo abrir la ventana del spike");
            println!(
                "SPIKE-REPLAY: {total} eventos cargados desde {}",
                path.display()
            );
        });
}

/// Modo live/autorun: worker tokio + canales, como el resto de spikes.
fn run_live() {
    let (cmd_tx, cmd_rx) = async_channel::unbounded::<UiToChat>();
    let (ui_tx, ui_rx) = async_channel::unbounded::<ChatToUi>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    spawn_chat_worker(cmd_rx, ui_tx, done_tx);
    let cmd_tx_main = cmd_tx.clone();
    let autorun = std::env::var_os("LSX_SPIKE_AUTORUN").is_some();

    gpui_kit::application()
        .with_assets(AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            apply_theme(cx);
            gpui_kit::open_window(window_options(), cx, |window, cx| {
                let cmd_tx = cmd_tx.clone();
                cx.new(move |cx| SpikeChatDemo::live(window, cx, cmd_tx.clone(), ui_rx, autorun))
            })
            .expect("no se pudo abrir la ventana del spike");
        });

    // Al cerrarse la app: parar el server aunque el usuario no pulsara «Parar»
    // (evita procesos opencode huérfanos) y esperar a que el worker termine.
    let _ = cmd_tx_main.try_send(UiToChat::Stop);
    let _ = done_rx.recv_timeout(Duration::from_secs(5));
}
