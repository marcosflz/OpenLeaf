---
name: lsx-knowledge-base
description: Usar al empezar cualquier tarea de LaTeX Studio/OpenLeaf para recuperar contexto del OKF (docs/kb), al cerrar una tarea para actualizar la nota afectada y el changelog incremental, y al detectar un hecho durable que deba sobrevivir a la sesión. Define el formato de nota (frontmatter, source, verified), el índice de recuperación, los estados y la regla de no duplicar los contratos de docs/plan. Aplica a todos los streams, orchestrator, docs-writer y QA.
---

# OKF — conocimiento del proyecto

Fuente: `docs/kb/README.md` (contrato), `docs/kb/index.md` (puerta de entrada),
`docs/kb/schema.md` (frontmatter), `docs/kb/changes/PLAN-0.2.md` (plan vivo) y
`docs/kb/changes/CHANGELOG.md` (historia).

## Al empezar

1. Abre `docs/kb/index.md` y busca por la **pregunta** que tienes, no por el archivo.
2. Lee solo las notas relevantes. Cada una declara `source`; si el código cambió, corrige la
   nota antes de seguir.
3. Para temas de UI, carga además `gpui-kit` y `gpui-kit-design-guides`; para arquitectura,
   `lsx-architecture`; para gates, `lsx-quality-gates`.
4. Si el OKF no cubre el tema, no inventes: investiga el código, crea una nota con
   `_templates/note.md` y enlázala desde `index.md`.

## Al terminar

1. Actualiza la nota afectada (`updated`, `source`, `verified`). Si el hecho cambió, corrige;
   si dejó de aplicar, marca `deprecated` + `superseded_by`.
2. Añade una entrada `CHG-NNNN` a `CHANGELOG.md` con tarea, territorio, porqué y evidencia.
3. Actualiza el estado de la tarea en `PLAN-0.2.md` (no marques `DONE` sin gates).
4. Si la tarea cambió un contrato, actualiza `docs/plan/INTERFACES.md` o abre ADR; el OKF
   enlaza, no sustituye.

## Reglas

- Una nota = un concepto; título concreto; sin duplicar tablas normativas.
- `status`: `stable` (verificada), `draft` (sin verificar), `planned` (aún no existe),
  `deprecated` (histórica).
- `verified: true` solo si se contrastó con `source` en la fecha de `updated`.
- Sin secretos, rutas personales ni contenido de `workspace/`/`config/settings.json`.
- Español; enlaces relativos; IDs `kb/<área>/<slug>`.

## Checklist rápido

- [ ] ¿La nota responde a una pregunta real?
- [ ] ¿Tiene `source` verificable?
- [ ] ¿Está enlazada desde `index.md`?
- [ ] ¿El changelog tiene la entrada?
- [ ] ¿El plan refleja el estado?
- [ ] ¿No hay duplicación ni secretos?
