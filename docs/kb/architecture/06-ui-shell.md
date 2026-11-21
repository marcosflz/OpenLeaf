---
id: kb/architecture/ui-shell
title: Shell de cuatro hojas, paneles y estado visual
status: stable
updated: 2026-11-15
source:
  - crates/ui/src/app.rs
  - crates/ui/src/layout.rs
  - crates/ui/src/toolbar.rs
  - crates/ui/src/status_bar.rs
  - crates/ui/src/theme.rs
  - crates/ui/src/settings_ui.rs
  - crates/ui/src/quick_open.rs
  - crates/ui/src/panels/editor.rs
  - crates/ui/src/panels/pdf.rs
  - crates/ui/src/panels/tree.rs
tags: [arquitectura, ui, gpui]
related: [kb/product/promesas-ux, kb/quality/01-auditoria-2026-10-24]
verified: true
---

# Shell de cuatro hojas, paneles y estado visual

## Estructura

Estética VS Code «Modern» desde 0.3: una sola barra superior y superficies neutras.

```
TitleBar (35 px): menús · [buscador: proyecto ▾] · Compilar · toggles de panel · sync · focus · ajustes · controles del SO
DockArea (split horizontal de 4 hojas; títulos en mayúsculas de 11 px)
  ├─ Tree   (cabecera = nombre del proyecto + árbol + dock inferior PROBLEMAS · SALIDA · SÍMBOLOS)
  ├─ Editor (pestañas + find/replace + diagnósticos; sin línea de estado propia)
  ├─ Pdf    (toolbar de 35 px: página, zoom, ancho/página; métricas solo con LSX_PDF_DEBUG=1)
  └─ Chat   (modo · permisos · modelo · sesiones + conversación + composer)
StatusBar (compilación · proyecto · Ln/Col · palabras · página · motor · IA)
```

- La barra superior sustituye a las tres filas de 0.2: título, menú y toolbar.
- El buscador central abre la paleta. Su desplegable lleva abrir carpeta, nuevo proyecto y
  recientes.
- El motor TeX se muestra en la barra de estado; el bloque es accionable y abre el
  diálogo del motor.
- `advanced.texBinOverride` y `advanced.pythonOverride` se aplican al resolver las rutas.

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

- `theme::apply` sigue este orden: modo `dark`/`light`/`system` → `apply_palette` → `apply_accent`.
- `apply_palette` fija las superficies VS Code «Dark Modern» / «Light Modern»:
  - fondos: editor `#1F1F1F` y chrome `#181818` en oscuro; `#FFFFFF` y `#F8F8F8` en claro;
  - bordes, texto, pestañas, scrollbars y radio de 4 px;
  - colores del editor del kit (`highlight_theme.style`: fondo, línea activa, números).
- Escala en `theme::tokens`: texto 11/12/13/14 y alturas 35 (título, cabecera, pestañas) y
  22 (estado).
- Acento por defecto `#0078D4`.
  - Presets en `theme::ACCENT_PRESETS`, más hex libre validado con `parse_accent`.
  - Ajustes previsualiza en vivo y Cancelar/Escape restaura.
- `ui.theme = "system"` usa `Theme::sync_system_appearance`; `AppRoot` reaplica el tema
  completo si el SO cambia de modo.
- `ui.reducedMotion` se proyecta en `App::set_reduce_motion`.

## Ventana

- `app/src/main.rs` restaura `ui.window` (x, y, width, height, maximized) con mínimo
  960×600; si la posición guardada ya no está en ningún monitor, centra la ventana. El
  primer arranque abre maximizado.
- `AppRoot` observa los cambios de bounds con debounce de 500 ms y guarda la geometría en
  `settings.json`; el cierre también la persiste.
- El título incluye el proyecto y un `•` cuando hay documentos sucios
  (`window.set_window_title` + `set_window_edited`).
- Menús integrados en la barra superior (`AppMenuBar`): **Archivo · Editar · Ver ·
  LaTeX · IA · Ayuda**.
  - Editar usa las acciones del kit (deshacer, portapapeles, buscar), que atiende el editor
    enfocado.
  - Los checks de paneles se reconstruyen al cambiar el layout (`sync_app_menus`).
- Cierre seguro: la X (`on_window_should_close`) y **Ctrl+Q / Salir** (`request_quit`)
  detectan documentos sucios y abren Guardar / Descartar / Cancelar. En 0.2, Ctrl+Q cerraba
  sin avisar.
- Atajos:
  - `Ctrl+B` compila, `Ctrl+S` guarda, `Ctrl+Alt+S` guarda todo, `Ctrl+W` cierra la pestaña;
  - `Ctrl+P` abre un archivo del proyecto y `Ctrl+Shift+P` la paleta;
  - `Ctrl+/` comenta o descomenta líneas con `%`, en la sangría mínima del bloque;
  - `Ctrl+,` ajustes, `Ctrl+Q` sale, `F1` atajos;
  - `Ctrl+Alt+F` arregla errores con IA y `Ctrl+Alt+N` abre una conversación nueva.
- `Ctrl+S` tiene una sola acción (`panels::editor::SaveDocument`): el editor la registra y
  el shell la atiende cuando el foco está fuera del editor. En 0.2 había dos acciones con
  el mismo atajo.
- `LSX_QA_OPEN_SETTINGS=1|ai|api|subscriptions` abre Ajustes (o esa sección) al arrancar
  para capturas deterministas de QA; no se usa en producción.
