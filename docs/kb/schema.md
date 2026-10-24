# Esquema del OKF

## Frontmatter obligatorio

```yaml
---
id: kb/area/slug
title: Frase corta y concreta
status: stable | draft | deprecated | planned
updated: 2026-10-24
source:
  - ruta/al/archivo.rs
  - comando verificado
tags: [area, tema]
related: [kb/otra/nota]
verified: true | false
---
```

- `id`: estable y único. Formato `kb/<área>/<slug-en-kebab-case>`.
- `title`: una frase; si la nota es una decisión, que se lea como decisión.
- `status`:
  - `stable`: verificada contra el código/docs y vigente.
  - `draft`: escrita pero sin verificación completa (o pendiente de una tarea).
  - `planned`: describe algo que todavía no existe; no confundir con el plan.
  - `deprecated`: ya no aplica; debe enlazar `superseded_by`.
- `updated`: fecha de la última edición real (`YYYY-MM-DD`).
- `source`: archivos, comandos o URLs que permiten verificar la nota.
- `verified`: `true` solo si se contrastó con la fuente en la fecha de `updated`.
- `related`: IDs de notas relacionadas (no rutas).

## Cuerpo

- Un `#` que repita el título (para lectura directa).
- Párrafos cortos; tablas solo cuando comparan.
- Enlaces relativos a otras notas (`../architecture/...`) y rutas de código con backticks.
- Si hay una decisión: **Contexto → Decisión → Consecuencias**.
- Si hay un procedimiento: pasos numerados y comandos copiables.
- Si hay un bug: reproducción, causa, fix y test que lo cubre.

## Identificadores de cambio y de tarea

- Cambio: `CHG-0001`, `CHG-0002`… en `changes/CHANGELOG.md`.
- Tarea del plan 0.2: `P2-<stream>-<n>` (p. ej. `P2-INST-1`, `P2-UI-2`).
- Cada entrada del changelog enlaza la tarea, los archivos y la evidencia (test, captura, informe).

## Validación rápida (manual o script)

1. ¿El `id` coincide con la ruta? (`kb/area/slug` ↔ `area/slug.md`)
2. ¿`updated` es la fecha real del cambio?
3. ¿Cada afirmación técnica tiene `source`?
4. ¿Los enlaces relativos existen?
5. ¿La nota duplica una tabla normativa en vez de enlazarla?
6. ¿Hay secretos, rutas personales o datos de `workspace/`?
7. ¿El changelog tiene una entrada para el cambio?
8. ¿El estado del plan refleja la realidad (no se deja `DONE` sin gates)?

## Convención de nombres

- Carpetas en minúsculas y sin acentos: `architecture`, `operations`, `quality`, `changes`.
- Archivos con prefijo numérico solo cuando el orden de lectura importa:
  `01-crates.md`, `02-flujos.md`. **El prefijo no forma parte del `id`**: el id de
  `01-crates.md` es `kb/architecture/crates`.
- Los nombres en mayúsculas (`CHANGELOG.md`, `PLAN-0.2.md`) se normalizan a minúsculas para
  el id (`kb/changes/changelog`, `kb/changes/plan-0.2`).
- Nunca renombrar un `id` sin dejar una nota `deprecated` que apunte al nuevo.

## Validador

```powershell
cargo run -p xtask -- kb check
```

Comprueba frontmatter obligatorio, `id` ↔ ruta (ignorando el prefijo numérico), estados,
fechas, enlaces relativos y que el changelog tenga entradas `## CHG-`. Es parte del cierre
de tarea junto a los gates de código.
