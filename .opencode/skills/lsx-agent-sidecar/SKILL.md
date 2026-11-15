---
name: lsx-agent-sidecar
description: Usar cuando se trabaje en el asistente IA (stream E, crate lsx-chat y python/lsx_agent): proceso Python sobre nas-adk, protocolo NDJSON por stdio, herramientas LaTeX, permisos con diff, revert por checkpoints, proveedores y claves, empaquetado del Python embebido y pruebas sin tokens (pytest con modelo guionizado, fixtures y chat-replay). Sustituye a lsx-opencode-api desde 0.3.
---

# Agente LaTeX (sidecar nas-adk)

Fuentes: `docs/decisions/ADR-001-agente-nas-adk.md`, `docs/kb/architecture/04-chat-agente.md`,
`docs/plan/INTERFACES.md` §5.

## Mapa

- `python/lsx_agent/`:
  - `server.py`: RPC, turnos, eventos, permisos.
  - `tools.py`: herramientas y sandbox de rutas.
  - `latex.py`: latexmk y parseo del log.
  - `store.py`: transcript y checkpoints.
  - `providers.py`: catálogo y `ModelRouter`.
  - `prompts.py`: modos `build`, `latex-fix` y `ask`.
- `crates/chat/src/sidecar.rs`: proceso, job object, cliente RPC y lector de eventos.
- `crates/chat/src/service.rs`: worker `ChatCmd`/`ChatEvent`.
- `crates/ui/src/ai_setup.rs`: tabla de proveedores (paridad con `providers.py`), claves y `--probe`.

## Reglas

- **Contrato**: los eventos conservan la forma opencode v1 (`{type, properties}`). Si añades un
  evento, mapea su tipo en `events.rs` y cúbrelo con una fixture.
- **Sin shell genérico**: cualquier herramienta nueva valida rutas con `Workspace.resolve` y, si
  escribe, pasa por `WRITE_TOOLS` (permiso con diff y checkpoint).
- **Cwd**: el proceso nunca arranca con cwd en el proyecto del usuario.
- **Nombres**: `edit` y `write` deben coincidir con los que usan `tool_icon`/`tool_summary` en la UI.
- **Proveedores**: si añades o cambias uno, toca `providers.py` **y** `ai_setup::PROVIDERS`; el
  test de paridad lo exige.

## Probar sin tokens

- `cd python && uv sync && uv run pytest -q`: modelo `ScriptedLlm` (`tests/conftest.py`).
- `uv run python tools/record_fixture.py ../fixtures/agent/events/<nombre>.jsonl`: graba una
  fixture determinista (rutas anonimizadas).
- `cargo test -p lsx-chat`: incluye `agent_fixture` y `agent_process` (proceso real; se salta
  sin `python/.venv`).
- `cargo run -p xtask -- chat-replay [fixture]`.
- `LSX_CHAT_REPLAY=<fixture>`: el panel pinta la fixture sin agente.

## Empaquetado

- `make agent` ejecuta `scripts/get-agent.ps1`: Python 3.12.10 embebido + nas-adk 0.3.0
  (versiones y SHA256 fijados).
- Al subir versiones, actualiza el script, `python/pyproject.toml` y `docs/release.md`.
