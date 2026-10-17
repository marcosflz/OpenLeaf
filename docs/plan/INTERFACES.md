# INTERFACES — Contratos congelados (seed M0)

> Estas interfaces son el **borrador normativo** que el orchestrator congela al final de M0 (gate G0).
> Todo cambio posterior exige ADR en `docs/decisions/` y actualización de este documento.
> Los firmas son Rust indicativo (nombres definitivos al implementar, semántica no negociable).

---

## 1. Grafo de crates y módulos

```
crates/
  core/      lsx-core      : config · root · project · events · fs · errors · log
  latex/     lsx-latex     : engine · texlive · queue · logparse · synctex · tlmgr
  pdf/       lsx-pdf       : document · render · cache · coords
  chat/      lsx-chat      : supervisor · protocol · client · reducer · prompt
  editor/    lsx-editor    : doc · languages(tree-sitter) · diagnostics · find · lsp(M4)
  ui/        lsx-ui        : app · shell(dock) · panels/{tree,editor,pdf,chat} · theme · settings · palette · shortcuts
app/         latex-studio  : main.rs (wiring) · cli
xtask/       xtask         : dist · fixtures · pdf-golden · bench
```

Reglas de dependencia (verificadas en review/CI):
`lsx-core` ← {`lsx-latex`, `lsx-pdf`, `lsx-chat`, `lsx-editor`} ← `lsx-ui` ← `latex-studio`.
`lsx-core` prohibido: gpui, tokio, reqwest, winit. `lsx-latex|pdf|chat` prohibido: gpui.
`lsx-editor` y `lsx-ui` sí dependen de `gpui-kit`.

---

## 2. lsx-core

```rust
// root.rs — detección de la raíz portable
pub struct PortableRoot(PathBuf);
impl PortableRoot {
    /// Orden: env LATEX_STUDIO_ROOT → dir del exe → raíz del repo (dev, CARGO_MANIFEST_DIR/../..).
    pub fn detect() -> Result<Self, RootError>;
    pub fn tex_dir(&self) -> PathBuf;        // <root>/tex
    pub fn latexmk(&self) -> PathBuf;        // tex/bin/windows/latexmk.exe
    pub fn synctex_exe(&self) -> PathBuf;    // tex/bin/windows/synctex.exe
    pub fn agent_dir(&self) -> PathBuf;      // <root>/agent
    pub fn opencode_exe(&self) -> PathBuf;   // agent/opencode.exe
    pub fn opencode_config(&self) -> PathBuf;// agent/opencode.json
    pub fn config_dir(&self) -> PathBuf;     // <root>/config
    pub fn logs_dir(&self) -> PathBuf;       // <root>/logs
    pub fn default_workspace(&self) -> PathBuf; // <root>/workspace
    pub fn is_writable(&self) -> bool;
}

// config.rs — config/settings.json (defaults completos en §7)
pub struct Settings { /* ver §7 */ }
impl Settings {
    pub fn load(root: &PortableRoot) -> (Self, Vec<ConfigWarning>); // nunca falla: defaults + warnings
    pub fn save(&self, root: &PortableRoot) -> Result<()>;          // atómico temp+rename
}

// project.rs
pub struct Project { pub root: PathBuf, pub name: String, pub recent: Vec<PathBuf> }
pub struct ProjectService { /* open/close/recent/create-from-template(list templates) */ }

// fs.rs
pub struct FsService;
impl FsService {
    pub fn list_dir(&self, rel: &Path) -> Result<Vec<Entry>>; // Entry{name, kind, size, mtime, ignored}
    pub fn read_text(&self, rel: &Path) -> Result<TextFile>;  // TextFile{text, eol, had_bom, encoding}
    pub fn write_text_atomic(&self, rel: &Path, text: &str) -> Result<()>;
    pub fn create/rename/delete(...) -> Result<()>;
    pub fn watch(&self, on_event: impl Fn(FsEvent) + Send + 'static) -> WatcherGuard; // debounce 200ms
}
pub enum FsEvent { Created(PathBuf), Removed(PathBuf), Modified(PathBuf), Renamed(PathBuf, PathBuf) }

// events.rs — bus central (async_channel::broadcast o fan-out manual)
pub enum AppEvent {
    Compile(CompileEvent), Pdf(PdfEvent), Chat(ChatEvent), Fs(FsEvent), Ui(UiEvent),
}
pub struct EventBus { /* publish/subscribe tipado */ }

// errors.rs — thiserror; código LSX-....
pub enum RootError { NotFound, NotWritable(PathBuf) }
// Regla: mensajes en español orientados a usuario; detalles técnicos en `source` y tracing.
```

