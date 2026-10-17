#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Spike M0-S4 — demo UI del pipeline **UI → supervisor → REST/SSE**.
//!
//! Botones mínimos: iniciar sesión (arranca `opencode serve` del bundle vía
//! `lsx-chat`), enviar el prompt corto y parar (mata el server). El texto del
//! asistente se muestra en streaming en una burbuja simple (sin markdown).
//!
//! Arquitectura del demo (patrón del skill `lsx-gpui-patterns`):
//! - runtime tokio multi-hilo en hilo propio; nunca se bloquea el hilo UI.
//! - UI → servicios: `UiToChat` por `async_channel`.
//! - servicios → UI: `ChatToUi` por `async_channel` + `cx.spawn` que reduce y
//!   llama `cx.notify`.
//!
//! Ejecutar: `cargo run -p latex-studio --bin spike-chat` (desde la raíz).
// El linker MSVC localizado (es-ES) imprime por stdout "Creando biblioteca …"
// al generar la .lib/.exp del binario; es ruido del toolchain, no nuestro.
#![allow(linker_messages)]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use lsx_chat::events::{SpikeEvent, SpikePartUpdate, SpikeToolState};
use lsx_chat::spike::{SpikeChat, SpikeConfig};

/// Prompt del spike (idéntico al del grabador de fixtures).
const PROMPT: &str = "Responde exactamente: hola";

/// UI → worker tokio.
enum UiToChat {
    Start,
    Prompt(String),
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

/// Tipo de parte correlacionado desde `message.part.updated` (los deltas no
/// traen tipo; ver `lsx_chat::events`).
#[derive(Clone, Copy, PartialEq)]
enum PartKind {
    Text,
    Reasoning,
    Other,
}

#[derive(Default)]
struct DemoState {
    phase: String,
    notes: Vec<String>,
    text_parts: BTreeMap<String, String>,
    reasoning: String,
    part_kinds: HashMap<String, PartKind>,
    busy: bool,
    /// Modo de verificación (`LSX_SPIKE_AUTORUN=1`): arranca, envía el prompt
    /// y sale solo al ver `session.idle` (usado por el smoke test).
    autorun: bool,
    autorun_done: bool,
    autorun_reported: bool,
    prompt_sent: bool,
}

struct SpikeChatDemo {
    state: DemoState,
    cmd_tx: async_channel::Sender<UiToChat>,
}

impl SpikeChatDemo {
    fn new(
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
                    cx.notify();
                    if demo.state.autorun_done && !demo.state.autorun_reported {
                        demo.state.autorun_reported = true;
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

        let demo = Self {
            state: DemoState {
                phase: "listo — pulsa «Iniciar sesión demo»".to_owned(),
                autorun,
                ..DemoState::default()
            },
            cmd_tx,
        };
        if autorun {
            let _ = demo.cmd_tx.try_send(UiToChat::Start);
        }
        demo
    }

    fn on_start(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.state.busy {
            return;
        }
        self.state.busy = true;
        let _ = self.cmd_tx.try_send(UiToChat::Start);
        cx.notify();
    }

    fn on_prompt(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.state.busy {
            return;
        }
        self.state.push_note(format!("» usuario: {PROMPT}"));
        let _ = self.cmd_tx.try_send(UiToChat::Prompt(PROMPT.to_owned()));
        cx.notify();
    }

    fn on_stop(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.cmd_tx.try_send(UiToChat::Stop);
        cx.notify();
    }

    fn apply(&mut self, message: ChatToUi) {
        match message {
            ChatToUi::Starting => self.state.phase = "arrancando opencode serve…".to_owned(),
            ChatToUi::Ready {
                port,
                session_id,
                model,
            } => {
                self.state.phase = format!("listo · puerto {port} · {session_id} · modelo {model}");
                self.state
                    .push_note(format!("[sesión {session_id} creada en 127.0.0.1:{port}]"));
                if self.state.autorun && !self.state.prompt_sent {
                    self.state.prompt_sent = true;
                    self.state.push_note(format!("» usuario: {PROMPT}"));
                    let _ = self.cmd_tx.try_send(UiToChat::Prompt(PROMPT.to_owned()));
                }
            }
            ChatToUi::Event(event) => self.apply_chat_event(*event),
            ChatToUi::Error(error) => {
                self.state.phase = format!("error: {error}");
                self.state.push_note(format!("[error] {error}"));
            }
            ChatToUi::Stopped => {
                self.state.busy = false;
                self.state.phase = "server detenido (sin procesos huérfanos)".to_owned();
            }
        }
    }

    /// Reducer de eventos de chat (semilla del reducer de M1-E2).
    fn apply_chat_event(&mut self, event: SpikeEvent) {
        match event {
            SpikeEvent::PartUpdated { part, .. } => match part {
                SpikePartUpdate::Text {
                    part_id,
                    markdown,
                    streaming: false,
                    ..
                } => {
                    self.state
                        .part_kinds
                        .insert(part_id.clone(), PartKind::Text);
                    // Snapshot final: reconcilia si faltan deltas.
                    let entry = self.state.text_parts.entry(part_id).or_default();
                    if entry.len() < markdown.len() {
                        *entry = markdown;
                    }
                }
                SpikePartUpdate::Text { part_id, .. } => {
                    self.state.part_kinds.insert(part_id, PartKind::Text);
                }
                SpikePartUpdate::Reasoning { part_id, done, .. } => {
                    self.state.part_kinds.insert(part_id, PartKind::Reasoning);
                    if done {
                        self.state.push_note("[razonamiento completo]".to_owned());
                    }
                }
                SpikePartUpdate::Tool {
                    part_id,
                    name,
                    state,
                    title,
                    error,
                    ..
                } => {
                    self.state.part_kinds.insert(part_id, PartKind::Other);
                    let label = title.unwrap_or_default();
                    let detail = match state {
                        SpikeToolState::Pending => "pendiente".to_owned(),
                        SpikeToolState::Running => format!("ejecutando {label}"),
                        SpikeToolState::Completed => "completada".to_owned(),
                        SpikeToolState::Error => format!(
                            "error: {}",
                            error.unwrap_or_else(|| "desconocido".to_owned())
                        ),
                    };
                    self.state.push_note(format!("[tool {name}: {detail}]"));
                }
                SpikePartUpdate::Step { .. }
                | SpikePartUpdate::File { .. }
                | SpikePartUpdate::Other { .. } => {}
            },
            SpikeEvent::PartDelta {
                part_id,
                delta,
                field,
                ..
            } if field == "text" => match self.state.part_kinds.get(&part_id) {
                Some(PartKind::Reasoning) => self.state.reasoning.push_str(&delta),
                Some(PartKind::Text) | None => {
                    self.state
                        .text_parts
                        .entry(part_id)
                        .or_default()
                        .push_str(&delta);
                }
                Some(PartKind::Other) => {}
            },
            SpikeEvent::MessageUpdated {
                role, completed, ..
            } if role == "assistant" => {
                if completed {
                    self.state.push_note("[respuesta completada]".to_owned());
                }
            }
            SpikeEvent::SessionStatus { status, .. } => {
                self.state.phase = format!("sesión: {status}");
            }
            SpikeEvent::SessionIdle { .. } => {
                self.state
                    .push_note("[— session.idle: fin de turno —]".to_owned());
                if self.state.autorun {
                    self.state.autorun_done = true;
                }
            }
            SpikeEvent::PermissionAsked {
                permission,
                patterns,
                ..
            } => {
                self.state.push_note(format!(
                    "[⚠ permission.asked: {permission} sobre {patterns:?} — modal en M1-A4/E4]"
                ));
            }
            SpikeEvent::SessionError { message, .. } => {
                self.state.push_note(format!("[error de sesión] {message}"));
            }
            _ => {}
        }
    }
}

impl DemoState {
    fn push_note(&mut self, note: String) {
        self.notes.push(note);
        if self.notes.len() > 40 {
            self.notes.remove(0);
        }
    }

