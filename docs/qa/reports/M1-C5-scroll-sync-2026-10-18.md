# M1-C5 — Feedback del usuario: foco de teclado y scroll sync `.tex` ↔ PDF

- **Fecha**: 2026-10-18
- **Origen**: validación manual del usuario sobre `dist/m0-demo/latex-studio.exe` (G1)
- **Alcance**: `crates/ui` (`panels/pane.rs`, `panels/editor.rs`, `panels/pdf.rs`, `app.rs`,
  `settings_ui.rs`, `toolbar.rs`, `locales/es.yml`) + `crates/core/src/config.rs`
- **Commits**: `9938b75` (foco) · `(sync)` scroll sync M1-C5

## 1. Bug reportado: no se podía escribir en el chat

**Síntoma**: el usuario no podía teclear en el composer del chat (ni, en realidad,
en el documento del editor).

**Causa raíz**: el chrome de cada hoja (`ChatSlot`/`EditorSlot`, M1-A2) registra
`.on_mouse_down(Left, activate_by_user)` en el contenedor del panel, y
`activate_by_user` hacía `focus_handle().focus(window, cx)` **sin condición**.

En GPUI el ratón se reparte en fase *bubble* de dentro hacia fuera
(`window.rs::dispatch_mouse_event`, `iter().rev()`), así que el orden real al
hacer clic en el composer era:

1. el `Input`/`Editor` interno enfocaba su control de texto (fase bubble, el más
   interno primero),
2. el contenedor de la hoja **sobreescribía** ese foco con el de la hoja.

Resultado: `window.focused()` apuntaba al `<div>` de la hoja y las teclas no
llegaban a ningún input. El cursor sí se colocaba (por eso Ctrl+Alt+J funcionaba:
la posición del cursor la fija el `on_mouse_down` del propio input, no el foco).

**Fix** (`panels/pane.rs`):

```rust
pub fn focus_if_outside<T: Render>(&self, window: &mut Window, cx: &mut Context<T>) {
    if !self.focus.contains_focused(window, cx) {
        self.focus.focus(window, cx);
    }
}
```

`Pane::activate_by_user` ahora usa `focus_if_outside`: el foco solo se mueve a la
hoja si estaba **fuera** de ella (se conserva el comportamiento del focus mode
`Ctrl+Alt+M` y del panel activo), y un control interno ya enfocado por el clic lo
mantiene.

**Evidencia**: tests de UI nuevos (harness `TestAppContext` + `gpui-kit`
`test-support`, primera vez que se usa en `crates/ui`):

| Test | Antes del fix | Después |
|---|---|---|
| `panels::chat::tests::clic_en_el_composer_permite_escribir` | ❌ `""` ≠ `"hola"` | ✅ |
| `panels::editor::tests::clic_en_el_editor_permite_escribir` | ❌ (mismo fallo) | ✅ |

Los dos tests hacen clic real (`window.click(("input", entity_id))`) y teclean
(`window.input("hola")`) dentro de un `Host` que replica el chrome de hoja, así
que la regresión no puede volver sin que fallen.

## 2. Petición: scroll sync `.tex` ↔ PDF

Decisión del usuario (2 preguntas, 2026-10-18): **ambas direcciones** y el avance
se dispara **solo al mover el cursor** (no con el scroll del editor).

### Comportamiento implementado

- `latex.syncScroll` (default `true`): interruptor en la toolbar
  (`IconName::ArrowLeftRight`, `#toolbar-sync-scroll`) y en Ajustes
  (`#settings-sync-scroll`); se persiste en `config/settings.json`.
- **Avance (`.tex` → PDF)**: al cambiar el cursor (línea/columna), 250 ms después
  de la última señal se resuelve `synctex view` **en el executor de fondo** y el
  visor se desplaza a la caja resultante **sin resaltado**. Si el destino
  (`ruta:línea`) no cambió, no se vuelve a invocar el CLI.
- **Inverso (PDF → `.tex`)**: al desplazarse el visor (rueda, barra o teclado),
  250 ms después el shell toma el borde superior del viewport
  (`source_point_at_top`), resuelve `synctex edit` y desplaza el editor a esa
  línea **sin mover el cursor y sin robar el foco** (no interrumpe lo que
  escribes). Si la línea pertenece a otro `.tex` ya abierto, cambia de pestaña; si
  no está abierto, no hace nada (el sync nunca abre pestañas).
