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
//! Estado: M1-A0 — `root`, `config` y `events` implementados y testeados;
//! `project`/`errors` siguen en el backlog del stream F.

pub mod config;
pub mod events;
pub mod fs;
pub mod root;
