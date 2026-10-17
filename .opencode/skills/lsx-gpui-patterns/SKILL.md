---
name: lsx-gpui-patterns
description: Usar cuando se toque cualquier superficie GPUI de LaTeX Studio (streams A y B) — ownership de entidades por panel, patrón reducer (estado, evento) → estado, puente tokio→GPUI con async_channel y cx.spawn, cuándo llamar cx.notify, registro de actions y keybinds, tests con TestAppContext y repaso de antipatrones como bloquear el hilo UI o repaint storms. Leer antes los Coding Guides del skill oficial gpui-kit.
---

# Patrones GPUI de LaTeX Studio

Normativo primero: Coding Guides del skill oficial `gpui-kit` (`.opencode/skills/gpui-kit/references/coding-guides.md`) y Design Guides (`gpui-kit-design-guides`) antes de decidir estado, API o UI.
Fuentes del proyecto: `docs/plan/PLAN-MAESTRO.md` §5.2–§5.3, `docs/plan/INTERFACES.md` §8.

## Ownership de entidades por panel

- Una entidad raíz por panel (`TreePanel`, `EditorPanel`, `PdfPanel`, `ChatPanel`); las crea y posee el shell/dock de `lsx-ui`.
- Padre → hijo: handle / `Entity<T>`. Hijo → padre: `cx.emit` + `subscribe` o AppEvent. Nada de singletons ocultos.
- Estado global: solo Settings, PortableRoot y handles de servicios; el resto vive en su panel.
- `RenderOnce` vs `Entity<T>`: seguir "Choose the right unit" y "State ownership" de las Coding Guides; no inventar variantes propias.

## Reducer (estado, evento) → estado

- Cada panel define su `State` y `fn reduce(&mut self, ev: &AppEvent)`; el render solo lee estado.
- Los eventos del bus se enrutan al reducer del panel según la variante (`Compile`, `Pdf`, `Chat`, `Fs`, `Ui`).
- Descartar eventos obsoletos ANTES de tocar estado (`generation` / `doc_id`).
- El reducer se testea sin GPUI (unit puro); requisito de DoD.

## Puente tokio → GPUI

- Runtime tokio multi-hilo en hilo propio, arrancado en `main`; jamás bloquear el hilo UI (PLAN §5.3).
- Servicios → UI por `async_channel` (M→N). Forma indicativa del plan (sustituir por la API real del pin):

```rust
// PLAN §5.3 — validar sintaxis con gpui-pre 0.3.9 en M0-S1
cx.spawn(async move { while let Ok(ev) = rx.recv().await { this.update(&mut cx, /* reduce + cx.notify */) } })
```

- UI → servicios por el mismo canal en sentido inverso (los intents: `ChatCmd` / `CompileRequest` / `FsIntent`).
- Trabajo CPU caro (render PDF, parseo): `background_spawn` del kit + resultado por canal; nunca dentro de `render()`.

## cx.notify() y repintado

- Se llama tras mutar estado en respuesta a un evento; nunca durante el render; nada de polling continuo.
- No notificar por token/delta suelto: agrupar deltas del streaming por lote — `TODO(M0-S4): verificar` la cadencia real del stream de opencode y medir con el overlay dev `F12` (M2-11).
- Excepción permitida por el plan: spinner de compilación (PLAN §5.3).

## actions y keybinds

- Macro `actions!` del kit + registro de keybindings por contexto (app vs panel).
- Atajos por defecto: ver `docs/plan/PLAN-MAESTRO.md` §4.2 (Ctrl+S, Ctrl+Alt+B, Ctrl+Alt+J, Ctrl+Alt+1/3/4, Ctrl+P...). No inventar atajos nuevos sin nota/ADR.
- M1-A1 prueba persistencia del layout y atajos; los que registres quedan documentados en el PR.

## Tests con TestAppContext

- `#[gpui_kit::test]` + `TestAppContext`; referencias del kit: `.opencode/skills/gpui-kit/references/gpui/test*.md`.
- Casos que el plan exige: dock serializa/restaura layout (M0-S1, I-7), teclas simuladas → buffer/cursor (M0-S2), estados del shell, accesibilidad (M3-5, patrón `ACCESSIBILITY-UI-TESTING.md` del kit).
- Correr un crate concreto: `cargo nextest run -p lsx-ui` (o el crate bajo prueba).

## Antipatrones (prohibidos)

- Bloquear el hilo UI: `std::process` síncrono, I/O en render, esperas o sleeps.
- Repaint storms: `notify` por cada delta; re-render de listas completas sin virtualización/memo.
- Estado global mutable "por comodidad"; paneles que leen/escriben archivos directamente (deben emitir intents).
- Mezclar `gpui` 0.2.x con `gpui-pre` 0.3.9; duplicar widgets que el kit ya trae (`bubble`, `virtual_list`, `diff`, `dock`...).

## Referencias del kit (repo gpui-kit)

- Ejemplos a leer: `examples/editor` (tree-sitter + LSP), `examples/dock` (4 hojas), `examples/stream-markdown` (streaming).
- `TODO(M0-S1/S2): verificar` las APIs exactas del dock serializable y del update del editor y volcarlas aquí con snippets reales.
