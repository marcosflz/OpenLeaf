# ADR-001: Sustituir opencode por un agente LaTeX propio sobre nas-adk

- Fecha: 2026-11-13
- Estado: aceptado
- Decisores: usuario + orchestrator

## Contexto

Hasta 0.2 el chat lanzaba `agent/opencode.exe serve`, un agente de programación
generalista. Ese diseño tenía estos problemas:

- **Peso**: unos 180 MB y su propia gestión de proveedores, credenciales (`opencode auth`),
  skills (`config/opencode.runtime.json`) y auto-actualización.
- **Superficie**: `bash` genérico (con allow-list por patrones), `webfetch` y edición de
  cualquier ruta. Era demasiado para editar y reparar LaTeX y obligaba a mantener una matriz
  de permisos.
- **Fragilidad**: servidor HTTP con puerto, contraseña, SSE sin `Last-Event-ID` y health
  checks. Además había que fijar la versión (1.18.30), porque la API v2 cambia el contrato
  (riesgo R5).

El usuario pidió un agente más ligero y especializado, construido con su paquete
[`nas-adk`](https://github.com/marcosflz/nas-adk) (v0.3.0, wheel en GitHub Releases).

## Decisión

El chat usa un **sidecar Python propio** (`python/lsx_agent`) sobre nas-adk:

- **Transporte**: NDJSON por stdio. Petición `{id, method, params}` → respuesta
  `{id, result|error}`; eventos `{type, properties}`. Sin puertos, sin contraseña, sin SSE.
- **Contrato estable**: los eventos, sesiones, catálogo e historial conservan la **forma de la
  API v1 de opencode**. La tabla I-4 (`events.rs`), el reductor `Conversation`, las fixtures y
  el panel de chat no cambian.
- **Herramientas cerradas**: `list`, `read`, `search`, `edit`, `write`, `compile` (latexmk +
  errores parseados), `log`, `kpsewhich`, `tlmgr_install` y `skill`. **No hay shell
  genérico.** Las rutas se validan contra la raíz del proyecto.
- **Permisos**: `before_tool_call` → `ToolDecision.interrupt` con el diff unificado. La
  respuesta de la UI vuelve como `ResumeEntry`. `always` se recuerda por sesión.
- **Revert propio**: checkpoints antes/después de cada escritura en SQLite (`config/agent`).
- **Proveedores**: `opencode` (Go), `openrouter`, `anthropic` (API) y `claude-code`
  (suscripción vía CLI `claude`). **Sin proveedor por defecto**: el catálogo solo lista los
  configurados. Las claves se guardan en `config/secrets/ai.json`.
- **Distribución**: Python embebido oficial 3.12 (`agent/python`, con `._pth`), nas-adk y
  dependencias en `agent/lib`, el paquete en `agent/lsx_agent`. Lo monta
  `scripts/get-agent.ps1` con versiones y SHA256 fijados.

## Alternativas consideradas

- **Seguir con opencode**: descartado por el peso, la superficie y el acoplamiento a su API v1.
- **Agente en Rust llamando a las APIs**: habría que reimplementar proveedores, tool-calling,
  sesiones y streaming que nas-adk ya resuelve y prueba; la suscripción de Claude solo es
  viable a través del CLI.
- **Python del sistema o uv en la máquina del usuario**: más ligero en la distro, pero frágil
  (versiones, PATH). Se eligió el Python embebido.
- **Congelar con PyInstaller**: un solo binario, pero builds lentos y falsos positivos de
  antivirus.
- **Protocolo nuevo de eventos**: más limpio, pero obligaba a reescribir el reductor y el
  panel en el mismo cambio.

## Consecuencias

- **Positivas**:
  - `agent/` pasa de unos 180 MB a unos 74 MB.
  - Arranque sin puertos ni health polling.
  - Herramientas mínimas y auditables.
  - Errores de compilación ya estructurados para el modelo.
  - Revert con checkpoints propios.
  - Log del agente en `logs/agent.log` (antes stderr se descartaba).
- **Negativas / coste**:
  - Se mantiene código Python (con tests `pytest` y `ruff` en CI).
  - La tabla de proveedores existe en Python y en Rust (`ai_setup.rs`); la paridad la vigila
    un test.
  - Truncar el historial tras un revert usa la conexión interna del `SQLiteSessionService`
    (nas-adk no expone esa operación).
- **Riesgos residuales**:
  - `claude-code` es de uso personal y local según las condiciones de Anthropic; la UI lo
    indica.
  - Cambios de API en nas-adk: la versión está fijada en el script y en `python/pyproject.toml`.

## Referencias

- `docs/kb/architecture/04-chat-agente.md`
- `crates/chat/src/sidecar.rs`, `python/lsx_agent/`
- `scripts/get-agent.ps1`
- `fixtures/agent/events/agent-fix-edit.jsonl`
- `docs/plan/INTERFACES.md` §5
