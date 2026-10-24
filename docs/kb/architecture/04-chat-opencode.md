---
id: kb/architecture/chat-opencode
title: El chat es un cliente de opencode; la configuración de proveedores vive en opencode
status: stable
updated: 2026-10-24
source:
  - crates/chat/src/supervisor.rs
  - crates/chat/src/service.rs
  - crates/chat/src/client.rs
  - crates/ui/src/panels/chat.rs
  - crates/ui/src/settings_ui.rs
  - crates/ui/src/ai_setup.rs
  - docs/plan/INTERFACES.md §5
  - https://opencode.ai/docs/server/
  - https://opencode.ai/docs/cli/
tags: [arquitectura, chat, opencode, ia]
related: [kb/architecture/flujos, kb/quality/01-auditoria-2026-10-24]
verified: true
---

# El chat es un cliente de opencode; la configuración de proveedores vive en opencode

## Qué

`lsx-chat` no habla con OpenAI/Anthropic directamente. Lanza un **opencode server local**
(`agent/opencode.exe serve --hostname 127.0.0.1 --port <libre>`) y usa su API REST/SSE. El
modelo, los proveedores y las credenciales son de opencode.

## Ciclo de vida

- `ChatSupervisor`: `start`/`stop`/`restart`, health `GET /global/health`, backoff
  0.5/1/2/5 s ×3, password `OPENCODE_SERVER_PASSWORD`, job object para matar el árbol.
- `config/opencode.runtime.json`: copia de `agent/opencode.json` con rutas absolutas y
  `skills.paths`; opencode 1.18.30 no descubre skills de proyecto por sí solo.
- `PATH` del server incluye `tex/bin/windows` y `agent/`.
- El `cwd` es la raíz del proyecto activo; sin `workspace/` el server no arranca.

## Endpoints usados por el cliente

| Uso | Método y ruta |
|---|---|
| Salud | `GET /global/health` |
| Eventos | `GET /event` (SSE) |
| Sesiones | `POST/GET/PATCH/DELETE /session`, `GET /session/:id/message` |
| Prompt | `POST /session/:id/prompt_async` |
| Abortar | `POST /session/:id/abort` |
| Permisos | `POST /session/:id/permissions/:permissionID` |
| Catálogo | `GET /config/providers`, `GET /agent` |

## Configuración de proveedores desde Ajustes (P2-AI-1)

`crates/ui/src/ai_setup.rs` encapsula el binario de la distro y
`crates/ui/src/settings_ui.rs` pinta el estado. La UI no bloquea el hilo de GPUI: los
comandos se ejecutan en `cx.background_spawn` con timeout.

| Acción | Comando | Notas |
|---|---|---|
| Estado del binario | `PortableRoot::detect()` + `agent/opencode.exe` | si falta: “No encontrado. Restaura agent/opencode.exe o reinstala.” + abrir `agent/` |
| Ver conectados | `opencode auth list` | 10 s; parser tolera lista con viñetas, tabla y `proveedor: método` |
| Catálogo | `opencode models` | 25 s; parser acepta solo `provider/model`, descarta ANSI/metadatos |
| Conectar proveedor | `opencode auth login` | se abre en `cmd /K` (Windows); el usuario completa la suscripción/API key |
| Probar | `opencode models` | 15 s; informa del número de modelos |
| Abrir configuración | `agent/opencode.json` | editor asociado del sistema |

Todos los comandos heredan `OPENCODE_CONFIG=<raíz>/agent/opencode.json`,
`OPENCODE_DISABLE_AUTOUPDATE=1` y `cwd` = `workspace/` de la distro (o la raíz si aún no
existe). `auth login` abre una consola aparte y deja el proceso vivo (`cmd /K`) para leer
el resultado.

Lo que sigue abierto:
- Los cambios de modelo/agente hechos en Ajustes se guardan y se aplican al reiniciar el
  panel; el picker del chat ya persiste sus cambios en caliente (P2-AI-2).
- La auth nativa por API (`/provider`, `/provider/auth`, `/auth/:id`, OAuth) queda para
  P2-AI-3. La v1 usa el CLI porque cubre suscripciones sin duplicar el flujo.
- `ai.permissionPreset` en el esquema acepta `ask`/`project`/`all` (y el alias legado
  `auto`); la UI expone los tres presets y el panel los normaliza en
  `ChatPanelConfig`.

## Modelo por defecto

`DEFAULT_AI_MODEL = "opencode-go/deepseek-v4.1-flash"`. Se manda explícitamente en cada
prompt para no heredar el modelo global del usuario. Si el usuario elige otro en el picker,
debe persistirse y mandarse igual.

## Permisos

`ai.permissionPreset` (`ask`/`project`/`all`) decide si el panel abre el modal o responde
solo. El valor se guarda en `settings.json`; el resto de la configuración de permisos vive
en `agent/opencode.json`.
