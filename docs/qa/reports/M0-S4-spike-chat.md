# Spike M0-S4 — Chat opencode (REST + SSE + permisos)

- Fecha: 2026-10-17 · Sesión: `ses_eb6356f5affeYRRCp2uc8uotkm`
- **ESTADO: OK** (demo funcional, fixtures SSE reales, tabla I-4 congelada en código, 21 tests verdes, 0 procesos huérfanos)

## Entregable

- `crates/chat/src/` — `protocol.rs` (API v1 + `SseRecord`), `sse.rs` (parser incremental), `events.rs` (**tabla I-4 en código**), `supervisor.rs` (spawn/health/stop), `client.rs` (REST+SSE), `recorder.rs`, `replay.rs`, `spike.rs`, `error.rs`
- `crates/chat/src/bin/chat-record.rs` — grabador de fixtures
- `crates/chat/tests/{sse_parser,replay_fixture}.rs` — 9 + 2 tests
- `app/src/bin/spike-chat.rs` — demo UI (+ modo `LSX_SPIKE_AUTORUN`)
- Fixtures: `fixtures/opencode/events/spike-20261017-123649.jsonl` (169 líneas) + `…-mini.jsonl` (120) + `fixtures/opencode/README.md` (formato contractual)
- Desviación: `app/Cargo.toml` (+`lsx-chat`, +`tokio`) → reconciliar en consolidación

## Demo

```powershell
cargo run -p lsx-chat --bin chat-record          # grabar fixtures (--model, --port, --no-permission)
cargo run -p latex-studio --bin spike-chat       # demo UI: iniciar sesión → enviar → streaming → parar
```
E2E real: `LSX_SPIKE_AUTORUN=1` → `SPIKE-AUTORUN-OK: streaming + session.idle en la entidad UI`. Puerto 14096; cierre limpio verificado.

## I-4 — Tabla exacta SSE → dominio (congelada)

Envoltorio: `{id:"evt_…", type, properties}`; el stream emite **solo `data: {json}`** (sin `event:`/`id:`).

| Evento SSE | → variante | Campos clave de `properties` |
|---|---|---|
| `server.connected` | ServerConnected | `{}` (primero; también tras reconexión) |
| `server.heartbeat` | Heartbeat | `{}` |
| `session.created` / `session.updated` | SessionCreated/Updated | `sessionID`, `info{id,slug,title,projectID,directory,path,agent?,model{id,providerID,variant?},version,cost,tokens,time{created,updated}}` |
| `session.deleted` | SessionDeleted | `sessionID` |
| `message.updated` | MessageUpdated | `sessionID`, `info{id,role:user\|assistant,time{created,completed?},modelID?,providerID?,agent?,mode?,path{cwd,root},cost,tokens,error?}` (**resultados de tools llegan como role=user**; `time.completed` = cerrado) |
| `message.part.updated` | PartUpdated{part} | snapshot completo de `part{id,messageID,type,…}` (nace vacía, se re-emite final) |
| `message.part.delta` | PartDelta | `sessionID,messageID,partID,field:"text",delta` (**sin tipo de parte** → correlacionar con `part.updated`; 72 en la grabación) |
| `message.part.removed` | PartRemoved | `sessionID,messageID,partID` |
| `permission.asked` | PermissionAsked | `id:"per_…",sessionID,permission:"edit",patterns:[…],metadata{filepath,diff:unified,files[{relativePath,patch,additions,deletions}]},always["*"],tool{messageID,callID}` |
| `permission.replied` | PermissionReplied | `sessionID,requestID,reply:"once\|always\|reject"` |
| `session.status` | SessionStatus | `sessionID,status{type:"busy"\|"idle"\|"retry"}` |
| `session.idle` | SessionIdle | `sessionID` (fin de turno) |
| `session.error` | SessionError | `sessionID,error{…data.message}` |
| `session.diff` | SessionDiff | `sessionID,diff:[…]` (vacío con reject) |
| `file.edited` | FileEdited | `file` |
| `plugin.added`(×45), `catalog.updated`, `reference.updated`, `integration.updated` | Ignored | ruido de arranque (excluido del mini) |

**Partes (`part.type`)**: `text{text,time{start,end?}}` (end⇒final); `reasoning{text,time}` (los deltas también `field:"text"` → distinguir por partID); `tool{tool,callID,state{status:pending→running→completed|error,input,title?,output?,error?,metadata?,time}}`; `step-start`/`step-finish{reason,cost,tokens}`; `file{mime,filename,url,source?}`.

**Flujo de permiso (evidencia real)**: tool pending→running → `permission.asked` (JSON completo) → `POST …/permissions/:id {"response":"reject"}` → `true` → `permission.replied` → tool `status:"error"`. Tool observada: `apply_patch` (**no hardcodear**) → M1-E4: grabar un `once` real.

## Otras respuestas

- **I-5**: instancia global viable; `POST /session?directory=<abs>` crea sesiones por proyecto sin reiniciar; `GET /event` global etiquetado por `sessionID`.
- **Reconexión**: `Last-Event-ID` **no soportado** → gap recovery = `GET /session/:id/message?limit=` (`refetch_history`).
- **Modelos**: `opencode-go` default `gpt-5.6-luna` (~30 modelos); `/doc` = OpenAPI 3.1 (478 KB, fuente de verdad).

## Riesgos / TODO

1. `xtask chat-replay` stub → cablear con `lsx_chat::replay::{replay_file, find_latest}` (orchestrator).
2. §5.2/§5.3 de INTERFACES + skill `lsx-opencode-api` → volcados (hecho en esta consolidación).
3. Job object kill-on-close → M1-E1.
4. Basic auth soportado pero no ejercitado live.
5. Restart/backoff y refetch de historial: diseñados, sin prueba live (fuera del alcance mínimo).
