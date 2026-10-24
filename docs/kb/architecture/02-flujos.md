---
id: kb/architecture/flujos
title: Flujos de extremo a extremo (abrir, editar, compilar, ver, chatear)
status: stable
updated: 2026-10-24
source:
  - crates/ui/src/app.rs
  - crates/latex/src/controller.rs
  - crates/chat/src/service.rs
  - docs/plan/INTERFACES.md §8
tags: [arquitectura, flujos]
related: [kb/architecture/crates, kb/architecture/chat-opencode]
verified: true
---

# Flujos de extremo a extremo (abrir, editar, compilar, ver, chatear)

## Arranque

1. `app/src/main.rs` detecta `PortableRoot`, carga `Settings` y aplica el tema.
2. Abre la ventana (`AppRoot`) con el dock de cuatro hojas.
3. `AppRoot::new` crea árbol, editor, PDF, chat, problemas y símbolos; conecta eventos.
4. `ProjectService::startup_project` restaura el último proyecto o cae al workspace.
5. `ensure_layout()` crea `workspace/` y `config/` si faltan.

## Abrir y editar

- El árbol emite `TreePanelEvent::OpenFile`; el shell llama `editor.open_path`.
- `EditorPanel` usa `FsService::read_text` y guarda con `write_text_atomic` (temp + rename).
- `EditorPanelEvent` informa al shell de `Opened`, `Saved`, `DirtyChanged` y `CursorMoved`.
- El watcher del proyecto recarga el árbol y avisa al editor de cambios externos.

## Compilar

1. `Compilar` (toolbar/atajo) o el guardado con `latex.autoCompile` llama a
   `CompileController`.
2. `detect_main` elige el `.tex` principal (setting, heurística o documento activo).
3. `LatexEngine` lanza `latexmk` con `-synctex=1 -interaction=nonstopmode -file-line-error
   -pdf` y timeout; job object mata el árbol si se cancela.
4. `LogParser` produce diagnósticos; el shell los pinta en Problemas, status bar y squiggles.
5. Al terminar, el PDF se recarga con una nueva `generation`; el visor conserva página/zoom.

## Ver y sincronizar

- `PdfPanel` renderiza con hayro en background y sube `RenderImage` a GPUI.
- `Ctrl+Alt+J` hace SyncTeX forward (cursor → página) y resalta el punto.
- `latex.syncScroll` hace forward al mover el cursor (debounce 250 ms) y reverse al
  desplazar el PDF (guarda anti-bucle de 400 ms).
- `Ctrl+clic` en el PDF hace inverse SyncTeX (página → línea).

## Chatear

- `ChatSupervisor` arranca `opencode serve` en un puerto libre con password y job object.
- `ChatService` consume SSE (`/event`), reduce a `ChatEvent` y mantiene la sesión.
- El panel pinta mensajes, razonamiento plegado, tool cards y permisos.
- Aprobar un permiso llama `POST /session/:id/permissions/:permissionID`; si el preset es
  `project`/`all`, el panel responde solo.
- “Arreglar con IA” compone un prompt con diagnósticos y cola del log, y usa el agente
  `latex-fix`.
