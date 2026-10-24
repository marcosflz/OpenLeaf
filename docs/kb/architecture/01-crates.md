---
id: kb/architecture/crates
title: Grafo de crates y regla de dependencias
status: stable
updated: 2026-10-24
source:
  - Cargo.toml
  - docs/plan/INTERFACES.md §1
  - crates/*/Cargo.toml
tags: [arquitectura, crates]
related: [kb/architecture/flujos, kb/architecture/ui-shell]
verified: true
---

# Grafo de crates y regla de dependencias

## Qué

```
latex-studio (app)
  └─ lsx-ui
       ├─ lsx-editor
       ├─ lsx-pdf
       ├─ lsx-chat
       ├─ lsx-latex
       └─ lsx-core
```

Regla: `lsx-core` no depende de UI, tokio, reqwest ni winit. `lsx-latex`, `lsx-pdf` y
`lsx-chat` no dependen de GPUI. `lsx-editor` y `lsx-ui` sí dependen de `gpui-kit`.

## Responsabilidades

| Crate | Responsabilidad | No debe |
|---|---|---|
| `lsx-core` | raíz portable, `settings.json`, proyectos, FS atómico, watcher, eventos | pintar, lanzar procesos de UI |
| `lsx-latex` | `LatexEngine`, latexmk, `CompileController`, `LogParser`, SyncTeX, tlmgr | conocer paneles |
| `lsx-pdf` | abrir/renderizar PDF con hayro, caché LRU, coordenadas | conocer el dock |
| `lsx-chat` | supervisor de opencode, REST/SSE, reducer, permisos | editar archivos del usuario |
| `lsx-editor` | documento, tree-sitter, diagnósticos, find/replace, sesión | lógica de compilación |
| `lsx-ui` | shell, dock, paneles, toolbar, status bar, ajustes, temas, palette | reimplementar dominio |
| `app` | wiring, ventana, arranque | lógica de negocio |
| `xtask` | dist, fixtures, pdf-golden, chat-replay | código de producto |

## Dónde va un cambio

1. ¿Es un dato/contrato de dominio? → `lsx-core`.
2. ¿Habla con LaTeX/PDF/opencode? → crate de dominio.
3. ¿Es estado o render de una pantalla? → `lsx-ui`.
4. ¿Es wiring de ventana/arranque? → `app` (orchestrator).
5. ¿Es empaquetado/CI? → `xtask`, `installer`, `.github`.

Si un cambio cruza crates, el orchestrator extrae primero el cambio base a `main` y luego
los streams construyen encima. Ver `docs/plan/SWARM.md` §2.
