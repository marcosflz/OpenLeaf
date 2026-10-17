//! Paneles montables por el shell (stream A los integra en el dock).
//!
//! - [`editor`]: panel del editor (M1-B1..B5) — pestañas multi-documento,
//!   guardado atómico vía `FsService`, conflictos del watcher, diagnósticos
//!   inline, buscar/reemplazar, autosave y recuperación.
//! - [`pdf`]: panel del visor PDF (M1-C1..C4) — zoom/virtualización, caché LRU
//!   con render progresivo, recarga por `generation` y SyncTeX forward/inverse.
//! - [`chat`]: panel de chat IA (M1-E3..E6) — conversación tipada sobre
//!   `lsx-chat`, tool cards, permisos, pickers y «Arreglar errores con IA».
//! - [`tree`]: explorador de archivos de producción (M1-A1/F2 seed).
//! - [`placeholder`]: hojas aún sin stream propio.
//! - [`pane`]: contrato común ([`Pane`], [`PaneState`], [`PaneEvent`]).
//!
//! Integración en el dock: [`tree::TreePanel`] y [`placeholder::PlaceholderPanel`]
//! los monta `crate::app::AppRoot`; los paneles de editor/PDF/chat exponen su API
//! de comandos con callbacks y el shell los cablea por slots (`EditorSlot`,
//! `PdfSlot`, `ChatSlot`).

pub mod chat;
pub mod editor;
pub mod pane;
pub mod pdf;
pub mod placeholder;
pub mod problems;
pub mod tree;

pub use chat::{ChatPanel, ChatPanelConfig};
pub(crate) use pane::pane_container;
pub use pane::{Pane, PaneEvent, PaneState};
pub use placeholder::PlaceholderPanel;
pub use tree::TreePanel;
