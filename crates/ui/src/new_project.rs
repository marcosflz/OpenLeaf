//! Wizard «Nuevo proyecto» (M1-F4).
//!
//! Diálogo modal del kit: nombre, plantilla (`article`/`report`/`beamer`) y
//! carpeta destino. La carpeta se puede escribir a mano o elegir con el
//! selector nativo (`rfd`, en modo async para no bloquear el hilo de UI).
//!
//! Al crear llama al callback de [`open_new_project_dialog`] con el `Project`
//! resultante: el shell decide abrirlo (`AppRoot::open_project`) o el panel
//! de árbol adoptarlo localmente y reenviar el evento.
//!
//! **Punto de enganche del shell** (`app.rs`, que no edita el stream F):
//!
//! ```ignore
//! // item «Nuevo proyecto…» del menú del toolbar (o comando de la paleta):
//! // `self.project` es el `ProjectService` de UI.
//! let cfg = new_project::NewProjectDialogConfig {
//!     default_parent: self.project.workspace().to_path_buf(),
//! };
//! let weak = cx.weak_entity();
//! new_project::open_new_project_dialog(window, cx, cfg, move |project, window, cx| {
//!     let _ = weak.update(cx, |root, cx| root.open_project(project.root.clone(), window, cx));
//! });
//! ```
//!
//! El andamiaje (`main.tex` desde plantilla + `AGENTS.md` +
//! `.opencode/agents/latex-fix.md`) lo ejecuta `lsx_core::project`.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogAction, DialogClose, DialogDescription, DialogFooter},
    h_flex,
    input::{Input, InputState},
    notification::Notification,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Entity, Focusable as _, IntoElement, ParentElement as _, SharedString,
    Styled as _, Window, div, px,
};

use crate::project::{Project, TemplateId};

/// Configuración del wizard.
#[derive(Clone, Debug)]
pub struct NewProjectDialogConfig {
    /// Carpeta padre por defecto (`<root>/workspace`); el usuario puede
    /// cambiarla escribiendo o con el selector nativo.
    pub default_parent: PathBuf,
}

/// Callback de proyecto creado (compartido entre repintados del diálogo).
type CreatedCallback = dyn Fn(Project, &mut Window, &mut App);

/// Estado interno del formulario: la plantilla elegida persiste entre
/// repintados del diálogo.
struct NewProjectForm {
    template: TemplateId,
}

impl Default for NewProjectForm {
    fn default() -> Self {
        Self {
            template: TemplateId::Article,
        }
    }
}

/// Abre el diálogo «Nuevo proyecto».
///
/// `on_created` se invoca una sola vez, ya con el proyecto materializado en
/// disco, antes de cerrar el diálogo.
pub fn open_new_project_dialog(
    window: &mut Window,
    cx: &mut App,
    config: NewProjectDialogConfig,
    on_created: impl Fn(Project, &mut Window, &mut App) + 'static,
) {
    let name_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(rust_i18n::t!("ui.app.new_project.name_placeholder").to_string())
    });
    let destination_input = cx.new(|cx| {
        InputState::new(window, cx).default_value(config.default_parent.display().to_string())
    });
    let form = cx.new(|_| NewProjectForm::default());
    let on_created: Rc<CreatedCallback> = Rc::new(on_created);
    let initial_focus = std::cell::Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            name_input.focus_handle(cx).focus(window, cx);
        }
        dialog
            .title(rust_i18n::t!("ui.app.new_project.title").to_string())
            .width(px(560.))
            .child(
                DialogDescription::new()
                    .child(rust_i18n::t!("ui.app.new_project.description").to_string()),
            )
            .child(field_label(
                cx,
                rust_i18n::t!("ui.app.new_project.name").to_string(),
            ))
            .child(Input::new(&name_input).cleanable(true))
            .child(field_label(
                cx,
                rust_i18n::t!("ui.app.new_project.template").to_string(),
            ))
            .child(template_choices(&form, cx))
            .child(field_label(
                cx,
                rust_i18n::t!("ui.app.new_project.destination").to_string(),
            ))
            .child(destination_row(&destination_input, &config.default_parent))
            .footer(
                DialogFooter::new()
                    .child(
                        DialogClose::new().child(
                            Button::new("new-project-cancel")
                                .label(rust_i18n::t!("ui.app.new_project.cancel").to_string())
                                .outline(),
                        ),
                    )
                    .child(
                        DialogAction::new().child(
                            Button::new("new-project-create")
                                .label(rust_i18n::t!("ui.app.new_project.create").to_string())
                                .primary(),
                        ),
                    ),
            )
            .on_ok({
                let name_input = name_input.clone();
                let destination_input = destination_input.clone();
                let form = form.clone();
                let on_created = on_created.clone();
                move |_, window, cx| {
                    let name = name_input.read(cx).value().trim().to_string();
                    let parent = PathBuf::from(
                        destination_input
                            .read(cx)
                            .value()
                            .trim()
                            .replace('/', std::path::MAIN_SEPARATOR_STR),
                    );
                    let template = form.read(cx).template;
                    match crate::project::create_project(&parent, &name, template) {
                        Ok(project) => {
                            on_created(project, window, cx);
                            true
                        }
                        Err(err) => {
                            window.push_notification(
                                Notification::error(
                                    rust_i18n::t!(
                                        "ui.app.new_project.error",
                                        error = err.to_string()
                                    )
                                    .to_string(),
                                ),
                                cx,
                            );
                            false
                        }
                    }
                }
            })
    });
}

