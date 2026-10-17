//! Notificaciones del shell (M1-A4) — envoltorios finos del kit.
//!
//! Todas las llamadas pasan por aquí para que el tono y el título sean
//! uniformes y para no repetir imports de `WindowExt`/`Notification`.

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::{App, SharedString, Window};

/// Título traducido de la variante (`Hecho`, `Aviso`, …).
fn title(kind: NotifyKind) -> String {
    match kind {
        NotifyKind::Info => rust_i18n::t!("ui.app.notify.info"),
        NotifyKind::Success => rust_i18n::t!("ui.app.notify.success"),
        NotifyKind::Warning => rust_i18n::t!("ui.app.notify.warning"),
        NotifyKind::Error => rust_i18n::t!("ui.app.notify.error"),
    }
    .to_string()
}

/// Variante de la notificación (para el título).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotifyKind {
    Info,
    Success,
    Warning,
    Error,
}

fn push(kind: NotifyKind, message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    let note = match kind {
        NotifyKind::Info => Notification::info(message),
        NotifyKind::Success => Notification::success(message),
        NotifyKind::Warning => Notification::warning(message),
        NotifyKind::Error => Notification::error(message),
    };
    window.push_notification(note.title(title(kind)), cx);
}

/// Notificación informativa.
pub fn info(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    push(NotifyKind::Info, message, window, cx);
}

/// Notificación de éxito.
pub fn success(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    push(NotifyKind::Success, message, window, cx);
}

/// Notificación de aviso.
pub fn warning(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    push(NotifyKind::Warning, message, window, cx);
}

/// Notificación de error.
pub fn error(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    push(NotifyKind::Error, message, window, cx);
}
