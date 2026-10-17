# M1 Stream E — Chat IA (M1-E1..E6 + integración)

- Fecha: 2026-10-17 · Sesión: `ses_eb5049c0fffeVXs8qCkzA75HM3` (stream E, ola O3)
- **ESTADO: DONE** (salvo los pendientes marcados abajo). Gates `cargo clippy -p lsx-chat -p lsx-ui --all-targets -- -D warnings` y `cargo test -p lsx-chat -p lsx-ui` verdes; build de `latex-studio` y `spike-chat` verde; humo live real verde (1 prompt trivial).

## Archivos

**`crates/chat`** (nuevo módulo de producción; el spike M0-S4 queda intacto):
- `src/job.rs` — job object kill-on-close (FFI copiada de `lsx-latex::proc`; el chat no depende de latex) + tests.
- `src/supervisor.rs` — `ServerSpec` (+`path_prepend`, `port_preference: Option<u16>`), `ServerState`, `RestartPolicy` (0.5/1/2/5 s), `ChatSupervisor` (`start`/`ensure_alive`/`stop`), puerto aleatorio, `random_password`; `RunningServer::stop` mata árbol (job; fallback `taskkill /T`).
- `src/conversation.rs` — reducer tipado: `Conversation` (sesiones, mensajes, partes, tool calls, permisos, estado de turno), `apply_stream`/`apply_history`, resumen de tools, helpers de diff/rutas.
- `src/service.rs` — `ChatService` (hilo tokio + canales `ChatCmd`/`ChatEvent`): arranque, sesiones por proyecto (`?directory=`), prompt con modelo/agente, abort, permisos, pickers; **reconexión SSE 1/2/5 s con health-check y refetch de historial**; restart del server con backoff (incluido el caso “padre zombie” visto live).
- `src/prompt.rs` — `compose_fix_prompt` (diagnósticos + cola del log + contexto), `compose_prompt_with_context`, `read_log_tail`, `last_lines`.
- `src/events.rs` — `history_events` (payload de `GET /session/:id/message` → eventos I-4).
- `src/protocol.rs` — `SessionInfo.time/display_title`, `ProviderCatalog::model_entries/pick_default_model`, `ModelEntry`, `AgentInfo`/`parse_agents`, `prompt_body_with`.
- `src/client.rs` — `GET /session`, `GET /session/:id`, `DELETE /session/:id`, `GET /agent`, `prompt_async_with`.
- `tests/conversation_fixture.rs` (fixtures reales → conversación), `tests/live_smoke.rs` (humo env-gated).

**`crates/ui`**:
- `src/panels/chat.rs` (**nuevo**, ~1.9 k líneas) — panel completo E3–E6 con el look aprobado de `spike-chat.rs`; `ChatPanelConfig`.
- `src/panels.rs` — exporta `ChatPanel`/`ChatPanelConfig`.
- `src/app.rs` — solo aditivo: `ChatSlot` (patrón exacto de `PdfSlot`), creación del panel, mapeo `AnyPanel::Chat`/`panel()`, `set_visible`; suscripciones del chat intactas.

**Otros**: `fixtures/opencode/events/m1e-edit-permission.jsonl` (derivado de la grabación 132346 para QA visual: turno de edición con tool + permiso), nota en `fixtures/opencode/README.md`. Evidencia: `shots/m1-e-replay.png` (tool card + card de permiso en el panel), `shots/m1-e-replay-conv.png` (conversación larga), `shots/m1-e-live.png` (live, dot “IA lista”, catálogo real).

## Verificación

