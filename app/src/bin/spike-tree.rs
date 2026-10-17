#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Spike M0-S6 — árbol de archivos + watcher con debounce.
//!
//! Demo de viabilidad para INTERFACES.md §2:
//! - árbol del kit (`component::tree`) con iconos por extensión, filtro de
//!   texto, expandir/colapsar y menú contextual (crear/renombrar/borrar);
//! - `lsx_core::fs::FsService` + watcher `notify` con debounce de 200 ms que
//!   refresca el árbol con eventos agrupados.
//!
//! La demo trabaja SIEMPRE sobre una copia de `fixtures/projects` en
//! `%TEMP%\spike-tree-projects` (nunca sobre el fixture original):
//! `copy_fixture_projects` la recrea al arrancar.
//!
//! Prueba del watcher: con la ventana abierta, desde otra consola
//! `Set-Content "$env:TEMP\spike-tree-projects\cv\prueba.txt" (Get-Random)`
//! y verás el lote en el panel derecho y el árbol refrescado.

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogDescription, DialogFooter};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tree::{TreeEntry, TreeEvent, TreeItem, TreeState, tree};
use gpui_kit::component::{Icon, Sizable as _, Theme, ThemeMode, h_flex, v_flex};
use gpui_kit::*;
use lsx_core::fs::{EntryKind, FsEvent, FsService, WatcherGuard};

/// Nombre de la carpeta de trabajo dentro de `%TEMP%`.
const COPY_DIR_NAME: &str = "spike-tree-projects";
/// Máximo de líneas del log de eventos.
const LOG_CAP: usize = 200;
/// Filas visibles del log (más recientes primero).
const LOG_ROWS: usize = 60;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Spike M0-S6 · Árbol de archivos + watcher".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(30.), px(30.)),
                    size(px(940.), px(600.)),
                ))),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| SpikeTree::new(window, cx))
            })
            .expect("no se pudo abrir la ventana del spike");
        });
}

/// Vista de la demo.
struct SpikeTree {
    fs: FsService,
    copy_root: PathBuf,
    tree: Entity<TreeState>,
    /// Índice id (= ruta relativa) → item, para restaurar la selección.
    tree_index: HashMap<String, TreeItem>,
    filter: Entity<InputState>,
    filter_text: String,
    filter_lower: String,
    /// Carpetas expandidas por id.
    expanded: HashSet<String>,
    /// Último resumen del watcher.
    last_batch: String,
    /// Última acción manual.
    status: String,
    /// Log de eventos (más nuevos al final).
    log: VecDeque<String>,
    _filter_sub: Subscription,
    _tree_sub: Subscription,
    _watcher: Option<WatcherGuard>,
    _fs_task: Task<()>,
}

impl SpikeTree {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let copy_root = std::env::temp_dir().join(COPY_DIR_NAME);
        let mut log = VecDeque::new();
        match copy_fixture_projects(&copy_root) {
            Ok(n) => log.push_back(format!(
                "{}  copia de prueba lista: {n} archivos en {}",
                hhmmss(),
                copy_root.display()
            )),
            Err(e) => log.push_back(format!(
                "{}  ! no se pudo copiar fixtures/projects: {e}",
                hhmmss()
            )),
        }

        let fs = FsService::new(&copy_root);
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filtrar por nombre…"));
        let tree = cx.new(|cx| TreeState::new(cx));

        let filter_sub = cx.subscribe_in(&filter, window, |view, state, event, _window, cx| {
            if matches!(event, InputEvent::Change) {
                view.filter_text = state.read(cx).value().to_string();
                view.filter_lower = view.filter_text.to_lowercase();
                view.rebuild_tree(cx);
            }
        });
        let tree_sub = cx.subscribe(&tree, |view, _state, event: &TreeEvent, _cx| match event {
            TreeEvent::Expanded(id) => {
                view.expanded.insert(id.to_string());
            }
            TreeEvent::Collapsed(id) => {
                view.expanded.remove(id.as_ref());
            }
        });

        // Puente watcher (hilo de debounce) → hilo de UI (async_channel + Task).
        let (tx, rx) = async_channel::unbounded::<Vec<FsEvent>>();
        let watcher = match fs.watch(move |batch| {
            let _ = tx.send_blocking(batch);
        }) {
            Ok(guard) => Some(guard),
            Err(e) => {
                log.push_back(format!("{}  ! watcher: {e}", hhmmss()));
                None
            }
        };

