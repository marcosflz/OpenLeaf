//! `TreePanel` — explorador de archivos de producción (M1-A1/F2 seed).
//!
//! Modelo y estilo vienen del spike M0-S6 **aprobado** (VS Code Explorer):
//! filas de 22 px, fuente 13 px, iconos de 16 px, toolbar compacto icon-only,
//! hover `foreground`@7 % y selección con el acento al 24 % (tokens en
//! [`crate::theme::tokens`]). Lo único que cambia respecto al spike es la
//! procedencia del proyecto: aquí se explota con [`FsService`] sobre la raíz
//! del proyecto activo (`workspace/<proyecto>`) en vez de una copia de
//! fixtures, y el panel de SALIDA vive como sección inferior del propio panel
//! para caber en una columna del dock.
//!
//! El panel nunca escribe fuera de su proyecto: crear/renombrar/borrar pasan
//! por `FsService`, que valida y usa `write_text_atomic`/rename seguros.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_channel::Sender;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants as _},
    dialog::{DialogAction, DialogClose, DialogDescription, DialogFooter},
    dock::{BasePanel, DockArea, Panel as DockPanel, PanelEvent},
    h_flex,
    input::{Input, InputEvent, InputState},
    list::ListItem,
    menu::{PopupMenu, PopupMenuItem},
    notification::Notification,
    tree::{TreeEntry, TreeEvent, TreeItem, TreeState, tree},
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Task, WeakEntity, Window, div, px,
};
use lsx_core::fs::{EntryKind, FsEvent, FsService, WatcherGuard};

use crate::layout::SlotKind;
use crate::panels::{Pane, PaneEvent, PaneState, pane_container};
use crate::project::ProjectInfo;
use crate::status::{StatusHandle, StatusMessage};
use crate::theme::tokens;

/// Máximo de líneas del log de SALIDA.
const LOG_CAP: usize = 200;
/// Filas visibles del log (más recientes primero).
const LOG_ROWS: usize = 40;

/// Explorador de archivos del proyecto activo.
pub struct TreePanel {
    state: PaneState,
    status: StatusHandle,
    project: Option<ProjectInfo>,
    fs: Option<FsService>,
    tree: Entity<TreeState>,
    /// Índice id (= ruta relativa `/`) → item, para restaurar la selección.
    tree_index: HashMap<String, TreeItem>,
    filter: Entity<InputState>,
    filter_text: String,
    filter_lower: String,
    /// Carpetas expandidas por id.
    expanded: HashSet<String>,
    /// Último resumen del watcher.
    last_batch: String,
    /// Última acción manual.
    last_action: String,
    /// Log de eventos (más nuevos al final).
    log: VecDeque<String>,
    /// Si la sección inferior de SALIDA está a la vista.
    show_output: bool,
    /// Emisor del watcher (compartido con el guard vivo; se reutiliza al
    /// cambiar de proyecto).
    tx: Sender<Vec<FsEvent>>,
    _filter_sub: Subscription,
    _tree_sub: Subscription,
    _watcher: Option<WatcherGuard>,
    watcher_alive: bool,
    _fs_task: Task<()>,
}

