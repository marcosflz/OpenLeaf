# Board — LaTeX Studio

> Actualizado: 2026-10-17 · Orchestrator (sesión "LaTeX Studio · Orchestrator (P0)").
> Fuente de tareas: `docs/plan/TAREAS.md`. Aquí solo el estado vivo.

## P0 — Preflight

| ID | Estado | Evidencia / Nota |
|----|--------|------------------|
| P0.1 Backup + git init | ✅ DONE | Backup 10.8 MB en `..\latex-ide-backup-2026-10-17\` (incluye `chatLanguageModels.json.bak` con la clave BYOK); `git init -b main`; primer commit `c8ccbf9`; `.gitignore` (legacy/tex/workspace/logs/settings fuera de git) |
| P0.2 Mover legacy | ✅ DONE | `legacy/`: app, data, downloads, sandbox, extensions, installer, config-vscode, scripts, Makefile, README viejo, LaTeX.code-workspace. `tex/`, `agent/`, `workspace/` intactos (verificado) |
| P0.3 Esqueleto | ✅ DONE | 8 miembros (`crates/core|latex|pdf|chat|editor|ui` + `app` + `xtask`); `cargo build -p latex-studio` OK (822 crates; pin `gpui-kit =0.7.3` → gpui-pre 0.3.9 + hayro 0.8); **smoke test: ventana viva 8 s, cierre limpio**; `Cargo.lock` commiteado; gates locales fmt/clippy/test verdes |
| P0.4 CI | 🟡 PARCIAL | Workflow (fmt/clippy/nextest/deny) + `deny.toml` commiteados. **Pendiente P0.4b**: remoto en GitHub + primer push (requiere decisión del usuario: cuenta/visibilidad) |
| P0.5 Skills + agentes | ✅ DONE | Skills oficiales vendorizados (`gpui-kit`, `gpui-kit-design-guides`; 29 archivos) + 9 skills `lsx-*` + 9 agentes en `.opencode/agent/`. Verificados como cargables |
| P0.6 Fixtures | 🟡 PARCIAL | `fixtures/projects/{cv, cv-sidebar, broken-undefined, broken-missingpkg}` (anonimizados, sin datos personales), `fixtures/logs` (cv-ok, cv-sidebar-ok, undefined-command, missing-package — errores verificados en el log), `fixtures/pdf`, `fixtures/synctex`. **Pendiente**: `fixtures/opencode/events` (M0-S4), `fixtures/settings` (M1-A5), implementar `xtask fixtures` |
| P0.7 AGENTS.md + ADRs + board + e2e | ✅ DONE | `AGENTS.md`, `docs/decisions/ADR-000-template.md`, `docs/qa/e2e.md` (12 pasos), plan maestro copiado a `docs/plan/` |
| P0.8 Gates vivos | ✅ DONE | `docs/gates.md` (R1–R10, I-1..I-7, registro de gates) |

## Gates

| Gate | Estado | Nota |
|------|--------|------|
| P0 | ✅ CERRADO 2026-10-17 | Informe: `docs/qa/reports/P0-2026-10-17.md`; gates locales verdes (fmt/clippy/test); CI remota pendiente de P0.4b (no bloquea) |

## M0 — Spikes (ola O1 lanzada 2026-10-17; 6 agentes ∥, trabajo sobre main con propiedad estricta de archivos)

| ID | Spike | Estado | Sesión |
|----|-------|--------|--------|
| M0-S1 | shell / dock 4 paneles | ✅ DONE | informe: `docs/qa/reports/M0-S1-spike-shell.md` · **I-7 resuelto** (dock suficiente, colapso vía `Panel::visible`) |
| M0-S2 | editor tree-sitter | ✅ DONE | informe: `docs/qa/reports/M0-S2-spike-editor.md` · I-1/I-3 resueltos · fix: `codebook-tree-sitter-latex 0.6.1` + tree-sitter 0.26.13 |
| M0-S3 | PDF hayro (render + RenderImage) | ✅ DONE | informe: `docs/qa/reports/M0-S3-spike-pdf.md` · I-2 confirmado (visor propio) · p50 4–8 ms release |
| M0-S4 | chat opencode (REST/SSE + fixtures) | ✅ DONE | informe: `docs/qa/reports/M0-S4-spike-chat.md` · **I-4/I-5 congelados** · fixtures SSE + flujo de permiso grabados |
| M0-S5 | engine latexmk + logparse + synctex | ✅ DONE | informe: `docs/qa/reports/M0-S5-spike-latex.md` · round-trip 78↔79 (±1) · auto-install `adjmulticol` real · hallazgo `-g` post-install |

**Consolidación M0 (2026-10-17)**: commits por spike (`7f9059b` pdf, `d99f6bf` core, `7943980` editor, `e446491` chat, `55e0e4a` ui, `4b1b49f` latex); `xtask chat-replay` cableado y deps alineadas (`tree-sitter 0.26` + `codebook-tree-sitter-latex 0.6.1`) en `3430d9f`; **los 6 spikes están DONE**; **build release OK (15m42s) y `.exe` entregados en `dist\m0-demo\`**; gates finales fmt/clippy/test verdes.

## M0.5 — Pulido de diseño (feedback del usuario, 2026-10-17)

| Panel | Objetivo | Sesión | Estado |
|---|---|---|---|
| Árbol / Explorer | Estilo **VS Code Explorer** (filas 22px, 13px, iconos 16px, toolbar compacto icon-only, panel de salida mono) | ses_eb5ee3e54ffecKAx6BqKH8Qqom | ✅ DONE (`shots\tree-after.png`, verificado por orchestrator) |
| Chat | Estilo **GitHub Copilot Chat** (sin burbujas, tool cards finas, composer) + **modo replay determinista** | ses_eb5ee3e52ffej5X95550JhBmV3 | ✅ DONE (`shots\chat-after.png`, verificado por orchestrator) |

- Commits: `b39a9aa` (restyle) · `3904d4e` (fixtures live extra) · gates workspace fmt/clippy/test verdes tras el pulido.
- Herramienta de evidencia: `scripts/capture-window.ps1` (topmost, DPI-aware) — receta en skill `lsx-quality-gates`.

## M1 — Núcleo usable (ola O2 lanzada 2026-10-17; 4 streams ∥)

**Desviación ratificada**: streams sobre `main` con **propiedad estricta de archivos** (sin worktrees) para compartir `target/` y evitar 4 rebuilds completos de GPUI; reglas: nunca git, `rustfmt` solo propios, gates por crate, evidencia visual con captura.

| Stream | Territorio | Tareas | Sesión | Estado |
|---|---|---|---|---|
| **A** shell/UX | `crates/ui/**`, `app/src/main.rs`, `app/src/bin/spike-shell.rs`, `lsx-core::{root,config,events}` (M1-A0) | A0 + A1..A6 + integración árbol real | ses_eb5b77d76ffeaY8fTWI6zM9E8E | ✅ **DONE** — `PortableRoot`+`Settings`+`EventBus<T>`, dock real con persistencia (debounce LayoutChanged), toolbar+status bar con `StatusHandle`, palette, Ajustes UI, notificaciones, `PermissionCard`, i18n es, **app real con árbol de producción** (`crates/ui/src/app.rs`). 36+75 tests, clippy limpio. Evidencia: `shots/m1-a-*.png`. Pendiente integración (orchestrator): montar editor/pdf/problems reales en el dock + wiring de compile/fs (editor+problems en cuanto cierre C) |
| **B** editor | `crates/editor/**`, `crates/ui/src/panels/editor.rs`, `app/src/bin/spike-editor.rs` | B1..B5 | ses_eb5b77d74ffexNMHalgSmSMOgq | ✅ **DONE** — 51 tests; pestañas+dirty, guardado atómico+conflictos (modal 3 opciones), diagnósticos inline (squiggle verificado), find/replace (22 casos), autosave+backups+recuperación. Evidencia: `shots/m1-b*.png`. API del panel en su informe |
| **C** PDF | `crates/pdf/**`, `crates/ui/src/panels/pdf.rs`, `app/src/bin/spike-pdf.rs` | C1..C4 (+ multipage fixture) | ses_eb5b77d73ffeHE0xX4Xi1k4qgJ | DOING |
| **D** latex | `crates/latex/**`, `crates/ui/src/panels/problems.rs` | D1..D5 | ses_eb5b77d71ffejCS2L6SG3WcPiO | ✅ **DONE** — job objects (timeout/cancel con cuelgue real, 0 huérfanos), `CompileController` (single-flight+coalescing+debounce), parser ampliado (Underfull/Info/BibError/correlación `l.NN`), auto-install (3/compilación, `adjmulticol` real), `clean()`, `detect_main`, panel Problemas. 48+7+9 tests verdes |
| **E** chat · **F** fs | en cola (O3, tras liberar slot) | M1-E*, M1-F* | — | TODO |

**Integración (orchestrator, 2026-10-17)**: ✅ editor real (M1-B) + panel de problemas (M1-D) montados en el shell con wiring completo (watcher→editor, `CompileController`→status/diagnósticos/problemas, `ProblemsEvent::OpenSource`→abrir en editor, `TreeFsEvent`→shell). ✅ **PDF real (M1-C) montado** (commit `d6e70ca`): recarga al compilar con generation, SyncTeX forward (Ctrl+Alt+J) e inverse, preload main.tex+pdf. Evidencia: `shots/m1-integration.png` y `shots/m1-pdf.png` (CV renderizado). Informe C (cancelada al final, implementación completa): `docs/qa/reports/M1-C-pdf.md`. ⏳ Falta el chat (M1-E, ola O3 en marcha).

## O3 — Streams en cola (lanzada 2026-10-17)

| Stream | Territorio | Tareas | Sesión | Estado |
|---|---|---|---|---|
| **E** chat IA | `crates/chat/**`, `crates/ui/src/panels/chat.rs`, wiring en `app.rs` | E1..E6 (supervisor prod, reducer, panel Copilot, permisos, fix-with-AI, pickers) | ses_eb5049c0fffeVXs8qCkzA75HM3 | DOING |
| **F** proyectos/fs | `crates/core/**` (+project), `ui/src/{project,new_project,tree,toolbar}.rs` | F1..F4 (ProjectService en core, tree production events, ops, wizard + plantillas) | ses_eb5049c0effetI9psO009sDCjz | DOING |

**G1 cerrado (2026-10-17): ✅ PASS** — informe `docs/qa/reports/G1-2026-10-17.md`. E2E: pasos 1–4 y 7 con evidencia visual (`shots/e2e-*.png`); 5–6/8 cubiertos por tests gated + 2 validaciones manuales pendientes (Ctrl+Alt+J y aprobación de permiso en vivo). Gates: fmt/clippy/test verdes (29 suites); live chat verde con **DeepSeek V4.1 Flash**. Siguiente: **M2 (G2)**.

**Feedback del usuario sobre G1 (2026-10-18)**: informe `docs/qa/reports/M1-C5-scroll-sync-2026-10-18.md`.
| Punto | Resultado |
|---|---|
| `Ctrl+Alt+J` (validación manual 1) | ✅ funciona — el usuario pide además **scroll sync** en vez de solo resaltado |
| Aprobar permiso en el chat (validación manual 2) | ❌ **bug**: no se podía escribir en el chat |

- **Fix de foco** (`9938b75`): `PaneState::focus_if_outside` — el chrome de la hoja ya no roba el foco al input que el clic acaba de enfocar (afectaba al editor **y** al chat; en GPUI el ratón burbujea de dentro hacia fuera y el slot sobreescribía el foco). Tests de UI nuevos con `TestAppContext` en `crates/ui` (primer uso del harness `gpui-kit/test-support`, pieza de M2): `clic_en_el_composer_permite_escribir`, `clic_en_el_editor_permite_escribir` (fallan sin el fix).
- **Scroll sync `.tex` ↔ PDF (M1-C5)**: `latex.syncScroll` (default ON) con interruptor en toolbar y Ajustes; avance al mover el cursor (debounce 250 ms, `synctex view` en background, scroll **sin** resaltado) e inverso al desplazar el visor (scroll del editor **sin mover el cursor**), con guarda anti-bucle de 400 ms. `Ctrl+Alt+J` sigue siendo el salto manual con resaltado. De paso: `Ln/Col` y `Pág. n/N` de la status bar ya se actualizan y el **Ctrl+clic inverse vuelve a funcionar** (`set_synctex_exe` no se llamaba nunca desde el shell). 3 tests nuevos + suites verdes.
- Pendiente de validación manual: (a) seguir el cursor → el PDF se desplaza; (b) desplazar el PDF → el editor se desplaza sin perder el cursor; (c) apagar el interruptor y comprobar que no se mueve nada.

**Segunda ronda de feedback (2026-10-18, misma sesión)**: informe `docs/qa/reports/M2-feedback-2026-10-18-chat-dock-skills.md`.

| Petición | Estado |
|---|---|
| `Enter` envía / `Shift+Enter` salto de línea | ✅ `5a71ab5` + corrección `d9260cc` (`submit_on_enter` del kit; el `capture_key_down` inicial no bastaba) |
| Botón de gestión de permisos (no preguntar por cada acción) | ✅ menú con 3 presets (`ask` / `project` / `all`) persistidos |
| Razonamiento en bloque plegado | ✅ `5a71ab5` + corrección `d9260cc`: los deltas que llegan antes del snapshot se bufferizan, así el bloque nace plegado (antes parpadeaba como texto) |
| Skills de LaTeX **sin instalar globales** | ✅ 4 skills en `assets/skills` + `skills.paths` en `config/opencode.runtime.json` (`2697121`); verificado con `opencode debug skill` |
| Volver atrás si el agente rompe algo | ✅ revert nativo de opencode por mensaje + «deshacer» |
| Unificar problemas + SALIDA en pestañas bajo el Explorador | ✅ `a8b95ac` (`BottomPanel`) |
| Pestaña de símbolos LaTeX insertables con un clic | ✅ ~90 símbolos por categorías con buscador |

Gates de la ronda: fmt/clippy/workspace verdes (lsx-ui 82→83, lsx-chat 40→42 tras las correcciones del 2026-10-18); evidencia visual `shots/m2-bottom-dock.png`. Pendiente de validación manual: Enter/razonamiento (revalidar tras `d9260cc`), preset de permisos sin modal, revert con vuelta de archivos, carga de una skill LaTeX y uso de la paleta.

**Instalador compilado y probado en local (2026-10-18)**: petición del usuario («olvidémonos de la Action: generamos el `.exe` en local»). Informe: `docs/qa/reports/M3-instalador-local-2026-10-18.md`.

- `dist\LaTeX-Studio-Setup.exe` — **53,8 MB**, SHA-256 `FE068434…5288C`, versión `0.1.0-20261018`, perfil **ligero** (app + agente + skills; sin `tex\` ni `workspace\`), montado con `cargo xtask dist --no-tex --no-zip --clean` y compilado con el Inno Setup del PC (`pwsh -File installer\build.ps1 -SkipBuild`).
- Instalado **por usuario** (sin UAC) en `%LOCALAPPDATA%\LaTeX Studio` y **verificado de punta a punta**: arranca, crea `workspace\`, arranca el agente (`IA lista`), **el chat responde y lista las 4 skills portables** (`latex-beamer`, `latex-compilar-depurar`, `latex-matematicas`, `latex-tablas-figuras`) y, con `tex\` enlazado (*junction*), compila (`TeX Live` en verde + `main.pdf`).
- **Cuatro fallos de primer arranque arreglados** (commit `3fd2b8b`): el agente no arrancaba por falta de `workspace\` (ahora `PortableRoot::ensure_layout()`); diálogo final en instalación silenciosa; pestañas del dock inferior cortadas en la columna estrecha (ahora envuelven en dos filas); y el **modelo por defecto fijado en los ajustes** (`DEFAULT_AI_MODEL`) para no heredar el de la config global de opencode.
- **Auditoría de secretos** (árbol versionado + historial completo): limpia; el único «match» es un blob cifrado de razonamiento dentro de un fixture (`sk-` por azar en base64).
- **Release publicada**: `v0.1.0` en GitHub (`releases/tag/v0.1.0`) con `LaTeX-Studio-Setup.exe` (53,8 MB) y su `.sha256`; el asset descargado se comprobó **bit a bit** contra el local. Runbook: sección «Ruta C» de `docs/release.md`. Ojo: el tag `v*` dispara el workflow `installer`, que publica su propio artifact del run (no toca los assets de la release).
- Gates: fmt/clippy/workspace verdes (**328 tests**). Pendiente: clic manual en **Compilar** desde la app instalada y decidir si el instalador se sigue haciendo en CI o en local.


**Modelo del chat (petición del usuario, 2026-10-17)**: el producto usa por defecto **`opencode-go/deepseek-v4.1-flash`** (antes gpt-5.6-luna) — fijado en `agent/opencode.json` (`model`) y `config/settings.json` (`ai.model`); verificado contra `GET /config` del server real (commit `aa8573b`). Todos los tests/demos nuevos del chat usan DeepSeek.

Gate de salida: **G1** (E2E pasos 1–8 + presupuestos §6.3) — requiere también E/F; el orchestrator integra los paneles en el shell conforme aterrizan.
| M0-S6 | fs/watcher + tree component | ✅ DONE | informe: `docs/qa/reports/M0-S6-spike-fs.md` · list_dir 5,6 ms/5k · tree sin evento de selección (→ M1-F2) |
| M0-G0 | Freeze `INTERFACES.md` | TODO | orchestrator, al cerrar S1–S6 — requiere revisión del usuario |

**Consolidación M0 (2026-10-17)**: commits por spike (`7f9059b` pdf, `d99f6bf` core, `7943980` editor, `e446491` chat, `55e0e4a` ui); `xtask chat-replay` cableado y deps alineadas (`tree-sitter 0.26` + `codebook-tree-sitter-latex 0.6.1`) en `3430d9f`; `crates/latex` (S5) sin commitear a propósito mientras el spike está en vuelo; **build release + entrega de .exe en curso**; gates completos del workspace tras el build.

## 0.2 — De MVP a producto instalable (2026-10-24)

**OKF creado**: `docs/kb/` (índice, esquema, notas de producto/arquitectura/operaciones/
calidad, plan y changelog). `AGENTS.md` obliga a leerlo y mantenerlo; skill
`lsx-knowledge-base` añadido.

| Tarea | Estado | Evidencia / nota |
|---|---|---|
| P2-KB-1 OKF + AGENTS + skill | ✅ DONE | `docs/kb/index.md`, `docs/kb/changes/PLAN-0.2.md` |
| P2-INST-1 Instalador con TinyTeX | ✅ DONE | `docs/qa/reports/P2-INST-1-2026-10-24.md`; instalación limpia + article/report/beamer |
| P2-UI-1 Ajustes 2.0 | ✅ DONE | `shots/p2-settings-*.png`; 104 tests UI |
| P2-UI-2 Temas (sistema + acento) | ✅ DONE | dark/light; chat adaptado a tokens |
| P2-AI-1 Conexión IA/opencode | ✅ DONE | `ai_setup.rs`; live pendiente de `agent/opencode.exe` |
| P2-AI-2 Persistir modelo/agente del chat | ✅ DONE | `ChatPanelEvent::SelectionChanged` + `validate` de permisos |
| P2-UI-3 Ventana/menús/cierre seguro | ✅ DONE | `ui.window`, `AppMenuBar`, cierre seguro; capturas |
| P2-INST-2 Recuperación del motor | 🟡 PARCIAL | probe real + overrides + diálogo; falta bootstrap guiado |
| P2-UI-4 Bienvenida/pulido/retirada demo | 🟡 DOING | demo retirada; estado vacío con acciones |
| P2-QA-1 Gate G2.5 | 🟡 PARCIAL | `docs/qa/reports/G2.5-2026-10-24.md`; falta payload release + chat live |

**Auditoría**: `docs/kb/quality/01-auditoria-2026-10-24.md` (42 hallazgos S1–S4).
**Plan**: `docs/kb/changes/PLAN-0.2.md` · **Changelog**: `docs/kb/changes/CHANGELOG.md`.

## Notas

- **P0.4b**: crear remoto GitHub (privado por defecto) y push para verificar CI verde. Necesita decisión del usuario.
  - **2026-10-18**: repo destino decidido por el usuario: **https://github.com/marcosflz/OpenLeaf**; el remoto y el primer push se harán desde otro PC. Todo lo demás está maquetado: `cargo xtask dist` (M3-1), `installer/` (M3-2) y `.github/workflows/installer.yml` (construye el `Setup.exe` en CI con el agente descargado de la versión fijada). Runbook: `docs/release.md`. Repo verificado sin claves ni estado de usuario (217 ficheros, `main` limpio).
- **R6** (AGENTS.md del repo vs producto): test explícito en G1; los proyectos del producto viven en `workspace/` (ignorado por git).
- **Hallazgo gpui-kit (P0)**: `Button` se importa con `use gpui_kit::component::button::*;` — propagar a los skills `lsx-gpui-patterns`/`lsx-editor-ts` en M0.
- **Desviación M0 (aprobada por orchestrator)**: los spikes trabajan **directamente sobre `main`** con propiedad estricta de archivos (sin worktrees), para evitar 6 builds completos del árbol GPUI; los streams de M1+ sí usan worktrees+branches. Cada spike: binario propio en `app/src/bin/spike-*.rs` + su crate.
- Los fixtures `main.tex` se compilan con el `tex/` portátil real: reproducible con `xtask fixtures` (implementación en M0).