        let fs_task = cx.spawn(async move |this, cx| {
            while let Ok(batch) = rx.recv().await {
                if this
                    .update(cx, |view, cx| view.on_fs_batch(batch, cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut this = SpikeTree {
            fs,
            copy_root,
            tree,
            tree_index: HashMap::new(),
            filter,
            filter_text: String::new(),
            filter_lower: String::new(),
            expanded: HashSet::from([String::new()]),
            last_batch: "esperando eventos…".into(),
            status: "listo".into(),
            log,
            _filter_sub: filter_sub,
            _tree_sub: tree_sub,
            _watcher: watcher,
            _fs_task: fs_task,
        };
        this.expand_top_level(cx);
        this
    }

    /// Expande las carpetas de primer nivel (primera impresión útil de la demo).
    fn expand_top_level(&mut self, cx: &mut Context<Self>) {
        if let Ok(entries) = self.fs.list_dir(Path::new("")) {
            for entry in entries {
                if entry.kind == EntryKind::Dir {
                    self.expanded.insert(path_key(Path::new(&entry.name)));
                }
            }
        }
        self.rebuild_tree(cx);
    }

    // --- modelo del árbol -------------------------------------------------

    fn build_tree_model(&self) -> (TreeItem, HashMap<String, TreeItem>) {
        let mut index = HashMap::new();
        let root_label = self
            .copy_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| COPY_DIR_NAME.to_owned());
        let root = self.build_node(Path::new(""), &root_label, false, &mut index);
        (root, index)
    }

    fn build_node(
        &self,
        rel: &Path,
        name: &str,
        ancestor_matched: bool,
        index: &mut HashMap<String, TreeItem>,
    ) -> TreeItem {
        let filtered = !self.filter_lower.is_empty();
        let self_matched = name_matches(&self.filter_lower, name);
        let show_all = ancestor_matched || !filtered || self_matched;
        let id = path_key(rel);
        let expanded = if filtered {
            show_all
        } else if id.is_empty() {
            true
        } else {
            self.expanded.contains(&id)
        };

        let mut item = TreeItem::new(id.clone(), name.to_owned()).expanded(expanded);
        if let Ok(entries) = self.fs.list_dir(rel) {
            for entry in entries {
                let child_rel = rel.join(&entry.name);
                match entry.kind {
                    EntryKind::Dir => {
                        let child = self.build_node(&child_rel, &entry.name, show_all, index);
                        if filtered && !show_all && child.children.is_empty() {
                            index.remove(&path_key(&child_rel));
                            continue;
                        }
                        item = if child.children.is_empty() {
                            // El kit decide «carpeta» por tener hijos; las
                            // vacías necesitan un placeholder deshabilitado.
                            let empty =
                                TreeItem::new(format!("{}/", path_key(&child_rel)), "(vacía)")
                                    .disabled(true);
                            item.child(child.child(empty))
                        } else {
                            item.child(child)
                        };
                    }
                    EntryKind::File => {
                        if show_all {
                            item = item.child(TreeItem::new(path_key(&child_rel), entry.name));
                        }
                    }
                }
            }
        }
        index.insert(id, item.clone());
        item
    }

    fn rebuild_tree(&mut self, cx: &mut Context<Self>) {
        let selected_before = self
            .tree
            .read(cx)
            .selected_entry()
            .map(|entry| entry.item().id.to_string());
        let (root, index) = self.build_tree_model();
        self.tree_index = index;
        let restore = selected_before.and_then(|id| self.tree_index.get(&id).cloned());
        self.tree.update(cx, |state, cx| {
            state.set_items(vec![root], cx);
            if let Some(item) = restore.as_ref() {
                state.set_selected_item(Some(item), cx);
            }
        });
    }

    /// Carpeta de destino para «nuevo archivo/carpeta» según la selección.
    fn selection_dir(&self, cx: &App) -> String {
        match self.tree.read(cx).selected_entry() {
            Some(entry) => {
                let id = entry.item().id.to_string();
                if entry.is_folder() {
                    id
                } else {
                    parent_of(&id)
                }
            }
            None => String::new(),
        }
    }

    // --- eventos del watcher ----------------------------------------------

    fn on_fs_batch(&mut self, batch: Vec<FsEvent>, cx: &mut Context<Self>) {
        let stamp = hhmmss();
        let mut created = 0usize;
        let mut modified = 0usize;
        let mut removed = 0usize;
        let mut renamed = 0usize;
        for event in &batch {
            let line = match event {
                FsEvent::Created(p) => {
                    created += 1;
                    format!("{stamp}  + {}", p.display())
                }
                FsEvent::Modified(p) => {
                    modified += 1;
                    format!("{stamp}  ~ {}", p.display())
                }
                FsEvent::Removed(p) => {
                    removed += 1;
                    format!("{stamp}  - {}", p.display())
                }
                FsEvent::Renamed(from, to) => {
                    renamed += 1;
                    format!("{stamp}  > {} => {}", from.display(), to.display())
                }
            };
            self.log.push_back(line);
        }
        while self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
        self.last_batch = format!(
            "{} evento(s) · {created} creados · {modified} modificados · {removed} borrados · {renamed} renombrados",
            batch.len()
        );
        self.rebuild_tree(cx);
    }

    // --- acciones ----------------------------------------------------------

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    fn notify_error(&mut self, message: String, window: &mut Window, cx: &mut Context<Self>) {
        self.log.push_back(format!("{}  ! {message}", hhmmss()));
        while self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
        self.status = message.clone();
        window.push_notification(Notification::error(message), cx);
    }

    fn open_create_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        parent: String,
        kind: EntryKind,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(if kind == EntryKind::File {
                "p. ej. seccion.tex"
            } else {
                "p. ej. figuras"
            })
        });
        let where_text = if parent.is_empty() {
            format!("En {}", self.copy_root.display())
        } else {
            format!(
                "En {}\\{}",
                self.copy_root.display(),
                parent.replace('/', "\\")
            )
        };
        let weak = cx.weak_entity();
        let initial_focus = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if initial_focus.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let input_entity = input.clone();
            let weak = weak.clone();
            let parent = parent.clone();
            dialog
                .title(if kind == EntryKind::File {
                    "Nuevo archivo"
                } else {
                    "Nueva carpeta"
                })
                .width(px(460.))
                .child(DialogDescription::new().child(where_text.clone()))
                .child(Input::new(&input).cleanable(true))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel").label("Cancelar").outline()),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("create").label("Crear").primary()),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let name = input_entity.read(cx).value().trim().to_string();
                    weak.update(cx, |view, view_cx| {
                        view.perform_create(kind, &parent, &name, window, view_cx)
                    })
                    .unwrap_or(true)
                })
        });
    }

    fn perform_create(
        &mut self,
        kind: EntryKind,
        parent: &str,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if name.is_empty() {
            self.notify_error("El nombre no puede estar vacío".into(), window, cx);
            return false;
        }
        let rel = join_rel(parent, name);
        let result = match kind {
            EntryKind::File => self.fs.create_file(&rel),
            EntryKind::Dir => self.fs.create_dir(&rel),
        };
        match result {
            Ok(()) => {
                self.set_status(format!("creado «{name}»"));
                self.rebuild_tree(cx);
                true
            }
            Err(e) => {
                self.notify_error(format!("No se pudo crear «{name}»: {e}"), window, cx);
                false
            }
        }
    }

    fn open_rename_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        id: String,
        name: String,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name.clone()));
        let weak = cx.weak_entity();
        let initial_focus = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if initial_focus.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let input_entity = input.clone();
            let weak = weak.clone();
            let id = id.clone();
            dialog
                .title(format!("Renombrar «{name}»"))
                .width(px(460.))
                .child(Input::new(&input).cleanable(true))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cancel").label("Cancelar").outline()),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("rename").label("Renombrar").primary()),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let new_name = input_entity.read(cx).value().trim().to_string();
                    weak.update(cx, |view, view_cx| {
                        view.perform_rename(&id, &new_name, window, view_cx)
                    })
                    .unwrap_or(true)
                })
        });
    }

    fn perform_rename(
        &mut self,
        id: &str,
        new_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if new_name.is_empty() {
            self.notify_error("El nombre no puede estar vacío".into(), window, cx);
            return false;
        }
        let target = join_rel(&parent_of(id), new_name);
        match self.fs.rename(Path::new(id), &target) {
            Ok(()) => {
                self.set_status(format!("renombrado a «{new_name}»"));
                self.rebuild_tree(cx);
                true
            }
            Err(e) => {
                self.notify_error(format!("No se pudo renombrar: {e}"), window, cx);
                false
            }
        }
    }

    fn confirm_delete(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        id: String,
        name: String,
        is_dir: bool,
    ) {
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak = weak.clone();
            let id = id.clone();
            alert
                .title(format!("¿Eliminar «{name}»?"))
                .description(if is_dir {
                    "Se borrará la carpeta y todo su contenido de la copia de prueba."
                } else {
                    "Se borrará el archivo de la copia de prueba."
                })
                .confirm()
                .ok_text("Eliminar")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    weak.update(cx, |view, view_cx| {
                        view.perform_delete(&id, window, view_cx)
                    })
                    .unwrap_or(true)
                })
        });
    }

    fn perform_delete(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.fs.delete(Path::new(id)) {
            Ok(()) => {
                self.set_status(format!("borrado «{id}»"));
                self.rebuild_tree(cx);
                true
            }
            Err(e) => {
                self.notify_error(format!("No se pudo borrar: {e}"), window, cx);
                false
            }
        }
    }

    fn reset_copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match copy_fixture_projects(&self.copy_root) {
            Ok(n) => {
                self.set_status(format!("copia restaurada ({n} archivos)"));
                self.rebuild_tree(cx);
            }
            Err(e) => self.notify_error(format!("No se pudo restaurar la copia: {e}"), window, cx),
        }
    }

    // --- render ------------------------------------------------------------

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_none()
            .px_3()
            .py_2()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Icon::new(IconName::Search)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .w(px(280.))
                    .child(Input::new(&self.filter).cleanable(true)),
            )
            .child(div().flex_1())
            .child(
                Button::new("new-file")
                    .icon(IconName::FilePlus)
                    .label("Nuevo archivo")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let parent = this.selection_dir(cx);
                        this.open_create_dialog(window, cx, parent, EntryKind::File);
                    })),
            )
            .child(
                Button::new("new-dir")
                    .icon(IconName::FolderPlus)
                    .label("Nueva carpeta")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let parent = this.selection_dir(cx);
                        this.open_create_dialog(window, cx, parent, EntryKind::Dir);
                    })),
            )
            .child(
                Button::new("refresh")
                    .icon(IconName::RefreshCw)
                    .ghost()
                    .tooltip("Recargar el árbol")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_status("árbol recargado a mano");
                        this.rebuild_tree(cx);
                    })),
            )
            .child(
                Button::new("reset")
                    .icon(IconName::RotateCcw)
                    .ghost()
                    .tooltip("Restaurar la copia de fixtures/projects")
                    .on_click(cx.listener(|this, _, window, cx| this.reset_copy(window, cx))),
            )
    }

    fn render_event_panel(&self, cx: &mut Context<Self>, watcher_alive: bool) -> impl IntoElement {
        let rows = self
            .log
            .iter()
            .rev()
            .take(LOG_ROWS)
            .map(|line| {
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(line.clone())
            })
            .collect::<Vec<_>>();
        let (health_icon, health_text) = if watcher_alive {
            (IconName::CircleCheck, "watcher activo")
        } else {
            (IconName::TriangleAlert, "watcher detenido")
        };

        v_flex()
            .w(px(360.))
            .h_full()
            .flex_none()
            .border_l_1()
            .border_color(cx.theme().border)
            .p_3()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(health_icon).small().text_color(if watcher_alive {
                        cx.theme().success
                    } else {
                        cx.theme().danger
                    }))
                    .child(div().text_sm().child("Eventos del watcher")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{health_text} · debounce 200 ms · {}",
                        self.last_batch
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Toca archivos desde otra consola (pwsh):"),
            )
            .child(
                div()
                    .text_xs()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted)
                    .p_2()
                    .child(format!(
                        "Set-Content \"$env:TEMP\\{COPY_DIR_NAME}\\cv\\prueba.txt\" (Get-Random)"
                    )),
            )
            .child(
                v_flex()
                    .id("event-log")
                    .flex_1()
                    .min_h_0()
                    .gap_1()
                    .overflow_hidden()
                    .children(rows),
            )
    }

    fn render_status(
        &self,
        cx: &mut Context<Self>,
        selection: Option<(String, bool)>,
    ) -> impl IntoElement {
        let selection_text = match selection {
            Some((label, is_dir)) => format!(
                "selección: {label}{}",
                if is_dir { " (carpeta)" } else { "" }
            ),
            None => "sin selección".into(),
        };
        h_flex()
            .flex_none()
            .px_3()
            .py_2()
            .gap_4()
            .border_t_1()
            .border_color(cx.theme().border)
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.copy_root.display().to_string()),
            )
            .child(self.status.clone())
            .child(selection_text)
    }
}