impl TreePanel {
    /// Crea el panel; `project` puede llegar después con [`TreePanel::set_project`].
    pub fn new(
        window: &mut Window,
        project: Option<ProjectInfo>,
        status: StatusHandle,
        dock_area: Option<WeakEntity<DockArea>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder_filter()));
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

        let mut this = Self {
            state: PaneState::new(SlotKind::Tree, dock_area, cx),
            status,
            project: None,
            fs: None,
            tree,
            tree_index: HashMap::new(),
            filter,
            filter_text: String::new(),
            filter_lower: String::new(),
            expanded: HashSet::new(),
            last_batch: String::new(),
            last_action: String::new(),
            log: VecDeque::new(),
            show_output: true,
            tx,
            _filter_sub: filter_sub,
            _tree_sub: tree_sub,
            _watcher: None,
            watcher_alive: false,
            _fs_task: fs_task,
        };
        if let Some(project) = project {
            this.set_project(project, cx);
        }
        this
    }

    /// Canal interno del watcher (guardado para re-suscribir al cambiar de proyecto).
    fn watcher_sender(&self) -> Sender<Vec<FsEvent>> {
        self.tx.clone()
    }

    /// Proyecto activo.
    pub fn project(&self) -> Option<&ProjectInfo> {
        self.project.as_ref()
    }

    /// Hoja que representa (compat con el shell).
    pub fn kind(&self) -> SlotKind {
        self.state.kind()
    }

    /// Si la hoja está visible.
    pub fn is_visible(&self) -> bool {
        self.state.is_visible()
    }

    /// Muestra/oculta sin avisar al shell.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.state.set_visible(visible, cx);
    }

    /// Cambia el proyecto: reabre el `FsService`, el watcher y el árbol.
    pub fn set_project(&mut self, project: ProjectInfo, cx: &mut Context<Self>) {
        self.project = Some(project);
        let root = self
            .project
            .as_ref()
            .map(|project| project.root.clone())
            .unwrap_or_default();
        self.fs = Some(FsService::new(&root));
        self.expanded.clear();
        self.expanded.insert(String::new());
        self.log.clear();

        // Watcher nuevo sobre la raíz del proyecto (el anterior se para al
        // sustituir el guard).
        self._watcher = None;
        self.watcher_alive = false;
        let tx = self.watcher_sender();
        if let Some(fs) = &self.fs {
            match fs.watch(move |batch| {
                let _ = tx.send_blocking(batch);
            }) {
                Ok(guard) => {
                    self._watcher = Some(guard);
                    self.watcher_alive = true;
                }
                Err(err) => {
                    self.push_log(format!(
                        "! {}",
                        rust_i18n::t!("ui.app.tree.error_watcher", error = err)
                    ));
                }
            }
        }

        let name = self
            .project
            .as_ref()
            .map(|project| project.name.clone())
            .unwrap_or_default();
        self.last_action = rust_i18n::t!("ui.app.tree.welcome", name = name).to_string();
        self.last_batch = String::new();
        self.status.send(StatusMessage::Project {
            name: Some(name.clone()),
        });
        self.expand_top_level(cx);
    }

    /// Recarga el árbol (botón de refresco / command palette).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.last_action = String::new();
        self.rebuild_tree(cx);
    }

    /// Expande las carpetas de primer nivel (primera impresión útil).
    fn expand_top_level(&mut self, cx: &mut Context<Self>) {
        if let Some(fs) = &self.fs
            && let Ok(entries) = fs.list_dir(Path::new(""))
        {
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
            .project
            .as_ref()
            .map(|project| project.name.clone())
            .unwrap_or_default();
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
        let Some(fs) = &self.fs else {
            return TreeItem::new(path_key(rel), name.to_owned());
        };
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
        if let Ok(entries) = fs.list_dir(rel) {
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
                            let empty = TreeItem::new(
                                format!("{}/", path_key(&child_rel)),
                                rust_i18n::t!("ui.app.tree.empty_dir"),
                            )
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
        let stamp = stamp();
        let mut created = 0usize;
        let mut modified = 0usize;
        let mut removed = 0usize;
        let mut renamed = 0usize;
        for event in &batch {
            let line = match event {
                FsEvent::Created(path) => {
                    created += 1;
                    format!("{stamp}  + {}", path.display())
                }
                FsEvent::Modified(path) => {
                    modified += 1;
                    format!("{stamp}  ~ {}", path.display())
                }
                FsEvent::Removed(path) => {
                    removed += 1;
                    format!("{stamp}  - {}", path.display())
                }
                FsEvent::Renamed(from, to) => {
                    renamed += 1;
                    format!("{stamp}  > {} => {}", from.display(), to.display())
                }
            };
            self.push_log(line);
        }
        self.last_batch = format!(
            "{} · {created}+ · {modified}~ · {removed}- · {renamed}>",
            batch.len()
        );
        self.rebuild_tree(cx);
    }

    fn push_log(&mut self, line: String) {
        self.log.push_back(line);
        while self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
    }

    // --- acciones ----------------------------------------------------------

    fn notify_error(&mut self, message: String, window: &mut Window, cx: &mut Context<Self>) {
        self.push_log(format!("{}  ! {message}", stamp()));
        self.last_action = message.clone();
        window.push_notification(Notification::error(message), cx);
        cx.notify();
    }

    fn open_create_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        parent: String,
        kind: EntryKind,
    ) {
        let placeholder = if kind == EntryKind::File {
            rust_i18n::t!("ui.app.tree.new_file_placeholder")
        } else {
            rust_i18n::t!("ui.app.tree.new_dir_placeholder")
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.to_string()));
        let where_text =
            rust_i18n::t!("ui.app.tree.where", path = self.display_path(&parent)).to_string();
        let title = if kind == EntryKind::File {
            rust_i18n::t!("ui.app.tree.new_file")
        } else {
            rust_i18n::t!("ui.app.tree.new_dir")
        }
        .to_string();
        let cancel = rust_i18n::t!("ui.app.tree.cancel").to_string();
        let create = rust_i18n::t!("ui.app.tree.create").to_string();

        let weak = cx.weak_entity();
        let initial_focus = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if initial_focus.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let input_entity = input.clone();
            let weak = weak.clone();
            let parent = parent.clone();
            dialog
                .title(title.clone())
                .width(px(460.))
                .child(DialogDescription::new().child(where_text.clone()))
                .child(Input::new(&input).cleanable(true))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .child(Button::new("tree-cancel").label(cancel.clone()).outline()),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("tree-create").label(create.clone()).primary()),
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
            self.notify_error(
                rust_i18n::t!("ui.app.tree.error_empty_name").to_string(),
                window,
                cx,
            );
            return false;
        }
        let Some(fs) = &self.fs else {
            return true;
        };
        let rel = join_rel(parent, name);
        let result = match kind {
            EntryKind::File => fs.create_file(&rel),
            EntryKind::Dir => fs.create_dir(&rel),
        };
        match result {
            Ok(()) => {
                self.last_action = format!("creado «{name}»");
                self.rebuild_tree(cx);
                cx.notify();
                true
            }
            Err(err) => {
                self.notify_error(
                    rust_i18n::t!("ui.app.tree.error_create", name = name, error = err).to_string(),
                    window,
                    cx,
                );
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
        let title = rust_i18n::t!("ui.app.tree.rename_title", name = name).to_string();
        let cancel = rust_i18n::t!("ui.app.tree.cancel").to_string();
        let rename = rust_i18n::t!("ui.app.tree.rename").to_string();
        let weak = cx.weak_entity();
        let initial_focus = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if initial_focus.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let input_entity = input.clone();
            let weak = weak.clone();
            let id = id.clone();
            dialog
                .title(title.clone())
                .width(px(460.))
                .child(Input::new(&input).cleanable(true))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().child(
                                Button::new("tree-rename-cancel")
                                    .label(cancel.clone())
                                    .outline(),
                            ),
                        )
                        .child(
                            DialogAction::new().child(
                                Button::new("tree-rename-ok")
                                    .label(rename.clone())
                                    .primary(),
                            ),
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
            self.notify_error(
                rust_i18n::t!("ui.app.tree.error_empty_name").to_string(),
                window,
                cx,
            );
            return false;
        }
        let Some(fs) = &self.fs else {
            return true;
        };
        let target = join_rel(&parent_of(id), new_name);
        match fs.rename(Path::new(id), &target) {
            Ok(()) => {
                self.last_action = format!("renombrado a «{new_name}»");
                self.rebuild_tree(cx);
                cx.notify();
                true
            }
            Err(err) => {
                self.notify_error(
                    rust_i18n::t!("ui.app.tree.error_rename", error = err).to_string(),
                    window,
                    cx,
                );
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
        let title = rust_i18n::t!("ui.app.tree.delete_title", name = name).to_string();
        let description = if is_dir {
            rust_i18n::t!("ui.app.tree.delete_dir_desc")
        } else {
            rust_i18n::t!("ui.app.tree.delete_file_desc")
        }
        .to_string();
        let confirm = rust_i18n::t!("ui.app.tree.delete_confirm").to_string();
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak = weak.clone();
            let id = id.clone();
            alert
                .title(title.clone())
                .description(description.clone())
                .confirm()
                .ok_text(confirm.clone())
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
        let Some(fs) = &self.fs else {
            return true;
        };
        match fs.delete(Path::new(id)) {
            Ok(()) => {
                self.last_action = format!("borrado «{id}»");
                self.rebuild_tree(cx);
                cx.notify();
                true
            }
            Err(err) => {
                self.notify_error(
                    rust_i18n::t!("ui.app.tree.error_delete", error = err).to_string(),
                    window,
                    cx,
                );
                false
            }
        }
    }

    /// Ruta visible de una carpeta relativa dentro del proyecto.
    fn display_path(&self, rel: &str) -> String {
        let root = self
            .project
            .as_ref()
            .map(|project| project.root.display().to_string())
            .unwrap_or_default();
        if rel.is_empty() {
            root
        } else {
            format!("{root}\\{}", rel.replace('/', "\\"))
        }
    }

    // --- render ------------------------------------------------------------

    /// Encabezado de sección estilo VS Code: etiqueta de 11 px versalitas a la
    /// izquierda y acciones solo-icono de 24 px a la derecha.
    fn render_section_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .flex_none()
            .h(px(tokens::BAR_HEIGHT))
            .px_2()
            .gap_1()
            .items_center()
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("ui.app.tree.header").to_string()),
            )
            .child(div().flex_1())
            .child(header_action(
                cx,
                "tree-new-file",
                IconName::FilePlus,
                &rust_i18n::t!("ui.app.tree.new_file"),
                cx.listener(|this, _, window, cx| {
                    let parent = this.selection_dir(cx);
                    this.open_create_dialog(window, cx, parent, EntryKind::File);
                }),
            ))
            .child(header_action(
                cx,
                "tree-new-dir",
                IconName::FolderPlus,
                &rust_i18n::t!("ui.app.tree.new_dir"),
                cx.listener(|this, _, window, cx| {
                    let parent = this.selection_dir(cx);
                    this.open_create_dialog(window, cx, parent, EntryKind::Dir);
                }),
            ))
            .child(header_action(
                cx,
                "tree-refresh",
                IconName::RefreshCw,
                &rust_i18n::t!("ui.app.tree.refresh"),
                cx.listener(|this, _, _, cx| this.refresh(cx)),
            ))
            .child(header_action(
                cx,
                "tree-toggle-output",
                IconName::PanelBottom,
                &rust_i18n::t!("ui.app.tree.toggle_output"),
                cx.listener(|this, _, _, cx| {
                    this.show_output = !this.show_output;
                    cx.notify();
                }),
            ))
    }

    /// Filtro slim (24 px de alto) bajo el encabezado.
    fn render_filter(&self) -> impl IntoElement {
        h_flex().flex_none().px_2().pb(px(6.)).child(
            div().w_full().child(
                Input::new(&self.filter)
                    .small()
                    .text_size(px(12.5))
                    .rounded(px(3.))
                    .cleanable(true),
            ),
        )
    }

    /// Sección «SALIDA»: cabecera de 11 px versalitas, líneas mono de 11 px y
    /// fondo ligeramente distinto al del árbol (estilo Output de VS Code).
    fn render_output_panel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mono = cx.theme().mono_font_family.clone();
        let rows = self
            .log
            .iter()
            .rev()
            .take(LOG_ROWS)
            .map(|line| {
                div()
                    .font_family(mono.clone())
                    .text_size(px(11.))
                    .text_color(cx.theme().muted_foreground)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(line.clone())
            })
            .collect::<Vec<_>>();
        let (health_icon, health_text, health_color) = if self.watcher_alive {
            (
                IconName::CircleCheck,
                rust_i18n::t!("ui.app.tree.watcher_active"),
                cx.theme().success,
            )
        } else {
            (
                IconName::TriangleAlert,
                rust_i18n::t!("ui.app.tree.watcher_stopped"),
                cx.theme().danger,
            )
        };
        let summary = if self.last_batch.is_empty() {
            self.last_action.clone()
        } else {
            rust_i18n::t!(
                "ui.app.tree.debounce",
                ms = 200,
                summary = self.last_batch.clone()
            )
            .to_string()
        };

        v_flex()
            .h(px(tokens::OUTPUT_HEIGHT))
            .flex_none()
            .w_full()
            .bg(cx.theme().background)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .flex_none()
                    .h(px(tokens::BAR_HEIGHT))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child(rust_i18n::t!("ui.app.tree.output_header").to_string()),
                    )
                    .child(div().flex_1())
                    .child(
                        Icon::new(health_icon)
                            .size(px(12.))
                            .text_color(health_color),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(health_text.to_string()),
                    ),
            )
            .child(
                v_flex()
                    .id("tree-output-log")
                    .flex_1()
                    .min_h_0()
                    .p_2()
                    .gap(px(3.))
                    .overflow_hidden()
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(summary),
                    )
                    .children(rows),
            )
    }

    /// Estado vacío: no hay proyecto abierto.
    fn render_no_project(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                Icon::new(IconName::FolderClosed)
                    .size_8()
                    .text_color(cx.theme().muted_foreground.opacity(0.6)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(rust_i18n::t!("ui.app.tree.no_project_title").to_string()),
            )
            .child(
                div()
                    .max_w(px(240.))
                    .text_xs()
                    .text_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("ui.app.tree.no_project_hint").to_string()),
            )
            .child(
                Button::new("tree-open-project")
                    .small()
                    .outline()
                    .icon(IconName::FolderOpen)
                    .label(rust_i18n::t!("ui.app.tree.open_folder").to_string())
                    .on_click(cx.listener(|_this, _, _window, cx| {
                        cx.emit(PaneEvent::OpenProjectRequested);
                    })),
            )
    }
}

