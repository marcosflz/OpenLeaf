//! `lsx-core` — núcleo de dominio de LaTeX Studio.
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §2):
//! - raíz portable ([`root::PortableRoot`])
//! - configuración ([`config::Settings`], `config/settings.json`)
//! - proyectos (abrir/recientes/plantillas) — M1 (stream F)
//! - servicio de archivos con watcher ([`fs`]: list/read/write atómico/CRUD)
//! - bus de eventos ([`events::EventBus`]) y errores tipados
//!
//! Regla dura: este crate NO conoce UI, tokio, reqwest ni gpui.
//!
//! Estado: M1 — `root`, `config`, `events`, `fs` y `project` implementados y
//! testeados; `errors` sigue en el backlog.

pub mod config;
pub mod events;
pub mod fs;
pub mod project;
pub mod root;
