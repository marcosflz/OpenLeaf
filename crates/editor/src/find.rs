//! Buscar/reemplazar (M1-B4) sobre el motor de búsqueda del editor del kit.
//!
//! El kit trae la maquinaria completa (`SearchSession` + `SearchMatcher`):
//! resaltado de coincidencias, contador `n/total`, navegación circular
//! (wrap-around) y reemplazo silencioso que pasa por la historia de deshacer.
//! Este módulo es el envoltorio de dominio para el panel:
//!
//! - una única puerta de entrada (`set_query`, `next`, `previous`,
//!   `replace_current`, `replace_all`, `close`);
//! - semántica documentada y **probada** (los 20 casos del guion QA viven en
//!   los tests de este archivo);
//! - `case_sensitive` al revés que el kit: aquí se piensa en "sensible a
//!   mayúsculas", el kit recibe `case_insensitive`.
//!
//! La búsqueda es de texto plano (sin regex), como pide el plan. El kit usa
//! `AhoCorasick` con plegado **ASCII** de mayúsculas: `Á` y `á` son distintas,
//! comportamiento aceptado en v1 y cubierto por un test.

use std::ops::Range;

use gpui_kit::component::input::EditorState;
use gpui_kit::{Context, Window};

/// Fija la consulta (y el modo de mayúsculas) y activa el resaltado de
/// coincidencias. Consulta vacía = sin coincidencias, pero la sesión sigue
/// activa hasta `close`.
pub fn set_query(
    state: &mut EditorState,
    query: &str,
    case_sensitive: bool,
    cx: &mut Context<EditorState>,
) {
    state.set_search_query(query, !case_sensitive, cx);
}

/// Termina la búsqueda: apaga resaltado y panel interno del kit.
pub fn close(state: &mut EditorState, cx: &mut Context<EditorState>) {
    state.close_search(cx);
}

/// Cuántas coincidencias hay para la consulta actual.
pub fn match_count(state: &EditorState) -> usize {
    state.search_session().matcher.len()
}

/// Etiqueta del contador, `n/total` (`0/0` sin coincidencias).
pub fn match_label(state: &EditorState) -> String {
    state.search_session().matcher.label()
}

/// Rango de bytes de la coincidencia activa.
pub fn current_range(state: &EditorState) -> Option<Range<usize>> {
    let matcher = &state.search_session().matcher;
    let index = matcher.current_match_index();
    matcher.matched_ranges().get(index).cloned()
}

/// Primera coincidencia (la que se selecciona al abrir la barra).
pub fn first_range(state: &EditorState) -> Option<Range<usize>> {
    state
        .search_session()
        .matcher
        .matched_ranges()
        .first()
        .cloned()
}

/// Avanza a la coincidencia siguiente, con wrap-around al llegar al final.
pub fn next(state: &mut EditorState, cx: &mut Context<EditorState>) -> Option<Range<usize>> {
    state.next_search_match(cx)
}

/// Retrocede a la coincidencia anterior, con wrap-around al principio.
pub fn previous(state: &mut EditorState, cx: &mut Context<EditorState>) -> Option<Range<usize>> {
    state.previous_search_match(cx)
}

/// Reemplaza la coincidencia activa y avanza. `false` si no había ninguna.
pub fn replace_current(
    state: &mut EditorState,
    replacement: &str,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) -> bool {
    state.replace_current_search_match(replacement, window, cx)
}

/// Reemplaza todas las coincidencias; devuelve cuántas reemplazó.
pub fn replace_all(
    state: &mut EditorState,
    replacement: &str,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) -> usize {
    state.replace_all_search_matches(replacement, window, cx)
}

/// Texto seleccionado actualmente (prefill de la barra al abrir).
pub fn selected_text(state: &EditorState) -> Option<String> {
    let range = state.selected_range();
    if range.is_empty() {
        return None;
    }
    Some(state.text().slice(range).to_string())
}