- `cargo test -p lsx-chat`: 38 unit + 2 fixtures + 9 SSE + 2 replay + 1 live (skip) — **verdes**.
- `cargo test -p lsx-ui`: 71 — **verdes**.
- `cargo clippy -p lsx-chat -p lsx-ui --all-targets -- -D warnings` — **verde**.
- `cargo build -p latex-studio --bin latex-studio --bin spike-chat` — **verde**.
- **Humo live** (`LSX_TEST_OPENCODE=1 cargo test -p lsx-chat --test live_smoke`): password basic-auth, sesión por proyecto, prompt→streaming→`session.idle`, **matar el server a mano → `Restarting{attempt:1}` → `Ready` nuevo puerto → refetch de historial**, parada limpia. 9,3 s, 1 prompt (“hola”). 0 huérfanos `agent\opencode.exe` verificados tras cada corrida.
- Capturas: `shots/m1-e-replay.png`, `shots/m1-e-replay-conv.png`, `shots/m1-e-live.png` (leídas; el look Copilot se conserva: Tú/opencode, tool rows plegables, razonamiento, card de permiso, composer con enviar/parar y chips, header sesión+agente+modelo+dot).

## API del panel (`crates/ui/src/panels/chat.rs`)

```rust
ChatPanel::new(window, cx, ChatPanelConfig) -> ChatPanel   // arranca si ai.autoStart
ChatPanelConfig::from_root(&PortableRoot, project: &Path, nombre, &AiSettings,
                           StatusHandle, editor: Option<Entity<EditorPanel>>,
                           problems: Option<Entity<ProblemsPanel>>) -> ChatPanelConfig
panel.conversation() -> &Conversation
panel.is_replay() -> bool
panel.start_server(cx)                     // arranque perezoso
panel.set_editor_context(file: Option<(String,u32)>, selection: Option<String>, cx)
panel.set_fix_context(diagnostics, log_tail, cx)
panel.request_fix(window, cx)              // «Arreglar errores con IA» (agente latex-fix)
panel.detect_main_file(root) -> Option<String>
```
Modo replay determinista: `LSX_CHAT_REPLAY=<fixture.jsonl>` (alias `LSX_SPIKE_REPLAY`), `LSX_CHAT_EXPAND=1` (alias `LSX_SPIKE_EXPAND`) despliega razonamiento/tool cards. Sin server ni tokens.

## Decisiones / desviaciones

1. **Password por sesión**: `OPENCODE_SERVER_PASSWORD` aleatorio + basic auth, verificado live (sin auth el server da 401). Ejercitado en el humo.
2. **Detección de “server muerto”**: matar al hijo que escucha deja vivo el padre de `opencode serve`; el supervisor comprueba `/global/health` y relanza el árbol completo (job terminate). Cubierto por el humo live.
3. **Liveness del transcript**: el flag `scroll_to_bottom` del kit no llega a aplicarse desde una tarea async (probado 3 variantes); el panel mantiene el comportamiento del spike aprobado (arranca arriba) y la fixture derivada `m1e-edit-permission.jsonl` permite la evidencia visual de tool+permiso sin scroll.
4. **E5 sin tocar el wiring de compilación** (regla de territorio): el panel lee diagnósticos del `ProblemsPanel` inyectado y la cola del log del disco (`<main>.log` o el `.log` más reciente); el shell solo necesita llamar `request_fix` (el botón del composer ya lo hace).
5. **Selección del editor**: `EditorPanel` no expone selección todavía → chip de selección implementado pero solo alimentable por API (`set_editor_context`); pendiente M1-B.
6. **i18n**: strings del panel en español inline (como el spike aprobado); no se tocó `locales/es.yml` (territorio A). Migrar en M2 junto a la tabla.
7. **Fixture vacía eliminada**: `fixtures/opencode/events/spike-20261017-152903.jsonl` (0 bytes, grabación incompleta de M0.5) rompía `every_recording_parses_as_jsonl`; se borró (pre-existente, no era fixture válida).

## Pendientes (fuera del territorio E o M2)

- Persistir `ai.model`/`ai.agent` elegidos en `settings.json` (hoy viven en memoria de la sesión; requiere wiring del shell).
- Cambio de proyecto en caliente: el panel mantiene el cwd del proyecto con el que se creó (igual que el editor en M1).
- `metadata.files[]` del permiso (patches por archivo) no se muestran aún; el modal usa `metadata.diff` (suficiente para 1 archivo).
- Historial de sesiones en lista lateral (M2-4) y tokens/coste en el mensaje (M2-5).
- i18n del panel (punto 6) y `assets/prompts` con hot reload (M2-6).
