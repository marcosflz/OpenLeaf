---
id: kb/architecture/ui-shell
title: Shell de cuatro hojas, paneles y estado visual
status: stable
updated: 2026-10-24
source:
  - crates/ui/src/app.rs
  - crates/ui/src/layout.rs
  - crates/ui/src/toolbar.rs
  - crates/ui/src/status_bar.rs
  - crates/ui/src/theme.rs
  - crates/ui/src/settings_ui.rs
tags: [arquitectura, ui, gpui]
related: [kb/product/promesas-ux, kb/quality/01-auditoria-2026-10-24]
verified: true
---

# Shell de cuatro hojas, paneles y estado visual

## Estructura

```
TitleBar (controles del SO + proyecto)
Toolbar  (proyecto · compilar · motor · toggles · sync · focus · ajustes · palette)
DockArea (split horizontal de 4 hojas)
  ├─ Tree   (árbol + dock inferior PROBLEMAS | SALIDA | SÍMBOLOS)
  ├─ Editor (pestañas + find/replace + diagnósticos)
  ├─ Pdf    (render hayro + zoom + SyncTeX)
  └─ Chat   (modelo/agente/permisos + conversación + composer)
StatusBar (compilación · proyecto · Ln/Col · palabras · página · motor · IA)
```

El indicador de motor de la toolbar es un botón: abre un diálogo con versión, SyncTeX,
biber, ruta y acciones **Reintentar / Abrir carpeta / Abrir Ajustes**. Al arrancar se hace
un `probe` real en background (`TexLiveEngine::probe`) y la etiqueta pasa de
“comprobando…” a `TeX Live 4.88` (o al error concreto). `advanced.texBinOverride` y
`advanced.opencodeExeOverride` se aplican al resolver las rutas. El bloque de motor y el
dot de IA de la status bar también son accionables (abren el diálogo del motor y Ajustes →
IA).

## Layout persistido

`ShellLayout` (en `layout.rs`) es Rust puro y serializa en `ui.dock`:

```json
{ "version": 1,
  "panels": [
    { "id": "tree", "kind": "tree", "width_fraction": 0.18, "collapsed": false },
    { "id": "editor", "kind": "editor", "width_fraction": 0.46, "collapsed": false },
    { "id": "pdf", "kind": "pdf", "width_fraction": 0.21, "collapsed": false },
    { "id": "chat", "kind": "chat", "width_fraction": 0.15, "collapsed": false }
  ] }
```

- `version` no puede ser mayor que `SCHEMA_VERSION`.
- Fracciones finitas ≥ 0; las ausentes/cero vuelven al default.
- `focus` guarda hojas ocultas y anchos previos; **no** se persiste al reiniciar (se pierde).
- El dock se guarda con debounce cuando cambia el layout.

## Tema

- `theme::apply` parte de `dark`/`light`/`system` y aplica `ui.accent` a `primary`,
  `button_primary`, `ring`, `selection`, `list_active` y `sidebar_accent`.
- `ui.theme = "system"` usa `Theme::sync_system_appearance(None, cx)`, que consulta
  `cx.window_appearance()`. `AppRoot` observa `observe_window_appearance` y reaplica
  `theme::apply` si el SO cambia de claro a oscuro con la app abierta.
- Acento: presets en `theme::ACCENT_PRESETS` (azul, violeta, verde, ámbar, rojo) + hex
  validado con `parse_accent`. Ajustes previsualiza en vivo y Cancelar/Escape restaura.
- Tema claro: los alphas de `selection`/`list_active`/hover bajan respecto a dark para no
  saturar fondos claros. Los paneles siguen leyendo tokens, sin hex.
- `ui.reducedMotion` se proyecta en `App::set_reduce_motion` (diálogos/spinners del kit);
  no se expone en Ajustes porque no hay animaciones propias en `lsx-ui`.

## Ventana

- `app/src/main.rs` restaura `ui.window` (x, y, width, height, maximized) con mínimo
  960×600; si la posición guardada ya no está en ningún monitor, centra la ventana. El
  primer arranque abre maximizado.
- `AppRoot` observa los cambios de bounds con debounce de 500 ms y guarda la geometría en
  `settings.json`; el cierre también la persiste.
- El título incluye el proyecto y un `•` cuando hay documentos sucios
  (`window.set_window_title` + `set_window_edited`).
- Barra de menús de escritorio (`AppMenuBar`): **Archivo · Ver · Herramientas · Ayuda**;
  los checks de paneles se reconstruyen al cambiar el layout (`sync_app_menus`).
- Cierre seguro: `Window::on_window_should_close` detecta documentos sucios, abre
  Guardar / Descartar / Cancelar y solo cierra con `force_close` tras la decisión.
- Atajos: `Ctrl+B` compila, `Ctrl+S` guarda, `Ctrl+,` ajustes, `Ctrl+Q` sale, `F1` atajos.
- `LSX_QA_OPEN_SETTINGS=1|ai` abre Ajustes (o su sección IA) al arrancar para capturas
  deterministas de QA; no se usa en producción.

## Ajustes y atajos

- `Ctrl+,` o el engranaje abren `settings_ui::open_dialog`: un `Dialog` de 880 px con
  navegación lateral **Apariencia · Editor · Compilación · PDF · IA · Avanzado**,
  borrador validado, estado sucio y Guardar/Cancelar. Escape cierra y restaura la
  previsualización de tema/acento.
- `AppRoot::apply_settings` reaplica tema/acento, autosave y modo/zoom del PDF en
  caliente; `latex.*` se propaga al `CompileController`.
- La sección IA localiza `PortableRoot::detect()`/`agent/opencode.exe` y ofrece
  `auth list`, `models`, `auth login` en consola, “Probar”, abrir `agent/` y abrir
  `agent/opencode.json`. Los comandos corren en `background_spawn` con timeout
  (ver [`04-chat-opencode`](04-chat-opencode.md)).
- `Ctrl+Shift+P` abre la palette; `Ctrl+Alt+1..4` toggles de panel; `Ctrl+Alt+J` SyncTeX
  forward; `Ctrl+S` guarda; `Ctrl+F`/`Ctrl+H` buscar/reemplazar.
- La palette tiene 10 comandos fijos (se retiró la demo de permisos); no incluye tema, IA
  ni ventana, que viven en Ajustes y en el menú.

## Puntos de extensión

- Para añadir un panel: `SlotKind`, `layout.rs`, `AppRoot::panel`, `Pane` impl, `DockLayout`.
- Para añadir un comando: `PaletteCommand` + `run_palette_command` + i18n + atajo.
- Para añadir un ajuste: `lsx-core::config` + validación + UI + aplicación real + test.