---

## 3. lsx-latex

```rust
pub enum EngineId { TexLive, Tectonic /* M4 */ }

pub struct CompileRequest {
    pub project: PathBuf,
    pub main: PathBuf,               // absoluto
    pub args: Vec<String>,           // default: -synctex=1 -interaction=nonstopmode -file-line-error -pdf
    pub timeout: Duration,           // default 600s
    pub clean: bool,                 // F5 limpio
}

pub enum CompileStatus { Success, Failed, Cancelled, Timeout }
pub struct CompileOutcome {
    pub status: CompileStatus,
    pub duration: Duration,
    pub pdf: Option<PathBuf>,
    pub log_tail: String,            // últimas ~200 líneas
    pub diagnostics: Vec<Diagnostic>,
    pub package_installs: Vec<PackageInstall>, // intentos tlmgr
}
pub enum CompileEvent {
    Queued, Started { generation: u64 }, Progress { line: String },
    Finished { generation: u64, outcome: CompileOutcome }, Cancelled { generation: u64 },
}

pub trait LatexEngine: Send + Sync {
    fn id(&self) -> EngineId;
    fn probe(&self) -> Result<EngineInfo, EngineError>; // versión, ruta, disponible
    fn compile(&self, req: &CompileRequest, on_event: &(dyn Fn(CompileEvent)+Send+Sync), cancel: &CancelToken) -> Result<CompileOutcome, EngineError>;
}
pub struct TexLiveEngine { /* latexmk, tlmgr, synctex paths */ }

pub struct CompileQueue; // single-flight + coalescing; API: submit(CompileRequest) -> generation; cancel(generation)

// logparse.rs
pub enum DiagnosticSeverity { Error, Warning, Info }
pub enum DiagnosticKind {
    MissingPackage { package: String },
    LatexError, BibError, UndefinedRef, Overfull, RerunNeeded, Unknown,
}
pub struct Diagnostic {
    pub severity: DiagnosticSeverity, pub kind: DiagnosticKind,
    pub file: Option<PathBuf>, pub line: Option<u32>, pub column: Option<u32>,
    pub message: String, pub raw_block: String,
}
pub fn parse_log(log: &str) -> Vec<Diagnostic>;         // tolerante, cap 8MB
pub fn parse_stdout_stream(line: &str) -> Option<LogHint>; // progreso en vivo

// synctex.rs
pub enum SynctexError { NotAvailable, NoMatch, Timeout, Parse(String) }
pub struct SyncPoint { pub page: u32, pub x: f32, pub y: f32, pub w: f32, pub h: f32 } // bp, origen arriba-izq
pub struct SourcePoint { pub file: PathBuf, pub line: u32, pub column: u32 }
pub struct Synctex; // impl con el CLI
impl Synctex {
    pub fn forward(&self, exe: &Path, pdf: &Path, src: &Path, line: u32, col: u32) -> Result<SyncPoint, SynctexError>;
    pub fn inverse(&self, exe: &Path, pdf: &Path, page: u32, x: f32, y: f32) -> Result<SourcePoint, SynctexError>;
}
// Convención de unidades: bp = 1/72" (PDF), sp = TeX. 1 bp = 65536·(72.27/72) sp aprox. El CLI ya emite bp.
// Concurrencia: spawn_blocking; timeout 2s; cache del último resultado por (pdf, gen).
```

---

## 4. lsx-pdf

```rust
pub struct PdfDocument { /* hayro::Pdf + metadata */ }
impl PdfDocument {
    pub fn open(bytes: Vec<u8>) -> Result<Self, PdfError>;
    pub fn page_count(&self) -> u32;
    pub fn page_size_pt(&self, page: u32) -> Result<(f32, f32), PdfError>; // bp
    pub fn render(&self, page: u32, scale: f32) -> Result<PageBitmap, PdfError>; // scale = zoom * dpr
}
pub struct PageBitmap { pub width: u32, pub height: u32, pub scale: f32, pub rgba: Vec<u8> }

pub struct PdfCache; // LRU por bytes (default 256MB); key: (generation, page, scale_bucket); prioriza visibles

// coords.rs — UNA sola implementación para todos los mapeos
impl PageBitmap {
    pub fn pdf_pt_to_px(&self, x_pt: f32, y_pt: f32) -> (f32, f32);  // origen arriba-izq
    pub fn px_to_pdf_pt(&self, x: f32, y: f32) -> (f32, f32);
}
pub const BP_PER_INCH: f32 = 72.0;
// Tests: round-trip ±0.01px; caso con page_rotation (hayro) documentado y cubierto.

pub trait PdfRender: Send + Sync { /* renders futuros: hayro hoy, pdfium M4 */ }
```

