//! Modal de permiso de la IA (M1-A4) — API reutilizable por el stream E.
//!
//! El chat (M1-E4) recibe `permission.asked` del servidor de opencode, lo
//! traduce a [`PermissionRequest`] y llama a [`PermissionCard::show`]; el
//! resultado llega por el canal que devuelve la propia función:
//!
//! ```ignore
//! let rx = PermissionCard::show(window, cx, request);
//! cx.spawn(async move |this, cx| {
//!     if let Ok(decision) = rx.recv().await { /* responder al server */ }
//! });
//! ```
//!
//! El flujo de datos (id del permiso → respuesta) vive fuera de este módulo:
//! aquí solo está la presentación (título, detalle, diff resumido y 3
//! botones) y el canal de resultado.

use async_channel::{Receiver, Sender, unbounded};
use gpui_kit::component::{
    ActiveTheme as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogDescription, DialogFooter},
    h_flex, v_flex,
};
use gpui_kit::{
    App, FontWeight, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div, px,
};

/// Respuesta del usuario a una petición de permiso.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Permitir solo esta vez.
    Once,
    /// Permitir siempre (el servidor recuerda el patrón).
    Always,
    /// Rechazar.
    Reject,
}

impl PermissionDecision {
    /// Etiqueta i18n del resultado.
    pub fn label(self) -> String {
        match self {
            Self::Once => rust_i18n::t!("ui.app.permission.decision_once"),
            Self::Always => rust_i18n::t!("ui.app.permission.decision_always"),
            Self::Reject => rust_i18n::t!("ui.app.permission.decision_reject"),
        }
        .to_string()
    }
}

/// Línea de un diff resumido.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffLine {
    /// Línea añadida.
    Added(String),
    /// Línea eliminada.
    Removed(String),
    /// Línea de contexto.
    Context(String),
}

/// Resumen de la edición propuesta (formato reducido del diff de opencode).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DiffSummary {
    /// Ruta relativa del archivo.
    pub path: String,
    /// Líneas añadidas.
    pub additions: u32,
    /// Líneas eliminadas.
    pub deletions: u32,
    /// Líneas mostradas (cap [`DiffSummary::MAX_LINES`]).
    pub lines: Vec<DiffLine>,
    /// Si el diff original tenía más líneas de las mostradas.
    pub truncated: bool,
}

impl DiffSummary {
    /// Máximo de líneas que muestra la tarjeta.
    pub const MAX_LINES: usize = 18;

    /// Crea un resumen vacío para `path`.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            ..Self::default()
        }
    }

    /// Parsea un patch unificado (`@@ …` + líneas `+`/`-`/` `).
    ///
    /// Tolerante: lo que no reconoce se ignora (los patches de opencode usan
    /// este formato estándar). Las cabeceras `---`/`+++` de fichero no cuentan
    /// como líneas del diff.
    pub fn parse_unified(path: impl Into<String>, patch: &str) -> Self {
        let mut summary = Self::new(path);
        for line in patch.lines() {
            if line.starts_with("@@") || line.starts_with("---") || line.starts_with("+++") {
                continue;
            }
            if let Some(rest) = line.strip_prefix('+') {
                summary.additions += 1;
                summary.push_line(DiffLine::Added(rest.to_owned()));
            } else if let Some(rest) = line.strip_prefix('-') {
                summary.deletions += 1;
                summary.push_line(DiffLine::Removed(rest.to_owned()));
            } else if let Some(rest) = line.strip_prefix(' ') {
                summary.push_line(DiffLine::Context(rest.to_owned()));
            }
            // "\ No newline at end of file" también se ignora.
        }
        summary
    }

    fn push_line(&mut self, line: DiffLine) {
        if self.lines.len() < Self::MAX_LINES {
            self.lines.push(line);
        } else {
            self.truncated = true;
        }
    }

    /// Resumen textual (`+N −M`).
    pub fn counts_label(&self) -> String {
        format!("+{} −{}", self.additions, self.deletions)
    }
}