- **Anti-bucle**: `reverse_allowed` ignora los avisos de scroll del visor durante
  400 ms después de un scroll programático (el scroll del sync se reporta igual
  que el del usuario).
- **`Ctrl+Alt+J`** se conserva como salto manual con resaltado temporal (útil para
  "¿dónde está esto en el PDF?"); el sync es el que hace el seguimiento continuo.
- **De regalo**: el `Ln/Col` y la `Pág. n/N` de la status bar ya se actualizan
  (antes nadie publicaba `StatusMessage::Cursor` ni `::Page`), y el Ctrl+clic
  inverse del visor vuelve a funcionar (`PdfPanel::set_synctex_exe` no se llamaba
  nunca desde el shell: el panel no tenía `synctex.exe`).

### Tests (todos verdes)

| Test | Qué cubre |
|---|---|
| `panels::pdf::tests::source_point_at_top_mapea_scroll_a_pagina_y_bp` | mapeo puro `scroll_top` → `(page0, x_bp, y_bp)` con zoom 1 y 2, recorte al final y casos degenerados |
| `panels::editor::tests::scroll_to_line_desplaza_sin_mover_el_cursor` | el scroll inverso desplaza el documento (offset < 0) y **no** cambia el cursor |
| `app::tests::el_inverso_del_scroll_sync_se_bloquea_tras_un_scroll_programatico` | guarda anti-bucle (`reverse_allowed`) |
| Suites existentes de PDF/editor | `SyncRect::from_record`, `scroll_to_rect`, zoom, etc. |

### Gates

```
cargo fmt --all -- --check                 → limpio
cargo clippy --workspace --all-targets -D  → limpio
cargo test --workspace                     → 29 suites OK (lsx-ui: 76 tests)
cargo build --release -p latex-studio      → OK · copiado a dist/m0-demo/latex-studio.exe
```

### Comprobación con el proyecto real (CLI, 2026-10-18)

`workspace/cv/plantilla-clasica` tiene `main.pdf` + `main.synctex.gz` (14 KB) y la distro
tiene `tex/bin/windows/synctex.exe`; las dos llamadas que usa el sync devuelven datos:

```
$ synctex view -i 25:0:<abs>/main.tex -o main.pdf
Page:1  x:160.33  y:54.65  v:54.88  W:540.0  H:18.88
  → SyncRect::from_record = (pág 1, x 160.33, y 36.0, w 540.0, h 18.88)

$ synctex edit -o 1:300:100:main.pdf
Input:.../plantilla-clasica/./main.tex      (normalizado por lsx-latex)
Line:61  Column:-1
  → origen del borde superior del visor = main.tex:61
```

Es decir, la capa de datos de ambas direcciones está verificada contra el corpus real;
lo único que falta es el gesto en vivo (cursor/rueda), que requiere interacción humana.

### Pendiente de validación manual (no automatizable)
1. Mover el cursor en `main.tex` con el sync activado → el PDF se desplaza solo a
   la zona correspondiente (sin resaltado).
2. Desplazar el PDF con la rueda → el editor se desplaza a la línea origen, el
   cursor no se mueve y se puede seguir escribiendo.
3. Interruptor de la toolbar: al apagarlo, ninguna de las dos direcciones actúa.

Requiere `main.pdf` **con** sidecar `main.synctex.gz` en el proyecto (lo genera
`latexmk`; si falta, SyncTeX responde `NotAvailable` y no se mueve nada).

## 3. Notas para el siguiente gate

- El harness de UI de `crates/ui` queda montado (`gpui-kit` con `test-support`):
  es la pieza que M2 pedía (`test-support`) y ya tiene 4 tests de interacción.
  Ojo: los módulos de test **no** pueden usar `use super::*` + `use gpui_kit::*`
  porque con `test-support` el glob trae la macro `#[test]` de GPUI y sombrea la
  de Rust (lo documenta el propio kit); se importa explícito.
- `spike-pdf.rs` / `spike-editor.rs` ampliados con las variantes nuevas de los
  enums (exhaustividad).
