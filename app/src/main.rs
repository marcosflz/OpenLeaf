#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! LaTeX Studio — binario principal.
//!
//! P0 (este estado): ventana mínima sobre `gpui-kit` para validar el stack.
//! M1+: wiring de servicios (`lsx-core/latex/pdf/chat`) + dock de 4 paneles.

use gpui_kit::component::*;
use gpui_kit::*;

struct LaTeXStudio;

impl Render for LaTeXStudio {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child("LaTeX Studio")
            .child(Button::new("p0").label("P0 · esqueleto OK"))
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| LaTeXStudio))
                .expect("no se pudo abrir la ventana principal");
        });
}