    fn transcript_lines(&self) -> Vec<String> {
        let mut lines = self.notes.clone();
        if !self.reasoning.is_empty() {
            lines.push("· razonamiento (muted):".to_owned());
            lines.extend(self.reasoning.lines().map(|line| format!("  {line}")));
        }
        for text in self.text_parts.values() {
            lines.extend(text.lines().map(str::to_owned));
        }
        lines
    }
}

impl Render for SpikeChatDemo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines: Vec<AnyElement> = self
            .state
            .transcript_lines()
            .into_iter()
            .map(|line| div().child(SharedString::from(line)).into_any_element())
            .collect();
        div()
            .v_flex()
            .size_full()
            .p_6()
            .gap_3()
            .bg(rgb(0x10151C))
            .text_color(rgb(0xE8EDF2))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        Button::new("m0s4-start")
                            .label("Iniciar sesión demo")
                            .on_click(cx.listener(Self::on_start)),
                    )
                    .child(
                        Button::new("m0s4-prompt")
                            .label("Enviar «Responde exactamente: hola»")
                            .on_click(cx.listener(Self::on_prompt)),
                    )
                    .child(
                        Button::new("m0s4-stop")
                            .label("Parar")
                            .on_click(cx.listener(Self::on_stop)),
                    ),
            )
            .child(
                div()
                    .child(SharedString::from(format!("estado: {}", self.state.phase)))
                    .text_color(rgb(0x9FB3C8)),
            )
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .flex_1()
                    .gap_1()
                    .p_4()
                    .rounded_lg()
                    .bg(rgb(0x1B2530))
                    .child(div().child("burbuja spike M0-S4").text_color(rgb(0x6E8091)))
                    .children(lines),
            )
    }
}

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

fn main() {
    // Canales UI ↔ servicios, creados en `main` (el worker arranca antes de
    // abrir la ventana; el runtime tokio vive en un hilo propio).
    let (cmd_tx, cmd_rx) = async_channel::unbounded::<UiToChat>();
    let (ui_tx, ui_rx) = async_channel::unbounded::<ChatToUi>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    spawn_chat_worker(cmd_rx, ui_tx, done_tx);
    let cmd_tx_main = cmd_tx.clone();
    let autorun = std::env::var_os("LSX_SPIKE_AUTORUN").is_some();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                let cmd_tx = cmd_tx.clone();
                cx.new(move |cx| SpikeChatDemo::new(cx, cmd_tx.clone(), ui_rx, autorun))
            })
            .expect("no se pudo abrir la ventana del spike");
        });

    // Al cerrarse la app: parar el server aunque el usuario no pulsara «Parar»
    // (evita procesos opencode huérfanos) y esperar a que el worker termine.
    let _ = cmd_tx_main.try_send(UiToChat::Stop);
    let _ = done_rx.recv_timeout(Duration::from_secs(5));
}
