# Board — LaTeX Studio

> Actualizado: 2026-10-17 · Orchestrator (sesión "LaTeX Studio · Orchestrator (P0)").
> Fuente de tareas: `docs/plan/TAREAS.md`. Aquí solo el estado vivo.

## P0 — Preflight

| ID | Estado | Evidencia / Nota |
|----|--------|------------------|
| P0.1 Backup + git init | ✅ DONE | Backup 10.8 MB en `..\latex-ide-backup-2026-10-17\` (incluye `chatLanguageModels.json.bak` con la clave BYOK); `git init -b main`; primer commit `c8ccbf9`; `.gitignore` (legacy/tex/workspace/logs/settings fuera de git) |
| P0.2 Mover legacy | ✅ DONE | `legacy/`: app, data, downloads, sandbox, extensions, installer, config-vscode, scripts, Makefile, README viejo, LaTeX.code-workspace. `tex/`, `agent/`, `workspace/` intactos (verificado) |
| P0.3 Esqueleto | ✅ DONE | 8 miembros (`crates/core|latex|pdf|chat|editor|ui` + `app` + `xtask`); `cargo build -p latex-studio` OK (822 crates; pin `gpui-kit =0.7.3` → gpui-pre 0.3.9 + hayro 0.8); **smoke test: ventana viva 8 s, cierre limpio**; `Cargo.lock` commiteado; gates locales fmt/clippy/test verdes |
| P0.4 CI | 🟡 PARCIAL | Workflow (fmt/clippy/nextest/deny) + `deny.toml` commiteados. **Pendiente P0.4b**: remoto en GitHub + primer push (requiere decisión del usuario: cuenta/visibilidad) |
| P0.5 Skills + agentes | ✅ DONE | Skills oficiales vendorizados (`gpui-kit`, `gpui-kit-design-guides`; 29 archivos) + 9 skills `lsx-*` + 9 agentes en `.opencode/agent/`. Verificados como cargables |
| P0.6 Fixtures | 🟡 PARCIAL | `fixtures/projects/{cv, cv-sidebar, broken-undefined, broken-missingpkg}` (anonimizados, sin datos personales), `fixtures/logs` (cv-ok, cv-sidebar-ok, undefined-command, missing-package — errores verificados en el log), `fixtures/pdf`, `fixtures/synctex`. **Pendiente**: `fixtures/opencode/events` (M0-S4), `fixtures/settings` (M1-A5), implementar `xtask fixtures` |
| P0.7 AGENTS.md + ADRs + board + e2e | ✅ DONE | `AGENTS.md`, `docs/decisions/ADR-000-template.md`, `docs/qa/e2e.md` (12 pasos), plan maestro copiado a `docs/plan/` |
| P0.8 Gates vivos | ✅ DONE | `docs/gates.md` (R1–R10, I-1..I-7, registro de gates) |

## Gates

| Gate | Estado | Nota |
|------|--------|------|
| P0 | ✅ CERRADO 2026-10-17 | Informe: `docs/qa/reports/P0-2026-10-17.md`; gates locales verdes (fmt/clippy/test); CI remota pendiente de P0.4b (no bloquea) |

## M0 — Spikes (ola O1 lanzada 2026-10-17; 6 agentes ∥, trabajo sobre main con propiedad estricta de archivos)

| ID | Spike | Estado | Sesión |
|----|-------|--------|--------|
| M0-S1 | shell / dock 4 paneles | ✅ DONE | informe: `docs/qa/reports/M0-S1-spike-shell.md` · **I-7 resuelto** (dock suficiente, colapso vía `Panel::visible`) |
| M0-S2 | editor tree-sitter | ✅ DONE | informe: `docs/qa/reports/M0-S2-spike-editor.md` · I-1/I-3 resueltos · fix: `codebook-tree-sitter-latex 0.6.1` + tree-sitter 0.26.13 |
| M0-S3 | PDF hayro (render + RenderImage) | ✅ DONE | informe: `docs/qa/reports/M0-S3-spike-pdf.md` · I-2 confirmado (visor propio) · p50 4–8 ms release |
| M0-S4 | chat opencode (REST/SSE + fixtures) | ✅ DONE | informe: `docs/qa/reports/M0-S4-spike-chat.md` · **I-4/I-5 congelados** · fixtures SSE + flujo de permiso grabados |
| M0-S5 | engine latexmk + logparse + synctex | ✅ DONE | informe: `docs/qa/reports/M0-S5-spike-latex.md` · round-trip 78↔79 (±1) · auto-install `adjmulticol` real · hallazgo `-g` post-install |

**Consolidación M0 (2026-10-17)**: commits por spike (`7f9059b` pdf, `d99f6bf` core, `7943980` editor, `e446491` chat, `55e0e4a` ui, `4b1b49f` latex); `xtask chat-replay` cableado y deps alineadas (`tree-sitter 0.26` + `codebook-tree-sitter-latex 0.6.1`) en `3430d9f`; **los 6 spikes están DONE**; **build release OK (15m42s) y `.exe` entregados en `dist\m0-demo\`**; gates finales fmt/clippy/test verdes.

## M0.5 — Pulido de diseño (feedback del usuario, 2026-10-17)

| Panel | Objetivo | Sesión | Estado |
|---|---|---|---|
| Árbol / Explorer | Estilo **VS Code Explorer** (filas 22px, 13px, iconos 16px, toolbar compacto icon-only, panel de salida mono) | ses_eb5ee3e54ffecKAx6BqKH8Qqom | ✅ DONE (`shots\tree-after.png`, verificado por orchestrator) |
| Chat | Estilo **GitHub Copilot Chat** (sin burbujas, tool cards finas, composer) + **modo replay determinista** | ses_eb5ee3e52ffej5X95550JhBmV3 | ✅ DONE (`shots\chat-after.png`, verificado por orchestrator) |

- Commits: `b39a9aa` (restyle) · `3904d4e` (fixtures live extra) · gates workspace fmt/clippy/test verdes tras el pulido.
- Herramienta de evidencia: `scripts/capture-window.ps1` (topmost, DPI-aware) — receta en skill `lsx-quality-gates`.

## M1 — Núcleo usable (ola O2 lanzada 2026-10-17; 4 streams ∥)

**Desviación ratificada**: streams sobre `main` con **propiedad estricta de archivos** (sin worktrees) para compartir `target/` y evitar 4 rebuilds completos de GPUI; reglas: nunca git, `rustfmt` solo propios, gates por crate, evidencia visual con captura.

| Stream | Territorio | Tareas | Sesión | Estado |
|---|---|---|---|---|
| **A** shell/UX | `crates/ui/**`, `app/src/main.rs`, `app/src/bin/spike-shell.rs`, `lsx-core::{root,config,events}` (M1-A0) | A0 + A1..A6 + integración árbol real | ses_eb5b77d76ffeaY8fTWI6zM9E8E | DOING |
| **B** editor | `crates/editor/**`, `crates/ui/src/panels/editor.rs`, `app/src/bin/spike-editor.rs` | B1..B5 | ses_eb5b77d74ffexNMHalgSmSMOgq | ✅ **DONE** — 51 tests; pestañas+dirty, guardado atómico+conflictos (modal 3 opciones), diagnósticos inline (squiggle verificado), find/replace (22 casos), autosave+backups+recuperación. Evidencia: `shots/m1-b*.png`. API del panel en su informe |
| **C** PDF | `crates/pdf/**`, `crates/ui/src/panels/pdf.rs`, `app/src/bin/spike-pdf.rs` | C1..C4 (+ multipage fixture) | ses_eb5b77d73ffeHE0xX4Xi1k4qgJ | DOING |
| **D** latex | `crates/latex/**`, `crates/ui/src/panels/problems.rs` | D1..D5 | ses_eb5b77d71ffejCS2L6SG3WcPiO | ✅ **DONE** — job objects (timeout/cancel con cuelgue real, 0 huérfanos), `CompileController` (single-flight+coalescing+debounce), parser ampliado (Underfull/Info/BibError/correlación `l.NN`), auto-install (3/compilación, `adjmulticol` real), `clean()`, `detect_main`, panel Problemas. 48+7+9 tests verdes |
| **E** chat · **F** fs | en cola (O3, tras liberar slot) | M1-E*, M1-F* | — | TODO |

Gate de salida: **G1** (E2E pasos 1–8 + presupuestos §6.3) — requiere también E/F; el orchestrator integra los paneles en el shell conforme aterrizan.
| M0-S6 | fs/watcher + tree component | ✅ DONE | informe: `docs/qa/reports/M0-S6-spike-fs.md` · list_dir 5,6 ms/5k · tree sin evento de selección (→ M1-F2) |
| M0-G0 | Freeze `INTERFACES.md` | TODO | orchestrator, al cerrar S1–S6 — requiere revisión del usuario |

**Consolidación M0 (2026-10-17)**: commits por spike (`7f9059b` pdf, `d99f6bf` core, `7943980` editor, `e446491` chat, `55e0e4a` ui); `xtask chat-replay` cableado y deps alineadas (`tree-sitter 0.26` + `codebook-tree-sitter-latex 0.6.1`) en `3430d9f`; `crates/latex` (S5) sin commitear a propósito mientras el spike está en vuelo; **build release + entrega de .exe en curso**; gates completos del workspace tras el build.

## Notas

- **P0.4b**: crear remoto GitHub (privado por defecto) y push para verificar CI verde. Necesita decisión del usuario.
- **R6** (AGENTS.md del repo vs producto): test explícito en G1; los proyectos del producto viven en `workspace/` (ignorado por git).
- **Hallazgo gpui-kit (P0)**: `Button` se importa con `use gpui_kit::component::button::*;` — propagar a los skills `lsx-gpui-patterns`/`lsx-editor-ts` en M0.
- **Desviación M0 (aprobada por orchestrator)**: los spikes trabajan **directamente sobre `main`** con propiedad estricta de archivos (sin worktrees), para evitar 6 builds completos del árbol GPUI; los streams de M1+ sí usan worktrees+branches. Cada spike: binario propio en `app/src/bin/spike-*.rs` + su crate.
- Los fixtures `main.tex` se compilan con el `tex/` portátil real: reproducible con `xtask fixtures` (implementación en M0).
