---
id: kb/changes/plan-0.2
title: Plan incremental 0.2 — de MVP a producto instalable
status: stable
updated: 2026-10-24
source:
  - docs/kb/quality/01-auditoria-2026-10-24.md
  - docs/kb/product/02-promesas-ux.md
  - docs/board.md
tags: [plan, cambios, swarm]
related: [kb/changes/changelog, kb/quality/01-auditoria-2026-10-24]
verified: true
---

# Plan incremental 0.2 — de MVP a producto instalable

> **Objetivo:** que una persona sin TeX pueda instalar, abrir, configurar su IA y compilar
> sin tocar una terminal; y que la app deje de parecer una demo.
>
> **Regla:** cada tarea termina con código + tests + gates + evidencia + nota del OKF +
> entrada en `CHANGELOG.md`. Nada se marca `DONE` sin gates verdes.

## Estado de la ola

| Ola | Contenido | Estado |
|---|---|---|
| O-KB | OKF + enlace con AGENTS + plan | ✅ DONE |
| O-1 | Instalador TeX + Ajustes 2.0 + temas + IA + ventana/menús | ✅ DONE (P2-UI-4 sigue en O-2) |
| O-2 | Primer arranque/bienvenida + pulido toolbar/status + QA visual | ⏳ PLANNED |
| G2.5 | Gate: instalación limpia + E2E + capturas dark/light | ⏳ PLANNED |

## Tareas

### P2-KB-1 — OKF y contrato de mantenimiento

- **Estado:** ✅ DONE (2026-10-24)
- **Territorio:** `docs/kb/**`, `AGENTS.md`, `.opencode/skills/lsx-knowledge-base/**`
- **Entrega:** índice, esquema, notas de producto/arquitectura/operaciones/calidad,
  plantillas, plan y changelog.
- **Aceptación:** `AGENTS.md` obliga a leerlo y actualizarlo; el índice permite recuperar
  los temas de la auditoría; no hay secretos.
- **Evidencia:** este árbol + `git diff --stat`.

### P2-INST-1 — Instalador con motor LaTeX automático

- **Estado:** ✅ DONE (2026-10-24; instalación limpia real verificada)
- **Territorio:** `installer/**`, `docs/release.md`, `docs/kb/architecture/05-instalador.md`
- **Depende de:** nada.
- **Entrega:**
  - Página “Motor LaTeX” en Inno (existente / descargar TinyTeX / omitir).
  - Descarga con progreso y SHA-256; extracción `-y -o{app}` → `{app}\tex`.
  - `tlmgr update --self` + paquetes base (`synctex`, `biber`, `beamer`, `pgf`,
    `titlesec`, `enumitem`, `preprint`, `marvosym`, `roboto`, `fancyhdr`, `tools`,
    `babel-spanish`, `babel-english`) con log/progreso y timeout.
  - Verificación final de `latexmk`, `synctex`, `biber`; fallo no aborta la instalación.
  - `build.ps1` documenta el nuevo comportamiento; CI no cambia de perfil.
- **Aceptación:**
  - `pwsh installer\build.ps1 -SkipBuild` genera el `.exe` sin errores.
  - Instalación silenciosa en carpeta temporal limpia con red deja los 3 binarios.
  - Sin red / URL caída: mensaje claro y la app sigue instalada.
  - El instalador completo `/DBundledTex` sigue funcionando y no descarga nada.
- **Evidencia:** log de instalación, `latexmk -v`, `synctex -v`, `biber --version`, captura
  de la página de progreso (si se puede), informe en `docs/qa/reports/`.

### P2-INST-2 — Recuperación y diagnóstico del motor en la app

- **Estado:** 🟡 PARCIAL (probe, overrides y diálogo hechos; falta bootstrap guiado y banner)
- **Territorio:** `crates/ui/**`, `app/**`, `installer/**`, `xtask/**`
- **Depende de:** P2-INST-1.
- **Entrega:**
  - Al arrancar, probe real (`latexmk -v`, `synctex -v`, `biber --version`) con timeout.
  - Si falta: banner/acción “Instalar motor LaTeX” que ejecuta el bootstrap (script incluido
    en el dist) o explica cómo reinstalar; reintento y “abrir carpeta”.
  - `advanced.texBinOverride` y `opencodeExeOverride` aplicados de verdad (o retirados del
    esquema si no se van a soportar).
  - Status bar/toolbar: motor clickable → Ajustes → Compilación.
- **Aceptación:** en una copia sin `tex/`, la app ofrece la acción y, tras el bootstrap,
  compila sin reiniciar; el estado pasa de warning a ok.
- **Evidencia:** `shots/p2-engine-probe.png` (TeX Live 4.88 real) y
  `shots/p2-engine-dialog.png` (versión/SyncTeX/biber/ruta + Reintentar/Abrir carpeta/
  Ajustes). Tests de `resolve_tex_bin`/`resolve_opencode_exe`. Pendiente: botón que lance el
  bootstrap desde la app y banner persistente.

### P2-UI-1 — Ajustes 2.0

