# TAREAS — Board de ejecución (agent swarm)

> Fuente de verdad de la ejecución. El orchestrator copia este archivo a `docs/board.md` del repo y lo mantiene actualizado con estados.
> Estados: `TODO → DOING → REVIEW → DONE` · `BLOCKED` con motivo. Cada tarea se cierra solo si cumple su **DoD** y sus **criterios de aceptación**.
> Estimación en puntos relativos: 1 (horas), 2 (≈1 sesión), 3 (2–3 sesiones), 5 (≈1 semana-agente), 8 (grande, dividir si puede).
> `∥` = paralelizable tras su dependencia. `▲` = gate/decision critic.

---

## 0. Grafo de ejecución

```
P0 (preflight, secuencial-ish)
 │
 ├─► M0-S1 shell ──┐
 ├─► M0-S2 editor ─┤
 ├─► M0-S3 pdf ────┼──► G0 (freeze INTERFACES) ──► M1 (7 streams ∥) ──► G1 ──► M2 ──► G2 ──► M3 ──► G3
 ├─► M0-S4 chat ───┤
 ├─► M0-S5 engine ─┤
 └─► M0-S6 fs/watcher ─┘
```

---

## P0 — Preflight (sin esto no arranca nada)

| ID | Tarea | Entregable | Deps | Pts | Criterios de aceptación |
|----|-------|-----------|------|-----|-------------------------|
| P0.1 | **Backup y git init** | Copia de seguridad de archivos pequeños (`config\`, `data\`, scripts, `installer\`) en `latex-ide-backup-2026-10-17\` (fuera del repo); `git init` en la raíz con `.gitignore` (ver §1) | — | 1 | `git status` limpio; `tex\` y `legacy\` ignorados; ningún archivo de usuario dentro del repo |
| P0.2 | **Mover legacy** | `legacy\` creado; ver manifiesto §2 movido con `git mv`/robocopy + verificación de tamaño/hash | P0.1 | 2 | Manifiesto 100% movido; `tex\`, `agent\`, `workspace\` intactos (verificado por listado+hash); app vieja ya no arranca desde la raíz (solo desde `legacy\`) |
| P0.3 | **Esqueleto de workspace** | `Cargo.toml` workspace, `rust-toolchain.toml`, 7 crates con stubs que compilan, `cargo fmt/clippy/test` verdes, `latex-studio` abre ventana "Hola" con `gpui-kit` | P0.2 | 3 | `cargo run -p latex-studio` muestra ventana; gates §6.3 de PLAN-MAESTRO pasan; versiones pinneadas de §5.1 |
| P0.4 | **CI** | GitHub Actions windows-latest: fmt, clippy `-D warnings`, nextest, deny; cache de cargo | P0.3 | 2 | CI verde en el primer push |
| P0.5 | **Skills del swarm** | Instalar skills oficiales GPUI Kit (`npx skills add longbridge/gpui-kit` o vendorizar `skills/` a `.opencode/skills/`) + crear skills `lsx-*` (contenido en SWARM.md §4) | P0.3 | 3 | Cada skill carga con el tool `skill`; descripciones ≤1024 chars; referencias disponibles |
| P0.6 | **Fixture base** | `fixtures/projects/cv` (copia de `workspace\cv`), `fixtures/logs/*` (4 tipos de error reales), `fixtures/opencode/events/` (vacío, se llena en S4), script `xtask fixtures` | P0.3 | 2 | `cargo xtask fixtures` restaura fixtures; tamaños < 5 MB total |
| P0.7 | **AGENTS.md + ADRs + board** | `AGENTS.md` del repo (reglas de desarrollo, punteros a docs), `docs/decisions/ADR-000-template.md`, `docs/board.md` (copia de este archivo), `docs/qa/e2e.md` (guion 12 pasos) | P0.3 | 2 | Estándar de referencia: todo agente que entra al repo encuentra reglas y board |
| P0.8 | **Registro de riesgos/gates** | `docs/gates.md` con R1–R10 (PLAN-MAESTRO §7) e I-1..I-7 (INTERFACES §11) como checklist viva | P0.3 | 1 | Orchestrator puede marcar gates sin releer todo el plan |

**Regla P0**: nadie empieza M0 hasta que P0.1–P0.5 estén `DONE` (P0.6–P0.8 pueden ir en ∥ con M0).

---

## 1. `.gitignore` mínimo (referencia P0.1)

```
/target
/dist
/logs
/legacy
/tex
/agent/opencode.exe
/agent/auth*.json
/config/backups
*.aux  *.log  *.fls  *.fdb_latexmk  *.synctex.gz  *.out  *.toc  *.bbl  *.blg  *.bcf  *.run.xml  *.spl
!fixtures/**/*.log
```

---

## 2. Manifiesto de mover a `legacy\` (referencia P0.2)

| Origen | Destino | Nota |
|---|---|---|
| `app\` | `legacy\app\` | VS Code portable (~350 MB) |
| `data\` | `legacy\data\` | Perfil VS Code + extensiones instaladas |
| `extensions\` | `legacy\extensions\` | `.vsix` y fuentes de extensiones de la distro |
| `downloads\` | `legacy\downloads\` | Se puede borrar si hace falta |
| `installer\` | `legacy\installer\` | Referencia para el Inno nuevo (M3) |
| `sandbox\` | `legacy\sandbox\` | Desechable |
| `config\` | `legacy\config-vscode\` | ⚠️ contiene `chatLanguageModels.json` con API key → **no commitear**; el nuevo `config\` lo crea la app |
| `bootstrap.ps1`, `launch.ps1`, `launch.cmd`, `sandbox.ps1`, `sandbox.cmd`, `Makefile`, `README.md` | `legacy\` | El nuevo README/Makefile los escribe M3 |
| `workspace\LaTeX.code-workspace` | `legacy\` | Especifico de VS Code |
| **NO MOVER** | `tex\`, `agent\`, `workspace\` (excepto el `.code-workspace`), `config\settings.json` nuevo | Activos reutilizados |

Verificación post-move: `Get-ChildItem legacy -Recurse | Measure-Object -Sum Length`, hash de `workspace\cv\template.tex` antes/después, y `test -f tex\bin\windows\latexmk.exe`.

---

## 3. M0 — Spikes (1 demo por spike; gate G0 al final)

> Cada spike demuestra la viabilidad **y responde su pregunta de interfaz**. El código de spike puede ser sucio; el **aprendizaje** se vuelca a INTERFACES.md + skills.

| ID | Spike | Pregunta a responder / Entregable | Deps | Pts | Fallback si falla |
|----|-------|-----------------------------------|------|-----|-------------------|
| M0-S1 | **Shell 4 paneles** | Ventana `gpui-kit` con `dock` de 4 hojas, tema oscuro custom, title bar, status bar; serializa/restaura layout | P0.3 | 3 | Construir layout con `resizable.rs` del kit a mano |
| M0-S2 | **Editor TS** ▲ | Abrir `fixtures/projects/cv/template.tex` en el editor del kit; resaltado con `tree-sitter-latex` (query propia si no hay feature); editar+guardar; responder I-1/I-3 | P0.3 | 5 | `highlighter` propio: tree-sitter → spans → `TextRun`s sobre `Input`/`Textarea` |
| M0-S3 | **PDF** ▲ | Renderizar `fixtures/pdf/cv.pdf` con hayro en panel (zoom/scroll/página), medir render p50/p95; evaluar patrones de `gpui-pdf` (solo lectura, GPL) | P0.3 | 5 | pdfium-render tras trait `PdfRender` (DLL nativa, BSD) |
| M0-S4 | **Chat opencode** ▲ | Arrancar `opencode serve` del bundle, sesión nueva, prompt con respuesta streameada a burbuja markdown, tool card, **permiso grabado**, apagado limpio; grabar fixtures SSE y **congelar tabla evento→ChatEvent** | P0.3 | 5 | Si SSE no emite permisos/part events esperados → mapear con polling de `/session/:id/message` (peor UX, funcional) |
| M0-S5 | **Engine + log + synctex** | Compilar fixtures `cv` y `broken` con el `TexLiveEngine`; `parse_log` con insta sobre fixtures; forward/inverse reales contra `cv.pdf`; auto-install de `lastpage` con tlmgr en el corpus | P0.3, P0.6 | 5 | — (es el camino D3, riesgo bajo) |
| M0-S6 | **FS + watcher + árbol** | `tree.rs` del kit con iconos/filtros, watcher con debounce, menú contextual (crear/renombrar/borrar en copia de fixtures) | P0.3 | 3 | `virtual_list` propio con flat-list expandible |
| M0-G0 | **Freeze interfaces** | `INTERFACES.md` sin TODOs de gate; decisiones I-1..I-7 marcadas; skills actualizadas con hallazgos (especialmente `lsx-gpui-patterns` y `lsx-opencode-api`) | S1–S6 | 2 | — |

**Criterio G0**: los 6 spikes demostrados en una sesión de revisión (grabación/captura por spike), gates verdes, R2/R3 resueltos o con fallback decidido.

---

## 4. M1 — Núcleo usable (streams ∥)

### Stream A — Shell/UX (head: `lsx-ui`)
| ID | Tarea | Deps | Pts | Criterios |
|----|-------|------|-----|-----------|
| M1-A1 | Dock real con colapsos, focus mode, atajos §4.2, persistencia | G0 | 3 | Layout sobrevive reinicio; atajos documentados funcionan |
| M1-A2 | Toolbar + status bar (estado compilación, Ln/Col, página, dot IA) | G0 | 2 | Todos los estados visibles y accesibles (tooltips) |
| M1-A3 | Temas (dark pulido + acento configurable) + fuente mono bundle | G0 | 3 | Comparativa visual contra mock de referencia (docs/design/) |
| M1-A4 | Notificaciones/toasts + modales (confirmaciones) + **modal de permiso IA** | G0, S4 | 3 | Flujo permiso E2E con server real |
| M1-A5 | Command palette + Ajustes UI (editor básico de `settings.json` con validación) | G0 | 3 | Cambios aplican en caliente (zoom, fuente, debounce) |
| M1-A6 | i18n es (rust-i18n) en todas las strings | G0 | 2 | Ninguna string hardcodeada fuera de tablas i18n |

### Stream B — Editor (head: `lsx-editor`)
| ID | Tarea | Deps | Pts | Criterios |
|----|-------|------|-----|-----------|
| M1-B1 | Pestañas multi-documento + dirty + restauración de sesión | G0 | 3 | Reabrir app restaura pestañas/cursores; cierre con dirty pide confirmación |
| M1-B2 | Guardar atómico + watcher (conflictos externos con modal 3 opciones) | G0, S6 | 3 | Test: editar fuera (script) mientras dirty → modal correcto |
| M1-B3 | Diagnósticos inline del LogParser (o gutter+tooltip si sin squiggles) | G0, S5 | 3 | Error en línea X de `template.tex` visible y navegable |
| M1-B4 | Buscar/reemplazar (barra, contador, toggles, wrap-around) | S2 | 3 | 20 casos del guion QA pasan |
| M1-B5 | Autosave opcional + backups `config\backups\` + recuperación | B1,B2 | 2 | Crash simulado → proponer recuperación al abrir |

### Stream C — PDF (head: `lsx-pdf`)
| ID | Tarea | Deps | Pts | Criterios |
|----|-------|------|-----|-----------|
| M1-C1 | Visor: zoom modes, scroll continuo, virtualización, indicador página | S3 | 5 | 100 páginas a 60 fps de scroll (contador fps dev) |
| M1-C2 | Caché LRU + render progresivo + invalidación por generación | C1 | 3 | Presupuesto RAM §6.3; zoom agresivo sin bloquear UI |
| M1-C3 | Recarga al recompilar conservando página/offset + flash "actualizado" | C1, D2 | 2 | Ciclo guardar→compilar→PDF sin saltos de scroll |
| M1-C4 | SyncTeX forward (atajo + rectángulo animado + scroll) y **inverse** (ctrl+click) | C1, S5 | 5 | Round-trip exacto en `cv` y `brok…` fixtures; latencia < 300 ms |

### Stream D — LaTeX/Engine (head: `lsx-latex`)
| ID | Tarea | Deps | Pts | Criterios |
|----|-------|------|-----|-----------|
| M1-D1 | LatexEngine TeXLive completo (probe, compile, timeout, cancel, job object) | G0 | 5 | Kill de latexmk colgado; sin procesos huérfanos tras cerrar app |
| M1-D2 | CompileQueue (single-flight, coalescing, generaciones) + integración autoCompile debounce | D1 | 3 | Tests de estados y de coalescing (fake engine) |
| M1-D3 | LogParser producción + panel de Problemas (lista, filtros, navegar a línea) | D1 | 5 | 4 fixtures → diagnostics correctos; click → cursor en línea |
| M1-D4 | Auto-install tlmgr (tope 3, configurable) + banner de progreso | D3 | 3 | Fixture `broken` (falta lastpage) se auto-repara |
| M1-D5 | "Limpiar auxiliares" + detección de `main` + settings de engine | D1 | 2 | CV limpio de auxiliares; heurística documentada |

### Stream E — Chat IA (head: `lsx-chat`)
| ID | Tarea | Deps | Pts | Criterios |
|----|-------|------|-----|-----------|
| M1-E1 | Supervisor robusto (start/health/restart/stop, puerto libre, password, job object) | G0 | 5 | Matar server a mano → reconexión automática; sin huérfanos |
| M1-E2 | Cliente REST + SSE + reducer (con fixtures S4) + historial | E1 | 5 | Reproductor de fixtures: dado `events.jsonl` → `ChatEvent`s correctos |
| M1-E3 | Chat UI: burbujas, stream markdown, tool cards, code copy, scroll-pin | E2 | 5 | Streaming fluido a 60 fps; historial grande virtualizado |
| M1-E4 | Permisos end-to-end (modal diff + once/always/deny/remember) | E2, A4 | 5 | Edición real de `.tex` por el agente requiere aprobación y se aplica |
| M1-E5 | Contexto + acciones: chips archivo/selección, "Arreglar errores con IA" (usa `latex-fix`) | E2, D3 | 5 | Flujo completo: error → botón → fix propuesto → compile verde |
| M1-E6 | Modelo/agente pickers + abort + nueva sesión + borrar sesión | E2 | 3 | Cambios persisten; abort corta streaming |

### Stream F — FS/Proyectos (head: `lsx-core`+UI tree)
| ID | Tarea | Deps | Pts | Criterios |
|----|-------|------|-----|-----------|
| M1-F1 | ProjectService: abrir/recientes/último proyecto + resolución main.tex | G0 | 3 | Reabre último proyecto al arrancar |
| M1-F2 | Árbol producción: filtros, iconos, refresh por watcher, ocultar auxiliares | S6 | 3 | Sin parpadeos; 5k archivos en < 200 ms |
| M1-F3 | Operaciones de archivo con confirmación + validación de nombres Windows | S6 | 2 | Casos inválidos (`CON`, `..`, espacios finales) bloqueados |
| M1-F4 | Wizard "nuevo proyecto" (plantillas article/beamer + `.opencode\agent\latex-fix.md` + `AGENTS.md`) | F1 | 3 | Proyecto nuevo compila de fábrica |

**Gate G1**: E2E pasos 1–8 (§7) + presupuestos §6.3 + QA sign-off + R6 verificado (AGENTS.md correcto para proyectos en `workspace\`).

---

## 5. M2 — IA completa + pulido

| ID | Tarea | Stream | Pts | Criterios |
|----|-------|--------|-----|-----------|
| M2-1 | Outline de documento (queries TS: secciones/labels) + uso para navegar | B | 3 | Click → salta a sección |
| M2-2 | Thumbnails de páginas PDF en barra lateral del panel + búsqueda de texto en PDF (si hay API; si no → M4) | C | 5 | Navegación por miniaturas fluida |
| M2-3 | Tema claro completo + toggle + `reducedMotion` | A | 3 | Ambos temas con contraste AA |
| M2-4 | Historial de sesiones de chat: lista, cambiar, renombrar, exportar md | E | 3 | Última sesión se restaura al abrir panel |
| M2-5 | Reasoning plegable + uso de tokens + errores del server con retry | E | 3 | Estados visibles y accionables |
| M2-6 | Quick actions configurables (reescribir/traducir/resumir) con plantillas en `assets/prompts` | E | 3 | Añadir una plantilla nueva no requiere recompilar (hot reload prompts) |
| M2-7 | Guardado de layout por proyecto + focus modes por panel | A | 2 | Cambiar de proyecto restaura su layout |
| M2-8 | Buscar en archivos (`/find` propio: ignore+globset) + abrir resultado en editor | F | 3 | Búsqueda en `workspace` con resultados clicables |
| M2-9 | Panel de problemas ampliado (agrupar por archivo, severidad, filtro texto) | D | 2 | Con 200 warnings sigue fluido |
| M2-10 | Atajos configurables (keybindings.json) | A | 3 | Editar y aplicar sin reiniciar |
| M2-11 | Métricas dev (F12): fps, tiempo render PDF, ms compilación | A | 2 | Overlay con datos reales |
| M2-12 | "Explicar error" desde problemas + diff viewer en tool cards con `diff` del kit | E | 3 | Diffs legibles con colores del tema |

**Gate G2**: E2E 9–12 + QA adversarial (bash raro del agente, PDF corrupto, server caído a mitad de stream, archivo 10 MB, 300 páginas).

---

## 6. M3 — Distribución

| ID | Tarea | Pts | Criterios |
|----|-------|-----|-----------|
| M3-1 | `xtask dist` (assemble + zips full/app + checksum) | 3 | Zip full arranca en carpeta nueva con acentos/espacios en la ruta |
| M3-2 | Instalador Inno adaptado de `legacy\installer` (por usuario) | 3 | Instalar/desinstalar limpio en VM o usuario nuevo; no rompe si `tex\` ya existe |
| M3-3 | Primer arranque: chequeos (tex/agent/config), mensajes, `--diagnose` CLI | 3 | Carpeta sin `tex\` → UI clara; `--diagnose` vuelca informe |
| M3-4 | README.md + LICENSE + THIRD-PARTY-LICENSES (generado por `cargo about`) | 2 | `cargo deny` + about verdes |
| M3-5 | Accesibilidad: roles/nombres, navegación por teclado de paneles/menús | 3 | Patrón del kit (`ACCESSIBILITY-UI-TESTING.md`) aplicado a los 4 paneles |
| M3-6 | Benchmarks formales (criterion: render PDF, logparse, synctex, reducer) + informe | 2 | Números publicados en `docs/perf.md` vs presupuestos |
| M3-7 | Demo/video + guion de presentación + limpieza final de `legacy\` (decisión usuario) | 2 | Orden explícita del usuario para borrar legacy |
| M3-8 | Post-milestone: bump de snapshot GPUI (si hay) como tarea aislada | 3 | Gates verdes tras el bump; si rompe > 1 día → revert |

**Gate G3**: instalación limpia desde zip + E2E completo + sin regresiones de presupuesto + sign-off del usuario.

---

## 7. Guion E2E (resumen; detalle en `docs/qa/e2e.md`)

1. Arranque con `workspace\cv` como último proyecto → restaura layout, pestañas y PDF.
2. Editar `template.tex` → guardar → compila (status bar) → PDF se actualiza sin perder scroll.
3. Introducir `\undefinedcmd` → error en problemas + gutter → deshacer → recompila verde.
4. Borrar `\usepackage{lastpage}` y quitar la instalación → auto-install tlmgr → compile verde.
5. Ctrl+Alt+J sobre una línea conocida → PDF salta y resalta la zona correcta.
6. Ctrl+click sobre esa zona en el PDF → editor vuelve a la línea correcta.
7. Abrir panel IA → nueva sesión → "resume el documento" → streaming OK.
8. Pedir "añade una sección de experiencia con la fecha actual" → permiso de edición → aprobar → archivo cambia → compila.
9. "Arregla el error actual" con error inyectado → diagnóstico → fix → verde.
10. Matar `opencode.exe` desde Task Manager → el panel se reconecta solo.
11. Cerrar app con compilación en curso → sin procesos huérfanos (Task Manager limpio).
12. Copiar carpeta portable a otra ruta (`D:\prueba con espacios\`) → arranca y compila.

---

## 8. M4 — Backlog (no ejecutar sin orden)

| ID | Tema | Nota |
|----|------|------|
| M4-1 | texlab LSP (completion, diagnósticos propios, goto-def, formato) | Binario GPL separado; `example-editor` como referencia |
| M4-2 | Backend Tectonic | `LatexEngine` ya lo permite; UI de selección de motor |
| M4-3 | Selección/búsqueda de texto en PDF | Requiere capa de texto (pdfium o extracción hayro si aparece) |
| M4-4 | Spellcheck (ltex-ls o hunspell) | Peso y complejidad altos |
| M4-5 | i18n en + selector de idioma | Estructura ya preparada |
| M4-6 | CI Linux (+ macOS si hay máquina) | Empaquetado y fuentes |
| M4-7 | `gpui-updater` auto-update | Requiere hosting de releases |
| M4-8 | Multi-cursor / vim mode | Solo si hay demanda real |
| M4-9 | Parser nativo `.synctex.gz` | Optimización; CLI ya cumple |
| M4-10 | Integración Git (estado/commit rápido) | Proyecto personal |
| M4-11 | Impresión/export PDF y "abrir carpeta de artefactos" | Pulido |

---

## 9. Reglas de paralelización para el swarm

- **Un stream = un head de crate = una worktree = una sesión de agente**. Máximo 4 streams simultáneos con archivos vivos en paralelo; el resto en cola.
- Archivos de propiedad exclusiva del orchestrator: `Cargo.toml` raíz, `rust-toolchain.toml`, `docs/board.md`, `INTERFACES.md`, `.opencode/**`, `AGENTS.md`, CI workflows. Cambios de otros → PR pequeño revisado.
- Cada stream puede crear módulos nuevos dentro de su crate sin pedir permiso; cualquier cambio en su API pública requiere nota en el PR y, si afecta a otro stream, ADR corto.
- Los spikes M0 los ejecuta preferentemente **1 agente por spike** (contexto fresco), y el orchestrator consolida.
- QA agent entra al final de cada stream y en cada gate; nunca el mismo agente que implementó revisa su propio gate.
