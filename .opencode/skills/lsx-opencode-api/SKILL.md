---
name: lsx-opencode-api
description: Usar cuando se trabaje en la integración con opencode (stream E, crate lsx-chat) — arranque del server local con agent/opencode.exe, tabla de endpoints de la API v1, stream SSE y su parseo, flujo de permisos, variables de entorno, política de restart/backoff y el reproductor de fixtures cargo xtask chat-replay para probar el reducer sin gastar tokens. Fuente normativa en docs/plan/INTERFACES.md §5.
---

# opencode serve — API v1 (binario 1.18.30)

Fuente normativa: `docs/plan/INTERFACES.md` §5. Binario: `agent\opencode.exe` (1.18.30); config: `agent\opencode.json`. La app PINEA este binario; nunca usa el del PATH (PLAN §5.5).

## Arranque y ciclo de vida

- Args: `agent\opencode.exe serve --hostname 127.0.0.1 --port {p}` (puerto libre aleatorio si settings `ai.port = null`).
- cwd = raíz del proyecto activo. I-5: instancia global por app — `TODO(M0-S4): verificar` si cambiar de proyecto exige restart del server o basta re-scopear sesiones.
- Env: `OPENCODE_CONFIG=<raíz>\agent\opencode.json`, `OPENCODE_SERVER_PASSWORD=<aleatorio por sesión>`, `PATH += tex\bin\windows;agent`.
- Auth del cliente: basic `opencode:{password}` (reqwest).
- Health: `GET /global/health` → `{healthy, version}`; polling al arrancar hasta 15 s.
- Restart: backoff 0.5/1/2/5 s ×3 → estado `Error`; `stop()` espera 3 s y fuerza; job object kill-on-close (INTERFACES §5.1 / §5.4).
- Reconexión SSE: 1 s / 2 s / 5 s; si hubo gap → refetch de historial. `TODO(M0-S4): verificar` si el server emite `Last-Event-ID` (INTERFACES §5.4).

## Endpoints (API v1, opencode 1.18.30)

| Uso | Método y ruta | Notas |
|---|---|---|
| Salud | `GET /global/health` | `{healthy, version}` |
| Eventos | `GET /event` | SSE; primer evento `server.connected` |
| Sesiones | `POST /session`, `GET /session`, `GET /session/:id`, `PATCH /session/:id`, `DELETE /session/:id` | `PATCH {title}` |
| Historial | `GET /session/:id/message?limit=` | `{info, parts}[]` |
| Enviar | `POST /session/:id/prompt_async` | body `{ messageID?, model?, agent?, system?, tools?, parts }`; 204 |
| Abortar | `POST /session/:id/abort` | botón stop |
| Permisos | `POST /session/:id/permissions/:permissionID` | body `{response, remember?}` |
| Estado | `GET /session/status` | mapa por sesión |
| Catálogos | `GET /config/providers`, `GET /provider`, `GET /agent` | pickers de modelo/agente |

## SSE y mapeo a dominio

- Parseo propio: split por `\n\n`, campos `event:` / `data:`, `data:` multilínea concatenado (INTERFACES §5.4).
- Dominio esperado (`ChatEvent`): `Server`, `Sessions`, `Message`, `PartUpdated` (text/reasoning/tool/file), `Permission`, `SessionIdle`, `Providers`, `Agents`, `Error` (INTERFACES §5.2).
- La tabla **evento SSE → `ChatEvent`** se congela en M0-S4 con fixtures reales: `TODO(M0-S4): verificar` los nombres exactos de `type` y campos de `parts` contra `types.gen.ts` del SDK y grabarlos en `fixtures/opencode/events/*.jsonl`. Sin esa tabla NO se implementa el reducer.

## Permisos (flujo completo)

1. Llega un evento de permiso → `PermissionRequest {id, tool, args, diff?}`.
2. La UI muestra el modal (M1-A4 / M1-E4) con once / always / deny (+ remember).
3. `POST /session/:id/permissions/:permissionID` body `{response, remember?}` con `PermissionResponse {Once, Always, Reject}`.
4. Edición del agente → watcher ve el archivo → editor recarga si está limpio; prompt si dirty (PLAN §4.3).
- La config distribuye `edit: ask` (PLAN §5.5); el modal es la UX de aprobación.

## Probar sin gastar tokens

- Reproductor de fixtures: `cargo run -p xtask -- chat-replay` — hoy stub que imprime `TODO(M0-S4)`; `TODO(M0-S4): verificar` implementación: lee `fixtures/opencode/events/*.jsonl` y alimenta el reducer (también usable en tests).
- Fixtures SSE aún no existen en el repo → se graban en M0-S4; después son inmutables (nuevas versiones = archivos nuevos).
- Tests de integración del supervisor: `wiremock` sirviendo REST/SSE grabados (PLAN §6.1).

## Deriva v1→v2 (riesgo R5)

- TODO el manejo queda aislado en `lsx-chat::protocol`; detectar versión al conectar (`/global/health.version`) y degradar con elegancia.
- Si el SSE no emite los eventos esperados, fallback del plan (M0-S4): mapear con polling de `GET /session/:id/message`.
