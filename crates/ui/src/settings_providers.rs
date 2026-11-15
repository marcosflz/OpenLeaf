//! Tarjetas de proveedores de IA de Ajustes: **Claves de API** y **Suscripciones**.
//!
//! Replica la gestión de modelos de nexus sobre [`crate::ai_setup`]:
//!
//! - Cada familia ([`ProviderKind`]) tiene su sección; varios proveedores pueden estar activos
//!   a la vez.
//! - Cada tarjeta muestra el estado (Activo · Desactivado · Sin configurar · Revisar), un
//!   interruptor, la credencial (clave o CLI vinculado) y los **modelos activos** como chips.
//!   Pulsar un chip lo hace predeterminado (★, `ai.model`).
//! - «Elegir modelos» abre, dentro de la tarjeta, el catálogo real del proveedor (con
//!   búsqueda, activar todos/ninguno y alta manual por nombre).
//! - En «Claves de API» solo se ven los servicios añadidos; el resto, en «Añadir servicio».
//!   Las suscripciones se muestran siempre.
//!
//! Las credenciales, el interruptor y los modelos se guardan al momento en
//! `config/secrets/ai.json` y se emite [`ProvidersEvent::Changed`]. El predeterminado es un
//! campo de `settings.json`: se emite [`ProvidersEvent::SetDefault`] y lo guarda el botón
//! Guardar del diálogo, como el resto de ajustes.

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement as _,
    switch::Switch,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
    px,
};

use crate::ai_setup::{
    AiSetup, CredentialSource, LinkFlow, ModelChoice, ProviderKind, ProviderStatus,
};

/// Filas máximas del catálogo a la vez (OpenRouter lista cientos): se pide refinar.
const MAX_VISIBLE_MODELS: usize = 200;

/// Eventos hacia el formulario de Ajustes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvidersEvent {
    /// Cambió la configuración guardada (clave, interruptor, modelos…): hay que releer el
    /// catálogo y avisar al chat.
    Changed,
    /// La persona eligió un modelo predeterminado (`proveedor/modelo`).
    SetDefault(String),
}

