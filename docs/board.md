# Board — LaTeX Studio

> Actualizado: 2026-10-17 · Orchestrator (sesión "LaTeX Studio · Orchestrator (P0)").
> Fuente de tareas: `docs/plan/TAREAS.md`. Aquí solo el estado vivo.

## P0 — Preflight

| ID | Estado | Evidencia / Nota |
|----|--------|------------------|
| P0.1 Backup + git init | ✅ DONE | Backup 10.8 MB en `..\latex-ide-backup-2026-10-17\` (incluye `chatLanguageModels.json.bak` con la clave BYOK); `git init -b main`; primer commit `c8ccbf9`; `.gitignore` (legacy/tex/workspace/logs/settings fuera de git) |
| P0.2 Mover legacy | ✅ DONE | `legacy/`: app, data, downloads, sandbox, extensions, installer, config-vscode, scripts, Makefile, README viejo, LaTeX.code-workspace. `tex/`, `agent/`, `workspace/` intactos (verificado) |
| P0.3 Esqueleto | ✅ DONE | 8 miembros (`crates/core|latex|pdf|chat|editor|ui` + `app` + `xtask`); `cargo build -p latex-studio` OK (822 crates; pin `gpui-kit =0.7.3` → gpui-pre 0.3.9 + hayro 0.8); **smoke test: ventana viva 8 s, cierre limpio**; `Cargo.lock` commiteado |
| P0.4 CI | 🟡 PARCIAL | Workflow (fmt/clippy/nextest/deny) + `deny.toml` commiteados. **Pendiente P0.4b**: remoto en GitHub + primer push (requiere decisión del usuario: cuenta/visibilidad) |
| P0.5 Skills + agentes | ✅ DONE | Skills oficiales vendorizados (`gpui-kit`, `gpui-kit-design-guides`; 29 archivos) + 9 skills `lsx-*` + 9 agentes en `.opencode/agent/`. Verificados como cargables |
| P0.6 Fixtures | 🟡 PARCIAL | `fixtures/projects/{cv, cv-sidebar, broken-undefined, broken-missingpkg}` (anonimizados, sin datos personales), `fixtures/logs` (cv-ok, cv-sidebar-ok, undefined-command, missing-package — errores verificados en el log), `fixtures/pdf`, `fixtures/synctex`. **Pendiente**: `fixtures/opencode/events` (M0-S4), `fixtures/settings` (M1-A5), implementar `xtask fixtures` |
| P0.7 AGENTS.md + ADRs + board + e2e | ✅ DONE | `AGENTS.md`, `docs/decisions/ADR-000-template.md`, `docs/qa/e2e.md` (12 pasos), plan maestro copiado a `docs/plan/` |
| P0.8 Gates vivos | ✅ DONE | `docs/gates.md` (R1–R10, I-1..I-7, registro de gates) |

## Gates

| Gate | Estado | Nota |
|------|--------|------|
| P0 | 🟡 pendiente de cierre | Gates locales (fmt/clippy/test) en curso; CI remota bloqueada por P0.4b. Reporte al usuario al cerrar |

## M0 — Spikes (⏸ bloqueado hasta confirmación del usuario)

| ID | Estado |
|----|--------|
| M0-S1..S6 + M0-G0 (freeze INTERFACES) | TODO — esperar "adelante con M0" |

## Notas

- **P0.4b**: crear remoto GitHub (privado por defecto) y push para verificar CI verde. Necesita decisión del usuario.
- **R6** (AGENTS.md del repo vs producto): test explícito en G1; los proyectos del producto viven en `workspace/` (ignorado por git).
- Los fixtures `main.tex` se compilan con el `tex/` portátil real: reproducible con `xtask fixtures` (implementación en M0).
