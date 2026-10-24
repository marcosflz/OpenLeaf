---
id: kb/operations/gates
title: Gates obligatorios y evidencia
status: stable
updated: 2026-10-24
source:
  - AGENTS.md
  - docs/plan/SWARM.md §3/§5
  - .opencode/skills/lsx-quality-gates/SKILL.md
tags: [operaciones, calidad, gates]
related: [kb/operations/desarrollo, kb/quality/03-checklist-ui]
verified: true
---

# Gates obligatorios y evidencia

## Antes de cerrar cualquier tarea

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace     # o: cargo test --workspace
cargo run -p xtask -- kb check    # OKF: frontmatter, ids, enlaces y changelog
```

Si el cambio toca el instalador:

```powershell
pwsh -File installer\build.ps1 -SkipBuild
```

Si toca LaTeX real (requiere `tex/`):

```powershell
$env:LSX_TEST_TEXLIVE = "1"
cargo nextest run -p lsx-latex
```

## DoD

1. Código + tests que cubren los criterios de aceptación.
2. Gates verdes en local (y CI cuando aplique).
3. Sin `todo!()`/`unwrap()` en rutas de producción; errores tipados con mensaje de usuario.
4. Docs/OKF actualizados (nota afectada + changelog).
5. Evidencia para UI: captura DPI-aware (`scripts/capture-window.ps1`) o test de interacción.
6. `docs/board.md` actualizado (estado, notas, enlaces).
7. Rendimiento dentro del presupuesto si toca render/IO.

## Evidencia de UI

- Test de interacción con `TestAppContext` cuando el comportamiento sea verificable.
- Captura de ventana real para jerarquía/estado visual.
- En el changelog: ruta de la captura, test o informe.

## Qué no vale

- “Compila” sin tests del comportamiento nuevo.
- Captura de una build antigua.
- Cerrar un bug sin reproducción + test de regresión.
- Dejar una nota del OKF desactualizada tras cambiar el contrato.
