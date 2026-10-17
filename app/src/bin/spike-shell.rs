#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Binario del spike M0-S1: ventana con dock de 4 hojas, title bar y status bar.
//!
//! Uso: `cargo run -p latex-studio --bin spike-shell`
//!
//! Persistencia de demo: el layout se guarda/restaura en
//! `%TEMP%\lsx-spike-s1-layout.json` (o `LSX_SPIKE_LAYOUT`). El guardado real a
//! `config/settings.json` es de M1.

use gpui_kit::component::TitleBar;
use gpui_kit::{AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use lsx_ui::spike::SpikeShell;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            lsx_ui::spike::init(cx);

            let options = WindowOptions {
                // Maximizada: con escala de pantalla >100% un tamaño lógico
                // fijo puede exceder el área visible.
                window_bounds: Some(WindowBounds::Maximized(Bounds::default())),
                window_min_size: Some(size(px(960.), px(600.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("LaTeX Studio — spike S1".into()),
                    ..TitleBar::title_bar_options()
                }),
                ..TitleBar::window_options()
            };
            let _ = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| SpikeShell::new(window, cx))
            })
            .expect("no se pudo abrir la ventana del spike");
        });
}
