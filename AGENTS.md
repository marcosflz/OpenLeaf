# AGENTS.md — LaTeX Studio (repo de desarrollo)

Este repositorio ES la aplicación: un IDE LaTeX nativo en Rust (GPUI) + distro portable.
No confundir con `workspace/AGENTS.md`, que gobierna al agente del **producto** cuando edita el LaTeX del usuario.

## Antes de tocar nada

1. Lee **`docs/kb/index.md`** (OKF: wiki del proyecto) y la nota que corresponda a tu tema.
   Es la vía rápida para recuperar arquitectura, flujos, configuración, instalador, UI y
   calidad sin releer el plan completo.
2. Lee `docs/plan/README.md` (índice) y `docs/board.md` (estado actual).
3. Tu tarea indica qué secciones de `docs/plan/INTERFACES.md` y `docs/plan/TAREAS.md` aplican.
4. Carga los skills de `.opencode/skills/`: `gpui-kit` y `gpui-kit-design-guides` (oficiales), `lsx-knowledge-base` y el `lsx-*` que corresponda.

## Mantén el OKF (obligatorio)

- El OKF es `docs/kb/`. **No es un chat log**: son notas atómicas con frontmatter, `source`,
  `updated` y `verified`.
- Si descubres un hecho durable, actualízalo en su nota o crea una con
  `docs/kb/_templates/note.md`. Si contradice el código, gana el código y corriges la nota.
- Cada tarea cerrada añade una entrada a `docs/kb/changes/CHANGELOG.md` con tarea, territorio,
  porqué y evidencia; y actualiza el estado en `docs/kb/changes/PLAN-0.2.md`.
- No dupliques tablas normativas de `INTERFACES.md`/`TAREAS.md`: enlázalas.
- Nunca escribas secretos, rutas personales ni contenido de `workspace/` o
  `config/settings.json`.

## Reglas duras

- **Solo tu territorio** (matriz en `docs/plan/SWARM.md` §2). Si necesitas tocar otro crate o un archivo del orchestrator: para y pide.
- **Gates obligatorios** antes de dar algo por terminado:
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo nextest run --workspace` (o `cargo test --workspace` si no tienes nextest)
  - `cargo run -p xtask -- kb check` (OKF sin notas/enlaces rotos)
- **DoD y evidencia**: `docs/plan/SWARM.md` §3.
- **No tocar** (activos del usuario/distro): `tex/`, `agent/`, `workspace/`, `legacy/`.
- Secretos fuera de git. `config/settings.json` es estado de usuario (ignorado).
- Commits convencionales (`feat(editor): ...`). Ramas `feat/<stream>-<tema>` en worktrees fuera de `main`.

## Estructura

```
crates/core | latex | pdf | chat | editor | ui     (ver docs/plan/INTERFACES.md)
app/            binario `latex-studio` (wiring)
xtask/          herramientas (fixtures, pdf-golden, chat-replay, dist)
docs/ plan/ (plan maestro) · board.md · decisions/ (ADR) · qa/ (E2E, reports)
.opencode/ agent/ (charters del swarm) · skills/ (conocimiento del proyecto)
tex/ agent/ workspace/ legacy/   activos portables — NO versionados, NO tocar
```

## Estado

Vivo en `docs/board.md`. Gates y riesgos en `docs/gates.md`.
