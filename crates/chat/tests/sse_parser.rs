//! Tests del parser SSE (M0-S4): formato WHATWG + lo que realmente emite
//! opencode 1.18.30 (`data: {json}\n\n`, sin `event:`/`id:`).

use lsx_chat::sse::{SseEvent, SseParser};

fn event(data: &str) -> SseEvent {
    SseEvent {
        event: None,
        id: None,
        data: data.to_owned(),
    }
}

#[test]
fn parses_single_data_line() {
    let mut parser = SseParser::new();
    let events = parser.push("data: {\"type\":\"server.connected\"}\n\n");
    assert_eq!(events, vec![event("{\"type\":\"server.connected\"}")]);
}

#[test]
fn concatenates_multiline_data_with_newline() {
    let mut parser = SseParser::new();
    let events = parser.push("data: primera\ndata: segunda\ndata: tercera\n\n");
    assert_eq!(events, vec![event("primera\nsegunda\ntercera")]);
}

#[test]
fn handles_crlf_and_bare_cr() {
    let mut parser = SseParser::new();
    assert_eq!(parser.push("data: a\r\n\r\n"), vec![event("a")]);
    assert_eq!(parser.push("data: b\r\r"), vec![event("b")]);
}

#[test]
fn survives_chunk_boundaries_mid_line_and_mid_crlf() {
    let mut parser = SseParser::new();
    assert!(parser.push("da").is_empty());
    assert!(parser.push("ta: ho").is_empty());
    assert!(parser.push("la\n").is_empty());
    // El "\r" de apertura del bloque queda en el chunk anterior... probamos
    // también el corte entre \r y \n.
    let events = parser.push("\r");
    assert_eq!(events, vec![event("hola")]);
    assert!(parser.push("\n").is_empty());
}

#[test]
fn ignores_comments_and_dispatches_the_rest() {
    let mut parser = SseParser::new();
    let events = parser.push(": keep-alive\n\ndata: x\n\n");
    assert_eq!(events, vec![event("x")]);
}

#[test]
fn captures_event_and_id_fields() {
    let mut parser = SseParser::new();
    let events = parser.push("id: 42\nevent: custom\ndata: z\n\n");
    assert_eq!(
        events,
        vec![SseEvent {
            event: Some("custom".to_owned()),
            id: Some("42".to_owned()),
            data: "z".to_owned(),
        }]
    );
    assert_eq!(parser.last_event_id(), Some("42"));
}

#[test]
fn unknown_fields_are_ignored_and_finish_flushes_pending_data() {
    let mut parser = SseParser::new();
    let events = parser.push("foo: bar\ndata: pendiente");
    assert!(events.is_empty());
    assert_eq!(parser.finish(), vec![event("pendiente")]);
}

#[test]
fn empty_data_field_dispatches_empty_event() {
    let mut parser = SseParser::new();
    assert_eq!(parser.push("data:\n\n"), vec![event("")]);
}

#[test]
fn blank_lines_without_data_do_not_dispatch() {
    let mut parser = SseParser::new();
    assert!(parser.push("\n\n\n").is_empty());
    assert!(parser.finish().is_empty());
}
