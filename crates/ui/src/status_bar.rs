//! Status bar del shell (M1-A2).
//!
//! Renderiza un [`StatusModel`] puro con la barra del kit. Cada bloque es un
//! botón fantasma sin acción (aspecto de texto) con tooltip y etiqueta
//! accesible: todos los estados son visibles y explicables sin abrir nada.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    status_bar::StatusBar,
};
use gpui_kit::{App, IntoElement, ParentElement as _, Styled as _, div, px};
use rust_i18n::t;

use crate::status::{AiState, CompileState, StatusModel};

/// Color del dot de IA según su estado.
fn ai_color(state: AiState, cx: &App) -> gpui_kit::Hsla {
    match state {
        AiState::Stopped => cx.theme().muted_foreground,
        AiState::Starting => cx.theme().warning,
        AiState::Ready => cx.theme().success,
        AiState::Error => cx.theme().danger,
    }
}

/// Texto + icono del estado de compilación.
fn compile_text(state: &CompileState) -> String {
    match state {
        CompileState::Idle => t!("ui.app.status.compile_ready").to_string(),
        CompileState::Queued => t!("ui.app.status.compile_queued").to_string(),
        CompileState::Running { .. } => t!("ui.app.status.compile_running").to_string(),
        CompileState::Success { duration_ms } => {
            t!("ui.app.status.compile_ok", ms = duration_ms).to_string()
        }
        CompileState::Failed { .. } => t!("ui.app.status.compile_failed").to_string(),
    }
}

/// Icono + color del estado de compilación.
fn compile_mark(state: &CompileState, cx: &App) -> (IconName, gpui_kit::Hsla) {
    match state {
        CompileState::Idle | CompileState::Queued => {
            (IconName::CircleDot, cx.theme().muted_foreground)
        }
        CompileState::Running { .. } => (IconName::Loader, cx.theme().accent),
        CompileState::Success { .. } => (IconName::CircleCheck, cx.theme().success),
        CompileState::Failed { .. } => (IconName::TriangleAlert, cx.theme().danger),
    }
}

/// Bloque con icono y tooltip (botón fantasma sin acción).
fn marked(
    id: &'static str,
    icon: IconName,
    color: gpui_kit::Hsla,
    text: String,
    tooltip: String,
) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .tab_stop(false)
        .rounded(px(3.))
        .icon(Icon::new(icon).size(px(12.)).text_color(color))
        .label(text)
        .tooltip(tooltip.clone())
        .accessibility_label(tooltip)
        .text_size(px(11.))
}

/// Bloque solo-texto con tooltip.
fn plain(id: &'static str, text: String, tooltip: String) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .tab_stop(false)
        .rounded(px(3.))
        .label(text)
        .tooltip(tooltip.clone())
        .accessibility_label(tooltip)
        .text_size(px(11.))
}

/// Bloque del dot de IA.
fn ai_block(model: &StatusModel, cx: &App) -> Button {
    let label = match model.ai {
        AiState::Stopped => t!("ui.app.status.ai_stopped"),
        AiState::Starting => t!("ui.app.status.ai_starting"),
        AiState::Ready => t!("ui.app.status.ai_ready"),
        AiState::Error => t!("ui.app.status.ai_error"),
    }
    .to_string();
    let tooltip = t!("ui.app.status.ai_tooltip").to_string();
    Button::new("status-ai")
        .ghost()
        .xsmall()
        .tab_stop(false)
        .rounded(px(3.))
        .child(
            div()
                .size(px(8.))
                .rounded_full()
                .bg(ai_color(model.ai, cx))
                .flex_none(),
        )
        .label(label)
        .tooltip(tooltip.clone())
        .accessibility_label(tooltip)
        .text_size(px(11.))
}

/// Pinta la status bar completa.
pub fn render(model: &StatusModel, cx: &App) -> impl IntoElement {
    let project = model
        .project
        .clone()
        .unwrap_or_else(|| t!("ui.app.status.no_project").to_string());
    let (compile_icon, compile_color) = compile_mark(&model.compile, cx);

    let left = div()
        .flex()
        .items_center()
        .gap_2()
        .child(marked(
            "status-compile",
            compile_icon,
            compile_color,
            compile_text(&model.compile),
            t!("ui.app.status.compile_idle_tooltip").to_string(),
        ))
        .child(plain(
            "status-project",
            project,
            t!("ui.app.status.project_tooltip").to_string(),
        ));

    let engine_text = if model.engine_label.is_empty() {
        t!("ui.app.engine.missing").to_string()
    } else {
        model.engine_label.clone()
    };

    let right = div()
        .flex()
        .items_center()
        .gap_2()
        .child(plain(
            "status-cursor",
            t!(
                "ui.app.status.ln_col",
                line = model.line,
                column = model.column
            )
            .to_string(),
            t!("ui.app.status.cursor_tooltip").to_string(),
        ))
        .child(plain(
            "status-words",
            t!("ui.app.status.words", count = model.words).to_string(),
            t!("ui.app.status.words_tooltip").to_string(),
        ))
        .child(plain(
            "status-page",
            model.page_text(),
            t!("ui.app.status.page_tooltip").to_string(),
        ))
        .child(marked(
            "status-engine",
            if model.engine_available {
                IconName::CircleCheck
            } else {
                IconName::TriangleAlert
            },
            if model.engine_available {
                cx.theme().success
            } else {
                cx.theme().warning
            },
            engine_text,
            t!("ui.app.status.engine_tooltip").to_string(),
        ))
        .child(ai_block(model, cx));

    StatusBar::new().left(left).right(right)
}
