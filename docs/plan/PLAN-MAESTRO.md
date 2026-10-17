# PLAN MAESTRO — LaTeX Studio

> Clon open-source de Overleaf, portable y nativo en Rust.
> Visor de 4 paneles: archivos → editor → PDF → chat IA.
> Fecha del plan: 2026-10-17 · Estado: **pendiente de orden de ejecución** · Autor: sesión de planificación.

Documentos hermanos:
- `INTERFACES.md` — contratos congelados (tipos, traits, esquemas, convenciones).
- `TAREAS.md` — board completo con dependencias, gates y criterios de aceptación.
- `SWARM.md` — topología del agent swarm, skills y protocolo de ejecución.
- `README.md` — índice, resumen y checklist de lanzamiento.

---

## 1. Resumen ejecutivo

LaTeX Studio es una aplicación de escritorio **nativa Rust** para escribir, compilar y previsualizar LaTeX con asistencia de IA, distribuida de forma **100 % portable** (sin instaladores ni cambios en el sistema). Sustituye a la distro actual basada en VS Code + extensiones (`latex-ide\`), reutilizando sus dos activos valiosos:

1. **TeX Live portátil** (`tex\`) — latexmk + synctex + biber + tlmgr, paquetes offline.
2. **opencode portable** (`agent\`) — usado como backend del chat mediante `opencode serve` (HTTP + SSE).

La interfaz se construye **desde cero con GPUI**, vía el ecosistema **GPUI Kit / gpui-component** (componentes de escritorio, dock, editor con tree-sitter + LSP, markdown nativo, listas virtuales, diff, chat primitives). El resultado esperado: arranque < 1.5 s, RAM < 400 MB, un único exe + carpeta portable, cuatro paneles redimensionables, compilación al guardar, SyncTeX bidireccional y un panel de IA que puede leer el log, editar `.tex` y recompilar (auto-reparación como la distro actual, pero integrado).

---

## 2. Decisiones registradas (inmutables salvo ADR)

| ID | Decisión | Elección | Rationale |
|----|----------|----------|-----------|
| D1 | Framework UI | **GPUI** (vía `gpui-kit` 0.7.x) | Elegido por el usuario. Verificado: ecosistema maduro (gpui-kit 17.8k★, apps comerciales), editor tree-sitter + LSP incluido, dock serializable, 120 fps. |
| D2 | Cómo consumir GPUI | **`gpui-kit` umbrella** que pinea `gpui-pre =0.3.9` (snapshot de zed@93d7456, Apache-2.0) | El crate `gpui` de crates.io está congelado en 0.2.2 (oct-2025). El ecosistema Longbridge publica snapshots `gpui-pre-*` al día. Prohibido mezclar el `gpui` 0.2.2 de Zed con `gpui-pre` (tipos incompatibles). |
| D3 | Motor LaTeX | **TeX Live portátil existente** (`tex\bin\windows\latexmk.exe`) tras un trait `LatexEngine` | Elegido por el usuario. Ya funciona, paquetes offline, tlmgr para auto-reparación. Tectonic queda como backend futuro (M4). |
| D4 | Plataformas | **Windows primero**, código portable | Elegido por el usuario. CI Windows; jobs Linux/macOS opcionales en M4. |
| D5 | Editor | **tree-sitter desde el día 1** (grammar `tree-sitter-latex`, MIT) | Elegido por el usuario. El editor de gpui-component ya soporta lenguajes tree-sitter conectables (patrón del `example-editor`). |
| D6 | Ubicación del proyecto | Raíz del repo = `C:\Users\marco\Documents\.vsc\latex-ide\`; lo viejo se mueve a `legacy\` | Elegido por el usuario ("sustituye lo de aquí"). `tex\`, `agent\`, `workspace\` se conservan en su sitio. |
| D7 | Visor PDF | **Propio** sobre `hayro 0.8` + `RenderImage` de GPUI. `gpui-pdf`/`ratex-gpui` (GPL-3.0) solo como **referencia de patrones**, nunca como dependencia ni copia de código | Mantiene el stack MIT/Apache y el control de caché/zoom/SyncTeX. Gate de reevaluación en M0-S3. |
| D8 | Integración IA | `opencode serve` (binario embebido 1.18.30, API v1) + cliente HTTP/SSE propio en `lsx-chat` | Doc del server verificada. API aislada en un módulo para absorber la futura v2. SDKs Rust de terceros descartados (drift). |
| D9 | Licencia del código propio | **MIT** (dual MIT/Apache opcional) | Compatible con gpui-pre (Apache-2.0), hayro (MIT/Apache), tree-sitter-latex (MIT). texlab (GPL-3.0) se invoca/bundlea como binario separado con avisos (M4). |
| D10 | Nombre y crates | App: **LaTeX Studio** · binario `latex-studio.exe` · crates `lsx-*` | — |
| D11 | Configuración | `config\settings.json` (JSON plano) + estado de dock serializado; resolución de raíz portable documentada en INTERFACES | Estilo de la distro anterior (todo relativo a la raíz portable). |
| D12 | Repo | Un solo repo git en la raíz, conventional commits, ADRs en `docs/decisions/` | Requisito del swarm (worktrees, merges, gates). |

---

## 3. Alcance

### Dentro de v1 (M0–M3)
- Shell de 4 paneles: árbol de archivos, editor con pestañas, visor PDF, chat IA (dock redimensionable, colapsable, layout persistente).
- Editor: buffer con pestañas, resaltado tree-sitter LaTeX, números de línea, guardar/autoguardado opcional, búsqueda/reemplazo, marcadores de diagnóstico, outline de secciones (M2).
- Compilación: al guardar (debounce), botón manual, estados en barra de estado, parser de log estructurado, panel de problemas, auto-instalación de paquetes vía tlmgr (configurable).
- PDF: render nítido con zoom/escala, scroll continuo, navegación por páginas, recarga al recompilar conservando posición, SyncTeX forward (Ctrl+Alt+J) e inverse (Ctrl+click).
- Chat: sesiones múltiples, streaming markdown, tarjetas de tool calls, diffs, **permisos aprobables**, selector de modelo/agente, abortar, acción "Arreglar errores con IA", contexto (archivo/Selección).
- Portabilidad: carpeta autosuficiente (exe + assets + tex\ + agent\ + config\ + workspace\), sin registro, logs locales, cierre limpio de procesos hijos (job objects).
- Distribución: `xtask dist` que ensambla la carpeta/zips; instalador Inno opcional adaptado del existente.

### Fuera de v1 (backlog M4+)
- Backend Tectonic, LSP texlab (autocompletado/formato/goto), spellcheck (ltex), selección/búsqueda de texto dentro del PDF, i18n inglés, builds Linux/macOS oficiales, auto-actualizador (`gpui-updater`), multi-cursor avanzado, paginación/impresión PDF, Git UI, colaboración (fuera de alcance permanentemente).

---

## 4. Visión de producto y UX

### 4.1 Los cuatro paneles

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Title bar (nativa Windows) · toolbar: proyecto │ ▶ Compilar │ sync │ layout  │
├───────────┬───────────────────────────┬───────────────────────┬──────────────┤
│ 1 ÁRBOL   │ 2 EDITOR                  │ 3 PDF                 │ 4 CHAT IA    │
│           │                           │                       │              │
│ proyecto  │ pestañas: cv.tex, *.bib   │ zoom / páginas        │ sesiones     │
│ filtro    │ gutter + resaltado TS     │ scroll continuo       │ mensajes     │
│ nuevo/    │ problemas subrayados      │ ctrl+click → fuente   │ herramientas │
│ renombrar │ outline (M2)              │ sync flash rect       │ permisos     │
│ borrar    │ búsqueda/reemplazo (M1.5) │                       │ modelos      │
├───────────┴───────────────────────────┴───────────────────────┴──────────────┤
│ Status bar: Ln/Col · palabras · compilación (estado+ms) · página · IA (modelo) │
└──────────────────────────────────────────────────────────────────────────────┘
```

- **Panel 1 — Árbol**: roots = proyecto activo (por defecto `workspace\`), expandir/colapsar, iconos por extensión, menú contextual (nuevo archivo/carpeta, renombrar, borrar con confirmación), oculta artefactos de compilación (`.aux .log .synctex.gz .out .toc .bbl .blg .fls .fdb_latexmk .bcf .run.xml`), watcher con debounce, filtro incremental, recientes, botón "nuevo proyecto" (plantillas article/report/beamer + `.opencode\agent\latex-fix.md` + `AGENTS.md`).
- **Panel 2 — Editor**: pestañas con dirty-dot, cerrar con confirmación, restaurar sesión al abrir, buffer utf-8, EOL LF/CRLF detectada y preservada, indentación por espacios (2 por defecto), tree-sitter LaTeX, buscar/reemplazar con resaltado, `Ctrl+S` guarda (atómico: temp+rename), autosave opcional (30 s) + copia de recuperación en `config\backups\`, marcado de diagnósticos del compilador (squiggle si el componente lo permite; si no, gutter + tooltip), scroll sync desde PDF.
- **Panel 3 — PDF**: zoom (fit-width / fit-page / 50–400 %), scroll continuo virtualizado, caché LRU de páginas como textura (re-render progresivo: 1× inmediato, 2×/3× en idle), rectángulo de sync animado 1.5 s, doble clic → abrir en visor externo, indicador de "recompilando…" con PDF anterior visible (no parpadea).
- **Panel 4 — Chat**: historial tipo burbujas; secciones de assistant con markdown nativo (y streaming), bloques de código con botón copiar; tool cards (nombre, estado running/ok/error, salida plegable; **diffs** para edit/write); modal de **permiso** con Allow once / Always / Deny (mapea a `remember`); drop-down de modelo (de `/config/providers`) y agente (`build`, `plan`, `latex-fix`); estado del server (dot: connecting/ready/error) con botón reiniciar; input multilínea (Enter=envía, Shift+Enter=salto), chips de contexto (archivo actual, selección), stop (abort), "nueva sesión", listado conmutador de sesiones.
- **Acciones IA rápidas** (toolbar del chat y menú contextual): `Arreglar errores` (compone prompt con los diagnósticos actuales + extractos), `Explicar error` (desde panel problemas), `Reescribir selección`, `Traducir selección`, `Preguntas sobre el documento`. Definidas en `assets\prompts\*.md`.

### 4.2 Atajos por defecto (Windows)

| Atajo | Acción |
|---|---|
| `Ctrl+S` / `Ctrl+Shift+S` | Guardar / Guardar como |
| `Ctrl+Alt+B` | Compilar ahora |
| `Ctrl+Alt+V` | Mostrar/ocultar panel PDF |
| `Ctrl+Alt+J` | SyncTeX forward (cursor → PDF) |
| `Ctrl+Click` (PDF) | SyncTeX inverse (PDF → fuente) |
| `Ctrl+Alt+1 / 3 / 4` | Colapsar/expandir paneles árbol / PDF / chat |
| `Ctrl+Alt+I` | Foco al chat |
| `Ctrl+Alt+M` | Maximizar panel activo (oculta los demás) |
| `Ctrl+F` / `Ctrl+H` | Buscar / Reemplazar |
| `Ctrl+P` | Command palette (componente `command`) |
| `Ctrl+,` | Ajustes |
| `F5` / `Ctrl+F5` | Compilar + ir a primer error / compilar limpio |

### 4.3 Flujos clave

**Guardar → compilar → PDF (auto)**
```
Ctrl+S ──► save atómico ──► watcher detecta ──► CompileQueue (debounce 400ms)
        ──► latexmk (tokio::process, cancelable) ──► .log
        ──► LogParser ──► Diagnostics ──► UI (estado, problemas, marcadores)
        ──► OK: main.pdf mtime cambia ──► PdfPanel recarga (conserva página/offset)
        ──► FAIL: estado rojo + botón "Arreglar errores con IA"
```

**SyncTeX**
```
Forward:  cursor → `synctex view -i línea:col:archivo -o main.pdf`
          → parseo (page, x, y, w, h en bp, origen arriba-izq) → flash + scroll
Inverse:  Ctrl+click → px → bp (escala/zoom) → `synctex edit -o página:x:y:main.pdf`
          → abrir archivo + mover cursor (y marcador temporal)
```

**Chat + permisos**
```
UI input ──► POST /session/:id/prompt_async ──► SSE GET /event
          ──► reducer ──► PartUpdated(text) ──► stream-markdown
                        └► PartUpdated(tool) ──► tool card
Agent edita .tex ──► evento permission ──► modal diff ──► POST /session/:id/permissions/:id
          ──► watcher ve el archivo cambiado ──► editor recarga si está limpio; prompt si dirty
          ──► recompila al guardar (flujo normal)
```

**Arranque**
```
latex-studio.exe ──► PortableRoot::detect() ──► carga config/settings.json
  ├─ lazy: ChatService.start() al abrir el panel 4 (o warm-up en background si config)
  ├─ lazy: latexmk se resuelve en probe() al primer compile (badge de motor en toolbar)
  └─ restaura layout dock + sesión de editor + proyecto reciente
```

---

## 5. Arquitectura técnica

### 5.1 Stack pinneado

| Capa | Crate / componente | Versión | Notas |
|---|---|---|---|
| UI | `gpui-kit` | `=0.7.3` | Umbrella; pinnea `gpui-pre =0.3.9` (zed@93d7456). Features: `tree-sitter-languages`, `inspector` (dev). NO activar `gpui-fast` en v1. |
| UI base | `gpui-pre*` | `=0.3.9` | Solo si faltan APIs en el re-export del kit; mismo snapshot. |
| Editor | componentes `text`/editor + `highlighter` + `tree` + `dock` + `diff` + `bubble` + `message_scroller` + `virtual_list` + `command` | (del kit) | Validados en M0-S2/S6. |
| Gramática | `tree-sitter` + `tree-sitter-latex` | 0.27 / 0.1+ (o git pin si hace falta) | MIT. |
| Buffer | `ropey` | 1.6 | Solo si el editor del kit no expone buffer rope; decisión en M0-S2. |
| PDF | `hayro` | 0.8 | MIT/Apache, pure Rust; features default (`embed-fonts`, `embed-cmaps`, `simd`). |
| Watcher | `notify` | 8.2 | Con debounce propio. |
| Diálogos | `rfd` | 0.17 | Nativos Windows. |
| Async | `tokio` (rt-multi-thread, process, time) | 1.53 | Runtime dedicado en hilo propio; puente a GPUI por `async_channel` + `cx.spawn`. |
| HTTP | `reqwest` (json, stream) | 0.13 | SSE parseado a mano (línea a línea) — sin dependencias extra. |
| Serde | `serde`, `serde_json` | últimos | Config, protocolos, fixtures. |
| Errores | `anyhow` (bin) + `thiserror` (libs) | últimos | Taxonomía en INTERFACES. |
| Rutas | `camino`, `dunce` | últimos | UTF-8 y normalización Windows. |
| Filtros | `ignore`, `globset` | últimos | Árbol + buscar en archivos (M2). |
| Observabilidad | `tracing`, `tracing-subscriber`, `tracing-appender` | últimos | `logs\app.log` rotativo + niveles por config. |
| Diff | `similar` | últimos | Datos para `diff` component y fallback. |
| Windows | `windows` | últimos | Job objects, ShellExecuteW, DPI manifest. |
| Dev/QA | `insta`, `wiremock`, `criterion`, `cargo-nextest`, `cargo-deny`, `typos`, `taplo` | últimos | Gates en SWARM.md. |
| Toolchain | stable (fijar en `rust-toolchain.toml`) | ≥1.92 | MSRV de hayro 1.92; GPUI pide stable reciente. |

Reglas de versiones: crates con `=` para el stack UI (`gpui-kit`, snapshot) y PDF (`hayro`); el resto `^` con `Cargo.lock` commiteado. Actualizaciones de snapshot GPUI **solo** como tarea dedicada entre hitos (riesgo de breaking changes).

### 5.2 Grafo de crates

```
lsx-core   (config, portable root, proyectos, bus de eventos, fs model, errores, logging)
   ▲
   ├── lsx-latex   (LatexEngine trait, TeXLive backend, CompileQueue, LogParser, Synctex, tlmgr)
   ├── lsx-pdf     (PdfDocument hayro, cache, coordenadas, bitmaps)          [sin UI]
   ├── lsx-chat    (supervisor opencode, REST/SSE, reducer de dominio)       [sin UI]
   ├── lsx-editor  (buffer, tree-sitter, diagnósticos, LSP glue – con gpui-kit)
   └── lsx-ui      (shell, dock, 4 paneles, temas, ajustes, keybinds)
             ▲
             └── latex-studio (bin)  +  xtask (dist/fixtures/bench)
```

Reglas duras (verificadas por CI con `cargo tree` o revisión en PR):
1. `lsx-core` no conoce UI, tokio, reqwest ni gpui (solo std + serde + camino + tracing).
2. `lsx-latex`, `lsx-pdf`, `lsx-chat` no dependen de `gpui*`. Producen datos puros y consumen comandos/eventos.
3. Solo `lsx-editor`, `lsx-ui` y el binario dependen de `gpui-kit`.
4. Prohibido dependencia directa `gpui = "0.2"` (Zed) — explosión de tipos.

### 5.3 Runtime y concurrencia

- **Hilo principal**: GPUI (app, ventanas, entidades).
- **Hilo I/O**: runtime tokio multi-thread (`lsx-chat` supervisor + reqwest/SSE + `tokio::process` para latexmk/tlmgr/opencode; `spawn_blocking` para parseo de PDF y render).
- **Puentes**: canales `async_channel` (M→N); las entidades GPUI reciben eventos con `cx.spawn(async move { while let Ok(ev) = rx.recv().await { this.update(&mut cx, ...) } })`; los comandos UI→servicios van por el mismo canal en sentido inverso. Prohibido bloquear el hilo UI (nada de `std::process` síncrono, nada de I/O directo en render).
- **Cancelación**: `CancelToken` (AtomicBool + broadcast) por compilación y por sesión de chat; `kill-on-drop` de procesos hijos + **Job Object** con `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` asignado a todo hijo (opencode, latexmk, biber, texlab) para que un crash de la app no deje huérfanos.
- **Repintado**: los eventos marcan `cx.notify()`; nada de polling continuo salvo el spinner de compilación.

### 5.4 Servicios internos

- **CompileQueue** (`lsx-latex`): single-flight; coalescing (si llegan cambios durante compilación → 1 rerun); estados `Idle→Queued→Running→Success/Failed/Cancelled`; timeout configurable (10 min); emite progreso (líneas de stdout con `%`/páginas si disponible; si no, elapsed).
- **PdfService** (`lsx-pdf`): abre documento con hayro; cachea `PageBitmap` (LRU por memoria, ~256 MB max); prioriza página visible; generación por `doc_generation` para invalidar en recompilación; re-render progresivo por buckets de escala.
- **ChatService** (`lsx-chat`): supervisor de proceso (start/health/restart backoff 0.5/1/2/5 s máx 3, luego estado `Error`), cliente REST, stream SSE (reconexión con `Last-Event-ID` si el server lo emite; si no, refetch de mensajes), reducer que traduce eventos crudos a `ChatEvent` de dominio.
- **FsService** (`lsx-core`): watcher recursivo con debounce 200 ms, filtros por proyecto, API `list_dir`, `create/rename/delete`, `read/write` atómico, detección de cambios externos (hash por mtime+size).

### 5.5 Integración opencode (AI backend)

- **Arranque**: `agent\opencode.exe serve --hostname 127.0.0.1 --port <libre>` con `cwd = proyecto activo`, env: `OPENCODE_CONFIG=<raíz>\agent\opencode.json`, `OPENCODE_SERVER_PASSWORD=<aleatorio>` (basic auth en el cliente), `PATH` ampliado. Health check `GET /global/health` hasta 15 s.
- **Endpoints usados** (API v1 del binario 1.18.30): `GET /global/health`, `GET /event` (SSE), `POST /session`, `GET /session`, `GET /session/:id/message`, `POST /session/:id/prompt_async`, `POST /session/:id/abort`, `POST /session/:id/permissions/:permissionID`, `GET /config/providers`, `GET /provider`, `GET /agent`, `GET /session/status`.
- **Eventos SSE**: primera tarea de M0-S4 es **grabar un stream real** (mensaje + tool call + permiso) a `fixtures/opencode/events/*.jsonl` y mapear los tipos exactos a `ChatEvent`. Todo el manejo queda aislado en `lsx-chat::protocol` para amortiguar la migración a v2.
- **Permisos**: la config distribuye `edit: ask` (se mantiene); el modal de permiso es la UX de aprobación; `remember` → "Always".
- **Auto-reparación**: agente `latex-fix` (definición ya existente en `workspace\.opencode\agents\latex-fix.md`; se promueve a plantilla en `assets\templates\project\.opencode\agents\latex-fix.md`). El botón "Arreglar errores" crea/usa sesión con ese agente y adjunta diagnósticos + log tail.
- **Compatibilidad**: si en el futuro se actualiza a OpenCode v2, el único módulo a tocar es `lsx-chat::protocol`. La app **pinea** el binario embebido; no usa el del PATH.

### 5.6 Integración LaTeX

- **Backend TeXLive**: resolución de `tex\bin\windows\latexmk.exe` (relativa a raíz portable; override en settings). Args por defecto: `-synctex=1 -interaction=nonstopmode -file-line-error -pdf -outdir=<proyecto> <main.tex>`. Detección de `main.tex`: campo settings → heurística (archivo abierto con `\documentclass` → `\input` raíz → único `.tex` con `\documentclass`).
- **LogParser**: corpus de fixtures (`fixtures/logs/`) → `Diagnostic[]` con severidad, archivo/línea (gracias a `-file-line-error`), tipo (`MissingPackage{name}`, `LatexError`, `BibError`, `UndefinedRef`, `Overfull`, `RerunNeeded`, `Unknown`), mensaje y bloque crudo. Reglas: elegir el fragmento del último run (`Job started:` … EOF), tolerar logs truncados, cap 8 MB.
- **Auto-install**: si `MissingPackage` y `latex.autoInstallPackages=true` → `tlmgr install <pkg>` (respeta `tlmgr` actions ya permitidas en `agent\opencode.json`), luego rerun; tope 3 intentos por sesión; si falla, sugerencia + botón IA.
- **SyncTeX**: envoltorio del CLI `synctex.exe` (view/edit) con parseo robusto (`SyncTeX result begin…end`), tolerancia a campos faltantes, timeouts (2 s) y normalización de unidades (bp, origen superior-izquierdo) — tabla de conversiones y tests de round-trip en INTERFACES. Parser nativo `.synctex.gz` (flate2) queda como optimización M4 (no hay crate Rust maduro; `synctex_sys` exige lib C).
- **Limpieza**: comando "limpiar auxiliares" (`latexmk -c` con confirmación).

### 5.7 Integración PDF

- **Apertura**: `hayro::Pdf::new(bytes)` en `spawn_blocking`; guardar `page_count`, tamaños en pt.
- **Render**: `render_page(idx, scale)` → `PageBitmap{rgba, w, h, scale}`; UI lo sube a textura (`RenderImage` de gpui-pre). Escala = zoom × dpr de ventana para nitidez Retina/HiDPI.
- **Coordenadas**: PDF en bp (1/72"), origen arriba-izquierda para SyncTeX; conversión a px con la misma transformación del render. **Una sola implementación** en `lsx-pdf::coords` con tests (unit + round-trip con `synctex`).
- **Cache**: LRU por bytes (config, default 256 MB); página visible y vecinas en prioridad alta; re-render en idle a escala 2×.
- **Evaluación gpui-pdf (M0-S3)**: se lee su implementación como referencia de patrones (virtualización, scroll, overlays); si el esfuerzo propio de virtualización resultara alto, gate G0-S3 permite reconsiderar adoptarla **solo** si el usuario acepta licencia GPL-3.0 para la app. Default del plan: no adoptar.

### 5.8 Editor (tree-sitter desde día 1)

- **Modelo**: documento = path + contenido (buffer del componente o `ropey` si M0-S2 lo exige) + versión + dirty + EOL/encoding.
- **Lenguaje**: registrar `tree-sitter-latex` en el registry de lenguajes del editor del kit; consultas `highlights.scm` vendorizadas en `assets/tree-sitter/latex/` (con licencia); mapear capturas → tokens de tema.
- **Diagnósticos**: del `LogParser` → rangos subrayados (API del componente; fallback: gutter + tooltip + panel de problemas del shell).
- **Outline (M2)**: queries para `\part|\chapter|\section|...|\paragraph` + labels; panel lateral desplegable en la columna del árbol (tab "Outline").
- **Búsqueda/reemplazo (M1.5)**: barra superior del editor, contador, regex off por defecto, case-sensitive toggle.
- **texlab (M4, gate temprano opcional)**: proceso `texlab.exe` (zip Windows), LSP stdio, config `latex.build.enabled=false` (nosotros compilamos), `latex.forwardSearch`/`inverseSearch` desactivados (hacemos synctex nosotros). El `example-editor` del kit documenta el wiring de LSP; el ejemplo queda como referencia en el skill `lsx-editor-lsp`.

### 5.9 Portabilidad

- **Raíz portable** = directorio del exe; en desarrollo, `LATEX_STUDIO_ROOT` env o fallback a la raíz del repo. Estructura esperada:
```
latex-ide\                     ← raíz portable = raíz del repo (dev)
  latex-studio.exe             ← en dist; en dev: target\release\latex-studio.exe
  assets\  config\  tex\  agent\  workspace\  logs\  dist\  legacy\  crates\ ...
```
- Todo lo escribible vive en `config\`, `logs\` y los proyectos. Si la raíz no es escribible → fallback documentado a `%LOCALAPPDATA%\latex-studio\` con aviso en UI (modo solo lectura parcial).
- Sin registro, sin PATH del sistema, sin DLLs externas (GPUI usa DirectX/DirectWrite del SO; hayro es puro Rust).
- Cierre: job object mata hijo(s); flush de settings; backup de buffers sucios a `config\backups\`.

---

## 6. Calidad

### 6.1 Estrategia de tests

| Nivel | Qué | Herramientas |
|---|---|---|
| Unit (rápido, core) | LogParser (corpus), SynctexParser y conversión de unidades, Settings serde/round-trip, resolución de raíz portable (rutas con espacios/acentos/UNC), cache LRU, parser SSE, reducer de chat (fixtures de eventos), filtro de árbol | `cargo test`, `insta` para snapshots de parseo |
| Integración engine | Compilar `fixtures/projects/cv` de verdad con latexmk (env-gated `LSX_TEST_TEXLIVE=1`), asserts: PDF existe/páginas, synctex forward↔inverse round-trip a ±2 pt, tlmgr detecta paquete faltante | `nextest` + features |
| Integración chat | `wiremock` sirviendo grabaciones SSE/REST; estados del supervisor; reconexión | `wiremock` |
| PDF golden | Render de fixtures (`cv.pdf`, documento con figuras/tablas/fuentes no embebidas) → PNG vs referencias (tolerancia perceptual simple) + no-blank + page count | hayro + script `xtask pdf-golden` |
| UI (GPUI) | `TestAppContext`: dock serializa/restaura, atajos, estados del shell, editor: teclas simuladas → buffer/cursor; accesibilidad básica (patrón del kit: `docs/ACCESSIBILITY-UI-TESTING.md`) | test-support de GPUI (del pin `gpui-pre`) |
| Manual E2E | Guion de 12 pasos por hito (`docs/qa/e2e.md`): abrir proyecto, editar, compile-on-save, error → IA → fix, syncTeX ambos sentidos, permiso de edición, kill del server y reconexión, USB-simulación (ruta distinta) | checklist QA agent |

### 6.2 Gates (obligatorios para cada tarea; en CI y local)

```
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace            (unit; integración según env)
cargo deny check licenses bans sources
taplo fmt --check **/Cargo.toml   ·   typos
```
DoD por tarea: código + tests + docs afectadas + gates verdes + sin `todo!()`/`unwrap()` en rutas de producción (salvo justificado) + captura/GIF para cambios visibles + entrada en `docs/board.md`.

### 6.3 Presupuestos de rendimiento (QA los mide en cada gate de hito)

| Métrica | Objetivo |
|---|---|
| Arranque en frío hasta ventana pintada | < 1.2 s |
| RAM idle (1 .tex + PDF 1 página abiertos, sin chat) | < 250 MB |
| RAM con chat + PDF 50 páginas | < 450 MB |
| Latencia tecla → pintado (10k líneas) | < 16 ms |
| Render página PDF típica 100 % (150 dpi) | < 120 ms (caché miss) |
| Ciclo guardar→PDF actualizado (CV 1 pág, máquina de referencia) | < 2.5 s |
| Forward/inverse SyncTeX (respuesta UI) | < 300 ms |

### 6.4 Observabilidad

- `tracing` → `logs\app.log` (rotación diaria, 7 días) + `logs\crashes\` con panic hook (backtrace + versión + config sanitizada).
- Inspector GPUI (feature dev), `fps_monitor` example adaptado como overlay de dev (`F12`).
- Botón "Copiar diagnóstico" en Ajustes→Avanzado (versiones, rutas, estado server, últimas 200 líneas de log) — sin secretos.

---

## 7. Riesgos y mitigaciones

| # | Riesgo | Prob. | Impacto | Mitigación / fallback | Gate |
|---|---|---|---|---|---|
| R1 | Breaking changes de GPUI snapshots | Alta | Medio | Pin exacto `=0.3.9` del snapshot, upgrades como tarea dedicada, `[patch]` vendored si bug bloqueante; crates `lsx-*` sin UI preservan el valor ante un cambio de framework | Mensual |
| R2 | El editor del kit no cubre alguna necesidad (multi-pestaña con modelo de archivo, rangos de diagnóstico, custom grammar LaTeX) | Media | Alto | M0-S2 responde con spike real sobre `cv.tex`; fallbacks: registry de lenguajes propio (tree-sitter directo + `TextRun`s), o capa `lsx-editor` propia sobre `Input/Textarea` conservando el resto del shell | G0 |
| R3 | Rendimiento PDF (hayro sin optimizar) | Media | Medio | Re-render progresivo, caché agresiva, medir con `criterion`; fallback: `pdfium-render` (DLL nativa, licencia BSD) tras el mismo trait `PdfRender` — la idea de un trait de render PDF ya está en `lsx-pdf` | G0/G1 |
| R4 | Formatos de `synctex.exe` distintos entre versiones TeX Live | Baja | Medio | Parser tolerante + fixtures de la versión embebida; fallback parser nativo M4 | G1 |
| R5 | Deriva de la API opencode (v1→v2) | Media | Medio | Aislamiento en `protocol.rs`, fixtures grabadas, detección de versión al conectar (`/global/health.version`), degradación elegante | Continuo |
| R6 | AGENTS.md/instrucciones del producto contaminadas por el `AGENTS.md` del repo de desarrollo al estar los proyectos dentro del repo | Media | Bajo | Test explícito en M1 (proyecto en `workspace\` recibe solo instrucciones del proyecto); plan B: mover default de proyectos a `%USERPROFILE%\Documents\LaTeX Studio\` o usar config opencode dedicada con `instructions` explícitas | G1 |
| R7 | Compilación concurrente/estados raros (latexmk pegado, PDF bloqueado) | Media | Medio | Single-flight + timeout + cancelación + rename atómico del PDF por latexmk; matar job object | G1 |
| R8 | Windows: rutas largas/acentos/antivirus bloqueando temp+rename | Baja | Medio | `dunce`/paths absolutos, retry con backoff en rename, tests con ruta con acentos y espacios | G1 |
| R9 | Deriva de dependencias del kit (tamaño de binario, duplicación) | Media | Bajo | `cargo tree -d` revisado en gate; features mínimas | Mensual |
| R10 | Alcance: parálisis por detalles de UI | Alta | Alto | Este plan + DoD + gates por hito; todo lo no listado va al backlog M4 sin discusión | Continuo |

---

## 8. Licencias y terceros

| Componente | Licencia | Uso |
|---|---|---|
| Código propio (`crates\*`) | MIT | — |
| GPUI / gpui-pre / gpui-kit / gpui-component | Apache-2.0 | Dependencia |
| hayro | MIT OR Apache-2.0 | Dependencia |
| tree-sitter, tree-sitter-latex | MIT | Dependencia + queries vendorizadas (mantener aviso) |
| ropey, notify, rfd, tokio, reqwest, etc. | MIT/Apache | Dependencia (`cargo deny` valida) |
| opencode.exe | MIT | Binario redistribuido en `agent\` |
| TeX Live / TinyTeX | LPPL / GPL (componentes) | Redistribuido como carpeta `tex\` tal cual, sin modificar |
| synctex.exe | GPL (TeX Live) | Invocado como proceso separado |
| texlab (M4) | GPL-3.0 | Solo binario separado con `THIRD-PARTY-LICENSES` + oferta de fuente; nunca enlazado |
| Fuentes (Inter, JetBrains Mono) | OFL | Bundle en `assets\fonts\` con sus licencias |
| Iconos Lucide (vía kit) | ISC | — |

Regla: **nunca copiar código GPL** (zorite/gpui-pdf/ratex-gpui se leen solo como referencia). `cargo deny` bloquea licencias no permitidas (allowlist: MIT, Apache-2.0, BSD-3-Clause, ISC, OFL-1.1, Zlib, Unicode-3.0).

---

## 9. Distribución

- **Modo desarrollo**: `cargo run -p latex-studio` con raíz = repo (env `LATEX_STUDIO_ROOT` opcional).
- **`xtask dist`** (M3): compila release, ensambla `dist\latex-ide\`:
```
dist\latex-ide\
  latex-studio.exe  assets\  config\ (defaults)  README.txt  LICENSES\
  tex\     ← copiada o enlazada desde la raíz del repo (variante full)
  agent\   ← opencode.exe + opencode.json
  workspace\ (vacío con plantilla de ejemplo cv\)
```
  Genera `latex-studio-<ver>-portable-full.zip` (~1.5–2 GB con tex\) y `-app.zip` (solo exe+assets, ~25 MB; para quien ya tiene la distro).
- **Instalador Inno** (M3, opcional): adaptar `legacy\installer\latex-ide.iss` a los nuevos ejecutables/paths; por usuario, sin admin.
- **Primer arranque**: si falta `tex\` → aviso con enlace a la distro; si falta `agent\opencode.exe` → chat deshabilitado con explicación (el resto funciona).
- Firmado: no (queda en backlog; SmartScreen puede avisar la primera vez).

---

## 10. Hitos

| Hito | Contenido | Gate de salida (resumen) |
|---|---|---|
| **P0 Preflight** | Git init, backup, `legacy\`, esqueleto de crates que compila, CI, skills instaladas, fixtures iniciales, board en repo | CI verde vacío + `legacy\` verificado + skills disponibles |
| **M0 Spikes + interfaces** | 6 spikes con demos (shell/dock, editor TS, PDF hayro, chat opencode, engine+log+synctex, fs/árbol); `INTERFACES.md` congelado; decisiones de gate registradas | G0: 6 demos + interfaces firmadas + sin bloqueantes R2/R3 |
| **M1 Núcleo usable** | Árbol + pestañas + guardar/watcher; auto-compile + problemas + recarga PDF; SyncTeX ↑↓; temas; config persistente; sesión restaurada | G1: E2E 1–8 en verde, presupuestos §6.3 medidos, QA sign-off |
| **M2 IA completa + pulido** | Chat completo (sesiones, permisos, diffs, modelos, fix-with-AI), buscar/reemplazar, outline, thumbnails PDF, tema claro, palette, ajustes UI | G2: E2E 9–12 en verde + QA adversarial |
| **M3 Distribución** | `xtask dist`, zips, instalador opcional, READMEs, demo, accesibilidad, limpieza legacy (decisión usuario) | G3: instalación limpia desde zip en carpeta nueva + arranque sin red OK |
| **M4 Backlog** | texlab LSP, Tectonic, spellcheck, i18n en, Linux CI, updater, selección PDF | Por demanda |

Detalle de tareas, dependencias y criterios: `TAREAS.md`. Protocolo del swarm: `SWARM.md`.

---

## 11. Apéndice — Fuentes de investigación (verificadas 2026-10-17)

- OpenCode Server API (endpoints/SSE/auth): https://opencode.ai/docs/server/ · Skills: https://opencode.ai/docs/skills/
- GPUI (Zed) README y estado: https://github.com/zed-industries/zed (`crates/gpui/README.md`)
- GPUI Kit: https://github.com/longbridge/gpui-kit (17.8k★) · docs https://gpui-kit.com · skills en `skills/` del repo
- Ficha crates: `gpui-kit 0.7.3` → depende de `gpui-pre =0.3.9` (snapshot zed@93d7456) · `hayro 0.8.0` · `tree-sitter 0.27` · `ropey 1.6.1` · `notify 8.2` · `rfd 0.17` · `egui 0.37` (descartado) · `iced 0.14` (descartado)
- Componentes del kit verificados en `crates/component/src`: `dock/`, `tree.rs`, `diff/`, `highlighter/`, `bubble.rs`, `message_scroller.rs`, `virtual_list.rs`, `command/`, `tab/`, `input/`, `text/`, `notification`, `theme`, `title_bar.rs`, `status_bar.rs`, `inspector.rs`, más `examples/editor` (tree-sitter + LSP), `examples/stream-markdown`, `examples/dock`
- zorite (referencia de patrones, GPL-3.0): https://github.com/packetThrower/zorite (`gpui-pdf`, `ratex-gpui`, packaging winget, i18n)
- hayro: https://github.com/LaurenzV/hayro (limitación confirmada: sin selección/búsqueda de texto)
- tree-sitter-latex (MIT): https://github.com/tree-sitter/tree-sitter-latex y variante latex-lsp
- texlab v5.26 (binarios Windows en releases): https://github.com/latex-lsp/texlab
- Distro anterior (reutilizada): `tex\` TinyTeX/TeX Live 2026 (`latexmk.exe`, `synctex.exe`, `biber.exe`, `tlmgr`) y `agent\opencode.exe 1.18.30`