---

## 5. lsx-chat (opencode)

### 5.1 Ciclo de vida

```rust
pub struct ChatConfig {
    pub opencode_exe: PathBuf, pub config_json: PathBuf,
    pub project_cwd: PathBuf, pub port: Option<u16>,  // None → puerto libre aleatorio
    pub password: String,                              // OPENCODE_SERVER_PASSWORD
}
pub enum ServerState { Stopped, Starting, Ready { version: String }, Restarting { attempt: u8 }, Error(String) }
pub struct ChatSupervisor; // start()/stop()/restart(); backoff 0.5/1/2/5s ×3; health GET /global/health
pub enum ChatCmd {
    EnsureSession { project: PathBuf },
    NewSession, SwitchSession { id: String }, DeleteSession { id: String },
    SendPrompt { text: String, context: Vec<PromptContext> },
    Abort, RespondPermission { permission_id: String, response: PermissionResponse, remember: bool },
    RefreshProviders, SetModel { provider: String, model: String }, SetAgent { name: String },
}
pub enum PermissionResponse { Once, Always, Reject }
pub enum PromptContext { CurrentFile { path: PathBuf }, Selection { path: PathBuf, range: TextRange, text: String }, DiagnosticsSummary }
```

### 5.2 Eventos de dominio (SSE crudo → `reducer` → estos)

```rust
pub enum ChatEvent {
    Server(ServerState),
    Sessions(Vec<SessionInfo>),                 // GET /session
    Message(IncomingMessage),                   // creado/completado (rol, id, modelo)
    PartUpdated(PartUpdate),                    // texto (delta o full), reasoning, tool, file
    Permission(PermissionRequest),              // {id, tool, args, diff?}
    SessionIdle { session_id: String },
    Providers(ProvidersInfo),                  // /config/providers + /provider
    Agents(Vec<AgentInfo>),                     // GET /agent
    Error { message: String, recoverable: bool },
}
pub enum PartUpdate {
    Text { message_id: String, part_id: String, markdown: String, streaming: bool },
    Reasoning { message_id: String, part_id: String, text: String, done: bool },
    Tool { message_id: String, part_id: String, name: String, state: ToolState, input_summary: String, output: Option<String>, diff: Option<DiffData> },
    File { message_id: String, part_id: String, path: PathBuf },
}
pub enum ToolState { Pending, Running, Completed, Error }
pub struct DiffData { pub path: PathBuf, pub hunks: Vec<Hunk>, pub before: String, pub after: String }
```

### 5.3 Endpoints (API v1, opencode 1.18.30) — tablas operativas

| Uso | Método y ruta | Notas |
|---|---|---|
| Salud | `GET /global/health` | `{healthy, version}`; polling al arrancar |
| Eventos | `GET /event` | SSE; primer evento `server.connected` |
| Sesiones | `POST /session`, `GET /session`, `GET /session/:id`, `PATCH /session/:id`, `DELETE /session/:id` | `PATCH {title}` |
| Historial | `GET /session/:id/message?limit=` | `{info, parts}[]` |
| Enviar | `POST /session/:id/prompt_async` | body `{ messageID?, model?, agent?, system?, tools?, parts }`; 204 |
| Abortar | `POST /session/:id/abort` | botón stop |
| Permisos | `POST /session/:id/permissions/:permissionID` | body `{response, remember?}` |
| Estado | `GET /session/status` | mapa por sesión |
| Catálogos | `GET /config/providers`, `GET /provider`, `GET /agent` | pickers |

**Tarea obligatoria M0-S4**: grabar stream real (prompt → texto → tool bash → permiso de edición) en `fixtures/opencode/events/*.jsonl` y **fijar aquí la tabla de mapeo evento→`ChatEvent`** con los nombres exactos de `type` (verificar contra `types.gen.ts` del SDK). Sin esa tabla, no se implementa el reducer.

