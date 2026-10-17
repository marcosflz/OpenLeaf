//! Parser SSE propio (INTERFACES §5.4): split por líneas, campos
//! `data:`/`event:`/`id:`/`retry:`, `data:` multilínea concatenado con `\n`,
//! comentarios `:` ignorados, finales `\n`, `\r\n` y `\r`.
//!
//! Verificado contra opencode 1.18.30: el server emite `data: {json}\n\n`
//! y NADA más (sin `event:`/`id:`); aun así el parser soporta el formato
//! completo WHATWG por si v2 los usa.

/// Un evento SSE ya delimitado (bloque terminado en línea vacía).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// Campo `event:` (opencode v1 no lo envía).
    pub event: Option<String>,
    /// Campo `id:` (opencode v1 no lo envía → no hay `Last-Event-ID`).
    pub id: Option<String>,
    /// Concatena todas las líneas `data:` con `\n`.
    pub data: String,
}

/// Parser incremental: alimenta chunks de red, devuelve eventos completos.
#[derive(Debug, Default)]
pub struct SseParser {
    line: String,
    data: String,
    data_lines: usize,
    event: Option<String>,
    last_id: Option<String>,
    last_retry_ms: Option<u64>,
    pending_cr: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Último `id:` visto (contractual si v2 lo emite; v1 nunca).
    pub fn last_event_id(&self) -> Option<&str> {
        self.last_id.as_deref()
    }

    /// Alimenta un chunk (puede cortar a mitad de línea) y devuelve los
    /// eventos completos que se hayan cerrado con `\n\n`.
    pub fn push(&mut self, chunk: &str) -> Vec<SseEvent> {
        let mut out = Vec::new();
        for ch in chunk.chars() {
            match ch {
                '\r' => {
                    self.end_line(&mut out);
                    self.pending_cr = true;
                }
                '\n' => {
                    if !self.pending_cr {
                        self.end_line(&mut out);
                    }
                    self.pending_cr = false;
                }
                _ => {
                    self.pending_cr = false;
                    self.line.push(ch);
                }
            }
        }
        out
    }

    /// Fin de stream: procesa la línea parcial final y despacha el evento
    /// pendiente (si había `data:`).
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        self.end_line(&mut out);
        self.dispatch(&mut out);
        out
    }

    fn end_line(&mut self, out: &mut Vec<SseEvent>) {
        let line = std::mem::take(&mut self.line);
        if line.is_empty() {
            self.dispatch(out);
            return;
        }
        if line.starts_with(':') {
            // Comentario / heartbeat de keep-alive: ignorar.
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line.as_str(), ""),
        };
        match field {
            "data" => {
                if self.data_lines > 0 {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.data_lines += 1;
            }
            "event" => self.event = Some(value.to_owned()),
            "id" => self.last_id = Some(value.to_owned()),
            "retry" => self.last_retry_ms = value.parse().ok(),
            // Campos desconocidos: ignorar (forward-compat).
            _ => {}
        }
    }

    fn dispatch(&mut self, out: &mut Vec<SseEvent>) {
        if self.data_lines == 0 {
            // Línea vacía sin data pendiente: no hay evento.
            self.event = None;
            return;
        }
        out.push(SseEvent {
            event: self.event.take(),
            id: self.last_id.clone(),
            data: std::mem::take(&mut self.data),
        });
        self.data_lines = 0;
    }
}
