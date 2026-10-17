//! `lsx-core` — núcleo de dominio de LaTeX Studio.
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §2):
//! - raíz portable (`PortableRoot`)
//! - configuración (`config/settings.json`)
//! - proyectos (abrir/recientes/plantillas)
//! - servicio de archivos con watcher (list/read/write atómico/CRUD)
//! - bus de eventos (`AppEvent`) y errores tipados
//!
//! Regla dura: este crate NO conoce UI, tokio, reqwest ni gpui.
//!
//! Estado: P0 — stub. Se implementa en M1 (stream F) según INTERFACES.md §2.
