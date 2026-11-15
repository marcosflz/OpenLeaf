---
id: kb/architecture/chat-agente
title: El chat habla con un agente LaTeX propio (nas-adk) por stdio
status: stable
updated: 2026-11-15
source:
  - crates/chat/src/sidecar.rs
  - crates/chat/src/service.rs
  - crates/chat/src/events.rs
  - crates/ui/src/panels/chat.rs
  - crates/ui/src/ai_setup.rs
  - crates/ui/src/settings_ui.rs
  - crates/ui/src/settings_providers.rs
  - python/lsx_agent/server.py
  - python/lsx_agent/tools.py
  - python/lsx_agent/providers.py
  - scripts/get-agent.ps1
  - docs/decisions/ADR-001-agente-nas-adk.md
tags: [arquitectura, chat, agente, nas-adk, ia]
related: [kb/architecture/flujos, kb/architecture/configuracion]
verified: true
---

# El chat habla con un agente LaTeX propio (nas-adk) por stdio

## Qué

`lsx-chat` lanza `agent\python\python.exe -P -s -m lsx_agent` y le habla en NDJSON por
stdin/stdout. El agente (`python/lsx_agent`) usa [nas-adk](https://github.com/marcosflz/nas-adk)
0.3.0 para el bucle modelo ↔ herramientas, las sesiones y los proveedores. Sustituye a
opencode desde 0.3 ([ADR-001](../../decisions/ADR-001-agente-nas-adk.md)).

## Protocolo

| Dirección | Forma |
|---|---|
| app → agente | `{"id": n, "method": "session.prompt", "params": {...}}` |
| agente → app | `{"id": n, "result": ...}` o `{"id": n, "error": {"message": ...}}` |
| agente → app (evento) | `{"type": "message.part.updated", "properties": {...}}` |

Los eventos, `SessionInfo`, el catálogo (`config.providers`) y el historial
(`session.messages` → `[{info, parts}]`) conservan **la forma de la API v1 de opencode**. Por
eso la tabla I-4 de `events.rs`, el reductor y las fixtures antiguas siguen valiendo.

Métodos:
- `hello`, `config.providers`, `provider.status`, `agent.list`.
- `session.list|get|create|delete|messages`.
- `session.prompt|abort`, `permission.reply`.
- `session.revert|unrevert`.

## Ciclo de vida

- `AgentSupervisor` (`sidecar.rs`) se encarga de:
  - arrancar el proceso y esperar a `hello` (20 s);
  - crear un job object con kill-on-close;
  - relanzar con backoff 0.5/1/2/5 s;
  - parar limpio cerrando stdin.
- El **cwd del proceso es `agent/`, nunca el proyecto**: Python importa desde el cwd. Las
  sesiones guardan su `directory` y las herramientas trabajan siempre sobre él.
- El `PATH` del agente lleva `tex/bin/windows` delante (latexmk, tlmgr, kpsewhich).
- Datos:
  - `config/agent/history.db`: historial del modelo, de nas-adk;
  - `config/agent/transcript.db`: transcript visible y checkpoints;
  - `logs/agent.log`: log del agente.
- Modo desarrollo: si falta `agent/python`, se usa `python/.venv` (creado con `uv sync`).

## Herramientas y permisos

- **Lectura** (`list`, `read`, `search`, `log`, `kpsewhich`, `skill`): libres; se ejecutan en
  paralelo.
- **Escritura** (`edit`, `write`): el callback `before_tool_call` calcula el diff y lanza
  `ToolDecision.interrupt`. El turno se pausa y el agente emite `permission.asked` con
  `metadata.diff`. La UI responde `once|always|reject`, y el agente sigue con
  `ResumeEntry(approved|denied)`. Con `always`, la sesión deja de preguntar.
- **`edit`** no toca los saltos de línea del archivo: si el archivo usa CRLF y el modelo
  escribe `\n`, se adapta.
- **`compile`** ejecuta latexmk con `-file-line-error` y devuelve:
  - los errores `archivo:línea`;
  - los `.sty` que faltan;
  - las referencias sin definir;
  - la cola del log.
- **`tlmgr_install`** no pide permiso. Es lo mismo que en 0.2, donde `tlmgr *` estaba
  permitido.
- **No hay shell genérico.** Las rutas absolutas, `..`, `.git` y los nombres de credenciales
  se rechazan.
- **Modos**: `build` (todas las herramientas), `latex-fix` (instrucción de reparación mínima) y
  `ask` (solo lectura).

## Proveedores

Dos familias, como en nexus, cada una con su sección de Ajustes:

| Familia | Proveedores | Backend nas-adk |
|---|---|---|
| **Claves de API** | `openrouter`, `openai`, `anthropic`, `gemini`, `deepseek`, `lmstudio` | `openrouter`, `anthropic` y `openai-compatible` con URL fija (solo `lmstudio` la deja cambiar) |
| **Suscripciones** | `opencode` (OpenCode Go, con clave), `claude-code` (CLI `claude`) | `opencode`, `claude-code` |

- **Varios activos a la vez y sin default impuesto**: el catálogo solo incluye proveedores
  con credencial, activados y con algún modelo activo. Lleva `kind` por proveedor.
- **`config/secrets/ai.json`** por proveedor:
  `{"apiKey", "enabled", "baseUrl", "models": [{"id", "name"}]}`.
  - Sin `models` se usan los sugeridos de la tabla.
  - Una lista de cadenas es el formato antiguo y se suma a los sugeridos.
  - La clave también vale por variable de entorno.
  - `claude-code` requiere el CLI `claude` autenticado por la persona; es de uso personal y
    local.
- **Predeterminado**: `ai.model` de `settings.json` (★ en los chips). Si el modelo de la
  sesión deja de estar activo, `resolve_initial_model` elige otro (`ProviderCatalog::offers`).
- **Ajustes** (`ai_setup.rs` + `settings_providers.rs`): guarda claves, interruptor y modelos
  al momento. El listado de modelos va por `python -m lsx_agent --list-models <id>` y la
  prueba por `--probe`; los dos en segundo plano. Tras cada cambio el chat recibe
  `ChatCmd::RefreshProviders`.
- **Paridad**: la tabla de Rust (`ai_setup::PROVIDERS`) replica la de `providers.py` (orden,
  familia, modelos, variable de entorno, clave y URL), y un test lo comprueba.

## Revert

Cada escritura guarda un checkpoint (antes y después).

- `session.revert(messageID)` restaura los archivos al estado anterior a ese mensaje y oculta
  el transcript posterior.
- `unrevert` vuelve a aplicar los cambios.
- Un prompt nuevo sobre una sesión revertida descarta lo revertido, también en el historial
  del modelo.

## Distribución

`scripts/get-agent.ps1` (`make agent`) monta:

- `agent/python`: embeddable 3.12.10 con `._pth` → `..\lib`, `..`;
- `agent/lib`: nas-adk 0.3.0 con el extra `anthropic`, wheels win_amd64 instaladas con `uv pip`;
- `agent/lsx_agent`.

Versiones y SHA256 fijados. Unos 74 MB, frente a los ~180 MB de opencode.

## Pruebas

- `python/tests`: 26 tests con un modelo guionizado, sin red. Cubren protocolo, permisos,
  revert, abort, sandbox y CRLF.
- `crates/chat/tests/agent_fixture.rs`: reduce la grabación
  `fixtures/agent/events/agent-fix-edit.jsonl`, generada con `python/tools/record_fixture.py`.
- `crates/chat/tests/agent_process.rs`: lanza el proceso Python real desde el servicio. Se
  salta si no existe `python/.venv`.