/// Fila «carpeta destino»: campo editable + selector nativo de carpetas.
fn destination_row(
    destination: &Entity<InputState>,
    default_parent: &std::path::Path,
) -> impl IntoElement {
    let initial_dir = default_parent.to_path_buf();
    let destination_for_pick = destination.clone();

    let browse = Button::new("new-project-browse")
        .icon(IconName::FolderSearch)
        .label(rust_i18n::t!("ui.app.new_project.browse").to_string())
        .small()
        .outline()
        .on_click(move |_, window, cx| {
            let destination = destination_for_pick.clone();
            let initial_dir = initial_dir.clone();
            // Selector nativo en el pool async: nunca bloquea el hilo de UI.
            // `.detach()` porque el futuro vive hasta que el usuario elige.
            window
                .spawn(cx, async move |cx| {
                    let picked = rfd::AsyncFileDialog::new()
                        .set_directory(&initial_dir)
                        .set_title(rust_i18n::t!("ui.app.new_project.browse").to_string())
                        .pick_folder()
                        .await;
                    if let Some(handle) = picked {
                        let path = handle.path().to_string_lossy().into_owned();
                        let _ = cx.update(move |window, cx| {
                            destination.update(cx, |state, cx| {
                                state.set_value(path.clone(), window, cx);
                            });
                        });
                    }
                })
                .detach();
        });

    h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .child(
            div()
                .flex_1()
                .child(Input::new(destination).cleanable(true)),
        )
        .child(browse)
}

/// Fila de plantillas: tres opciones seleccionables con su descripción.
fn template_choices(form: &Entity<NewProjectForm>, cx: &App) -> impl IntoElement {
    let selected = form.read(cx).template;
    let mut column = v_flex().w_full().gap_1();
    for template in TemplateId::ALL {
        let (label, note) = match template {
            TemplateId::Article => (
                rust_i18n::t!("ui.app.new_project.template_article").to_string(),
                rust_i18n::t!("ui.app.new_project.template_article_note").to_string(),
            ),
            TemplateId::Report => (
                rust_i18n::t!("ui.app.new_project.template_report").to_string(),
                rust_i18n::t!("ui.app.new_project.template_report_note").to_string(),
            ),
            TemplateId::Beamer => (
                rust_i18n::t!("ui.app.new_project.template_beamer").to_string(),
                rust_i18n::t!("ui.app.new_project.template_beamer_note").to_string(),
            ),
        };
        let form = form.clone();
        column = column.child(
            Button::new(SharedString::from(format!(
                "new-project-template-{}",
                template.id()
            )))
            .small()
            .ghost()
            .selected(selected == template)
            .w_full()
            .on_click(move |_, _, cx| {
                form.update(cx, |form, cx| {
                    form.template = template;
                    cx.notify();
                });
            })
            .child(
                v_flex()
                    .w_full()
                    .items_start()
                    .gap_0p5()
                    .child(div().text_sm().child(label))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(note),
                    ),
            ),
        );
    }
    column
}

/// Etiqueta de campo (11 px, color apagado).
fn field_label(cx: &App, text: String) -> impl IntoElement {
    div()
        .mt_2()
        .text_size(px(11.))
        .text_color(cx.theme().muted_foreground)
        .child(text)
}
