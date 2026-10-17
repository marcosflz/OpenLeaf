# LaTeX Studio

IDE LaTeX nativo (Rust + GPUI) y **portable**: 4 paneles — archivos · editor · PDF · chat IA —
con compilación al guardar, SyncTeX bidireccional y auto-reparación por agente.

- **Estado**: P0 (preflight) — ver `docs/board.md`.
- **Plan de ejecución del swarm**: `docs/plan/` (README → PLAN-MAESTRO → INTERFACES → TAREAS → SWARM).
- **Gates y riesgos vivos**: `docs/gates.md`.

## Desarrollo

```powershell
cargo run -p latex-studio        # abre la ventana (requiere toolchain stable >= 1.92)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace    # o cargo test --workspace
```

La raíz portable se detecta por: `LATEX_STUDIO_ROOT` → directorio del exe → raíz del repo (dev).
`tex/`, `agent/`, `workspace/` y `legacy/` no se versionan (activos locales del usuario/distro).

Licencia: MIT (ver `docs/plan/PLAN-MAESTRO.md` §8 para terceros).