- **Estado:** ✅ DONE (2026-10-24; capturas `shots/p2-settings-*.png`, 106 tests UI)
- **Territorio:** `crates/ui/src/settings_ui.rs` (o `settings_window.rs`), `crates/ui/locales/es.yml`
- **Depende de:** nada (solo API existente de `AppRoot::apply_settings`).
- **Entrega:**
  - Diálogo de 880 px con navegación lateral: **Apariencia · Editor · Compilación ·
    PDF · IA · Avanzado**.
  - Campos: tema, acento, autosave; autoCompile, syncScroll, debounce, timeout,
    extra args, main file, auto-install, clean; zoom/mode/cache PDF; IA (modelo,
    agente, permisos, autoStart, showReasoning, puerto); log level.
  - Validación atómica (`SettingsDraft`) con resumen, errores inline, “Restablecer”
    (conserva recientes y dock), “Abrir carpeta de configuración” y “Abrir
    `opencode.json`”.
  - Estado sucio; Guardar/Cancelar; Escape cierra; preview de tema/acento con
    restauración al cancelar; foco inicial en Acento.
  - Los campos que aún no tienen efecto completo (`pdf.openExternalOnDoubleClick`,
    `advanced.logLevel`) se exponen anotados en la UI y en
    `architecture/03-configuracion.md`; autosave y zoom/modo PDF ya se aplican en
    caliente desde `AppRoot::apply_settings`.
- **Aceptación:** test de datos que aplica el borrador y recarga `settings.json`
  (equivalente a `AppRoot::apply_settings`); capturas dark/light de la superficie.
- **Evidencia:** `cargo test -p lsx-ui` (106 tests en la última ejecución); capturas
  `%TEMP%\lsx-ui-final-captures\settings-{dark,light}.png`;
  `cargo clippy -p lsx-ui --all-targets -- -D warnings` ✅;
  `docs/kb/architecture/03-configuracion.md`.

### P2-UI-2 — Sistema de temas

- **Estado:** ✅ DONE (2026-10-24; capturas dark/light, chat adaptado a tokens)
- **Territorio:** `crates/ui/src/theme.rs`, `settings_ui.rs`, `locales/es.yml`
- **Depende de:** P2-UI-1 (para la UI).
- **Entrega:**
  - `system` real con `Theme::sync_system_appearance` y test GPUI que compara con la
    apariencia de la plataforma.
  - Presets de acento (azul, violeta, verde, ámbar, rojo) + hex validado con
    `parse_accent` + preview en vivo.
  - Tema claro con alphas de selección/hover más suaves; tokens de navegación lateral
    ligados al acento.
  - `reducedMotion` se proyecta en `App::set_reduce_motion`; no se expone porque no hay
    animaciones propias.
- **Aceptación:** cambiar a sistema sigue la apariencia del SO al aplicar; el acento se
  refleja en botones/foco/selección; no se añaden hex a los paneles.
- **Evidencia:** tests de `theme.rs` (incluido `#[gpui_kit::test]`), presets validados.

### P2-AI-1 — Configuración de IA y conexiones opencode

- **Estado:** ✅ DONE (2026-10-24; tests de parser; live pendiente de `agent/opencode.exe` en el repo)
- **Territorio:** `crates/ui/src/ai_setup.rs` (nuevo), `settings_ui.rs`, `locales/es.yml`
- **Depende de:** P2-UI-1.
- **Entrega:**
  - Sección IA con estado del ejecutable, “Conectar proveedor” (`opencode auth login`
    en consola), “Actualizar” (`auth list` + `models`), “Probar” y apertura de
    `agent/` y `agent/opencode.json`.
  - Comandos en `background_spawn` con timeout (10/25/15 s), `OPENCODE_CONFIG` del
    producto y parsers tolerantes a ANSI/viñetas/tablas.
  - Selector de modelo desde `opencode models`; agente editable; modelo/agente se
    guardan en `ai.model`/`ai.agent`.
  - Si falta `agent/opencode.exe`: mensaje accionable + botón para abrir `agent/`.
- **Aceptación:** parsers testeados con salidas representativas; sin binario en el repo
  no se pudo probar el flujo live. No se muestra “Sin catálogo”.
- **Evidencia:** 8 tests de `ai_setup.rs`, `architecture/04-chat-opencode.md`.

### P2-AI-2 — Persistir modelo/agente del picker del chat

- **Estado:** ✅ DONE (2026-10-24; evento + persistencia + fix de `project`/`all` en `validate`)
- **Territorio:** `crates/ui/src/panels/chat.rs`, `crates/ui/src/app.rs`
- **Depende de:** nada.
- **Entrega:** `ChatPanelEvent::SelectionChanged { model, agent }`; el shell actualiza
  `settings.ai` y guarda atómico. Al reiniciar, el chat arranca con el último modelo/agente.
- **Aceptación:** test de UI que cambia el picker y verifica `settings.json`; el prompt usa
  el modelo persistido.
- **Evidencia:** test + entrada de changelog.

### P2-AI-3 — Auth nativa de proveedores (siguiente iteración)