impl Render for SpikeTree {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.weak_entity();
        let selection = self
            .tree
            .read(cx)
            .selected_entry()
            .map(|entry| (entry.item().label.to_string(), entry.is_folder()));
        let watcher_alive = self
            ._watcher
            .as_ref()
            .is_some_and(WatcherGuard::is_watching);

        let tree_element = tree(&self.tree, |_ix, entry, selected, _window, cx| {
            let is_dir = entry.is_folder();
            let icon = if is_dir {
                if entry.is_expanded() {
                    IconName::FolderOpen
                } else {
                    IconName::FolderClosed
                }
            } else {
                file_icon(entry.item().label.as_ref())
            };
            let icon_color = if is_dir {
                cx.theme().primary
            } else {
                cx.theme().muted_foreground
            };
            ListItem::new(entry.item().id.clone())
                .selected(selected)
                .pl(px(6. + 14. * entry.depth() as f32))
                .gap_2()
                .child(Icon::new(icon).small().text_color(icon_color))
                .child(entry.item().label.clone())
        })
        .context_menu(move |_ix, entry, menu, _window, _cx| context_menu_for(&weak, entry, menu));

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(div().flex_1().min_w_0().h_full().child(tree_element))
                    .child(self.render_event_panel(cx, watcher_alive)),
            )
            .child(self.render_status(cx, selection))
    }
}