/// Estado del selector de modelos abierto en una tarjeta.
struct Chooser {
    provider: &'static str,
    /// `None` mientras se consulta el proveedor.
    catalog: Option<Vec<ModelChoice>>,
    error: Option<String>,
    /// Selección en curso (ordenada).
    chosen: Vec<ModelChoice>,
    query: Entity<InputState>,
    custom: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl Chooser {
    fn is_chosen(&self, id: &str) -> bool {
        self.chosen.iter().any(|model| model.id == id)
    }

    fn toggle(&mut self, model: &ModelChoice, on: bool) {
        if on {
            if !self.is_chosen(&model.id) {
                self.chosen.push(model.clone());
            }
        } else {
            self.chosen.retain(|chosen| chosen.id != model.id);
        }
    }

    fn all(&self) -> Vec<ModelChoice> {
        merged(&self.chosen, self.catalog.as_deref())
    }

    fn visible(&self, cx: &App) -> Vec<ModelChoice> {
        filtered(self.all(), &self.query.read(cx).value())
    }

    fn ordered(&self) -> Vec<ModelChoice> {
        ordered(&self.chosen, self.catalog.as_deref())
    }
}

/// Catálogo + lo ya elegido que no aparece en él (escrito a mano o retirado), delante.
fn merged(chosen: &[ModelChoice], catalog: Option<&[ModelChoice]>) -> Vec<ModelChoice> {
    let catalog = catalog.unwrap_or_default();
    let mut list: Vec<ModelChoice> = chosen
        .iter()
        .filter(|model| !catalog.iter().any(|m| m.id == model.id))
        .cloned()
        .collect();
    list.extend(catalog.iter().cloned());
    list
}

/// Filtra por id o nombre, sin distinguir mayúsculas.
fn filtered(list: Vec<ModelChoice>, query: &str) -> Vec<ModelChoice> {
    let needle = query.trim().to_lowercase();
    list.into_iter()
        .filter(|model| {
            needle.is_empty()
                || model.id.to_lowercase().contains(&needle)
                || model.name.to_lowercase().contains(&needle)
        })
        .collect()
}

/// Selección en el orden de [`merged`], con el nombre que tenía al elegirse.
fn ordered(chosen: &[ModelChoice], catalog: Option<&[ModelChoice]>) -> Vec<ModelChoice> {
    merged(chosen, catalog)
        .into_iter()
        .filter_map(|model| chosen.iter().find(|c| c.id == model.id).cloned())
        .collect()
}

/// Lista de proveedores de una familia.
pub struct ProviderList {
    kind: ProviderKind,
    setup: Option<AiSetup>,
    statuses: Vec<ProviderStatus>,
    /// Servicios añadidos en esta sesión pero aún sin clave.
    drafts: Vec<&'static str>,
    key_inputs: Vec<(&'static str, Entity<InputState>)>,
    url_inputs: Vec<(&'static str, Entity<InputState>)>,
    default_model: Option<String>,
    chooser: Option<Chooser>,
    confirm_remove: Option<&'static str>,
    info: Option<String>,
    error: Option<String>,
}

impl EventEmitter<ProvidersEvent> for ProviderList {}

impl ProviderList {
    pub fn new(
        kind: ProviderKind,
        setup: Option<AiSetup>,
        default_model: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let statuses = Self::read_statuses(kind, setup.as_ref());
        let key_inputs = statuses
            .iter()
            .filter(|status| status.spec.link == LinkFlow::Key)
            .map(|status| {
                let placeholder = if status.spec.placeholder.is_empty() {
                    rust_i18n::t!("ui.app.settings.providers.key_placeholder").to_string()
                } else {
                    status.spec.placeholder.to_owned()
                };
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .masked(true)
                        .placeholder(placeholder)
                });
                (status.spec.id, input)
            })
            .collect();
        let url_inputs = statuses
            .iter()
            .filter_map(|status| {
                let default = status.spec.custom_base_url?;
                let value = status
                    .base_url
                    .clone()
                    .unwrap_or_else(|| default.to_owned());
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value)
                        .placeholder(default)
                });
                Some((status.spec.id, input))
            })
            .collect();
        Self {
            kind,
            setup,
            statuses,
            drafts: Vec::new(),
            key_inputs,
            url_inputs,
            default_model,
            chooser: None,
            confirm_remove: None,
            info: None,
            error: None,
        }
    }

    fn read_statuses(kind: ProviderKind, setup: Option<&AiSetup>) -> Vec<ProviderStatus> {
        setup
            .map(|setup| {
                setup
                    .status()
                    .into_iter()
                    .filter(|status| status.spec.kind == kind)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Relee `ai.json` (tras un cambio aquí o en la otra sección).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.statuses = Self::read_statuses(self.kind, self.setup.as_ref());
        cx.notify();
    }

    /// Sincroniza el predeterminado cuando cambia desde otra parte (selector general).
    pub fn set_default_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        if self.default_model != model {
            self.default_model = model;
            cx.notify();
        }
    }

    fn status(&self, id: &str) -> Option<&ProviderStatus> {
        self.statuses.iter().find(|status| status.spec.id == id)
    }

    fn input_for(
        inputs: &[(&'static str, Entity<InputState>)],
        id: &str,
    ) -> Option<Entity<InputState>> {
        inputs
            .iter()
            .find(|(input_id, _)| *input_id == id)
            .map(|(_, input)| input.clone())
    }

    fn shown(&self) -> Vec<&ProviderStatus> {
        self.statuses
            .iter()
            .filter(|status| {
                self.kind == ProviderKind::Subscription
                    || status.added
                    || self.drafts.contains(&status.spec.id)
            })
            .collect()
    }

    fn addable(&self) -> Vec<&'static crate::ai_setup::ProviderSpec> {
        self.statuses
            .iter()
            .filter(|status| {
                self.kind == ProviderKind::Api
                    && !status.added
                    && !self.drafts.contains(&status.spec.id)
            })
            .map(|status| status.spec)
            .collect()
    }

    fn report(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        match result {
            Ok(info) => {
                self.error = None;
                self.info = Some(info);
            }
            Err(error) => {
                self.info = None;
                self.error = Some(error);
            }
        }
        cx.notify();
    }

    /// Tras guardar: relee, avisa y, si el predeterminado ya no existe, propone otro.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.reload(cx);
        cx.emit(ProvidersEvent::Changed);
    }

    // --- acciones -----------------------------------------------------------------------

    fn add_draft(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if !self.drafts.contains(&id) {
            self.drafts.push(id);
        }
        self.info = None;
        self.error = None;
        cx.notify();
    }

    fn discard_draft(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.drafts.retain(|draft| *draft != id);
        cx.notify();
    }

    fn save_credentials(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(setup) = self.setup.clone() else {
            return;
        };
        let Some(status) = self.status(id).cloned() else {
            return;
        };
        let name = status.spec.name;
        let key = Self::input_for(&self.key_inputs, id)
            .map(|input| input.read(cx).value().trim().to_owned())
            .unwrap_or_default();
        let url = Self::input_for(&self.url_inputs, id)
            .map(|input| input.read(cx).value().trim().to_owned());

        let result = (|| {
            if let Some(url) = &url {
                setup.set_base_url(id, url)?;
            }
            if !key.is_empty() {
                setup.save_key(id, &key)?;
            } else if status.spec.needs_key && !status.has_credential {
                return Err(crate::ai_setup::AiSetupError::Invalid(
                    rust_i18n::t!("ui.app.settings.providers.key_required").to_string(),
                ));
            } else {
                setup.add_provider(id)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                if let Some(input) = Self::input_for(&self.key_inputs, id) {
                    input.update(cx, |state, cx| state.set_value("", window, cx));
                }
                self.drafts.retain(|draft| *draft != id);
                let first = !status.added;
                self.changed(cx);
                if first && self.default_model.is_none() {
                    // Primer proveedor: su primer modelo pasa a ser el predeterminado.
                    if let Some(model) = self.status(id).and_then(|s| s.models.first()) {
                        self.make_default(id, &model.id.clone(), cx);
                    }
                }
                let key = if first {
                    "ui.app.settings.providers.added"
                } else {
                    "ui.app.settings.providers.saved"
                };
                self.report(Ok(rust_i18n::t!(key, provider = name).to_string()), cx);
            }
            Err(error) => self.report(
                Err(rust_i18n::t!("ui.app.settings.providers.error", error = error).to_string()),
                cx,
            ),
        }
    }

    fn remove_key(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(setup) = self.setup.clone() else {
            return;
        };
        let name = self.status(id).map(|s| s.spec.name).unwrap_or(id);
        let result = setup.remove_key(id);
        self.changed(cx);
        self.report(
            result
                .map(|()| {
                    rust_i18n::t!("ui.app.settings.providers.key_removed", provider = name)
                        .to_string()
                })
                .map_err(|error| {
                    rust_i18n::t!("ui.app.settings.providers.error", error = error).to_string()
                }),
            cx,
        );
    }

    fn set_enabled(&mut self, id: &'static str, enabled: bool, cx: &mut Context<Self>) {
        let Some(setup) = self.setup.clone() else {
            return;
        };
        if let Err(error) = setup.set_enabled(id, enabled) {
            self.report(
                Err(rust_i18n::t!("ui.app.settings.providers.error", error = error).to_string()),
                cx,
            );
            return;
        }
        self.changed(cx);
    }

    fn remove_provider(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(setup) = self.setup.clone() else {
            return;
        };
        self.confirm_remove = None;
        if self.chooser.as_ref().is_some_and(|c| c.provider == id) {
            self.chooser = None;
        }
        let name = self.status(id).map(|s| s.spec.name).unwrap_or(id);
        let result = setup.remove_provider(id);
        self.drafts.retain(|draft| *draft != id);
        self.changed(cx);
        self.report(
            result
                .map(|()| {
                    rust_i18n::t!("ui.app.settings.providers.removed", provider = name).to_string()
                })
                .map_err(|error| {
                    rust_i18n::t!("ui.app.settings.providers.error", error = error).to_string()
                }),
            cx,
        );
    }

    fn make_default(&mut self, provider: &str, model: &str, cx: &mut Context<Self>) {
        let full = format!("{provider}/{model}");
        self.default_model = Some(full.clone());
        cx.emit(ProvidersEvent::SetDefault(full));
        cx.notify();
    }

    fn link_claude(&mut self, cx: &mut Context<Self>) {
        let Some(setup) = self.setup.clone() else {
            return;
        };
        let result = setup
            .login_claude()
            .map(|()| rust_i18n::t!("ui.app.settings.providers.claude_login_opened").to_string())
            .map_err(|error| {
                rust_i18n::t!("ui.app.settings.providers.error", error = error).to_string()
            });
        self.report(result, cx);
    }

    fn check_link(&mut self, cx: &mut Context<Self>) {
        self.changed(cx);
        let ready = self
            .statuses
            .iter()
            .any(|s| s.spec.link == LinkFlow::Cli && s.has_credential);
        let key = if ready {
            "ui.app.settings.providers.claude_ready"
        } else {
            "ui.app.settings.providers.claude_missing"
        };
        self.report(Ok(rust_i18n::t!(key).to_string()), cx);
    }

    // --- selector de modelos ------------------------------------------------------------

    fn open_chooser(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(status) = self.status(id).cloned() else {
            return;
        };
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(rust_i18n::t!("ui.app.settings.providers.search").to_string())
        });
        let custom = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(rust_i18n::t!("ui.app.settings.providers.custom").to_string())
        });
        let subscription = cx.subscribe(&query, |_this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let custom_enter = cx.subscribe_in(
            &custom,
            window,
            |this, _input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.add_custom(window, cx);
                }
            },
        );
        let can_list = status.spec.can_list && self.setup.is_some();
        self.chooser = Some(Chooser {
            provider: id,
            catalog: if can_list { None } else { Some(Vec::new()) },
            error: None,
            chosen: status.models.clone(),
            query,
            custom,
            _subscriptions: vec![subscription, custom_enter],
        });
        self.confirm_remove = None;
        cx.notify();

        let Some(setup) = self.setup.clone().filter(|_| can_list) else {
            return;
        };
        let task = cx.background_spawn(async move { setup.list_models(id) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(chooser) = this.chooser.as_mut().filter(|c| c.provider == id) else {
                    return;
                };
                match result {
                    Ok(models) => chooser.catalog = Some(models),
                    Err(error) => {
                        chooser.catalog = Some(Vec::new());
                        chooser.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn close_chooser(&mut self, cx: &mut Context<Self>) {
        self.chooser = None;
        cx.notify();
    }

    fn toggle_model(&mut self, model: ModelChoice, on: bool, cx: &mut Context<Self>) {
        if let Some(chooser) = &mut self.chooser {
            chooser.toggle(&model, on);
            cx.notify();
        }
    }

    fn set_all_visible(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(chooser) = &self.chooser else {
            return;
        };
        let visible = chooser.visible(cx);
        if let Some(chooser) = &mut self.chooser {
            for model in &visible {
                chooser.toggle(model, on);
            }
        }
        cx.notify();
    }

    fn add_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        let id = chooser.custom.read(cx).value().trim().to_owned();
        if id.is_empty() {
            return;
        }
        chooser.toggle(&ModelChoice::new(id.clone(), id), true);
        chooser
            .custom
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    fn save_models(&mut self, cx: &mut Context<Self>) {
        let (Some(setup), Some(chooser)) = (self.setup.clone(), &self.chooser) else {
            return;
        };
        let provider = chooser.provider;
        let models = chooser.ordered();
        match setup.set_models(provider, &models) {
            Ok(saved) => {
                self.chooser = None;
                self.changed(cx);
                // El predeterminado de este proveedor se ha desactivado: pasa al primero.
                let prefix = format!("{provider}/");
                if let Some(current) = self.default_model.clone()
                    && let Some(model) = current.strip_prefix(&prefix)
                    && !saved.iter().any(|m| m.id == model)
                    && let Some(first) = saved.first()
                {
                    self.make_default(provider, &first.id.clone(), cx);
                }
                self.report(
                    Ok(rust_i18n::t!(
                        "ui.app.settings.providers.models_saved",
                        count = saved.len()
                    )
                    .to_string()),
                    cx,
                );
            }
            Err(error) => {
                if let Some(chooser) = &mut self.chooser {
                    chooser.error = Some(error.to_string());
                }
                cx.notify();
            }
        }
    }

    // --- render -------------------------------------------------------------------------

    fn render_card(&self, status: &ProviderStatus, cx: &mut Context<Self>) -> AnyElement {
        let spec = status.spec;
        let id = spec.id;
        let theme = cx.theme();
        let cli_missing = spec.link == LinkFlow::Cli && !status.has_credential;
        let (label, color) = if cli_missing {
            (
                rust_i18n::t!("ui.app.settings.providers.state_unlinked"),
                theme.muted_foreground,
            )
        } else if status.has_credential && !status.enabled {
            (
                rust_i18n::t!("ui.app.settings.providers.state_disabled"),
                theme.muted_foreground,
            )
        } else if status.configured {
            (
                rust_i18n::t!("ui.app.settings.providers.state_active"),
                theme.success,
            )
        } else if status.has_credential {
            (
                rust_i18n::t!("ui.app.settings.providers.state_review"),
                theme.warning,
            )
        } else {
            (
                rust_i18n::t!("ui.app.settings.providers.state_missing"),
                theme.muted_foreground,
            )
        };
        let muted = theme.muted_foreground;
        let is_draft = self.drafts.contains(&id) && !status.added;

        let header = h_flex()
            .items_start()
            .justify_between()
            .gap_3()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(spec.name),
                    )
                    .child(div().text_xs().text_color(muted).child(spec.help)),
            )
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .child(div().text_xs().text_color(color).child(label.to_string()))
                    .when(status.has_credential, |row| {
                        row.child(
                            Switch::new(SharedString::from(format!("provider-enabled-{id}")))
                                .small()
                                .checked(status.enabled)
                                .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                    this.set_enabled(id, *checked, cx);
                                })),
                        )
                    }),
            );

        let credentials = match spec.link {
            LinkFlow::Cli => self.render_cli(status, cx),
            LinkFlow::Key => self.render_key(status, cx),
        };

        let models = status
            .has_credential
            .then(|| self.render_models(status, cx));
        let chooser = self
            .chooser
            .as_ref()
            .filter(|chooser| chooser.provider == id)
            .map(|chooser| self.render_chooser(chooser, cx));

        let footer = (status.added && spec.link == LinkFlow::Key || is_draft).then(|| {
            if self.confirm_remove == Some(id) {
                h_flex()
                    .justify_end()
                    .items_center()
                    .gap_2()
                    .child(
                        div().text_xs().text_color(muted).child(
                            rust_i18n::t!(
                                "ui.app.settings.providers.remove_confirm",
                                provider = spec.name
                            )
                            .to_string(),
                        ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("provider-remove-no-{id}")))
                            .small()
                            .ghost()
                            .label(rust_i18n::t!("ui.app.settings.providers.cancel").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_remove = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("provider-remove-yes-{id}")))
                            .small()
                            .danger()
                            .label(rust_i18n::t!("ui.app.settings.providers.remove").to_string())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_provider(id, cx);
                            })),
                    )
                    .into_any_element()
            } else {
                h_flex()
                    .justify_end()
                    .child(
                        Button::new(SharedString::from(format!("provider-remove-{id}")))
                            .small()
                            .ghost()
                            .icon(IconName::Delete)
                            .label(if is_draft {
                                rust_i18n::t!("ui.app.settings.providers.discard").to_string()
                            } else {
                                rust_i18n::t!("ui.app.settings.providers.remove").to_string()
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if is_draft {
                                    this.discard_draft(id, cx);
                                } else {
                                    this.confirm_remove = Some(id);
                                    cx.notify();
                                }
                            })),
                    )
                    .into_any_element()
            }
        });

        v_flex()
            .id(SharedString::from(format!("settings-provider-{id}")))
            .w_full()
            .gap_3()
            .px_3()
            .py_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .when(status.has_credential && !status.enabled, |card| {
                card.opacity(0.75)
            })
            .child(header)
            .child(credentials)
            .children(models)
            .children(chooser)
            .children(footer)
            .into_any_element()
    }

    fn render_cli(&self, status: &ProviderStatus, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let detail = match &status.source {
            CredentialSource::Cli(path) => rust_i18n::t!(
                "ui.app.settings.providers.cli_found",
                path = path.display().to_string()
            )
            .to_string(),
            _ => rust_i18n::t!("ui.app.settings.providers.cli_missing").to_string(),
        };
        v_flex()
            .gap_2()
            .child(div().text_xs().text_color(muted).child(detail))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("provider-claude-login")
                            .small()
                            .outline()
                            .icon(IconName::SquareTerminal)
                            .label(
                                rust_i18n::t!("ui.app.settings.providers.claude_login").to_string(),
                            )
                            .disabled(!status.has_credential)
                            .on_click(cx.listener(|this, _, _, cx| this.link_claude(cx))),
                    )
                    .child(
                        Button::new("provider-claude-check")
                            .small()
                            .ghost()
                            .icon(IconName::RefreshCw)
                            .label(
                                rust_i18n::t!("ui.app.settings.providers.claude_check").to_string(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.check_link(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_key(&self, status: &ProviderStatus, cx: &mut Context<Self>) -> AnyElement {
        let spec = status.spec;
        let id = spec.id;
        let muted = cx.theme().muted_foreground;
        let source = match &status.source {
            CredentialSource::Saved { last4 } => Some(
                rust_i18n::t!(
                    "ui.app.settings.providers.key_saved",
                    last4 = last4.as_str()
                )
                .to_string(),
            ),
            CredentialSource::Env(var) => {
                Some(rust_i18n::t!("ui.app.settings.providers.key_env", var = *var).to_string())
            }
            _ => None,
        };
        let saved = matches!(status.source, CredentialSource::Saved { .. });
        let key_label = if spec.needs_key {
            rust_i18n::t!("ui.app.settings.providers.key_label").to_string()
        } else {
            rust_i18n::t!("ui.app.settings.providers.key_label_optional").to_string()
        };
        let action = if status.added {
            rust_i18n::t!("ui.app.settings.providers.save").to_string()
        } else {
            rust_i18n::t!("ui.app.settings.providers.add").to_string()
        };
        let key_input = Self::input_for(&self.key_inputs, id);
        let url_input = Self::input_for(&self.url_inputs, id);

        v_flex()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .child(key_label),
            )
            .children(url_input.map(|input| {
                div()
                    .w_full()
                    .child(Input::new(&input).small())
                    .into_any_element()
            }))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .children(key_input.map(|input| {
                        div()
                            .flex_1()
                            .child(Input::new(&input).small().mask_toggle())
                    }))
                    .child(
                        Button::new(SharedString::from(format!("provider-save-{id}")))
                            .small()
                            .primary()
                            .label(action)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.save_credentials(id, window, cx);
                            })),
                    )
                    .when(saved, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("provider-unkey-{id}")))
                                .small()
                                .ghost()
                                .label(
                                    rust_i18n::t!("ui.app.settings.providers.key_remove")
                                        .to_string(),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_key(id, cx);
                                })),
                        )
                    }),
            )
            .children(source.map(|text| div().text_xs().text_color(muted).child(text)))
            .into_any_element()
    }

    fn render_models(&self, status: &ProviderStatus, cx: &mut Context<Self>) -> AnyElement {
        let id = status.spec.id;
        let theme = cx.theme();
        let chips = status.models.iter().map(|model| {
            let full = format!("{id}/{}", model.id);
            let is_default = self.default_model.as_deref() == Some(full.as_str());
            let model_id = model.id.clone();
            let tooltip = if is_default {
                rust_i18n::t!("ui.app.settings.providers.default_tooltip").to_string()
            } else {
                rust_i18n::t!(
                    "ui.app.settings.providers.make_default",
                    model = model.id.as_str()
                )
                .to_string()
            };
            Button::new(SharedString::from(format!("provider-model-{full}")))
                .xsmall()
                .when(is_default, |button| button.primary().icon(IconName::Star))
                .when(!is_default, |button| button.outline())
                .label(model.name.clone())
                .tooltip(tooltip)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !is_default {
                        this.make_default(id, &model_id, cx);
                    }
                }))
        });
        let open = self.chooser.as_ref().is_some_and(|c| c.provider == id);

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(
                                rust_i18n::t!(
                                    "ui.app.settings.providers.active_models",
                                    count = status.models.len()
                                )
                                .to_string(),
                            ),
                    )
                    .when(!open, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("provider-choose-{id}")))
                                .small()
                                .outline()
                                .icon(IconName::Search)
                                .label(
                                    rust_i18n::t!("ui.app.settings.providers.choose").to_string(),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_chooser(id, window, cx);
                                })),
                        )
                    }),
            )
            .when(status.models.is_empty(), |list| {
                list.child(
                    div()
                        .text_xs()
                        .text_color(theme.warning)
                        .child(rust_i18n::t!("ui.app.settings.providers.no_models").to_string()),
                )
            })
            .child(h_flex().flex_wrap().gap_1().children(chips))
            .into_any_element()
    }

    fn render_chooser(&self, chooser: &Chooser, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let visible = chooser.visible(cx);
        let total = chooser.all().len();
        let filtering = !chooser.query.read(cx).value().trim().is_empty();
        let provider = chooser.provider;

        let list: AnyElement = match &chooser.catalog {
            None => h_flex()
                .p_3()
                .gap_2()
                .items_center()
                .child(Icon::new(IconName::LoaderCircle).xsmall().text_color(muted))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(rust_i18n::t!("ui.app.settings.providers.loading").to_string()),
                )
                .into_any_element(),
            Some(_) if visible.is_empty() => div()
                .p_3()
                .text_xs()
                .text_color(muted)
                .child(if filtering {
                    rust_i18n::t!("ui.app.settings.providers.no_match").to_string()
                } else {
                    rust_i18n::t!("ui.app.settings.providers.cannot_list").to_string()
                })
                .into_any_element(),
            Some(_) => v_flex()
                .children(visible.iter().take(MAX_VISIBLE_MODELS).map(|model| {
                    let checked = chooser.is_chosen(&model.id);
                    let label = if model.name == model.id {
                        model.id.clone()
                    } else {
                        format!("{} · {}", model.name, model.id)
                    };
                    let entry = model.clone();
                    h_flex().px_2().py_1().child(
                        Checkbox::new(SharedString::from(format!(
                            "provider-pick-{provider}-{}",
                            model.id
                        )))
                        .checked(checked)
                        .label(label)
                        .on_click(cx.listener(
                            move |this, on: &bool, _, cx| {
                                this.toggle_model(entry.clone(), *on, cx);
                            },
                        )),
                    )
                }))
                .when(visible.len() > MAX_VISIBLE_MODELS, |list| {
                    list.child(
                        div().px_2().py_1().text_xs().text_color(muted).child(
                            rust_i18n::t!(
                                "ui.app.settings.providers.refine",
                                shown = MAX_VISIBLE_MODELS,
                                total = visible.len()
                            )
                            .to_string(),
                        ),
                    )
                })
                .into_any_element(),
        };

        v_flex()
            .gap_2()
            .p_2()
            .rounded(theme.radius)
            .bg(theme.muted)
            .child(
                Input::new(&chooser.query)
                    .small()
                    .prefix(Icon::new(IconName::Search).xsmall()),
            )
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div().text_xs().text_color(muted).child(
                            rust_i18n::t!(
                                "ui.app.settings.providers.chosen",
                                count = chooser.chosen.len(),
                                total = total
                            )
                            .to_string(),
                        ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("provider-pick-all")
                                    .xsmall()
                                    .ghost()
                                    .label(if filtering {
                                        rust_i18n::t!("ui.app.settings.providers.all_filtered")
                                            .to_string()
                                    } else {
                                        rust_i18n::t!("ui.app.settings.providers.all").to_string()
                                    })
                                    .on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.set_all_visible(true, cx)
                                        }),
                                    ),
                            )
                            .child(
                                Button::new("provider-pick-none")
                                    .xsmall()
                                    .ghost()
                                    .label(
                                        rust_i18n::t!("ui.app.settings.providers.none").to_string(),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.set_all_visible(false, cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("provider-catalog-{provider}")))
                    .max_h(px(260.))
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .overflow_y_scrollbar()
                    .child(list),
            )
            .children(chooser.error.as_ref().map(|error| {
                div()
                    .text_xs()
                    .text_color(theme.warning)
                    .child(error.clone())
            }))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&chooser.custom).small()))
                    .child(
                        Button::new("provider-pick-custom")
                            .small()
                            .outline()
                            .icon(IconName::Plus)
                            .label(rust_i18n::t!("ui.app.settings.providers.add").to_string())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_custom(window, cx)),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("provider-pick-cancel")
                            .small()
                            .ghost()
                            .label(rust_i18n::t!("ui.app.settings.providers.cancel").to_string())
                            .on_click(cx.listener(|this, _, _, cx| this.close_chooser(cx))),
                    )
                    .child(
                        Button::new("provider-pick-save")
                            .small()
                            .primary()
                            .label(rust_i18n::t!("ui.app.settings.providers.save").to_string())
                            .disabled(chooser.chosen.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.save_models(cx))),
                    ),
            )
            .into_any_element()
    }
}