- **Botones en la barra superior (Windows)**: la barra del `TitleBar` de gpui-kit es una
  zona `WindowControlArea::Drag`. GPUI responde `HTCAPTION` si esa zona está entre los
  hitboxes bajo el cursor **aunque haya un botón encima**; Windows convierte el clic en
  «mover ventana» y el botón no recibe el `click`. Todo grupo interactivo dentro del
  `TitleBar` debe llevar `.occlude()` (`render_title_bar`); los huecos siguen arrastrando.
  En 0.3.0 esto dejaba muertos el engranaje, los toggles de paneles y la barra de menús. Los
  tests con `TestAppContext` no lo detectan (la ventana de test no hace hit test de controles).
- Los menús de una **cabecera** se anclan con `Anchor::TopRight` (abren hacia abajo).
  `BottomRight` los abre hacia arriba sin darles la vuelta: en la cabecera del chat caían
  sobre la toolbar.

## Ajustes y atajos

- `Ctrl+,` o el engranaje abren `settings_ui::open_dialog`: un `Dialog` de 880 px con
  navegación lateral **Apariencia · Editor · Compilación · PDF · IA · Avanzado**,
  borrador validado, estado sucio y Guardar/Cancelar. Escape cierra y restaura la
  previsualización de tema/acento.
- `AppRoot::apply_settings` reaplica tema/acento, autosave y modo/zoom del PDF en
  caliente; `latex.*` se propaga al `CompileController`.
- La sección IA gestiona proveedores, claves (`config/secrets/ai.json`) y la prueba de modelo
  (ver [`04-chat-agente`](04-chat-agente.md)).
- La sección Editor expone fuente, tamaño (6–72), tabulación (1–16), ajuste de línea y
  autoguardado; `apply_settings` los aplica en caliente (`EditorPanel::set_preferences`).
- Paleta (`palette.rs`, `Ctrl+Shift+P`): 22 comandos con su atajo en la etiqueta. El
  diálogo F1 se genera de la misma tabla (`PaletteCommand::shortcut`), así que no divergen.
- Abrir archivo (`quick_open.rs`, `Ctrl+P`): recorre el proyecto (sin carpetas ocultas,
  `node_modules`, `target` ni auxiliares de LaTeX; tope de 5000 archivos) y abre texto en
  el editor y PDF en el visor. Sin proyecto abierto cae en la paleta.

## Editor, visor y árbol (0.3)

- **Archivos**: el árbol y `Ctrl+P` abren cualquier texto de `tree::TEXT_EXTENSIONS`
  (`.tex`, `.sty`, `.cls`, `.bib`, `.txt`, `.md`, `latexmkrc`…). La familia TeX se resalta
  como LaTeX; el resto como texto plano (`editor::language_for`).
- **Pestañas**: icono por tipo y barra de acento arriba en la activa. El `absolute` de
  taffy se ancla al padre directo, así que la barra vive en un prefijo de 32 px de alto y
  la recorta el `overflow_hidden` de la pestaña.
- **Breadcrumb**: `carpeta › archivo › sección`. La sección (`\chapter` … `\paragraph`,
  sin las comentadas) se recalcula solo cuando cambia la línea del cursor
  (`editor::section_at`).
- **Bienvenida**: sin documentos, el editor muestra Empezar (nuevo proyecto, abrir carpeta,
  abrir archivo, configurar IA), hasta 6 recientes vivos (`ProjectService::recent`) y los
  atajos clave.
- **Menús contextuales**:
  - Editor: menú nativo del SO con cortar/copiar/pegar, comentar, ir al PDF, preguntar a la
    IA sobre la selección (adjunta la selección como chip y enfoca el composer, sin enviar)
    y arreglar errores con IA.
  - Árbol: abrir, establecer como archivo principal (`.tex`; persiste
    `latex.mainFile` y reorienta la compilación), copiar ruta absoluta o relativa, mostrar
    en el Explorador, renombrar y eliminar.
  - PDF: zoom, ajustar al ancho o a la página, página oscura y visor externo.
  - Problemas: ir al código, copiar `archivo:línea: mensaje` y arreglar con IA.
- **Visor PDF**: la toolbar flota centrada abajo y aparece al pasar el ratón; los avisos
  van en el flash de la esquina y las métricas solo con `LSX_PDF_DEBUG=1`.
  - `pdf.darkPages` invierte la luminosidad HSL conservando el tono, y el blanco del
    papel pasa a `#1F1F1F`.
  - Los renders hechos antes de cambiar el modo se descartan.
- **Árbol**: las acciones de la cabecera flotan sobre el nombre y solo se ven al pasar el
  ratón. El dock inferior abierto es redimensionable (`v_resizable` del kit, estado en
  `TreePanel::split`).
- **Textos**: menús, editor, bienvenida, visor, problemas, dock, símbolos y diálogos leen de
  `ui.app.text.*` en `locales/es.yml`. Quedan fijos los textos que devuelven
  `&'static str`: categorías de símbolos, presets de permisos del chat, mensajes de
  SyncTeX y etiquetas del motor.

## Puntos de extensión

- Para añadir un panel: `SlotKind`, `layout.rs`, `AppRoot::panel`, `Pane` impl, `DockLayout`.
- Para añadir un comando: `PaletteCommand` + `run_palette_command` + i18n + atajo.
- Para añadir un ajuste: `lsx-core::config` + validación + UI + aplicación real + test.