### 5.4 Protocolo de proceso

- Args: `serve --hostname 127.0.0.1 --port {p}`; env `OPENCODE_CONFIG`, `OPENCODE_SERVER_PASSWORD`, `PATH+=tex\bin\windows;agent`.
- cwd = raíz del proyecto activo. Client: `reqwest` con basic auth (`opencode:{password}`).
- Parseo SSE propio: split por `\n\n`, campos `event:`/`data:`, `data:` multilínea concatenado; reconexión 1s/2s/5s y refetch de historial si hubo gap.
- Cierre: kill + job object; `stop()` espera 3s y fuerza.

---

## 6. lsx-editor

```rust
pub struct DocId(pub u64);
pub struct Document { pub id: DocId, pub path: PathBuf, pub version: u64, pub dirty: bool, pub eol: Eol, pub encoding: Encoding }
pub struct EditorState { pub docs: Vec<Document>, pub active: DocId, pub cursors: Vec<Cursor>, pub scroll: ScrollState, pub find: Option<FindState> }
pub enum EditorIntent { /* input model → gpui component o implementación propia (M0-S2 decide) */ }

// languages.rs
pub struct LanguageRegistry; // registra tree-sitter-latex + queries highlights.scm (assets/tree-sitter/latex/)
pub const LATEX_HIGHLIGHT_QUERY: &str = include_str!("../../../assets/tree-sitter/latex/highlights.scm");

// diagnostics.rs — mapea Diagnostic (lsx-latex) → rangos del buffer
pub fn map_diagnostics(doc: &Document, diags: &[Diagnostic]) -> Vec<InlineMarker>; // {line, col, len, severity, message}
```

Reglas: el editor **nunca** escribe en disco directamente — emite `EditorIntent::Save` que `lsx-core::FsService` ejecuta atómicamente; la UI escucha `FsEvent` para recargar/avisar conflictos (dirty + cambio externo → modal 3 opciones: recargar, sobrescribir, ver diff).

---

## 7. `config/settings.json` — esquema (v1)

```json
{
  "$schemaVersion": 1,
  "project": { "last": "workspace/cv", "recent": ["workspace/cv"] },
  "editor": {
    "fontFamily": "JetBrains Mono", "fontSize": 14.0, "tabSize": 2,
    "softWrap": false, "autosave": { "enabled": false, "intervalSecs": 30 },
    "showWhitespace": false
  },
  "latex": {
    "engine": "texlive", "mainFile": null, "autoCompile": true, "debounceMs": 400,
    "autoInstallPackages": true, "extraArgs": [], "timeoutSecs": 600, "cleanOnSuccess": false
  },
  "pdf": { "zoomMode": "fit-width", "zoom": 1.0, "maxCacheMB": 256, "openExternalOnDoubleClick": true },
  "ai": {
    "autoStart": true, "port": null, "model": null, "agent": "build",
    "permissionPreset": "ask", "showReasoning": true, "promptTemplatesDir": "assets/prompts"
  },
  "ui": { "theme": "dark", "accent": "#4C8DFF", "language": "es", "dock": {}, "reducedMotion": false },
  "advanced": { "logLevel": "info", "texBinOverride": null, "opencodeExeOverride": null }
}
```
- Rutas relativas a la raíz portable; separadores `/` en JSON (normalizados al cargar).
- `dock` = blob serializado del layout (versión incluida; si no parsea → layout por defecto sin error).
- Escritura atómica + validación por campo (rangos); campos desconocidos se preservan (forward-compat).

**Layout dock (forma estable para tests)**: 4 hojas en split horizontal `[tree, editor, pdf, chat]`, cada una serializa `{id, kind, width_fraction, collapsed}`; focus mode guarda `{hidden: [...], previous_fractions}`.

---

## 8. Flujo de datos UI ↔ servicios (regla de oro)

```
UI (gpui entities) ── ChatCmd/CompileRequest/FsIntent ───► servicios (tokio thread)
UI ◄── AppEvent (async_channel) ◄── servicios        ◄── reducer por panel: (state, event) → state
```
- Los paneles son **reducers puros** sobre `AppEvent` + estado propio; el render no decide nada.
- Un panel nunca llama I/O directo: emite intents.
- Los eventos llevan `generation`/`doc_id` para descartar mensajes obsoletos (p.ej. PDF viejo tras recompilar).

