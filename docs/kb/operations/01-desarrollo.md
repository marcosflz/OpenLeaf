---
id: kb/operations/desarrollo
title: Desarrollo local en Windows
status: stable
updated: 2026-10-24
source:
  - README.md
  - AGENTS.md
  - rust-toolchain.toml
  - docs/handoff.md
tags: [operaciones, desarrollo]
related: [kb/operations/gates, kb/architecture/crates]
verified: true
---

# Desarrollo local en Windows

## Requisitos

- Windows 10/11 x64, MSVC build tools (`link.exe`), Rust stable ≥ 1.92.
- `cargo-nextest` opcional (si no está, `cargo test --workspace`).
- Inno Setup 6 para el instalador (`installer/build.ps1 -InstallInno` lo instala por usuario).
- Para compilar de verdad: `tex/` (TeX Live reducido) y `agent/opencode.exe`, que **no están
  en git**.

## Arranque

```powershell
cargo run -p latex-studio
```

La raíz portable se detecta por `LATESTUDIO_ROOT` → carpeta del `.exe` → raíz del repo
(`CARGO_MANIFEST_DIR/../..`). En desarrollo, `tex/` y `agent/` se buscan en la raíz del repo.

## Activos fuera de git

| Activo | Cómo obtenerlo |
|---|---|
| `tex/` | copiar de una distro existente o instalar TinyTeX (ver `04-latex-bootstrap.md`) |
| `agent/opencode.exe` | release oficial `opencode-windows-x64.zip` (versión fijada en CI) |
| `workspace/` | proyectos del usuario; nunca se versiona |
| `config/settings.json` | se genera al primer arranque |

## Trampas conocidas

- No filtres procesos por nombre `opencode`: puedes matar el server del harness.
- `tex/tlpkg` es necesario para `tlmgr`; `tex/texmf-var` es caché y se puede omitir.
- En tests de UI con `gpui_kit::*`, la macro `test` del kit sombrea la de Rust: importa
  explícito.
- `Button` se importa de `gpui_kit::component::button::*`.
- El chrome de una hoja no debe robar el foco al input (`PaneState::focus_if_outside`).
- `cargo xtask dist` exige `target/release/latex-studio.exe` y `agent/opencode.exe`.

## Convenciones

- Commits convencionales (`feat(ui): …`, `fix(chat): …`).
- Ramas `feat/<stream>-<tema>` en worktrees fuera de `main` (los spikes históricos fueron
  una excepción ratificada).
- No tocar `tex/`, `agent/`, `workspace/`, `legacy/` ni `config/settings.json`.
