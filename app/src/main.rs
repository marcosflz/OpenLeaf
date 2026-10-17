#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! LaTeX Studio — binario principal.
//!
//! Wiring de M1 (stream A): detecta la raíz portable, carga
//! `config/settings.json`, prepara tema/idioma y abre la ventana con
//! [`lsx_ui::app::AppRoot`] (dock de 4 paneles, toolbar, status bar).
//!
//! Si la raíz portable no se encuentra, se muestra una ventana de error con
//! la instrucción para fijar `LATESTUDIO_ROOT` (los chequeos completos de
//! primer arranque son de M3-3).

use gpui_kit::component::{ActiveTheme as _, StyledExt as _, TitleBar, alert::Alert};
use gpui_kit::*;
use lsx_core::config::Settings;
use lsx_core::root::PortableRoot;
use lsx_ui::app::AppRoot;

fn main() {
    gpui_kit::application()
        // El catálogo completo de iconos Lucide (árbol, toolbar, estados).
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            lsx_ui::init(cx);

            let portable = match PortableRoot::detect() {
                Ok(portable) => portable,
                Err(err) => {
                    open_error_window(cx, err.to_string());
                    return;
                }
            };
            let (settings, warnings) = Settings::load(&portable);
            lsx_ui::theme::apply(&settings, cx);

            let options = WindowOptions {
                // Maximizada: con escala de pantalla >100 % un tamaño lógico
                // fijo puede exceder el área visible.
                window_bounds: Some(WindowBounds::Maximized(Bounds::default())),
                window_min_size: Some(size(px(960.), px(600.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("LaTeX Studio".into()),
                    ..TitleBar::title_bar_options()
                }),
                ..TitleBar::window_options()
            };
            let _ = gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| {
                    AppRoot::new(
                        portable.clone(),
                        settings.clone(),
                        warnings.clone(),
                        window,
                        cx,
                    )
                })
            })
            .expect("no se pudo abrir la ventana principal");
        });
}

/// Ventana de error cuando no hay raíz portable (antes de M3-3).
fn open_error_window(cx: &mut App, message: String) {
    struct RootErrorView {
        message: SharedString,
        hint: SharedString,
    }

    impl Render for RootErrorView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .v_flex()
                .size_full()
                .p_6()
                .gap_3()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::MEDIUM)
                        .child(lsx_ui::app::root_error_title()),
                )
                .child(Alert::error("root-error", self.message.clone()))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.hint.clone()),
                )
        }
    }

    let hint = lsx_ui::app::root_error_hint();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(80.), px(80.)),
            size(px(680.), px(240.)),
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("LaTeX Studio".into()),
            ..TitleBar::title_bar_options()
        }),
        ..TitleBar::window_options()
    };
    let _ = gpui_kit::open_window(options, cx, move |_, cx| {
        cx.new(move |_| RootErrorView {
            message: message.into(),
            hint: hint.into(),
        })
    });
}
