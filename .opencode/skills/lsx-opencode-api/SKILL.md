---
name: lsx-opencode-api
description: Usar cuando se trabaje en la integración con opencode (stream E, crate lsx-chat) — arranque del server local con agent/opencode.exe, endpoints de la API v1, stream SSE con la tabla de eventos CONGELADA en M0-S4, flujo de permisos verificado, variables de entorno, política de restart/backoff y replayer de fixtures para probar el reducer sin gastar tokens. Fuente normativa en docs/plan/INTERFACES.md §5 y §12.
---

# opencode serve — API v1 (binario 1.18.30)

Fuente normativa: `docs/plan/INTERFACES.md` §5 y §12. Binario: `agent\opencode.exe` (1.18.30); config: `agent\opencode.json`. La app PINEA este binario; nunca usa el del PATH (PLAN §5.5).

## Arranque y ciclo de vida

- Args: `agent\opencode.exe serve --hostname 127.0.0.1 --port {p}` (puerto libre aleatorio si settings `ai.port = null`).
- cwd = raíz del proyecto activo. **I-5 resuelto (M0-S4)**: instancia global por app; `POST /session?directory=<abs>` crea sesiones por proyecto sin reiniciar; `GET /event` es global etiquetado por `sessionID`.
- Env: `OPENCODE_CONFIG=<raíz>\agent\opencode.json`, `OPENCODE_SERVER_PASSWORD=<aleatorio por sesión>`, `PATH += tex\bin\windows;agent`.
- Auth del cliente: basic `opencode:{password}` (soportado en código; no ejercitado live en el spike).
- Health: `GET /global/health` → `{healthy, version}`; polling al arrancar hasta 15 s.
- Restart: backoff 0.5/1/2/5 s ×3 → estado `Error`; `stop()` espera 3 s y fuerza; **job object kill-on-close pendiente (M1-E1)**.
- Reconexión SSE: 1 s / 2 s / 5 s. **Resuelto (M0-S4)**: el server NO emite `Last-Event-ID` → gap recovery = refetch de historial (`GET /session/:id/message?limit=`).

## Endpoints (API v1, opencode 1.18.30)

| Uso | Método y ruta | Notas |
|---|---|---|
| Salud | `GET /global/health` | `{healthy, version}` |
| Eventos | `GET /event` | SSE; primer evento `server.connected` |
| Sesiones | `POST /session?directory=<abs>`, `GET /session`, `GET /session/:id`, `PATCH /session/:id`, `DELETE /session/:id` | `POST {title}`; `PATCH {title}` |
| Historial | `GET /session/:id/message?limit=` | `{info, parts}[]` |
| Enviar | `POST /session/:id/prompt_async` | body `{messageID?, model{providerID,modelID}, agent?, system?, tools?, parts[{type:"text",text}]}`; 204 |
| Abortar | `POST /session/:id/abort` | botón stop |
| Permisos | `POST /session/:id/permissions/:permissionID` | body `{"response":"once"\|"always"\|"reject"}` → `true` |
| Estado | `GET /session/status` | mapa por sesión `{type: busy\|idle\|retry}` |
| Catálogos | `GET /config/providers`, `GET /provider`, `GET /agent` | pickers; `opencode-go` default `gpt-5.6-luna` en este equipo |
| Esquema | `GET /doc` | OpenAPI 3.1 completo (~478 KB) — fuente de verdad |

## SSE y mapeo a dominio — **tabla CONGELADA en M0-S4**

- El stream emite **solo `data: {json}`** (sin `event:`/`id:`); envoltorio `{id:"evt_…", type, properties}`. Parser incremental en `lsx-chat/src/sse.rs`; fixtures en `fixtures/opencode/events/` (formato contractual en `fixtures/opencode/README.md`).
- Tabla completa evento→dominio: `docs/plan/INTERFACES.md` §12 (M0-S4); semilla en código `lsx-chat/src/events.rs`. Claves:
  - `message.part.delta` **NO lleva tipo de parte** → correlacionar `partID` con el `part.updated` previo.
  - `message.part.updated` son **snapshots** completos de la parte (nace vacía, se re-emite final con `time.end`).
  - Fin de turno: `session.status{type:"idle"}` + `session.idle{sessionID}`.
  - Resultados de tools llegan como `message.updated role=user`; tool real observada: `apply_patch` (**no hardcodear lista de tools**).
  - Ruido a ignorar: `plugin.added`(×45), `catalog.updated`, `reference.updated`, `integration.updated`; `server.heartbeat` es keep-alive.

## Permisos (flujo completo, verificado M0-S4)

1. `permission.asked` → `{id:"per_…", permission:"edit", metadata{filepath, diff(unified), files[{relativePath, patch, additions, deletions}]}, patterns, always["*"], tool{messageID, callID}}`.
2. La UI muestra el modal (M1-A4 / M1-E4) con once / always / reject.
3. `POST /session/:id/permissions/:permissionID` `{"response": …}` → `true`; después llega `permission.replied{sessionID, requestID, reply}`.
4. Edición aprobada → `file.edited` / `session.diff` → el watcher ve el archivo → editor recarga si está limpio; prompt si dirty (PLAN §4.3).
- La config distribuye `edit: ask` (PLAN §5.5); el modal es la UX de aprobación. **Pendiente M1-E4**: grabar un `once` real (la fixture M0-S4 rechazó a propósito para no escribir en el repo).

## Probar sin gastar tokens

- Reproductor: la lógica existe en `lsx_chat::replay::{replay_file, find_latest}` (usada por `crates/chat/tests/replay_fixture.rs`); el comando `cargo run -p xtask -- chat-replay` se cablea en consolidación.
- Grabador: `cargo run -p lsx-chat --bin chat-record [--model opencode-go/gpt-5.6-luna] [--no-permission]`. Las fixtures son inmutables: grabación nueva = archivo nuevo.
- Tests de integración del supervisor: `wiremock` sirviendo REST/SSE grabados (PLAN §6.1).

## Deriva v1→v2 (riesgo R5)

- TODO el manejo queda aislado en `lsx-chat::protocol`; detectar versión al conectar (`/global/health.version`) y degradar con elegancia.
- Fallback si faltan eventos: polling de `GET /session/:id/message`.
