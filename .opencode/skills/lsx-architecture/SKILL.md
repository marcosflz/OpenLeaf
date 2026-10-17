---
name: lsx-architecture
description: Usar cuando haya que decidir dónde vive un cambio de LaTeX Studio (crate, módulo o capa), al crear tipos, eventos o errores, al conectar UI y servicios, o al revisar la ley de dependencias entre crates. Cubre el grafo lsx-core ← lsx-latex/lsx-pdf/lsx-chat/lsx-editor ← lsx-ui ← latex-studio, el flujo intents → servicios → AppEvent → reducers y las convenciones de nombres, errores, rutas y unidades de docs/plan/INTERFACES.md. Aplica a todos los streams y a QA.
---

# Arquitectura de LaTeX Studio

Fuentes normativas: `docs/plan/INTERFACES.md` (§1, §2, §8, §9), `docs/plan/PLAN-MAESTRO.md` §5, `docs/plan/TAREAS.md` §9, `docs/plan/SWARM.md` §2.
Regla del plan: prohibido re-investigar lo ya documentado; si falta algo, se añade a INTERFACES.md y el orchestrator lo propaga.

## Grafo de crates y ley de dependencias

```
lsx-core ← {lsx-latex, lsx-pdf, lsx-chat, lsx-editor} ← lsx-ui ← latex-studio (app/)
xtask (herramientas dev) · assets/ (recursos)
```

- `lsx-core` (config, root, project, events, fs, errors, log): prohibido `gpui`, `tokio`, `reqwest`, `winit` (INTERFACES §1).
- `lsx-latex`, `lsx-pdf`, `lsx-chat`: prohibido `gpui`; producen datos puros y consumen comandos.
- `lsx-editor` y `lsx-ui`: únicos crates (con `app`) que dependen de `gpui-kit`.
- Pin único: `gpui-kit =0.7.3` → snapshot `gpui-pre =0.3.9`. Prohibido el `gpui` 0.2.x de crates.io (explosión de tipos).
- Verificación en PR: `cargo tree -p lsx-<crate>`; el CI aún no lo automatiza — revisarlo en el diff.

## ¿Dónde va este código?

| Necesidad | Va en |
|---|---|
| Raíz portable, config, proyectos, watcher, bus de eventos, errores, logging | `lsx-core` |
| latexmk, tlmgr, cola de compilación, LogParser, synctex | `lsx-latex` |
| Apertura hayro, render, caché, coordenadas | `lsx-pdf` |
| Supervisor opencode, REST/SSE, reducer de chat, prompts | `lsx-chat` |
| Documentos, tree-sitter, diagnósticos, find (LSP en M4) | `lsx-editor` |
| Dock, paneles, tema, ajustes UI, palette, atajos | `lsx-ui` |
| Wiring, CLI, arranque | `app/` (orchestrator) |
| Herramientas dev (fixtures, pdf-golden, chat-replay, bench, dist) | `xtask/` (orchestrator) |

Dudas frecuentes:
- ¿Toca dos crates? La lógica va en el crate inferior y el superior consume su API.
- ¿Cambia API pública? Nota en el PR; si afecta a otro stream → el orchestrator abre ADR (`docs/decisions/`, plantilla `ADR-000-template.md`).
- ¿Cambia un contrato congelado? PARAR: ADR + INTERFACES.md + nota a afectados; lo aplica el orchestrator.

## Flujo de datos (regla de oro, INTERFACES §8)

```
UI (entidades gpui) ── intents (ChatCmd / CompileRequest / FsIntent) ──► servicios (tokio, hilo propio)
UI ◄── AppEvent (async_channel) ◄── servicios        reducer por panel: (estado, evento) → estado
```

- Paneles = reducers puros; el render no decide nada; un panel NUNCA hace I/O directo.
- Los eventos llevan `generation`/`doc_id` para descartar mensajes obsoletos (PDF viejo, compilaciones antiguas).
- Bus central: `lsx-core::EventBus` (publish/subscribe tipado; variante interna por confirmar, ver TODO abajo).

## Convenciones clave (INTERFACES §9)

| Tema | Regla |
|---|---|
| Errores | `thiserror` en libs, `anyhow` en el binario; mensaje de usuario en español; detalle técnico en `source` y `tracing` (código `LSX-...`) |
| Rutas | `camino::Utf8PathBuf` en código; en config, relativas a la raíz portable y con `/` |
| Unidades | `pt = bp` (PDF/SyncTeX); px solo en UI; escala = zoom × dpr; origen arriba-izquierda siempre |
| Encoding/EOL | UTF-8 sin BOM (BOM preservado si existe); EOL detectado y preservado por documento |
| Logs | `tracing`, targets `lsx::<crate>`, eventos `sujeto.acción`, timestamps UTC ISO-8601 |
| Nombres | módulos `snake_case`; componentes UI `PascalCase` |
| i18n | claves `ui.<panel>.<elemento>`; v1 solo `es` (estructura multi-idioma desde el inicio) |
| Fixtures | `fixtures/**` inmutables una vez usadas; versión nueva = archivo nuevo |

## DoD y límites

- El DoD completo está en `docs/plan/SWARM.md` §3 y el checklist operativo en el skill `lsx-quality-gates`.
- Si apareces tocando otro crate o un archivo del orchestrator (ver matriz en SWARM §2): para y pide.

## TODO pendientes del plan

- `TODO(M0): verificar` la variante final del bus (`async_channel::broadcast` vs fan-out manual) al implementar `lsx-core` — INTERFACES §2 lo deja abierto.