// --- utilidades del spike ---------------------------------------------------

/// Menú contextual de una entrada del árbol.
fn context_menu_for(weak: &WeakEntity<SpikeTree>, entry: &TreeEntry, menu: PopupMenu) -> PopupMenu {
    let id = entry.item().id.to_string();
    let name = entry.item().label.to_string();
    let is_dir = entry.is_folder();
    let is_root = id.is_empty();
    let create_in = if is_dir { id.clone() } else { parent_of(&id) };

    let mut menu = menu;
    for (label, icon, kind) in [
        ("Nuevo archivo…", IconName::FilePlus, EntryKind::File),
        ("Nueva carpeta…", IconName::FolderPlus, EntryKind::Dir),
    ] {
        let weak = weak.clone();
        let target = create_in.clone();
        menu = menu.item(
            PopupMenuItem::new(label)
                .icon(icon)
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |view, view_cx| {
                        view.open_create_dialog(window, view_cx, target.clone(), kind)
                    });
                }),
        );
    }
    menu = menu.separator();
    {
        let weak = weak.clone();
        let id = id.clone();
        let name = name.clone();
        menu = menu.item(
            PopupMenuItem::new("Renombrar…")
                .icon(IconName::SquarePen)
                .disabled(is_root)
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |view, view_cx| {
                        view.open_rename_dialog(window, view_cx, id.clone(), name.clone())
                    });
                }),
        );
    }
    menu = menu.separator();
    {
        let weak = weak.clone();
        let id = id.clone();
        let name = name.clone();
        menu.item(
            PopupMenuItem::new("Eliminar…")
                .icon(IconName::Trash)
                .disabled(is_root)
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |view, view_cx| {
                        view.confirm_delete(window, view_cx, id.clone(), name.clone(), is_dir)
                    });
                }),
        )
    }
}

