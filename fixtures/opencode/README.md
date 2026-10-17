# Fixtures de opencode (M0-S4)

Grabaciones **reales** del stream SSE de `opencode serve` (binario del bundle,
1.18.30). Inmutables una vez usadas: nuevas grabaciones = ficheros nuevos.

## Formato contractual (`.jsonl`)

Una línea JSON por evento SSE, forma **congelada para `cargo xtask chat-replay`**:

```json
{"event": "session.idle", "data": {"id": "evt_…", "type": "session.idle", "properties": {"sessionID": "ses_…"}}}
```

- `event`: tipo del evento (`data.type`). Si el `data:` no parseara JSON:
  `"unparsed"` y `data` sería el string crudo.
- `data`: objeto JSON completo tal como llegó en la línea `data:` del SSE
  (re-serializado por `serde_json`, claves ordenadas alfabéticamente).
- El server 1.18.30 **no** emite líneas `event:`/`id:`; el framing es solo
  `data: {json}\n\n` + `server.heartbeat` periódicos. **No hay soporte de
  `Last-Event-ID`** (probado: re-conectar con la cabecera ignora los eventos
  perdidos → recuperar con `GET /session/:id/message`).

## Ficheros

| Fichero | Contenido |
|---|---|
| `spike-<YYYYMMDD-HHMMSS>.jsonl` | Grabación completa (todos los eventos del stream durante la sesión, incluido ruido de arranque: `plugin.added`, `catalog.updated`…) |
| `spike-<…>-mini.jsonl` | Derivado: eventos de la sesión + `server.connected`/`server.heartbeat`, sin tipos de ruido; es el que consumen los tests |

## Regenerar

```powershell
cargo run -p lsx-chat --bin chat-record            # prompt trivial + permiso de edición (reject)
cargo run -p lsx-chat --bin chat-record -- --no-permission
cargo run -p lsx-chat --bin chat-record -- --model opencode-go/mimo-v2.6-flash
```

El binario arranca el server en `fixtures/projects/cv` (puerto 14096, rango
14000–14100), responde `reject` al permiso de edición para no tocar fixtures,
mata el server al terminar y verifica que no queden procesos.

## Grabación actual

- `spike-20261017-123649.jsonl` (+ `-mini`): sesión `spike-s4`, modelo
  `opencode-go/gpt-5.6-luna`; turno 1 trivial («Responde exactamente: hola» →
  `hola`); turno 2 `apply_patch` → `permission.asked` (edit, con
  `metadata.diff`) → `reject` → estado `error` de la tool.
