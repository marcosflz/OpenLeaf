//! Ajustes UI mínima (M1-A5): editor acotado de `config/settings.json`.
//!
//! Campos editables v1: `autoCompile`, `debounceMs`, `zoom` del PDF, `theme`
//! y `logLevel` — exactamente el alcance de la tarea. Todo lo demás se
//! preserva intacto (el `Settings` completo viaja al shell, que lo valida de
//! nuevo con `Settings::validate` y lo guarda de forma atómica).
//!
//! La validación de los campos de texto es local a este módulo (`u32`/`f32`
//! en rango); si falla, el diálogo no se cierra y muestra el motivo.

use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    radio::{Radio, RadioGroup},
    switch::Switch,
    text::Text,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div, px,
};
use lsx_core::config::Settings;

use crate::app::AppRoot;

/// Rango de debounce aceptado en la UI (ms).
const DEBOUNCE_RANGE: (u32, u32) = (0, 10_000);
/// Rango de zoom aceptado en la UI.
const ZOOM_RANGE: (f32, f32) = (0.1, 8.0);

/// Temas ofrecidos (índice ↔ valor).
const THEMES: [&str; 2] = ["dark", "light"];
/// Niveles de log ofrecidos (índice ↔ valor).
const LOG_LEVELS: [&str; 5] = ["trace", "debug", "info", "warn", "error"];

/// Formulario de ajustes (contenido del diálogo).
pub struct SettingsForm {
    settings: Settings,
    root: gpui_kit::WeakEntity<AppRoot>,
    debounce: Entity<InputState>,
    zoom: Entity<InputState>,
    theme_index: usize,
    log_index: usize,
    error: Option<SharedString>,
}

impl SettingsForm {
    /// Crea el formulario con una copia de los ajustes actuales.
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        settings: Settings,
        root: gpui_kit::WeakEntity<AppRoot>,
    ) -> Self {
        let debounce_value = settings.latex.debounce_ms.to_string();
        let zoom_value = format!("{}", settings.pdf.zoom);
        let debounce = cx.new(|cx| InputState::new(window, cx).default_value(debounce_value));
        let zoom = cx.new(|cx| InputState::new(window, cx).default_value(zoom_value));
        let theme_index = THEMES
            .iter()
            .position(|theme| *theme == settings.ui.theme)
            .unwrap_or(0);
        let log_index = LOG_LEVELS
            .iter()
            .position(|level| *level == settings.advanced.log_level)
            .unwrap_or(2);
        Self {
            settings,
            root,
            debounce,
            zoom,
            theme_index,
            log_index,
            error: None,
        }
    }

    /// Valida y aplica; `true` si el diálogo puede cerrarse.
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let debounce_text = self.debounce.read(cx).value().trim().to_string();
        let zoom_text = self.zoom.read(cx).value().trim().to_string();

        let debounce = match parse_u32_in_range(&debounce_text, DEBOUNCE_RANGE) {
            Ok(value) => value,
            Err(message) => {
                self.error = Some(message.into());
                cx.notify();
                return false;
            }
        };
        let zoom = match parse_f32_in_range(&zoom_text, ZOOM_RANGE) {
            Ok(value) => value,
            Err(message) => {
                self.error = Some(message.into());
                cx.notify();
                return false;
            }
        };

        self.settings.latex.debounce_ms = debounce;
        self.settings.pdf.zoom = zoom;
        self.settings.ui.theme = THEMES[self.theme_index].to_owned();
        self.settings.advanced.log_level = LOG_LEVELS[self.log_index].to_owned();

        let settings = self.settings.clone();
        self.root
            .update(cx, |root, cx| root.apply_settings(settings, window, cx))
            .is_ok()
    }
}

/// Abre el diálogo de ajustes.
pub fn open_dialog(
    window: &mut Window,
    cx: &mut App,
    settings: Settings,
    root: gpui_kit::WeakEntity<AppRoot>,
) {
    let form = cx.new(|cx| SettingsForm::new(window, cx, settings, root));
    window.open_dialog(cx, move |dialog, _window, _cx| {
        dialog
            .title(rust_i18n::t!("ui.app.settings.title").to_string())
            .width(px(560.))
            .child(form.clone())
    });
}

/// Fila etiqueta + control.
fn row(label: impl Into<Text>, control: impl IntoElement, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .py_1()
        .items_center()
        .justify_between()
        .gap_4()
        .child(div().text_sm().child(label.into()))
        .child(control)
        .text_color(cx.theme().foreground)
}

/// Parsea un entero sin signo dentro de rango, con mensajes i18n.
fn parse_u32_in_range(value: &str, (min, max): (u32, u32)) -> Result<u32, String> {
    let parsed = value
        .parse::<u32>()
        .map_err(|_| rust_i18n::t!("ui.app.settings.error_number", value = value).to_string())?;
    if parsed < min || parsed > max {
        return Err(rust_i18n::t!(
            "ui.app.settings.error_range",
            value = value,
            min = min,
            max = max
        )
        .to_string());
    }
    Ok(parsed)
}