impl Pane for TreePanel {
    fn pane_state(&self) -> &PaneState {
        &self.state
    }

    fn pane_state_mut(&mut self) -> &mut PaneState {
        &mut self.state
    }
}

impl BasePanel for TreePanel {
    fn panel_name(&self) -> &'static str {
        self.state.kind().panel_name()
    }

    fn visible(&self, cx: &App) -> bool {
        self.pane_visible(cx)
    }

    fn closable(&self, cx: &App) -> bool {
        self.pane_closable(cx)
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        // Ver la nota en `PlaceholderPanel`: la activación real del shell
        // llega por clic (`Pane::activate_by_user`).
        self.state.set_active(active, cx);
    }
}

impl DockPanel for TreePanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.pane_title().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.pane_title()
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        Some(vec![self.collapse_button(cx)])
    }
}

impl EventEmitter<PanelEvent> for TreePanel {}

impl EventEmitter<PaneEvent> for TreePanel {}

impl Focusable for TreePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.state.focus_handle().clone()
    }
}

impl Render for TreePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.weak_entity();

        if self.project.is_none() {
            return pane_container(SlotKind::Tree, cx).child(self.render_no_project(cx));
        }

        let tree_element = tree(&self.tree, |_ix, entry, selected, _window, cx| {
            let is_dir = entry.is_folder();
            let expanded = entry.is_expanded();
            let label = entry.item().label.clone();
            let depth = entry.depth() as f32;

            let (icon, icon_color) = if is_dir {
                (
                    if expanded {
                        IconName::FolderOpen
                    } else {
                        IconName::FolderClosed
                    },
                    cx.theme().muted_foreground,
                )
            } else {
                let icon = file_icon(label.as_ref());
                (icon, file_icon_color(icon, cx))
            };
            // Columna fija de chevron: VS Code también la reserva en archivos.
            let chevron_slot: gpui_kit::AnyElement = if is_dir {
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(px(tokens::TREE_ICON_SIZE))
                .text_color(cx.theme().muted_foreground)
                .into_any_element()
            } else {
                div().size(px(tokens::TREE_ICON_SIZE)).into_any_element()
            };
            let name_color = if selected {
                cx.theme().foreground
            } else {
                cx.theme().foreground.opacity(0.85)
            };

            ListItem::new(entry.item().id.clone())
                .selected(selected)
                .h(px(tokens::TREE_ROW_HEIGHT))
                .py_0()
                .px_0()
                .pl(px(
                    tokens::TREE_INDENT_BASE + tokens::TREE_INDENT_STEP * depth
                ))
                .pr(px(6.))
                .text_size(px(tokens::TREE_FONT_SIZE))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap(px(4.))
                        .child(chevron_slot)
                        .child(
                            Icon::new(icon)
                                .size(px(tokens::TREE_ICON_SIZE))
                                .text_color(icon_color),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(name_color)
                                .child(label),
                        ),
                )
        })
        .context_menu(move |_ix, entry, menu, _window, _cx| context_menu_for(&weak, entry, menu));

        let output = if self.show_output {
            Some(self.render_output_panel(cx))
        } else {
            None
        };

        pane_container(SlotKind::Tree, cx)
            .flex()
            .flex_col()
            .track_focus(self.state.focus_handle())
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.activate_by_user(window, cx)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .bg(cx.theme().background.blend(cx.theme().border.opacity(0.5)))
                    .child(self.render_section_header(cx))
                    .child(self.render_filter())
                    .child(div().flex_1().min_h_0().child(tree_element)),
            )
            .children(output)
    }
}

