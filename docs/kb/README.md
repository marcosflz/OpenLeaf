# OKF — Open Knowledge Format de OpenLeaf

> Wiki de conocimiento del proyecto, pensada para que **personas y agentes** recuperen
> contexto sin releer 70 commits ni volver a investigar lo ya sabido.

Este directorio es la fuente de conocimiento **estable** de LaTeX Studio / OpenLeaf.
No sustituye a `docs/plan/INTERFACES.md` (contratos), `docs/board.md` (estado vivo) ni
`docs/qa/reports/` (evidencia). Los complementa y los enlaza.

## Qué es (y qué no es)

- **Es** un conjunto de notas atómicas en Markdown, con identificador estable, frontmatter,
  enlaces y un changelog incremental.
- **Es** el primer sitio donde un agente busca: “¿cómo se configura la IA?”, “¿dónde vive el
  instalador?”, “¿qué se decidió sobre TeX?”.
- **No es** un chat log, un volcado de código ni un lugar para secretos o datos de usuario.
- **No es** la norma: si una nota contradice el código o `INTERFACES.md`, gana la norma y la
  nota se corrige en el mismo cambio.

## Protocolo de recuperación (para agentes)

1. Lee `docs/kb/index.md` y localiza la nota por la pregunta, no por el nombre del archivo.
2. Lee **solo** las notas relevantes; cada una declara `source` para verificar el hecho.
3. Antes de tocar código, contrasta la nota con el archivo citado. Si cambió, actualiza la nota.
4. Al terminar una tarea, actualiza la nota afectada y añade una entrada a
   `docs/kb/changes/CHANGELOG.md`; actualiza el estado en `docs/kb/changes/PLAN-0.2.md`.
5. Si el hecho es nuevo y durable, crea una nota con la plantilla `_templates/note.md`.
6. Si el hecho caduca (decisión revertida, bug cerrado, versión vieja), marca la nota
   `status: deprecated` y enlaza `superseded_by`; no la borres sin más.

## Estructura

```
docs/kb/
  README.md          ← este contrato
  index.md           ← puerta de entrada y tabla de recuperación
  schema.md          ← frontmatter, IDs, estados y validación
  _templates/        ← plantillas de nota y de cambio
  product/           ← qué es el producto, promesas UX, primer arranque
  architecture/      ← crates, flujos, configuración, chat, instalador, shell
  operations/        ← desarrollo, gates, release, bootstrap de LaTeX
  quality/           ← auditoría, deuda técnica, checklist de UI
  changes/           ← plan incremental y changelog (historia viva)
```

## Reglas de mantenimiento

- **Una nota = un concepto.** Si necesita dos títulos de nivel 2 para cosas no relacionadas,
  probablemente son dos notas.
- **Hechos con fuente.** `source` debe apuntar a archivos, comandos o URLs verificables.
  Si no se ha verificado, `status: draft` y `verified: false`.
- **Sin duplicar.** Enlaza `docs/plan/INTERFACES.md`, `TAREAS.md`, `SWARM.md` o los skills en
  lugar de copiar tablas enteras.
- **Sin secretos.** Nunca claves, tokens, rutas de usuario completas ni contenido de
  `workspace/` o `config/settings.json`.
- **Idioma:** español, igual que el producto y el resto de docs.
- **Fecha:** `updated: YYYY-MM-DD` en cada edición; el changelog lleva la fecha del merge.

## Cómo se enlaza con el resto

- `AGENTS.md` obliga a leer `docs/kb/index.md` y a mantener el OKF.
- `.opencode/skills/lsx-knowledge-base/SKILL.md` explica el flujo a los agentes.
- `docs/board.md` sigue siendo el estado vivo; el OKF explica el **porqué** y el **cómo**.
- `docs/kb/changes/PLAN-0.2.md` es el plan incremental que ejecuta el swarm.
- `docs/kb/changes/CHANGELOG.md` es el registro de cambios con evidencia.

## Ownership

- **Orchestrator:** `README.md`, `index.md`, `schema.md`, `changes/**`, notas de arquitectura.
- **Streams:** notas de su dominio (UI, chat, LaTeX, PDF, FS) y la sección de calidad que toquen.
- **QA:** `quality/**` y la evidencia de los gates.
- **Docs writer:** puede proponer notas de producto; el orchestrator las integra.