---

## 9. Convenciones

| Tema | Regla |
|---|---|
| Unidades | pt = bp (PDF) para SyncTeX y geometría; px solo en UI final; escala = zoom × dpr |
| Origen | PDF/SyncTeX: arriba-izquierda; nunca asumir bottom-left |
| Rutas en config | relativas a raíz portable, `/` |
| Rutas en código | `camino::Utf8PathBuf`; Windows largas (`\\?\`) solo donde el SO obligue |
| Encoding | UTF-8 sin BOM (los BOM se preservan si existen); otros encodings → aviso y edición deshabilitada v1 |
| EOL | detectar y preservar LF/CRLF por documento |
| Tiempo | `std::time::Duration`; logs en UTC ISO-8601 |
| Capas de log | `tracing` con targets `lsx::<crate>`; nivel por módulo desde settings |
| Secretos | Nunca en config/logs; password del server generado por sesión en memoria |
| Nombres de archivos | `snake_case` módulos; componentes UI `PascalCase`; eventos `sujeto.acción` en tracing |
| i18n | Claves `ui.<panel>.<elemento>`; v1 solo `es` (estructura multi-idioma desde el inicio con rust-i18n, como el kit) |
| Fixtures | `fixtures/{logs,synctex,pdf,opencode,projects}`; inmutables; nuevas versiones = nuevos archivos |

---

## 10. Fixtures iniciales (se crean en P0 y M0)

| Fixture | Origen | Usado por |
|---|---|---|
| `projects/cv/` | copia de `workspace\cv` (template.tex + imagen) | engine/E2E |
| `projects/broken/` | falta `lastpage.sty`, control sequence inválido, biber roto | logparse/auto-install |
| `logs/*.log` | capturas reales de los 4 tipos de error | logparse (insta snapshots) |
| `pdf/cv.pdf` + `pdf/figures.pdf` | compilados con latexmk | pdf golden |
| `synctex/*.synctex.gz` + `expected.json` | par de queries grabadas | synctex |
| `opencode/events/*.jsonl` | stream SSE real grabado en M0-S4 | reducer chat |
| `settings/*.json` | válidos, corruptos, con campos futuros | config |

---

## 11. Decisiones de interfaz registradas (resolver en M0, gate G0)

| # | Pregunta | Default del plan | Gate |
|---|---|---|---|
| I-1 | ¿El editor del kit expone modelo de archivo + rangos de diagnóstico + registro de lenguajes? | Sí (demo spike S2) | G0 |
| I-2 | ¿Usamos `RenderImage` directo o el componente pdf del kit (no hay) → render propio? | Render propio `lsx-pdf` + `RenderImage` | G0 |
| I-3 | ¿Buffer propio con ropey o el del componente? | El del componente; ropey solo si falla | G0 |
| I-4 | Nombres exactos de eventos SSE y campos de `parts` | Tabla §5.3 congelada con fixtures | G0 |
| I-5 | Puerto/instancia de opencode por proyecto o global | Global por app, cwd = proyecto activo | G0 |
| I-6 | ¿`gpui-fast`? | No en v1 | G0 |
| I-7 | ¿Dock como layout fijo 4 o dockable libre? | Dock del kit con 4 hojas fijas (sin drag de pestañas entre paneles en v1) | G0 |

---

## 12. Hallazgos de spikes M0 (se consolidan al congelar en G0)

### M0-S3 — PDF (hayro 0.8 + RenderImage) — VERIFICADO (2026-10-17)

- `Pdf::new(data)` (error sin Display/Error — envolver); `pdf.pages()` → `len()/get(i)`; `Page::render_dimensions() -> (f32,f32)` (rotación aplicada).
- `hayro::render(page, &RenderCache, &InterpreterSettings, &RenderSettings, &PixmapSettings) -> vello_cpu::Pixmap`; `PixmapSettings { x_scale, y_scale, bg_color }` (bg BLANCO = papel).
- `Pixmap::data_as_u8_slice()` → RGBA8 **premultiplicado**, row-major.
- GPUI: `RenderImage::new(Frame::new(RgbaImage))` espera **BGRA** en Windows (swap en `lsx-ui`); `RenderImage::scale_factor` privado → `.w(px(w_bmp/dpr)).h(px(h_bmp/dpr))`.
- `RenderCache` NO puede vivir dentro de `PdfDocument` (invariancia `'a` por `RefCell<Font<'a>>`): cache a nivel de visor (sesión), no del documento.
- Medidas release (CV Letter 612×792 bp, página 1): p50 ≈ **4.3 / 5.6 / 7.9 ms** a escala 1.0/1.5/2.0 (reuso de cache: 2.2/4.3/6.6). Debug ≈ 91/131/180 ms. Presupuesto <120 ms ✓.
- Decisión I-2 confirmada: visor propio con `RenderImage`; `gpui-pdf` descartado (GPL + modelo ajeno).
- Pendiente para fixtures: PDF con `/Rotate` real.

### M0-S6 — FS / watcher / árbol — VERIFICADO (2026-10-17)

- `FsService::new(root)`; `list_dir(rel)` (oculta aux), `list_dir_with(rel, opts)`; `read_text` → `TextFile { text, eol: Lf|CrLf|Mixed, had_bom }`; `write_text_atomic` (temp+fsync+rename con reintentos; UTF-8 sin BOM); `create_file/create_dir/rename/delete`; `watch(on_batch: Fn(Vec<FsEvent>)+Send) -> WatcherGuard` (Drop = stop+join).
- Callback del watcher por **lote** (`Vec<FsEvent>`), debounce deslizante 200 ms (techo 1 s); paths **relativos** a la raíz del proyecto.
- Mapeo notify→FsEvent verificado (Windows ReadDirectoryChangesW); temporales `.lsx-tmp-*` se filtran y su rename → `Modified(destino)`.
- `validate_name` (reservados CON/NUL/COM1.., chars, punto/espacio final); `is_aux_artifact(name)`.
- `list_dir`: 5,62 ms / 5.000 archivos (margen ~35× sobre M1-F2).
- Tree del kit: constructor `tree(&Entity<TreeState>, render_item)` + `.context_menu(..)`; eventos SOLO `TreeEvent::Expanded/Collapsed`; selección se lee de `TreeState::selected_entry()` (**M1-F2 necesita evento propio o polling**); `set_items` resetea selección; carpetas vacías requieren placeholder `disabled`; id estable = ruta relativa con `/`.
- Desviaciones del borrador §2: callback en lote, rutas relativas, constructor con raíz, `rename` no sobreescribe, `FsError` en `fs` (módulo `errors.rs` pendiente).

### M0-S2 — Editor + tree-sitter — VERIFICADO (2026-10-17)

- **I-1**: registro de lenguajes SÍ (`LanguageRegistry::singleton().register(...)` + `register_parser_factory`); resaltado OK; diagnósticos inline SÍ (`state.diagnostics_mut()` + popover; `lsp_mut()` para providers); **NO hay modelo de archivo/pestañas**: `EditorState` = 1 documento sin path/dirty/save → M1-B1 = `tab::TabBar` + N entidades + dirty vía `InputEvent::Change`. Fallback de spans propios innecesario.
- **I-3**: buffer = el del componente (`Rope` re-export; ropey 2.0.0-beta.1 interno). ropey directo solo fuera del editor (LogParser), nunca buffer paralelo.
- **Grammar**: `tree-sitter-latex 0.1.0` (crates.io) **ROTO** (falta `scanner.c` → LNK2001). Adoptado `codebook-tree-sitter-latex = "0.6.1"` (mismo upstream, con scanner, `LANGUAGE: LanguageFn`). Pin `tree-sitter = 0.26.13` (el de gpui-component 0.7.3; 0.27 incompatible 0.x). → actualizar `[workspace.dependencies]` en consolidación (aliased como `tree-sitter-latex`).
- Query `highlights.scm` propia en `assets/tree-sitter/latex/` (la del plan da 404), validada contra `node-types.json` y compilada en test.
- Tema Default Dark sin color para `label/operator/punctuation/variable` (TODO M1-A3); captura desconocida → estilo default.
- `\titleformat{\section}` produce nodo `section` espurio (el outline M1 filtra).

### M0-S4 — Chat opencode — VERIFICADO (2026-10-17) · **tabla I-4 CONGELADA**

Envoltorio: `{id:"evt_…", type, properties}`; el SSE emite **solo `data: {json}`** (sin `event:`/`id:`).

| Evento SSE | → variante | Campos clave de `properties` |
|---|---|---|
| `server.connected` | ServerConnected | `{}` (primero; también tras reconexión) |
| `server.heartbeat` | Heartbeat | `{}` |
| `session.created` / `session.updated` | SessionCreated/Updated | `sessionID`, `info{id,slug,title,projectID,directory,path,agent?,model{id,providerID,variant?},version,cost,tokens,time{created,updated}}` |
| `session.deleted` | SessionDeleted | `sessionID` |
| `message.updated` | MessageUpdated | `sessionID`, `info{id,role:user\|assistant,time{created,completed?},modelID?,providerID?,agent?,mode?,path{cwd,root},cost,tokens,error?}` (resultados de tools llegan como role=user) |
| `message.part.updated` | PartUpdated{part} | snapshot de `part{id,messageID,type,…}` |
| `message.part.delta` | PartDelta | `sessionID,messageID,partID,field:"text",delta` (**sin tipo de parte** → correlacionar por partID) |
| `message.part.removed` | PartRemoved | `sessionID,messageID,partID` |
| `permission.asked` | PermissionAsked | `id:"per_…",sessionID,permission:"edit",patterns,metadata{filepath,diff,files[{relativePath,patch,additions,deletions}]},always["*"],tool{messageID,callID}` |
| `permission.replied` | PermissionReplied | `sessionID,requestID,reply:"once\|always\|reject"` |
| `session.status` | SessionStatus | `sessionID,status{type:"busy"\|"idle"\|"retry"}` |
| `session.idle` | SessionIdle | `sessionID` (fin de turno) |
| `session.error` | SessionError | `sessionID,error{…data.message}` |
| `session.diff` | SessionDiff | `sessionID,diff:[…]` |
| `file.edited` | FileEdited | `file` |
| `plugin.added`/`catalog.updated`/`reference.updated`/`integration.updated` | Ignored | ruido de arranque |

Partes: `text`, `reasoning`, `tool{tool,callID,state{pending→running→completed|error,input,title?,output?,error?,metadata?,time}}`, `step-start/finish`, `file`. Tool observada: `apply_patch` (no hardcodear lista).
Permiso: `permission.asked` → POST `{response}` → `permission.replied` → tool error/completado. `once` real pendiente M1-E4.
I-5: instancia global + `POST /session?directory=` por proyecto; `GET /event` global etiquetado por `sessionID`.
Reconexión: **sin `Last-Event-ID`** → refetch de historial (`GET /session/:id/message?limit=`).
Fixtures: `fixtures/opencode/events/spike-20261017-123649(. -mini).jsonl` + `fixtures/opencode/README.md`; seeds en `crates/chat/src/events.rs`; replayer `lsx_chat::replay`. Modelo del equipo: `opencode-go/gpt-5.6-luna`.

### M0-S1 — Shell / dock — VERIFICADO (2026-10-17) · **I-7 resuelto**

- Dock: `DockSkin::dock_area(id, version, window, cx)`; `DockLayout::h_split()/v_split()/tabs()` + `.child/panel/panel_view/active_index`; hojas fijas `set_locked(true)`; install `DockArea::set_center(...)`; `panel_handle<P>(..)` obligatorio para títulos.
- Colapso por hoja: **`Panel::visible(false)`** (el contenedor cede el slot); cualquier cambio debe notificar también al `DockArea` (el panel guarda `WeakEntity<DockArea>`). Dock regions sí tienen `toggle_dock`.
- Persistencia: (a) `DockArea::dump/load` (`DockAreaState` serde + `register_panel`); (b) esquema §7 `{id, kind, width_fraction, collapsed}` + `focus{hidden, previous_fractions}` con `set_split_sizes`; persistir con debounce de `DockEvent::LayoutChanged` (M1-A1).
- Tema: `Theme::change(ThemeMode::Dark, None, cx)`; acento con `Theme::update(cx, |t| t.accent = Hsla::from(rgb(0x004C8DFF)))`; tokens vía `ActiveTheme`.
- `TitleBar::new()` / `TitleBar::window_options()` / `StatusBar::new().left(..).center(..).right(..)`; titlebar OS-native en Windows. `WindowBounds::Maximized(Bounds::default())` (gpui-pre: lleva Bounds, no Option).
- **QA visual DPI**: capturas DPI-unaware a 200 % parecen 2×/recortadas — no es bug; usar captura DPI-aware o maximizada en los gates.