/// Si una búsqueda está activa (resaltando coincidencias).
pub fn is_active(state: &EditorState) -> bool {
    state.search_session().is_active()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::input::EditorState;
    use gpui_kit::{AppContext as _, Entity, Render, TestAppContext, VisualTestContext, Window};

    struct Host {
        state: Entity<EditorState>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui_kit::IntoElement {
            gpui_kit::component::input::Editor::new(&self.state)
        }
    }

    /// Editor en ventana de test con `text` de arranque.
    fn with_editor<R>(
        cx: &mut TestAppContext,
        text: &str,
        f: impl FnOnce(&mut VisualTestContext, Entity<EditorState>) -> R,
    ) -> R {
        cx.update(gpui_kit::init);
        let mut state = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let editor = cx.new(|cx| EditorState::new(window, cx).default_value(text));
            state = Some(editor.clone());
            Host { state: editor }
        });
        let state = state.unwrap();
        VisualTestContext::update(cx, |window, cx| window.draw(cx).clear(cx));
        f(cx, state)
    }

    fn set(cx: &mut VisualTestContext, state: &Entity<EditorState>, query: &str, case: bool) {
        cx.update(|_, app| {
            state.update(app, |st, cx| set_query(st, query, case, cx));
        });
    }

    fn label(cx: &VisualTestContext, state: &Entity<EditorState>) -> String {
        cx.read(|app| match_label(state.read(app)))
    }

    fn count(cx: &VisualTestContext, state: &Entity<EditorState>) -> usize {
        cx.read(|app| match_count(state.read(app)))
    }

    fn text(cx: &VisualTestContext, state: &Entity<EditorState>) -> String {
        cx.read(|app| state.read(app).text().to_string())
    }

    fn next_range(cx: &mut VisualTestContext, state: &Entity<EditorState>) -> Option<Range<usize>> {
        cx.update(|_, app| state.update(app, next))
    }

    fn prev_range(cx: &mut VisualTestContext, state: &Entity<EditorState>) -> Option<Range<usize>> {
        cx.update(|_, app| state.update(app, previous))
    }

    fn replace_one(
        cx: &mut VisualTestContext,
        state: &Entity<EditorState>,
        replacement: &str,
    ) -> bool {
        cx.update(|window, app| {
            state.update(app, |st, cx| replace_current(st, replacement, window, cx))
        })
    }

    fn replace_everything(
        cx: &mut VisualTestContext,
        state: &Entity<EditorState>,
        replacement: &str,
    ) -> usize {
        cx.update(|window, app| {
            state.update(app, |st, cx| replace_all(st, replacement, window, cx))
        })
    }

    // --- casos 1-10: conteo, case y navegación ----------------------------

    #[test]
    fn caso_01_consulta_vacia_no_tiene_coincidencias() {
        with_editor(&mut TestAppContext::single(), "foo foo", |cx, state| {
            set(cx, &state, "", true);
            assert_eq!(count(cx, &state), 0);
            assert_eq!(label(cx, &state), "0/0");
        });
    }

    #[test]
    fn caso_02_cuenta_coincidencias_simples() {
        with_editor(
            &mut TestAppContext::single(),
            "foo bar foo baz foo",
            |cx, state| {
                set(cx, &state, "foo", true);
                assert_eq!(count(cx, &state), 3);
                assert_eq!(label(cx, &state), "1/3");
            },
        );
    }

    #[test]
    fn caso_03_insensible_a_mayusculas_por_defecto_de_la_barra() {
        with_editor(&mut TestAppContext::single(), "foo FOO Foo", |cx, state| {
            set(cx, &state, "foo", false);
            assert_eq!(count(cx, &state), 3);
        });
    }

    #[test]
    fn caso_04_sensible_a_mayusculas_solo_exactas() {
        with_editor(
            &mut TestAppContext::single(),
            "foo FOO Foo foo",
            |cx, state| {
                set(cx, &state, "foo", true);
                assert_eq!(count(cx, &state), 2);
                set(cx, &state, "FOO", true);
                assert_eq!(count(cx, &state), 1);
            },
        );
    }

    #[test]
    fn caso_05_siguiente_avanza_y_finalmente_envuelve() {
        with_editor(
            &mut TestAppContext::single(),
            "uno dos tres",
            |cx, state| {
                set(cx, &state, "o", true);
                // `uno` (2..3) y `dos` (5..6); `tres` no tiene `o`.
                assert_eq!(count(cx, &state), 2);
                assert_eq!(next_range(cx, &state), Some(5..6), "primera avanzada");
                assert_eq!(next_range(cx, &state), Some(2..3), "wrap-around al inicio");
            },
        );
    }

    #[test]
    fn caso_06_anterior_envuelve_al_final() {
        with_editor(
            &mut TestAppContext::single(),
            "uno dos tres",
            |cx, state| {
                set(cx, &state, "o", true);
                assert_eq!(prev_range(cx, &state), Some(5..6), "wrap al último");
                assert_eq!(prev_range(cx, &state), Some(2..3), "hacia atrás");
                assert_eq!(prev_range(cx, &state), Some(5..6), "vuelve a envolver");
            },
        );
    }

    #[test]
    fn caso_07_sin_coincidencias_navegar_devuelve_none() {
        with_editor(&mut TestAppContext::single(), "texto", |cx, state| {
            set(cx, &state, "zzz", true);
            assert_eq!(next_range(cx, &state), None);
            assert_eq!(prev_range(cx, &state), None);
        });
    }

    #[test]
    fn caso_08_coincidencias_dentro_de_palabras_y_no_solapadas() {
        with_editor(&mut TestAppContext::single(), "aaaa", |cx, state| {
            set(cx, &state, "aa", true);
            // AhoCorasick: no solapa → `aa` + `aa`.
            assert_eq!(count(cx, &state), 2);
            let ranges = cx.read(|app| {
                state
                    .read(app)
                    .search_session()
                    .matcher
                    .matched_ranges()
                    .to_vec()
            });
            assert_eq!(ranges, vec![0..2, 2..4]);
        });
    }

    #[test]
    fn caso_09_encuentra_a_traves_de_lineas() {
        with_editor(
            &mut TestAppContext::single(),
            "linea\ncorte\n",
            |cx, state| {
                set(cx, &state, "a\ncor", true);
                assert_eq!(count(cx, &state), 1);
                assert_eq!(next_range(cx, &state), Some(4..9));
            },
        );
    }

    #[test]
    fn caso_10_unicode_exacto_se_encuentra() {
        with_editor(&mut TestAppContext::single(), "café y té", |cx, state| {
            set(cx, &state, "é", false);
            assert_eq!(count(cx, &state), 2, "café y té (ambas con é)");
        });
    }

    // --- casos 11-20: mayúsculas/unicode, reemplazo y sesión ---------------

    #[test]
    fn caso_11_unicode_insensible_es_plegado_ascii() {
        with_editor(&mut TestAppContext::single(), "café CAFÉ", |cx, state| {
            // Plegado ASCII: `É`/`é` no se igualan (documentado, v1).
            set(cx, &state, "café", false);
            assert_eq!(count(cx, &state), 1);
            set(cx, &state, "café", true);
            assert_eq!(count(cx, &state), 1);
        });
    }

    #[test]
    fn caso_12_reemplazar_activa_sustituye_y_mantiene_cuenta() {
        with_editor(&mut TestAppContext::single(), "foo foo foo", |cx, state| {
            set(cx, &state, "foo", true);
            assert!(replace_one(cx, &state, "bar"));
            assert_eq!(text(cx, &state), "bar foo foo");
            assert_eq!(label(cx, &state), "1/2");
        });
    }

    #[test]
    fn caso_13_reemplazar_todas_devuelve_el_conteo() {
        with_editor(&mut TestAppContext::single(), "foo foo foo", |cx, state| {
            set(cx, &state, "foo", false);
            assert_eq!(replace_everything(cx, &state, "bar"), 3);
            assert_eq!(text(cx, &state), "bar bar bar");
            assert_eq!(count(cx, &state), 0);
            assert_eq!(label(cx, &state), "0/0");
        });
    }

    #[test]
    fn caso_14_reemplazar_todas_sin_coincidencias_no_cambia_nada() {
        with_editor(&mut TestAppContext::single(), "texto", |cx, state| {
            set(cx, &state, "zzz", true);
            assert_eq!(replace_everything(cx, &state, "x"), 0);
            assert_eq!(text(cx, &state), "texto");
        });
    }

    #[test]
    fn caso_15_reemplazo_vacio_borra_las_coincidencias() {
        with_editor(&mut TestAppContext::single(), "a-b-c", |cx, state| {
            set(cx, &state, "-", true);
            assert_eq!(replace_everything(cx, &state, ""), 2);
            assert_eq!(text(cx, &state), "abc");
        });
    }

    #[test]
    fn caso_16_reemplazar_sin_consulta_no_hace_nada() {
        with_editor(&mut TestAppContext::single(), "texto", |cx, state| {
            set(cx, &state, "", true);
            assert!(!replace_one(cx, &state, "x"));
            assert_eq!(text(cx, &state), "texto");
        });
    }

    #[test]
    fn caso_17_editar_texto_refresca_las_coincidencias() {
        with_editor(&mut TestAppContext::single(), "foo bar", |cx, state| {
            set(cx, &state, "foo", true);
            assert_eq!(count(cx, &state), 1);
            // El usuario escribe `foo` de nuevo: la búsqueda se re-sincroniza
            // y la activa vuelve al inicio; el primer `next` avanza.
            cx.update(|window, app| {
                state.update(app, |st, cx| {
                    st.set_value("foo foo", window, cx);
                });
            });
            assert_eq!(
                cx.read(|app| first_range(state.read(app))),
                Some(0..3),
                "la primera coincidencia tras re-sincronizar"
            );
            assert_eq!(next_range(cx, &state), Some(4..7), "avanza a la segunda");
            assert_eq!(count(cx, &state), 2);
        });
    }

    #[test]
    fn caso_18_reabrir_con_la_misma_consulta_conserva_la_activa() {
        with_editor(
            &mut TestAppContext::single(),
            "foo bar foo baz foo",
            |cx, state| {
                set(cx, &state, "foo", true);
                let _ = next_range(cx, &state); // 2ª coincidencia activa
                assert_eq!(label(cx, &state), "2/3");
                set(cx, &state, "foo", true); // re-set de la misma consulta
                assert_eq!(label(cx, &state), "2/3", "no reinicia la activa");
            },
        );
    }

    #[test]
    fn caso_19_cerrar_desactiva_la_sesion_de_busqueda() {
        with_editor(&mut TestAppContext::single(), "foo", |cx, state| {
            set(cx, &state, "foo", true);
            assert!(cx.read(|app| is_active(state.read(app))));
            cx.update(|_, app| state.update(app, close));
            assert!(!cx.read(|app| is_active(state.read(app))));
        });
    }

    #[test]
    fn caso_20_seleccion_util_para_prefill_y_rango_activo() {
        with_editor(&mut TestAppContext::single(), "hola mundo", |cx, state| {
            cx.update(|_, app| {
                state.update(app, |st, cx| st.set_selected_range(0..4, cx));
            });
            assert_eq!(
                cx.read(|app| selected_text(state.read(app))),
                Some("hola".to_string())
            );

            set(cx, &state, "mundo", true);
            assert_eq!(cx.read(|app| first_range(state.read(app))), Some(5..10));
        });
    }

    #[test]
    fn caso_21_case_toggle_recalcula_sin_perder_la_consulta() {
        with_editor(&mut TestAppContext::single(), "Uno uno", |cx, state| {
            set(cx, &state, "uno", true);
            assert_eq!(count(cx, &state), 1);
            set(cx, &state, "uno", false);
            assert_eq!(count(cx, &state), 2);
        });
    }

    #[test]
    fn caso_22_reemplazar_activa_avanza_a_la_siguiente() {
        with_editor(&mut TestAppContext::single(), "foo foo", |cx, state| {
            set(cx, &state, "foo", true);
            assert!(replace_one(cx, &state, "bar"));
            assert_eq!(label(cx, &state), "1/1", "quedan 1 tras avanzar");
            // La activa es la única restante (`foo` en 4..7).
            assert_eq!(cx.read(|app| current_range(state.read(app))), Some(4..7));
        });
    }
}
