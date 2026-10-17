# Spike M0-S6 — FS / watcher / árbol

- Fecha: 2026-10-17 · Sesión: `ses_eb6356f57ffeFbP8y5J4YAl3yh`
- **ESTADO: OK** (demo funcional + 10 tests unitarios verdes)

## Entregable

- `crates/core/src/fs.rs` (**nuevo**) — `FsService` + watcher con debounce + 10 tests unitarios
- `app/src/bin/spike-tree.rs` — demo de árbol + watcher sobre copia en `%TEMP%\spike-tree-projects`
- Desviaciones: `crates/core/Cargo.toml` (+`notify`), `app/Cargo.toml` (+`lsx-core`, +`async-channel`) — reconciliar en consolidación (compartido con S2/S3/S4)

## Demo

```
cargo run -p latex-studio --bin spike-tree
```
Prueba del watcher (desde otra consola): crear/renombrar/borrar en `%TEMP%\spike-tree-projects\cv\` → el árbol se refresca solo y el panel muestra el lote agrupado. Smoke OK ×3, 0 huérfanos.

## Respuestas de interfaz

**Tree component (gpui-kit 0.7.3)**:
- `tree(&Entity<TreeState>, render_item)`; `render_item: Fn(usize,&TreeEntry,bool,&mut Window,&mut App) -> ListItem`; contexto: `.context_menu(Fn(...) -> PopupMenu)` + `PopupMenuItem::new(..).on_click(..)`.
- Modelo: `TreeItem::new(id, label)` + `.child/.children/.expanded/.disabled`; `is_folder()` ⇔ hijos no vacíos (carpeta vacía = hoja → placeholder `disabled`).
- Eventos: **solo** `TreeEvent::Expanded/Collapsed`. No hay evento de selección/click: la selección se lee con `TreeState::selected_entry()` (M1-F2 necesita polling o evento propio). `set_items` resetea selección → restaurar con `set_selected_item` (auto-expande ancestros).
- Id estable recomendado: ruta relativa con `/`. Virtualizado vía `uniform_list`.

**Mapeo `notify 8.2` (Windows ReadDirectoryChangesW) → `FsEvent`** (tras debounce 200 ms + coalescing; techo 1 s):
| notify | FsEvent |
|---|---|
| `Create(Any)` | `Created` (create+write coalescido) |
| `Modify(Any)` | `Modified` |
| `Remove(Any)` | `Removed` |
| `Modify(Name(From))`+`Name(To)` | `Renamed(from,to)`; huérfano → `Removed`/`Created` |
| `Access/Other` | descartado |
- Temperales propios (`.lsx-tmp-*` del write atómico) filtrados → `Modified(destino)` (la UI no ve ruido del guardado).
- Paths emitidos **relativos** a la raíz; callback por **lote** `Vec<FsEvent>` una vez por ventana de debounce.

**Rendimiento `list_dir`**: **5,62 ms / 5.000 archivos** (orden + filtro aux + metadata) — ~35× de margen sobre el presupuesto M1-F2 (<200 ms). Test permanente `list_dir_with_5k_files_stays_within_budget`.

## API descubierta (→ INTERFACES §12)

```rust
FsService::new(root) / root() / list_dir(rel) / list_dir_with(rel, opts)
read_text(rel) -> TextFile { text, eol: Lf|CrLf|Mixed, had_bom }
write_text_atomic(rel, text)  // temp + fsync + rename con reintentos; UTF-8 sin BOM
create_file / create_dir / rename (no sobreescribe) / delete
watch(on_batch: impl Fn(Vec<FsEvent>) + Send + 'static) -> WatcherGuard
pub fn is_aux_artifact(name) -> bool;  pub fn validate_name(name) -> Result<(), FsError>
DEBOUNCE = 200 ms; MAX_DEBOUNCE = 1 s
```

## Riesgos / TODO

1. `app/Cargo.toml` tocado (2 líneas) → integrar como cambio base en consolidación.
2. `tree` sin evento de selección (ver arriba) — decisión para M1-F2.
3. Rename partido entre ventanas de debounce → `Removed`+`Created` (raro, documentado en `fs.rs`).
4. `delete` sin papelera (M1-F3 decide reciclaje).
5. `write_text_atomic`: preservación de BOM pendiente; `sync_all()` por escritura (medir en autosave).
6. Warning ajeno detectado en `crates/editor/src/spike.rs:68` (S2 en vuelo) → revisar en gates de consolidación.
