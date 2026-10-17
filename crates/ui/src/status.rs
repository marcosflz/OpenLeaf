//! Estado compartido del shell por canal (M1-A2).
//!
//! Los paneles no conocen a `AppRoot`: emiten [`StatusMessage`]s por un
//! [`StatusHandle`] (async_channel) y el shell las reduce a [`StatusModel`].
//! El modelo es puro (sin GPUI) y se testea sin ventana.
//!
//! ```text
//! panel (editor/pdf/tree/chat) ── StatusMessage ──► StatusModel ──► status bar
//! ```

use async_channel::{Receiver, Sender, unbounded};

/// Estado de compilación visible en toolbar y status bar.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CompileState {
    /// Sin compilación en curso.
    #[default]
    Idle,
    /// Petición encolada (CompileQueue, M1-D2).
    Queued,
    /// Compilando; `hint` es la última línea de progreso.
    Running { hint: Option<String> },
    /// Terminó bien.
    Success { duration_ms: u64 },
    /// Terminó con error; `message` es el resumen para el usuario.
    Failed { message: String },
}

impl CompileState {
    /// `true` mientras hay trabajo pendiente o en curso.
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Queued | Self::Running { .. })
    }
}

/// Estado del asistente IA (supervisor opencode, M1-E1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AiState {
    /// Servidor parado.
    #[default]
    Stopped,
    /// Arrancando o reconectando.
    Starting,
    /// Servidor listo.
    Ready,
    /// Falló el arranque o se cayó por encima del backoff.
    Error,
}

/// Mensaje que un panel (o un servicio) envía al shell.
#[derive(Clone, Debug, PartialEq)]
pub enum StatusMessage {
    /// Proyecto activo (nombre visible).
    Project { name: Option<String> },
    /// Motor LaTeX y disponibilidad.
    Engine { label: String, available: bool },
    /// Posición del cursor del editor.
    Cursor { line: u32, column: u32 },
    /// Recuento de palabras del documento activo.
    Words { count: u32 },
    /// Página visible del PDF.
    Page { current: u32, total: u32 },
    /// Estado de compilación.
    Compile(CompileState),
    /// Estado del asistente IA.
    Ai(AiState),
}

/// Handle clonable para publicar mensajes de estado.
#[derive(Clone)]
pub struct StatusHandle {
    tx: Sender<StatusMessage>,
}

impl std::fmt::Debug for StatusHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusHandle").finish_non_exhaustive()
    }
}

impl StatusHandle {
    /// Envía un mensaje; `false` si el receptor ya no existe (app cerrándose).
    pub fn send(&self, message: StatusMessage) -> bool {
        self.tx.try_send(message).is_ok()
    }
}

/// Crea el par handle/receptor que conecta paneles con el shell.
pub fn channel() -> (StatusHandle, Receiver<StatusMessage>) {
    let (tx, rx) = unbounded();
    (StatusHandle { tx }, rx)
}

/// Modelo puro del estado que pinta la status bar.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusModel {
    /// Proyecto activo.
    pub project: Option<String>,
    /// Etiqueta del motor ("TeX Live").
    pub engine_label: String,
    /// Si el binario del motor existe.
    pub engine_available: bool,
    /// Fila del cursor (1-based).
    pub line: u32,
    /// Columna del cursor (1-based).
    pub column: u32,
    /// Palabras del documento activo.
    pub words: u32,
    /// Página visible y total del PDF (`0` = desconocido).
    pub page_current: u32,
    /// Páginas totales del PDF (`0` = desconocido).
    pub page_total: u32,
    /// Estado de compilación.
    pub compile: CompileState,
    /// Estado del asistente IA.
    pub ai: AiState,
}

impl Default for StatusModel {
    fn default() -> Self {
        Self {
            project: None,
            engine_label: String::new(),
            engine_available: false,
            line: 1,
            column: 1,
            words: 0,
            page_current: 0,
            page_total: 0,
            compile: CompileState::Idle,
            ai: AiState::Stopped,
        }
    }
}

