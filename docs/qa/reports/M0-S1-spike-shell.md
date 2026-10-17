# Spike M0-S1 — Shell / dock de 4 paneles

- Fecha: 2026-10-17 · Sesión: `ses_eb6356f6fffeSeXbfr5qfSoo72`
- **ESTADO: OK** (demo funcional + persistencia round-trip + 15 tests verdes + smoke 3/3 sin huérfanos)

## Entregable

- `crates/ui/src/spike/{mod,layout,panel,shell}.rs` (**nuevos**) — tema dark + acento `#4C8DFF`, `SpikeLayout` serde con validación, `SpikePanel` (colapso vía `visible`), `SpikeShell` (title bar + dock + status bar + guardar/restaurar)
- `app/src/bin/spike-shell.rs` — demo `cargo run -p latex-studio --bin spike-shell`
- `crates/ui/Cargo.toml` (+`serde`, `serde_json`, `thiserror`)

## Demo

Ventana maximizada "LaTeX Studio — spike S1": title bar con toggles de hoja + Guardar/Restaurar; dock `[ÁRBOL | EDITOR | PDF | CHAT]` resizable (hojas fijas con `set_locked(true)`); status bar. Persistencia en `%TEMP%\lsx-spike-s1-layout.json` (override `LSX_SPIKE_LAYOUT`); JSON inválido → defaults sin error.

Evidencia: dump en vivo `window 1280x840 (scale 2)`, `sizes [140.8, 640, 307.2, 192]` para fracciones `[0.11, 0.5, 0.24, 0.15]`; capture devuelve **exactamente** las mismas fracciones. Smoke 3/3 (default / JSON válido / JSON corrupto), 0 huérfanos.

## I-7 — resuelto: el Dock del kit SÍ cubre el IDE (v1)

- **Hojas/splits**: `DockSkin::dock_area(id, version, window, cx) → (Entity<DockArea>, Rc<DockSkin>)`; `DockLayout::h_split()/v_split()/tabs()`; `.child(layout, Option<Pixels>)`, `.panel()`, `.panel_view(handle, cx)`, `.active_index(ix)`; instalar con `DockArea::set_center(...)`.
- **Rename**: `Panel::title()`/`tab_name()` son elementos/estado (renombrar = cambiar lo que los alimenta); `panel_handle<P>(Entity<P>)` obligatorio para títulos.
- **Colapso por hoja**: el split central no tiene colapso nativo → vía soportada `Panel::visible(false)` (la hoja conserva su lugar, el contenedor cede el slot; **los cambios deben notificar también al DockArea** — el panel guarda un `WeakEntity<DockArea>`; documentado en `panel.rs`). Dock regions (Left/Right/Bottom) sí tienen `toggle_dock`.
- **Serialización** (2 capas): (a) `DockArea::dump/load` (`DockAreaState` serde + `register_panel(panel_name)`); (b) esquema estable §7 `{id, kind, width_fraction, collapsed}` + `focus{hidden, previous_fractions}` con `set_split_sizes` y lectura de px resueltos de `dump`. **Persistir con debounce de `DockEvent::LayoutChanged`** (M1).

## Otras APIs

```rust
Theme::change(ThemeMode::Dark, None, cx);
Theme::update(cx, |t| t.accent = Hsla::from(rgb(0x004C8DFF)));   // rgb() es fn, no const; usar Theme::update
// tokens por ActiveTheme: cx.theme().background/foreground/border/muted_foreground/status_bar…
TitleBar::new().child(..); TitleBar::window_options(); TitleBar::title_bar_options(); // controles los dibuja Windows
StatusBar::new().left(..).center(..).right(..);
WindowBounds::Maximized(Bounds::default())   // gpui-pre 0.3.9: Maximized lleva Bounds, no Option
```

## Riesgos / TODO

1. **QA visual con DPI 200 %**: capturas DPI-unaware (PrintWindow/CopyFromScreen desde pwsh) recortan a un cuadrante físico y parecen 2×; **no es bug de la UI** (mediciones lógicas correctas: container 1280, sizes [100,1180]). Para gates usar captura DPI-aware o maximizada → anotar en el playbook de QA.
2. `Panel::visible` requiere notificar al DockArea (ver arriba) — M1-A1 debe encapsularlo.
3. UI-integration tests `#[gpui_kit::test]` exigen feature `test-support` de gpui-kit (rebuild grande) — decidir en M1 si se activa en CI.
4. M1: mover `SpikeLayout` → `config/settings.json→ui.dock` + debounce + focus mode real.