/// Parsea un flotante dentro de rango, con mensajes i18n.
fn parse_f32_in_range(value: &str, (min, max): (f32, f32)) -> Result<f32, String> {
    let parsed = value
        .parse::<f32>()
        .map_err(|_| rust_i18n::t!("ui.app.settings.error_number", value = value).to_string())?;
    if !parsed.is_finite() || parsed < min || parsed > max {
        return Err(rust_i18n::t!(
            "ui.app.settings.error_range",
            value = value,
            min = min,
            max = max
        )
        .to_string());
    }
    Ok(parsed)
}

impl Render for SettingsForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let auto_compile = self.settings.latex.auto_compile;
        let theme_labels = [
            rust_i18n::t!("ui.app.settings.theme_dark").to_string(),
            rust_i18n::t!("ui.app.settings.theme_light").to_string(),
        ];

        v_flex()
            .gap_2()
            .p_1()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("ui.app.settings.subtitle").to_string()),
            )
            .child(row(
                rust_i18n::t!("ui.app.settings.auto_compile").to_string(),
                Switch::new("settings-auto-compile")
                    .checked(auto_compile)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        this.settings.latex.auto_compile = *checked;
                        cx.notify();
                    })),
                cx,
            ))
            .child(row(
                rust_i18n::t!("ui.app.settings.debounce").to_string(),
                div().w(px(120.)).child(Input::new(&self.debounce).small()),
                cx,
            ))
            .child(row(
                rust_i18n::t!("ui.app.settings.zoom").to_string(),
                div().w(px(120.)).child(Input::new(&self.zoom).small()),
                cx,
            ))
            .child(row(
                rust_i18n::t!("ui.app.settings.theme").to_string(),
                RadioGroup::horizontal("settings-theme")
                    .selected_index(Some(self.theme_index))
                    .children([
                        Radio::new("settings-theme-dark").label(theme_labels[0].clone()),
                        Radio::new("settings-theme-light").label(theme_labels[1].clone()),
                    ])
                    .on_click(cx.listener(|this, index, _window, cx| {
                        this.theme_index = *index;
                        cx.notify();
                    })),
                cx,
            ))
            .child(row(
                rust_i18n::t!("ui.app.settings.log_level").to_string(),
                RadioGroup::horizontal("settings-log-level")
                    .selected_index(Some(self.log_index))
                    .children(LOG_LEVELS.iter().map(|level| {
                        Radio::new(SharedString::from(format!("settings-log-{level}")))
                            .label(level.to_string())
                    }))
                    .on_click(cx.listener(|this, index, _window, cx| {
                        this.log_index = *index;
                        cx.notify();
                    })),
                cx,
            ))
            .children(self.error.as_ref().map(|error| {
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error.clone())
            }))
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .pt_2()
                    .child(
                        Button::new("settings-cancel")
                            .outline()
                            .label(rust_i18n::t!("ui.app.settings.cancel").to_string())
                            .on_click(cx.listener(|_this, _, window, cx| {
                                window.close_dialog(cx);
                            })),
                    )
                    .child(
                        Button::new("settings-save")
                            .primary()
                            .label(rust_i18n::t!("ui.app.settings.save").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.commit(window, cx) {
                                    window.close_dialog(cx);
                                }
                            })),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debounce_parser_accepts_only_ints_in_range() {
        assert_eq!(parse_u32_in_range("400", DEBOUNCE_RANGE), Ok(400));
        assert_eq!(parse_u32_in_range("0", DEBOUNCE_RANGE), Ok(0));
        assert!(parse_u32_in_range("99999", DEBOUNCE_RANGE).is_err());
        assert!(parse_u32_in_range("abc", DEBOUNCE_RANGE).is_err());
        assert!(parse_u32_in_range("-5", DEBOUNCE_RANGE).is_err());
    }

    #[test]
    fn the_zoom_parser_accepts_only_finite_floats_in_range() {
        assert_eq!(parse_f32_in_range("1.5", ZOOM_RANGE), Ok(1.5));
        assert!(parse_f32_in_range("9.9", ZOOM_RANGE).is_err());
        assert!(parse_f32_in_range("nan", ZOOM_RANGE).is_err());
        assert!(parse_f32_in_range("", ZOOM_RANGE).is_err());
    }

    #[test]
    fn the_index_tables_cover_the_settings_values() {
        assert_eq!(THEMES.len(), 2);
        assert_eq!(LOG_LEVELS.len(), 5);
        assert!(THEMES.contains(&"dark"));
        assert!(LOG_LEVELS.contains(&"info"));
    }
}