impl StatusModel {
    /// Reduce un mensaje; devuelve `true` si el modelo cambió (para `notify`).
    pub fn apply(&mut self, message: &StatusMessage) -> bool {
        let mut changed = true;
        match message {
            StatusMessage::Project { name } => {
                if self.project == *name {
                    changed = false;
                } else {
                    self.project = name.clone();
                }
            }
            StatusMessage::Engine { label, available } => {
                if self.engine_label == *label && self.engine_available == *available {
                    changed = false;
                } else {
                    self.engine_label = label.clone();
                    self.engine_available = *available;
                }
            }
            StatusMessage::Cursor { line, column } => {
                let line = (*line).max(1);
                let column = (*column).max(1);
                if self.line == line && self.column == column {
                    changed = false;
                } else {
                    self.line = line;
                    self.column = column;
                }
            }
            StatusMessage::Words { count } => {
                if self.words == *count {
                    changed = false;
                } else {
                    self.words = *count;
                }
            }
            StatusMessage::Page { current, total } => {
                if self.page_current == *current && self.page_total == *total {
                    changed = false;
                } else {
                    self.page_current = *current;
                    self.page_total = *total;
                }
            }
            StatusMessage::Compile(state) => {
                if self.compile == *state {
                    changed = false;
                } else {
                    self.compile = state.clone();
                }
            }
            StatusMessage::Ai(state) => {
                if self.ai == *state {
                    changed = false;
                } else {
                    self.ai = *state;
                }
            }
        }
        changed
    }

    /// Página visible como texto i18n (`Pág. –/–` si se desconoce).
    pub fn page_text(&self) -> String {
        if self.page_total == 0 {
            rust_i18n::t!("ui.app.status.page_unknown").to_string()
        } else {
            rust_i18n::t!(
                "ui.app.status.page",
                current = self.page_current.max(1),
                total = self.page_total
            )
            .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applying_a_message_updates_only_its_field() {
        let mut model = StatusModel::default();
        assert!(model.apply(&StatusMessage::Words { count: 120 }));
        assert_eq!(model.words, 120);
        assert_eq!(model.line, 1);
        assert!(model.apply(&StatusMessage::Cursor {
            line: 42,
            column: 7
        }));
        assert_eq!((model.line, model.column), (42, 7));
        assert_eq!(model.words, 120);
    }

    #[test]
    fn repeated_values_do_not_report_a_change() {
        let mut model = StatusModel::default();
        assert!(model.apply(&StatusMessage::Ai(AiState::Ready)));
        assert!(!model.apply(&StatusMessage::Ai(AiState::Ready)));
        assert!(model.apply(&StatusMessage::Ai(AiState::Stopped)));
    }

    #[test]
    fn cursor_values_are_clamped_to_one_based_positive() {
        let mut model = StatusModel::default();
        assert!(model.apply(&StatusMessage::Cursor { line: 9, column: 9 }));
        assert!(model.apply(&StatusMessage::Cursor { line: 0, column: 0 }));
        assert_eq!((model.line, model.column), (1, 1));
    }

    #[test]
    fn compile_states_reduce_and_report_busy() {
        let mut model = StatusModel::default();
        assert!(!model.compile.is_busy());
        assert!(model.apply(&StatusMessage::Compile(CompileState::Queued)));
        assert!(model.compile.is_busy());
        assert!(model.apply(&StatusMessage::Compile(CompileState::Running {
            hint: Some("% pdflatex".into())
        })));
        assert!(model.compile.is_busy());
        assert!(model.apply(&StatusMessage::Compile(CompileState::Success {
            duration_ms: 3380
        })));
        assert!(!model.compile.is_busy());
    }

    #[test]
    fn page_text_falls_back_when_totals_are_unknown() {
        let model = StatusModel::default();
        assert!(!model.page_text().is_empty());
        let model = StatusModel {
            page_current: 3,
            page_total: 10,
            ..StatusModel::default()
        };
        let text = model.page_text();
        assert!(text.contains('3') && text.contains("10"), "texto: {text}");
    }

    #[test]
    fn the_channel_delivers_messages_in_order() {
        let (handle, rx) = channel();
        handle.send(StatusMessage::Words { count: 1 });
        handle.send(StatusMessage::Words { count: 2 });
        assert_eq!(rx.try_recv(), Ok(StatusMessage::Words { count: 1 }));
        assert_eq!(rx.try_recv(), Ok(StatusMessage::Words { count: 2 }));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn sending_after_the_receiver_is_dropped_is_false_not_a_panic() {
        let (handle, rx) = channel();
        drop(rx);
        assert!(!handle.send(StatusMessage::Ai(AiState::Ready)));
    }
}