impl Render for ProviderList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        if self.setup.is_none() {
            return v_flex()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(rust_i18n::t!("ui.app.settings.path_unknown").to_string()),
                )
                .into_any_element();
        }
        let shown: Vec<ProviderStatus> = self.shown().into_iter().cloned().collect();
        let addable = self.addable();
        let weak = cx.weak_entity();

        let empty = shown.is_empty().then(|| {
            div()
                .w_full()
                .px_4()
                .py_6()
                .rounded(cx.theme().radius)
                .border_1()
                .border_dashed()
                .border_color(cx.theme().border)
                .text_center()
                .text_xs()
                .text_color(muted)
                .child(rust_i18n::t!("ui.app.settings.providers.empty_api").to_string())
        });

        let add = (!addable.is_empty()).then(|| {
            Button::new("provider-add")
                .small()
                .outline()
                .icon(IconName::Plus)
                .label(rust_i18n::t!("ui.app.settings.providers.add_service").to_string())
                .dropdown_menu(move |mut menu, _window, _cx| {
                    for spec in &addable {
                        let weak = weak.clone();
                        let id = spec.id;
                        menu = menu.item(PopupMenuItem::new(spec.name).on_click(
                            move |_, _window, cx| {
                                let _ = weak.update(cx, |this, cx| this.add_draft(id, cx));
                            },
                        ));
                    }
                    menu
                })
        });

        let feedback = v_flex()
            .gap_1()
            .children(self.error.as_ref().map(|error| {
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(
                        Icon::new(IconName::CircleAlert)
                            .xsmall()
                            .text_color(cx.theme().danger),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(error.clone()),
                    )
            }))
            .children(
                self.info
                    .as_ref()
                    .map(|info| div().text_xs().text_color(muted).child(info.clone())),
            );

        v_flex()
            .w_full()
            .gap_3()
            .child(feedback)
            .children(empty)
            .children(shown.iter().map(|status| self.render_card(status, cx)))
            .children(add.map(|button| h_flex().child(button)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[ModelChoice]) -> Vec<&str> {
        list.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn la_seleccion_conserva_el_orden_del_catalogo_y_los_manuales_delante() {
        let catalog = vec![
            ModelChoice::new("a", "A"),
            ModelChoice::new("b", "B"),
            ModelChoice::new("c", "C"),
        ];
        let chosen = vec![
            ModelChoice::new("c", "C"),
            ModelChoice::new("propio", "propio"),
            ModelChoice::new("a", "A"),
        ];
        assert_eq!(
            ids(&merged(&chosen, Some(&catalog))),
            ["propio", "a", "b", "c"]
        );
        assert_eq!(ids(&ordered(&chosen, Some(&catalog))), ["propio", "a", "c"]);
        // Sin catálogo (aún cargando o sin listado) se ve lo ya elegido.
        assert_eq!(ids(&merged(&chosen, None)), ["c", "propio", "a"]);
    }

    #[test]
    fn el_filtro_busca_en_id_y_nombre_sin_mayusculas() {
        let list = vec![
            ModelChoice::new("anthropic/claude-opus-5-5", "Claude Opus"),
            ModelChoice::new("openai/gpt-5.5", "GPT-5.5"),
        ];
        assert_eq!(
            ids(&filtered(list.clone(), "  OPUS ")),
            ["anthropic/claude-opus-5-5"]
        );
        assert_eq!(ids(&filtered(list.clone(), "gpt")), ["openai/gpt-5.5"]);
        assert_eq!(filtered(list, "").len(), 2);
    }
}
