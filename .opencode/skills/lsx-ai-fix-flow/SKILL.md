---
name: lsx-ai-fix-flow
description: Usar cuando se implemente o pruebe el flujo Arreglar errores con IA (streams E y QA) — composición de plantillas de prompt con diagnósticos y cola del log, definición y uso del agente latex-fix, flujo completo error → sesión → permiso de edición → watcher → recompilación, y cómo probarlo con fixtures y mocks (chat-replay, wiremock) sin gastar tokens reales. Ver TAREAS M1-E5, M2-6 y M2-12.
---

# Flujo "Arreglar errores con IA"

Fuente: `docs/plan/PLAN-MAESTRO.md` §4.1 / §4.3 / §5.5; `docs/plan/TAREAS.md` M1-E5, M2-6, M2-12 y M1-F4 (wizard).

## El modo latex-fix

- Es un **modo** del agente propio (`python/lsx_agent/prompts.py`, `FIX`), no un archivo
  `.opencode/agents/latex-fix.md` (eso era 0.2 con opencode).
- Proceso: compile → causa raíz del primer error → `kpsewhich`/`tlmgr_install` o `edit` mínimo
  → recompilar (hasta 5 ciclos) → informar.
- El panel lo envía con `ChatCmd::Prompt { agent: Some("latex-fix") }` desde «Arreglar errores».
- Modos ofrecidos: `build`, `latex-fix` y `ask` (`ai.agent`, por defecto `build`).

## Composición del prompt

- Entradas: diagnósticos actuales (`lsx-latex::parse_log`) + extractos del archivo + `log_tail` (~200 líneas) + contexto de proyecto.
- Contexto adjunto del chat: `PromptContext::{CurrentFile, Selection, DiagnosticsSummary}` (INTERFACES §5.1).
- Plantillas en `assets\prompts\*.md` (settings `ai.promptTemplatesDir`); quick actions reescribir/traducir/resumir/explicar error con hot reload sin recompilar (M2-6 / M2-12).
- `TODO(M1-E5): verificar` el texto exacto de la plantilla "Arreglar errores" — proponerla en el PR de la tarea y validarla en el E2E paso 9; no inventar claves de configuración nuevas.

## Flujo E2E (pasos 3, 8 y 9 del guion)

1. La compilación falla → status rojo + botón "Arreglar errores con IA" (M1-D3/E5).
2. Se abre o crea la sesión y se envía el prompt en modo `latex-fix` (`session.prompt`); enviar no requiere permiso.
3. El agente quiere editar un `.tex` → `permission.asked` con el diff → modal (M1-E4) → once/always/reject.
4. Aprobado → el archivo cambia en disco → watcher (`FsEvent`) → el editor recarga si está limpio; si dirty → modal de conflicto (3 opciones, M1-B2).
5. AutoCompile (debounce 400 ms) → recompila; si queda verde, la tool card final y la status bar lo reflejan.
- Rechazado → el archivo no cambia; el chat debe informar del rechazo.

## Probar sin gastar tokens reales

- `cargo run -p xtask -- chat-replay fixtures/agent/events/agent-fix-edit.jsonl`: el flujo latex-fix → read → edit con permiso → respuesta.
- `cd python && uv run pytest -q`: modelo guionizado (`ScriptedLlm`); nunca llamar a un proveedor real desde tests.
- Humo manual con costo mínimo: modelo barato y prompt trivial; documentar cualquier gasto en el reporte del PR.

## UX y errores

- Mensajes de usuario en español; el agente no deja el documento roto en silencio: al terminar informa paquetes instalados, archivos editados y estado.
- Si el fix no converge (p. ej. `tlmgr` sin red), mostrar el diagnóstico al usuario con alternativa en vez de reintentar sin límite (tope de 3 auto-installs; ver skill `lsx-latex-engine`).
- R6: el agente del producto NO debe recibir el `AGENTS.md` del repo de desarrollo; hay test explícito en M1 → G1. No tocar `workspace\` ni su configuración desde el repo.