/// Copia `fixtures/projects` a `dest` (nunca se observa el fixture original).
fn copy_fixture_projects(dest: &Path) -> io::Result<usize> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("fixtures")
        .join("projects");
    let src = src.canonicalize().map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("fixtures/projects no encontrado en {}: {e}", src.display()),
        )
    })?;
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;
    let mut copied = 0usize;
    copy_tree(&src, dest, &mut copied)?;
    Ok(copied)
}

fn copy_tree(src: &Path, dest: &Path, copied: &mut usize) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&target)?;
            copy_tree(&entry.path(), &target, copied)?;
        } else {
            fs::copy(entry.path(), &target)?;
            *copied += 1;
        }
    }
    Ok(())
}

/// Clave de un path relativo para ids del árbol (`/` como separador).
fn path_key(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

fn parent_of(id: &str) -> String {
    match Path::new(id).parent() {
        Some(parent) => path_key(parent),
        None => String::new(),
    }
}

fn join_rel(parent: &str, name: &str) -> PathBuf {
    if parent.is_empty() {
        PathBuf::from(name)
    } else {
        Path::new(parent).join(name)
    }
}

fn name_matches(filter_lower: &str, name: &str) -> bool {
    filter_lower.is_empty() || name.to_lowercase().contains(filter_lower)
}

/// Icono por extensión (catálogo completo de Lucide vía `AllAssets`).
fn file_icon(name: &str) -> IconName {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "tex" | "sty" | "cls" | "bib" => IconName::FileCode,
        "pdf" => IconName::FileType,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" => IconName::FileImage,
        "json" | "toml" | "yaml" | "yml" => IconName::FileBraces,
        "md" | "txt" => IconName::FileText,
        "sh" | "ps1" | "cmd" | "bat" => IconName::FileTerminal,
        _ => IconName::File,
    }
}

fn hhmmss() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}