/// Petición de permiso lista para pintar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionRequest {
    /// Identificador del permiso (`per_…` en opencode); viaja de vuelta en la
    /// respuesta del servidor, aquí solo se registra para trazabilidad.
    pub id: String,
    /// Título de la tarjeta (herramienta/acción).
    pub title: String,
    /// Detalle en una o dos frases.
    pub detail: String,
    /// Diff resumido, si la acción edita un archivo.
    pub diff: Option<DiffSummary>,
}

impl PermissionRequest {
    /// Petición de demostración (botón demo del shell).
    pub fn demo() -> Self {
        let patch = "\
@@ -12,6 +12,9 @@
 \\section{Experiencia}
+\\cventry{2024--2026}{Ingeniero}{Acme}{Madrid}{}{Desarrollo de producto}
+\\cventry{2022--2024}{Analista}{Otro}{Remoto}{}{Datos}
 \\section{Educación}
";
        Self {
            id: "per_demo".to_owned(),
            title: rust_i18n::t!("ui.app.permission.demo_title").to_string(),
            detail: rust_i18n::t!("ui.app.permission.demo_detail").to_string(),
            diff: Some(DiffSummary::parse_unified("template.tex", patch)),
        }
    }
}

/// Tarjeta modal de permiso; el resultado viaja por canal.
pub struct PermissionCard;

impl PermissionCard {
    /// Abre el modal y devuelve el receptor con la decisión.
    ///
    /// El canal es ilimitado y se cierra cuando se pierde el receptor; el
    /// llamante debería quedarse con él (p. ej. en un `cx.spawn`).
    pub fn show(
        window: &mut Window,
        cx: &mut App,
        request: PermissionRequest,
    ) -> Receiver<PermissionDecision> {
        let (tx, rx) = unbounded::<PermissionDecision>();
        Self::show_with_sender(window, cx, request, tx);
        rx
    }

    /// Variante que publica la decisión en un emisor ya existente (útil para
    /// el chat, que ya tiene un canal por petición).
    pub fn show_with_sender(
        window: &mut Window,
        cx: &mut App,
        request: PermissionRequest,
        sender: Sender<PermissionDecision>,
    ) {
        let title = request.title.clone();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let detail = request.detail.clone();
            let diff = request.diff.clone();
            let permission_id = request.id.clone();

            let mut dialog = dialog.title(title.clone()).width(px(600.)).child(
                DialogDescription::new()
                    .child(detail.clone())
                    .child(div().text_xs().child(permission_id)),
            );

            if let Some(diff) = &diff {
                dialog = dialog.child(render_diff(diff, cx));
            }

            let decision_button = |id: &'static str,
                                   label: String,
                                   decision: PermissionDecision,
                                   danger: bool,
                                   outline: bool|
             -> Button {
                let sender = sender.clone();
                let mut button = Button::new(id).label(label);
                button = if danger {
                    button.danger()
                } else if outline {
                    button.outline()
                } else {
                    button.primary()
                };
                button.on_click(move |_, window, cx| {
                    let _ = sender.try_send(decision);
                    window.close_dialog(cx);
                })
            };

            dialog.footer(
                DialogFooter::new()
                    .child(decision_button(
                        "permission-reject",
                        rust_i18n::t!("ui.app.permission.reject").to_string(),
                        PermissionDecision::Reject,
                        true,
                        false,
                    ))
                    .child(decision_button(
                        "permission-always",
                        rust_i18n::t!("ui.app.permission.always").to_string(),
                        PermissionDecision::Always,
                        false,
                        true,
                    ))
                    .child(decision_button(
                        "permission-once",
                        rust_i18n::t!("ui.app.permission.once").to_string(),
                        PermissionDecision::Once,
                        false,
                        false,
                    )),
            )
        });
    }
}