// --- utilidades -------------------------------------------------------------

/// Menú contextual de una entrada del árbol.
fn context_menu_for(weak: &WeakEntity<TreePanel>, entry: &TreeEntry, menu: PopupMenu) -> PopupMenu {
    let id = entry.item().id.to_string();
    let name = entry.item().label.to_string();
    let is_dir = entry.is_folder();
    let is_root = id.is_empty();
    let create_in = if is_dir { id.clone() } else { parent_of(&id) };

    let mut menu = menu;
    for (label, icon, kind) in [
        (
            rust_i18n::t!("ui.app.tree.menu_new_file"),
            IconName::FilePlus,
            EntryKind::File,
        ),
        (
            rust_i18n::t!("ui.app.tree.menu_new_dir"),
            IconName::FolderPlus,
            EntryKind::Dir,
        ),
    ] {
        let weak = weak.clone();
        let target = create_in.clone();
        menu = menu.item(PopupMenuItem::new(label.to_string()).icon(icon).on_click(
            move |_, window, cx| {
                let _ = weak.update(cx, |view, view_cx| {
                    view.open_create_dialog(window, view_cx, target.clone(), kind)
                });
            },
        ));
    }
    menu = menu.separator();
    {
        let weak = weak.clone();
        let id = id.clone();
        let name = name.clone();
        menu = menu.item(
            PopupMenuItem::new(rust_i18n::t!("ui.app.tree.menu_rename").to_string())
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
            PopupMenuItem::new(rust_i18n::t!("ui.app.tree.menu_delete").to_string())
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

/// Placeholder del filtro (i18n).
fn placeholder_filter() -> SharedString {
    rust_i18n::t!("ui.app.tree.filter_placeholder")
        .to_string()
        .into()
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

/// Icono por extensión (catálogo Lucide completo vía `AllAssets`).
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

/// Color del icono de archivo: tintes sobrios por tipo (Seti de VS Code).
fn file_icon_color(icon: IconName, cx: &App) -> gpui_kit::Hsla {
    match icon {
        IconName::FileCode => cx.theme().blue,
        IconName::FileImage => cx.theme().magenta,
        IconName::FileBraces => cx.theme().yellow,
        IconName::FileTerminal => cx.theme().green,
        _ => cx.theme().muted_foreground,
    }
}

/// Botón de acción del encabezado: solo icono (24×24), fondo transparente,
/// hover/active sutiles y tooltip.
fn header_action(
    cx: &App,
    id: &'static str,
    icon: IconName,
    tooltip: &str,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .icon(icon)
        .small()
        .rounded(px(3.))
        .custom(
            ButtonCustomVariant::new(cx)
                .foreground(cx.theme().muted_foreground)
                .hover(cx.theme().foreground.opacity(0.08))
                .active(cx.theme().foreground.opacity(0.13)),
        )
        .tooltip(tooltip.to_owned())
        .on_click(on_click)
}

/// Hora local `HH:MM:SS` para las líneas del log de SALIDA.
fn stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
        % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_keys_use_forward_slashes_and_stable_parents() {
        assert_eq!(path_key(Path::new("cv\\main.tex")), "cv/main.tex");
        assert_eq!(parent_of("cv/main.tex"), "cv");
        assert_eq!(parent_of("main.tex"), "");
        assert_eq!(join_rel("", "nuevo.tex"), PathBuf::from("nuevo.tex"));
        assert_eq!(join_rel("cv", "nuevo.tex"), PathBuf::from("cv/nuevo.tex"));
    }

    #[test]
    fn name_matching_is_case_insensitive_and_empty_matches_all() {
        assert!(name_matches("", "main.tex"));
        assert!(name_matches("main", "Main.TeX"));
        assert!(name_matches("tex", "main.tex"));
        assert!(!name_matches("bib", "main.tex"));
    }

    #[test]
    fn file_icons_follow_the_approved_extension_mapping() {
        assert!(matches!(file_icon("main.tex"), IconName::FileCode));
        assert!(matches!(file_icon("cv.pdf"), IconName::FileType));
        assert!(matches!(file_icon("foto.PNG"), IconName::FileImage));
        assert!(matches!(file_icon("settings.json"), IconName::FileBraces));
        assert!(matches!(file_icon("notas.md"), IconName::FileText));
        assert!(matches!(file_icon("script.ps1"), IconName::FileTerminal));
        assert!(matches!(file_icon("desconocido"), IconName::File));
    }

    #[test]
    fn the_output_log_is_capped() {
        let mut log: VecDeque<String> = VecDeque::new();
        for index in 0..(LOG_CAP + 25) {
            log.push_back(format!("linea {index}"));
            while log.len() > LOG_CAP {
                log.pop_front();
            }
        }
        assert_eq!(log.len(), LOG_CAP);
        assert!(log.front().is_some_and(|line| line.ends_with("25")));
    }

    #[test]
    fn timestamps_have_the_hh_mm_ss_shape() {
        let value = stamp();
        assert_eq!(value.len(), 8);
        assert_eq!(value.matches(':').count(), 2);
    }
}