- **Estado:** ⏳ PLANNED (backlog 0.3)
- **Territorio:** `crates/chat/**`, `crates/ui/**`
- **Entrega:** usar `GET /provider`, `GET /provider/auth`, `PUT /auth/:id`,
  `POST /provider/:id/oauth/authorize|callback` para pegar API keys y hacer OAuth sin salir
  de la app.
- **Aceptación:** conectar OpenCode Zen/Go y un provider de API key desde la UI, verificar
  que el modelo aparece en el picker.

### P2-UI-3 — Ventana, menús y cierre seguro

- **Estado:** ✅ DONE (2026-10-24; `ui.window`, `AppMenuBar`, cierre seguro y capturas)
- **Territorio:** `app/src/main.rs`, `crates/ui/src/app.rs`, `crates/ui/src/menu.rs` (nuevo),
  `crates/core/src/config.rs` (estado de ventana)
- **Depende de:** nada.
- **Entrega:**
  - Persistir y restaurar bounds/maximizado (clamp a monitores); mínimo 960×600.
  - Barra de menús: Archivo (Nuevo/Abrir/Recientes/Guardar/Cerrar/Salir), Editar
    (deshacer/rehacer/buscar), Ver (paneles/focus/reset layout), Herramientas (Ajustes/IA/
    palette), Ayuda (atajos/acerca de).
  - Título con proyecto + `•` si hay cambios sin guardar.
  - Cerrar ventana con documentos sucios → diálogo Guardar/Descartar/Cancelar vía
    `window.on_window_should_close`.
  - Diálogo de atajos y “Acerca de”.
- **Aceptación:** test de persistencia de bounds; prueba manual de cierre con dirty; menús
  funcionan con teclado; captura.
- **Evidencia:** tests + captura + nota `architecture/06-ui-shell.md`.

### P2-UI-4 — Primer arranque, bienvenida y retirada de demos

- **Estado:** 🟡 DOING (demo retirada, estado vacío con acciones y status bar accionable; falta bienvenida completa)
- **Territorio:** `crates/ui/src/{app,panels/placeholder,new_project,toolbar,status_bar,palette}.rs`
- **Depende de:** P2-INST-2, P2-UI-1/3.
- **Entrega:**
  - Pantalla de bienvenida sin proyecto: **Nuevo proyecto**, **Abrir carpeta**, recientes,
    “Instalar motor LaTeX” si falta.
  - Quitar “Probar permiso” y copy interna; revisar jerarquía de toolbar/status.
  - Acciones en status bar (motor → ajustes; IA → ajustes IA).
  - Validación inline del wizard de proyecto.
- **Aceptación:** primera apertura sin proyecto muestra la bienvenida; no hay botones demo;
  captura.
- **Evidencia:** test de UI + captura.

### P2-QA-1 — Gate G2.5

- **Estado:** 🟡 PARCIAL (PASS técnico; falta release real y validación manual)
- **Territorio:** `docs/qa/reports/**`, `docs/board.md`
- **Depende de:** P2-INST-1/2, P2-UI-1/2/3/4, P2-AI-1/2.
- **Entrega:** informe con:
  - instalación limpia (sin TeX) y compilación de una plantilla;
  - instalación sin red (degradación);
  - capturas de Ajustes/AI/tema claro/oscuro/ventana;
  - tests de cierre con dirty y persistencia de modelo;
  - gates fmt/clippy/test.
- **Aceptación:** PASS/FAIL explícito y bugs reproducibles con tarea de seguimiento.

## Orden de ejecución del swarm

1. **O-KB** (orchestrator): OKF + AGENTS + plan. ✅
2. **O-1a** (paralelo):
   - `general` → P2-INST-1 (installer only).
   - `stream-ui` → P2-UI-1 + P2-UI-2 + P2-AI-1 + P2-AI-2 (crates/ui only).
   - `orchestrator` → P2-UI-3 (app/core/window/menu) + integración.
3. **O-1b**: integrar, gates, corregir; QA visual.
4. **O-2**: P2-INST-2 + P2-UI-4 + P2-QA-1.

## Riesgos y mitigaciones

| Riesgo | Mitigación |
|---|---|
| El instalador de TinyTeX tarda/falla | opción omitir, timeout, log, reintento desde app |
| `tlmgr` desincronizado | `update --self` antes de instalar; fallback a auto-install de la app |
| Ajustes con campos muertos | solo exponer campos cableados; tests de efecto |
| UI agent y orchestrator tocan `app.rs` | territorio: UI solo `crates/ui`; app/core es del orchestrator |
| Regresión visual | capturas dark/light + checklist UI |
| Build lento | gates por crate durante el desarrollo; workspace al cerrar |

## Definition of Done de la ola

- [ ] P2-INST-1/2, P2-UI-1/2/3, P2-AI-1/2 con código y tests.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace` verdes.
- [ ] Instalador compilado y probado en carpeta limpia.
- [ ] Capturas de Ajustes (dark/light), IA, ventana con menú y bienvenida.
- [ ] `docs/board.md`, `CHANGELOG.md`, notas del OKF y `docs/qa/reports/G2.5-*.md`.
- [ ] Sin secretos, sin `workspace/`, sin `config/settings.json` en git.