/// Tarjeta de diff resumido: cabecera con ruta y contadores + líneas mono.
fn render_diff(diff: &DiffSummary, cx: &App) -> impl IntoElement {
    let mono = cx.theme().mono_font_family.clone();
    let rows = diff.lines.iter().map(|line| {
        let (prefix, text, color) = match line {
            DiffLine::Added(text) => ("+", text.as_str(), cx.theme().success),
            DiffLine::Removed(text) => ("−", text.as_str(), cx.theme().danger),
            DiffLine::Context(text) => (" ", text.as_str(), cx.theme().muted_foreground),
        };
        h_flex()
            .gap(px(6.))
            .font_family(mono.clone())
            .text_size(px(11.5))
            .text_color(color)
            .whitespace_nowrap()
            .child(prefix)
            .child(div().min_w_0().truncate().child(text.to_owned()))
    });

    v_flex()
        .gap_1()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.blend(cx.theme().border.opacity(0.4)))
        .p_2()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(diff.path.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(diff.counts_label()),
                )
                .child(div().flex_1())
                .children(diff.truncated.then(|| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("…")
                })),
        )
        .children(rows)
}

/// `SharedString` helper para consumidores que ya tienen un `String`.
pub fn shared(text: impl Into<SharedString>) -> SharedString {
    text.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unified_patch_is_summarized_with_counts_and_lines() {
        let patch = "\
--- a/template.tex
+++ b/template.tex
@@ -1,3 +1,4 @@
 \\section{A}
-viejo
+primera
+segunda
 contexto
";
        let summary = DiffSummary::parse_unified("template.tex", patch);
        assert_eq!(summary.path, "template.tex");
        assert_eq!(summary.additions, 2);
        assert_eq!(summary.deletions, 1);
        assert_eq!(summary.lines.len(), 5);
        assert!(matches!(summary.lines[0], DiffLine::Context(_)));
        assert!(matches!(summary.lines[1], DiffLine::Removed(_)));
        assert!(matches!(summary.lines[2], DiffLine::Added(_)));
        assert!(matches!(summary.lines[3], DiffLine::Added(_)));
        assert_eq!(summary.counts_label(), "+2 −1");
        assert!(!summary.truncated);
    }

    #[test]
    fn a_huge_patch_is_truncated_at_the_pixel_budget() {
        let mut patch = String::from("@@ -1 +1 @@\n");
        for index in 0..100 {
            patch.push_str(&format!("+linea {index}\n"));
        }
        let summary = DiffSummary::parse_unified("grande.tex", &patch);
        assert_eq!(summary.lines.len(), DiffSummary::MAX_LINES);
        assert_eq!(summary.additions, 100);
        assert!(summary.truncated);
    }

    #[test]
    fn unrecognized_patch_noise_is_ignored() {
        let summary = DiffSummary::parse_unified("x.tex", "\\ No newline at end of file");
        assert_eq!(summary.additions, 0);
        assert_eq!(summary.deletions, 0);
        assert!(summary.lines.is_empty());
    }

    #[test]
    fn the_demo_request_points_at_a_tex_file_with_a_diff() {
        let request = PermissionRequest::demo();
        let diff = request.diff.expect("demo con diff");
        assert_eq!(diff.path, "template.tex");
        assert!(diff.additions >= 1);
        assert!(!request.title.is_empty());
    }

    #[test]
    fn decisions_have_spanish_labels() {
        for decision in [
            PermissionDecision::Once,
            PermissionDecision::Always,
            PermissionDecision::Reject,
        ] {
            assert!(!decision.label().is_empty());
        }
    }

    #[test]
    fn the_channel_carries_the_decision() {
        let (tx, rx) = unbounded();
        tx.try_send(PermissionDecision::Always).expect("envío");
        assert_eq!(rx.try_recv(), Ok(PermissionDecision::Always));
        drop(tx);
    }
}
