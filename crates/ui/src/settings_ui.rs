//! Ajustes 2.0 (P2-UI-1 / P2-UI-2 / P2-AI-1).
//!
//! Sustituye el formulario plano de cinco campos por una superficie con
//! navegación lateral —Apariencia · Editor · Compilación · PDF · IA ·
//! Avanzado— y un borrador validado antes de guardar.
//!
//! Reglas de producto:
//! - Solo se exponen campos del esquema `config/settings.json`; los defaults
//!   viven en `lsx-core`.
//! - Los campos numéricos se validan con los mismos rangos que
//!   `Settings::validate`; un error no cierra el diálogo.
//! - El tema y el acento se previsualizan en vivo; Cancelar/Escape restauran
//!   el tema original. Guardar usa `AppRoot::apply_settings`, que valida de
//!   nuevo y persiste de forma atómica.
//! - La conexión de IA vive en [`crate::ai_setup`]: proveedores, claves en
//!   `config/secrets/ai.json` y prueba de modelo (en `background_spawn`).
//!   Como en nexus, los proveedores se gestionan en dos secciones propias,
//!   **Claves de API** y **Suscripciones** ([`crate::settings_providers`]); la
//!   sección IA conserva el modelo predeterminado y el comportamiento del
//!   asistente.
//!
//! La firma pública [`open_dialog`] se conserva para el shell.

use std::cell::Cell;

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    radio::{Radio, RadioGroup},
    scroll::ScrollableElement as _,
    select::{SearchableVec, Select, SelectEvent, SelectState},
    switch::Switch,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, WeakEntity, Window, div,
    px,
};
use lsx_core::config::{Settings, UiSettings, parse_accent};
use lsx_core::root::PortableRoot;

use crate::ai_setup::{self, AiSetup, AiSetupError, ProviderKind, ProviderStatus};
use crate::app::AppRoot;
use crate::settings_providers::{ProviderList, ProvidersEvent};
use crate::theme;

/// Rango de debounce aceptado (ms), alineado con `Settings::validate`.
const DEBOUNCE_RANGE: (u32, u32) = (0, 10_000);
/// Rango de timeout de compilación (s).
const TIMEOUT_RANGE: (u64, u64) = (5, 7200);
/// Rango de zoom del PDF.
const ZOOM_RANGE: (f32, f32) = (0.1, 8.0);
/// Rango de caché del PDF (MB).
const CACHE_RANGE: (u32, u32) = (16, 4096);
/// Rango de autoguardado (s).
const AUTOSAVE_RANGE: (u32, u32) = (5, 3600);
/// Rango del tamaño de fuente del editor (px).
const FONT_SIZE_RANGE: (f32, f32) = (6.0, 72.0);
/// Rango de la tabulación (espacios).
const TAB_SIZE_RANGE: (u32, u32) = (1, 16);

/// Temas ofrecidos (índice ↔ valor de `ui.theme`).
const THEMES: [&str; 3] = ["dark", "light", "system"];
/// Modos de zoom del PDF (índice ↔ valor de `pdf.zoomMode`).
const ZOOM_MODES: [&str; 4] = ["fit-width", "fit-page", "actual", "custom"];
/// Niveles de log (índice ↔ valor de `advanced.logLevel`).
const LOG_LEVELS: [&str; 5] = ["trace", "debug", "info", "warn", "error"];
/// Presets de permisos válidos según el esquema. `all` es el valor que escribe
/// el menú del chat; `project` limita la aprobación a ediciones del proyecto.
const PERMISSION_PRESETS: [&str; 3] = ["ask", "project", "all"];
/// Modos del asistente (índice ↔ valor de `ai.agent`).
const MODES: [&str; 3] = ["build", "latex-fix", "ask"];
/// Ancho de los campos numéricos.
const NUMBER_WIDTH: f32 = 128.;
/// Ancho de los campos de texto medio.
const TEXT_WIDTH: f32 = 300.;
/// Ancho del campo de argumentos extra.
const ARGS_WIDTH: f32 = 360.;

/// Sección visible de Ajustes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Appearance,
    Editor,
    Compilation,
    Pdf,
    Ai,
    ApiKeys,
    Subscriptions,
    Advanced,
}

impl Section {
    const ALL: [Section; 8] = [
        Section::Appearance,
        Section::Editor,
        Section::Compilation,
        Section::Pdf,
        Section::Ai,
        Section::ApiKeys,
        Section::Subscriptions,
        Section::Advanced,
    ];

    fn label(self) -> String {
        match self {
            Self::Appearance => rust_i18n::t!("ui.app.settings.sections.appearance").to_string(),
            Self::Editor => rust_i18n::t!("ui.app.settings.sections.editor").to_string(),
            Self::Compilation => rust_i18n::t!("ui.app.settings.sections.compilation").to_string(),
            Self::Pdf => rust_i18n::t!("ui.app.settings.sections.pdf").to_string(),
            Self::Ai => rust_i18n::t!("ui.app.settings.sections.ai").to_string(),
            Self::ApiKeys => rust_i18n::t!("ui.app.settings.sections.api_keys").to_string(),
            Self::Subscriptions => {
                rust_i18n::t!("ui.app.settings.sections.subscriptions").to_string()
            }
            Self::Advanced => rust_i18n::t!("ui.app.settings.sections.advanced").to_string(),
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Appearance => IconName::Palette,
            Self::Editor => IconName::FileText,
            Self::Compilation => IconName::Play,
            Self::Pdf => IconName::BookOpen,
            Self::Ai => IconName::Bot,
            Self::ApiKeys => IconName::Globe,
            Self::Subscriptions => IconName::CircleUser,
            Self::Advanced => IconName::Settings,
        }
    }
}

/// Campo con validación propia, para pintar el error junto al control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Theme,
    Accent,
    AutosaveInterval,
    FontSize,
    TabSize,
    Debounce,
    Timeout,
    ExtraArgs,
    PdfZoomMode,
    PdfZoom,
    PdfCache,
    AiAgent,
    LogLevel,
    PermissionPreset,
    Apply,
}

impl Field {
    /// Sección donde vive el campo (para llevar el foco al primer error).
    fn section(self) -> Option<Section> {
        match self {
            Self::Theme | Self::Accent => Some(Section::Appearance),
            Self::AutosaveInterval | Self::FontSize | Self::TabSize => Some(Section::Editor),
            Self::Debounce | Self::Timeout | Self::ExtraArgs => Some(Section::Compilation),
            Self::PdfZoomMode | Self::PdfZoom | Self::PdfCache => Some(Section::Pdf),
            Self::AiAgent | Self::PermissionPreset => Some(Section::Ai),
            Self::LogLevel => Some(Section::Advanced),
            Self::Apply => None,
        }
    }
}

/// Error de validación asociado a un campo.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldError {
    field: Field,
    message: String,
}

impl FieldError {
    fn new(field: Field, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

/// Borrador plano y validable de los campos que edita Ajustes.
///
/// Se separa del formulario para poder testear rangos, enums y persistencia
/// sin levantar GPUI.
#[derive(Debug, Clone, PartialEq)]
struct SettingsDraft {
    theme: String,
    accent: String,
    autosave_enabled: bool,
    autosave_interval: String,
    font_family: String,
    font_size: String,
    tab_size: String,
    soft_wrap: bool,
    auto_compile: bool,
    sync_scroll: bool,
    auto_install_packages: bool,
    clean_on_success: bool,
    debounce_ms: String,
    timeout_secs: String,
    main_file: String,
    extra_args: String,
    pdf_zoom_mode: String,
    pdf_zoom: String,
    pdf_max_cache_mb: String,
    pdf_open_external: bool,
    ai_auto_start: bool,
    ai_model: String,
    ai_agent: String,
    ai_permission_preset: String,
    ai_show_reasoning: bool,
    log_level: String,
}

impl SettingsDraft {
    fn from_settings(settings: &Settings) -> Self {
        Self {
            theme: settings.ui.theme.clone(),
            accent: settings.ui.accent.clone(),
            autosave_enabled: settings.editor.autosave.enabled,
            autosave_interval: settings.editor.autosave.interval_secs.to_string(),
            font_family: settings.editor.font_family.clone().unwrap_or_default(),
            font_size: format_float(settings.editor.font_size),
            tab_size: settings.editor.tab_size.to_string(),
            soft_wrap: settings.editor.soft_wrap,
            auto_compile: settings.latex.auto_compile,
            sync_scroll: settings.latex.sync_scroll,
            auto_install_packages: settings.latex.auto_install_packages,
            clean_on_success: settings.latex.clean_on_success,
            debounce_ms: settings.latex.debounce_ms.to_string(),
            timeout_secs: settings.latex.timeout_secs.to_string(),
            main_file: settings.latex.main_file.clone().unwrap_or_default(),
            extra_args: settings.latex.extra_args.join(" "),
            pdf_zoom_mode: settings.pdf.zoom_mode.clone(),
            pdf_zoom: format_float(settings.pdf.zoom),
            pdf_max_cache_mb: settings.pdf.max_cache_mb.to_string(),
            pdf_open_external: settings.pdf.open_external_on_double_click,
            ai_auto_start: settings.ai.auto_start,
            ai_model: settings.ai.model.clone().unwrap_or_default(),
            ai_agent: settings.ai.agent.clone(),
            ai_permission_preset: settings.ai.permission_preset.clone(),
            ai_show_reasoning: settings.ai.show_reasoning,
            log_level: settings.advanced.log_level.clone(),
        }
    }

    /// Valida todo y, solo si no hay errores, escribe sobre `settings`.
    ///
    /// Es atómico: trabaja sobre una copia y no deja valores a medias cuando
    /// algún campo falla.
    fn apply_to(&self, settings: &mut Settings) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();

        let theme = self.theme.trim().to_ascii_lowercase();
        if !THEMES.contains(&theme.as_str()) {
            errors.push(FieldError::new(
                Field::Theme,
                rust_i18n::t!("ui.app.settings.error_theme", value = self.theme.trim()),
            ));
        }

        let accent = match normalize_accent(&self.accent) {
            Some(value) => value,
            None => {
                errors.push(FieldError::new(
                    Field::Accent,
                    rust_i18n::t!("ui.app.settings.error_accent", value = self.accent.trim()),
                ));
                String::new()
            }
        };

        let autosave_interval = parse_u32_field(
            &self.autosave_interval,
            AUTOSAVE_RANGE,
            Field::AutosaveInterval,
            &mut errors,
        );
        let font_size = parse_f32_field(
            &self.font_size,
            FONT_SIZE_RANGE,
            Field::FontSize,
            &mut errors,
        );
        let tab_size = parse_u32_field(&self.tab_size, TAB_SIZE_RANGE, Field::TabSize, &mut errors);
        let debounce_ms = parse_u32_field(
            &self.debounce_ms,
            DEBOUNCE_RANGE,
            Field::Debounce,
            &mut errors,
        );
        let timeout_secs = parse_u64_field(
            &self.timeout_secs,
            TIMEOUT_RANGE,
            Field::Timeout,
            &mut errors,
        );
        let extra_args = parse_extra_args(&self.extra_args, &mut errors);
        let pdf_zoom = parse_f32_field(&self.pdf_zoom, ZOOM_RANGE, Field::PdfZoom, &mut errors);
        let pdf_cache = parse_u32_field(
            &self.pdf_max_cache_mb,
            CACHE_RANGE,
            Field::PdfCache,
            &mut errors,
        );
        let agent = self.ai_agent.trim().to_ascii_lowercase();
        if !MODES.contains(&agent.as_str()) {
            errors.push(FieldError::new(
                Field::AiAgent,
                rust_i18n::t!("ui.app.settings.error_agent_empty"),
            ));
        }

        let pdf_zoom_mode = self.pdf_zoom_mode.trim().to_ascii_lowercase();
        if !ZOOM_MODES.contains(&pdf_zoom_mode.as_str()) {
            errors.push(FieldError::new(
                Field::PdfZoomMode,
                rust_i18n::t!(
                    "ui.app.settings.error_zoom_mode",
                    value = self.pdf_zoom_mode.trim()
                ),
            ));
        }

        let log_level = self.log_level.trim().to_ascii_lowercase();
        if !LOG_LEVELS.contains(&log_level.as_str()) {
            errors.push(FieldError::new(
                Field::LogLevel,
                rust_i18n::t!(
                    "ui.app.settings.error_log_level",
                    value = self.log_level.trim()
                ),
            ));
        }

        let permission = self.ai_permission_preset.trim().to_ascii_lowercase();
        if !PERMISSION_PRESETS.contains(&permission.as_str()) && permission != "auto" {
            errors.push(FieldError::new(
                Field::PermissionPreset,
                rust_i18n::t!(
                    "ui.app.settings.error_permission",
                    value = self.ai_permission_preset.trim()
                ),
            ));
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let main_file = {
            let value = self.main_file.trim();
            (!value.is_empty()).then(|| value.to_owned())
        };
        let ai_model = {
            let value = self.ai_model.trim();
            (!value.is_empty()).then(|| value.to_owned())
        };

        let mut next = settings.clone();
        next.ui.theme = theme;
        next.ui.accent = accent;
        next.editor.autosave.enabled = self.autosave_enabled;
        next.editor.autosave.interval_secs = autosave_interval.expect("intervalo validado");
        next.editor.font_family = {
            let value = self.font_family.trim();
            (!value.is_empty()).then(|| value.to_owned())
        };
        next.editor.font_size = font_size.expect("tamaño validado");
        next.editor.tab_size = tab_size.expect("tabulación validada");
        next.editor.soft_wrap = self.soft_wrap;
        next.latex.auto_compile = self.auto_compile;
        next.latex.sync_scroll = self.sync_scroll;
        next.latex.auto_install_packages = self.auto_install_packages;
        next.latex.clean_on_success = self.clean_on_success;
        next.latex.debounce_ms = debounce_ms.expect("debounce validado");
        next.latex.timeout_secs = timeout_secs.expect("timeout validado");
        next.latex.main_file = main_file;
        next.latex.extra_args = extra_args.expect("args validados");
        next.pdf.zoom_mode = pdf_zoom_mode;
        next.pdf.zoom = pdf_zoom.expect("zoom validado");
        next.pdf.max_cache_mb = pdf_cache.expect("caché validada");
        next.pdf.open_external_on_double_click = self.pdf_open_external;
        next.ai.auto_start = self.ai_auto_start;
        next.ai.model = ai_model;
        next.ai.agent = agent;
        next.ai.permission_preset = permission;
        next.ai.show_reasoning = self.ai_show_reasoning;
        next.advanced.log_level = log_level;

        *settings = next;
        Ok(())
    }
}

/// Estado de la sección IA (proveedores, modelos y mensajes).
#[derive(Default)]
struct AiUiState {
    setup: Option<AiSetup>,
    setup_error: Option<String>,
    statuses: Vec<ProviderStatus>,
    models: Vec<String>,
    probing: bool,
    info: Option<String>,
    error: Option<String>,
}

/// Formulario de ajustes (contenido del diálogo).
struct SettingsForm {
    original: Settings,
    settings: Settings,
    root: WeakEntity<AppRoot>,
    root_dir: Option<PortableRoot>,
    root_error: Option<String>,
    section: Section,
    dirty: bool,
    committed: bool,
    errors: Vec<FieldError>,
    action_error: Option<String>,

    theme_index: usize,
    zoom_mode_index: usize,
    log_index: usize,
    permission_index: usize,
    mode_index: usize,

    accent_input: Entity<InputState>,
    autosave_interval: Entity<InputState>,
    font_family: Entity<InputState>,
    font_size: Entity<InputState>,
    tab_size: Entity<InputState>,
    debounce: Entity<InputState>,
    timeout: Entity<InputState>,
    main_file: Entity<InputState>,
    extra_args: Entity<InputState>,
    pdf_zoom: Entity<InputState>,
    pdf_cache: Entity<InputState>,
    ai_model: Entity<InputState>,
    model_select: Entity<SelectState<SearchableVec<SharedString>>>,
    /// Tarjetas de «Claves de API».
    api_list: Entity<ProviderList>,
    /// Tarjetas de «Suscripciones».
    subscription_list: Entity<ProviderList>,

    ai: AiUiState,
    _subscriptions: Vec<Subscription>,
}

impl SettingsForm {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        settings: Settings,
        root: WeakEntity<AppRoot>,
    ) -> Self {
        let original = settings.clone();
        let theme_index = index_of(&THEMES, &settings.ui.theme, 0);
        let zoom_mode_index = index_of(&ZOOM_MODES, &settings.pdf.zoom_mode, 0);
        let log_index = index_of(&LOG_LEVELS, &settings.advanced.log_level, 2);
        let permission_index = permission_index(&settings.ai.permission_preset);
        let mode_index = index_of(&MODES, &settings.ai.agent, 0);

        let accent_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.ui.accent.clone())
                .placeholder("#RRGGBB")
        });
        let autosave_interval = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.editor.autosave.interval_secs.to_string())
                .placeholder("30")
        });
        let font_family = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.editor.font_family.clone().unwrap_or_default())
                .placeholder("Cascadia Code, Consolas…")
        });
        let font_size = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format_float(settings.editor.font_size))
                .placeholder("14")
        });
        let tab_size = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.editor.tab_size.to_string())
                .placeholder("2")
        });
        let debounce = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.latex.debounce_ms.to_string())
                .placeholder("400")
        });
        let timeout = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.latex.timeout_secs.to_string())
                .placeholder("600")
        });
        let main_file = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.latex.main_file.clone().unwrap_or_default())
                .placeholder("p. ej. main.tex")
        });
        let extra_args = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.latex.extra_args.join(" "))
                .placeholder("-shell-escape")
        });
        let pdf_zoom = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format_float(settings.pdf.zoom))
                .placeholder("1.0")
        });
        let pdf_cache = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.pdf.max_cache_mb.to_string())
                .placeholder("256")
        });
        let ai_model = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.ai.model.clone().unwrap_or_default())
                .placeholder("proveedor/modelo")
        });
        let model_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<SharedString>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });

        let (root_dir, root_error) = match PortableRoot::detect() {
            Ok(root) => (Some(root), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let setup = root_dir.clone().map(AiSetup::from_root);
        let default_model = settings.ai.model.clone();
        let api_list = cx.new(|cx| {
            ProviderList::new(
                ProviderKind::Api,
                setup.clone(),
                default_model.clone(),
                window,
                cx,
            )
        });
        let subscription_list = cx.new(|cx| {
            ProviderList::new(
                ProviderKind::Subscription,
                setup.clone(),
                default_model,
                window,
                cx,
            )
        });
        let ai = AiUiState {
            statuses: setup.as_ref().map(AiSetup::status).unwrap_or_default(),
            models: setup.as_ref().map(AiSetup::models).unwrap_or_default(),
            setup,
            setup_error: root_error.clone(),
            ..AiUiState::default()
        };

        let mut form = Self {
            original,
            settings,
            root,
            root_dir,
            root_error,
            section: Section::Appearance,
            dirty: false,
            committed: false,
            errors: Vec::new(),
            action_error: None,
            theme_index,
            zoom_mode_index,
            log_index,
            permission_index,
            mode_index,
            accent_input,
            autosave_interval,
            font_family,
            font_size,
            tab_size,
            debounce,
            timeout,
            main_file,
            extra_args,
            pdf_zoom,
            pdf_cache,
            ai_model,
            model_select,
            api_list,
            subscription_list,
            ai,
            _subscriptions: Vec::new(),
        };
        form.subscribe_controls(cx);
        form.subscribe_providers(window, cx);
        if !form.ai.models.is_empty() {
            form.sync_model_select(window, cx);
        }
        form
    }

    /// Marca cambios de texto como sucios y previsualiza el acento válido.
    fn subscribe_controls(&mut self, cx: &mut Context<Self>) {
        let accent = self.accent_input.clone();
        self._subscriptions.push(
            cx.subscribe(&accent, |this, input, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let value = input.read(cx).value().trim().to_owned();
                if parse_accent(&value).is_some() {
                    this.settings.ui.accent = value.clone();
                    theme::apply_accent(&value, cx);
                    this.errors.retain(|error| error.field != Field::Accent);
                }
                this.dirty = true;
                cx.notify();
            }),
        );

        let inputs = [
            self.autosave_interval.clone(),
            self.font_family.clone(),
            self.font_size.clone(),
            self.tab_size.clone(),
            self.debounce.clone(),
            self.timeout.clone(),
            self.main_file.clone(),
            self.extra_args.clone(),
            self.pdf_zoom.clone(),
            self.pdf_cache.clone(),
            self.ai_model.clone(),
        ];
        for input in inputs {
            self._subscriptions.push(cx.subscribe(
                &input,
                |this, _input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.dirty = true;
                        cx.notify();
                    }
                },
            ));
        }

        let model_select = self.model_select.clone();
        self._subscriptions.push(cx.subscribe(
            &model_select,
            |this, _state, event: &SelectEvent<SearchableVec<SharedString>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.settings.ai.model = Some(value.to_string());
                    this.dirty = true;
                    this.sync_default_to_lists(cx);
                    cx.notify();
                }
            },
        ));
    }

    /// Escucha las tarjetas de proveedores: un cambio guardado relee el
    /// catálogo (aquí, en la otra sección y en el chat); una ★ fija el
    /// predeterminado del borrador.
    fn subscribe_providers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for list in [self.api_list.clone(), self.subscription_list.clone()] {
            self._subscriptions.push(cx.subscribe_in(
                &list,
                window,
                |this, _list, event: &ProvidersEvent, window, cx| match event {
                    ProvidersEvent::Changed => {
                        this.api_list.update(cx, |list, cx| list.reload(cx));
                        this.subscription_list
                            .update(cx, |list, cx| list.reload(cx));
                        this.reload_ai(window, cx);
                        let _ = this
                            .root
                            .update(cx, |root, cx| root.refresh_ai_providers(cx));
                    }
                    ProvidersEvent::SetDefault(model) => {
                        this.settings.ai.model = Some(model.clone());
                        this.dirty = true;
                        this.sync_default_to_lists(cx);
                        this.sync_model_select(window, cx);
                        cx.notify();
                    }
                },
            ));
        }
    }

    /// Lleva el predeterminado del borrador a las dos listas (★ de los chips).
    fn sync_default_to_lists(&mut self, cx: &mut Context<Self>) {
        let model = self.settings.ai.model.clone();
        for list in [self.api_list.clone(), self.subscription_list.clone()] {
            list.update(cx, |list, cx| list.set_default_model(model.clone(), cx));
        }
    }

    fn focus_initial(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.accent_input
            .update(cx, |state, cx| state.focus(window, cx));
    }

    fn set_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section == section {
            return;
        }
        self.section = section;
        cx.notify();
    }

    fn mark_dirty(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        cx.notify();
    }

    /// Aplica el tema/acento del borrador para ver el cambio sin guardar.
    fn preview_theme(&mut self, cx: &mut Context<Self>) {
        self.settings.ui.theme = THEMES[self.theme_index].to_owned();
        theme::apply(&self.settings, cx);
        self.dirty = true;
        cx.notify();
    }

    /// Restaura el tema original si el diálogo se cierra sin guardar.
    fn cancel_preview(&mut self, cx: &mut Context<Self>) {
        if self.committed {
            return;
        }
        if self.settings.ui.theme != self.original.ui.theme
            || !self
                .settings
                .ui
                .accent
                .eq_ignore_ascii_case(&self.original.ui.accent)
        {
            theme::apply(&self.original, cx);
        }
    }

    fn set_accent(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.ui.accent = value.to_owned();
        self.accent_input
            .update(cx, |state, cx| state.set_value(value, window, cx));
        theme::apply_accent(value, cx);
        self.errors.retain(|error| error.field != Field::Accent);
        self.dirty = true;
        cx.notify();
    }

    fn set_error(&mut self, field: Field, message: impl Into<String>) {
        self.errors.retain(|error| error.field != field);
        self.errors.push(FieldError::new(field, message));
    }

    fn draft(&self, cx: &App) -> SettingsDraft {
        let mut draft = SettingsDraft::from_settings(&self.settings);
        draft.theme = THEMES[self.theme_index].to_owned();
        draft.accent = self.accent_input.read(cx).value().trim().to_owned();
        draft.autosave_interval = self.autosave_interval.read(cx).value().trim().to_owned();
        draft.font_family = self.font_family.read(cx).value().trim().to_owned();
        draft.font_size = self.font_size.read(cx).value().trim().to_owned();
        draft.tab_size = self.tab_size.read(cx).value().trim().to_owned();
        draft.debounce_ms = self.debounce.read(cx).value().trim().to_owned();
        draft.timeout_secs = self.timeout.read(cx).value().trim().to_owned();
        draft.main_file = self.main_file.read(cx).value().trim().to_owned();
        draft.extra_args = self.extra_args.read(cx).value().trim().to_owned();
        draft.pdf_zoom_mode = ZOOM_MODES[self.zoom_mode_index].to_owned();
        draft.pdf_zoom = self.pdf_zoom.read(cx).value().trim().to_owned();
        draft.pdf_max_cache_mb = self.pdf_cache.read(cx).value().trim().to_owned();
        draft.ai_agent = MODES[self.mode_index].to_owned();
        draft.ai_permission_preset = PERMISSION_PRESETS[self.permission_index].to_owned();
        draft.log_level = LOG_LEVELS[self.log_index].to_owned();
        draft.ai_model = if self.ai.models.is_empty() {
            self.ai_model.read(cx).value().trim().to_owned()
        } else {
            self.model_select
                .read(cx)
                .selected_value()
                .map(|value| value.to_string())
                .unwrap_or_else(|| self.ai_model.read(cx).value().trim().to_owned())
        };
        draft
    }

    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let draft = self.draft(cx);
        let mut next = self.settings.clone();
        match draft.apply_to(&mut next) {
            Ok(()) => {
                self.errors.clear();
                if self
                    .root
                    .update(cx, |root, cx| root.apply_settings(next, window, cx))
                    .is_ok()
                {
                    self.committed = true;
                    self.dirty = false;
                    true
                } else {
                    self.set_error(Field::Apply, rust_i18n::t!("ui.app.settings.error_apply"));
                    cx.notify();
                    false
                }
            }
            Err(errors) => {
                if let Some(section) = errors.first().and_then(|error| error.field.section()) {
                    self.section = section;
                }
                self.errors = errors;
                cx.notify();
                false
            }
        }
    }

    fn open_config_folder(&mut self, cx: &mut Context<Self>) {
        let Some(root) = &self.root_dir else {
            self.action_error = Some(
                self.root_error
                    .clone()
                    .unwrap_or_else(|| rust_i18n::t!("ui.app.settings.path_unknown").to_string()),
            );
            cx.notify();
            return;
        };
        let dir = root.config_dir();
        if !dir.is_dir()
            && let Err(error) = std::fs::create_dir_all(&dir)
        {
            self.action_error = Some(
                rust_i18n::t!(
                    "ui.app.settings.open_error",
                    path = dir.display().to_string(),
                    error = error
                )
                .to_string(),
            );
            cx.notify();
            return;
        }
        match ai_setup::open_folder(&dir) {
            Ok(()) => {
                self.action_error = None;
                cx.notify();
            }
            Err(error) => {
                self.action_error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn open_agent_log(&mut self, cx: &mut Context<Self>) {
        let result = match &self.ai.setup {
            Some(setup) => setup.open_log(),
            None => Err(AiSetupError::Root(self.root_error.clone().unwrap_or_else(
                || rust_i18n::t!("ui.app.settings.path_unknown").to_string(),
            ))),
        };
        self.action_error = result.err().map(|error| error.to_string());
        cx.notify();
    }

    fn open_agent_folder(&mut self, cx: &mut Context<Self>) {
        let result = if let Some(setup) = &self.ai.setup {
            setup.open_agent_dir()
        } else if let Some(root) = &self.root_dir {
            let dir = root.agent_dir();
            if !dir.is_dir()
                && let Err(error) = std::fs::create_dir_all(&dir)
            {
                Err(AiSetupError::Io {
                    path: dir.display().to_string(),
                    error: error.to_string(),
                })
            } else {
                ai_setup::open_folder(&dir)
            }
        } else {
            Err(AiSetupError::Root(self.root_error.clone().unwrap_or_else(
                || rust_i18n::t!("ui.app.settings.path_unknown").to_string(),
            )))
        };
        match result {
            Ok(()) => {
                self.action_error = None;
                cx.notify();
            }
            Err(error) => {
                self.action_error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    // --- IA -----------------------------------------------------------------

    /// Relee el estado de proveedores y modelos (lectura local, instantánea).
    fn reload_ai(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(setup) = &self.ai.setup {
            self.ai.statuses = setup.status();
            self.ai.models = setup.models();
        }
        self.sync_model_select(window, cx);
        cx.notify();
    }

    /// Lanza una petición mínima con el modelo elegido (en segundo plano).
    fn probe_ai(&mut self, cx: &mut Context<Self>) {
        if self.ai.probing {
            return;
        }
        let Some(setup) = self.ai.setup.clone() else {
            return;
        };
        let model = self
            .model_select
            .read(cx)
            .selected_value()
            .map(|value| value.to_string())
            .or_else(|| self.settings.ai.model.clone());
        let Some(model) = model.filter(|model| !model.is_empty()) else {
            self.ai.info = None;
            self.ai.error = Some(rust_i18n::t!("ui.app.settings.ai_test_no_model").to_string());
            cx.notify();
            return;
        };
        self.ai.probing = true;
        self.ai.error = None;
        self.ai.info =
            Some(rust_i18n::t!("ui.app.settings.ai_probing", model = model.as_str()).to_string());
        cx.notify();

        let target = model.clone();
        let task = cx.background_spawn(async move { setup.probe(&target) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.ai.probing = false;
                match result {
                    Ok(text) => {
                        this.ai.error = None;
                        this.ai.info = Some(
                            rust_i18n::t!(
                                "ui.app.settings.ai_test_ok",
                                model = model.as_str(),
                                text = text.as_str()
                            )
                            .to_string(),
                        );
                    }
                    Err(error) => {
                        this.ai.info = None;
                        this.ai.error = Some(
                            rust_i18n::t!("ui.app.settings.ai_test_error", error = error)
                                .to_string(),
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Rellena el selector con el catálogo y conserva el modelo actual aunque
    /// ya no aparezca en la lista (así el usuario ve lo que tiene configurado).
    fn sync_model_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.settings.ai.model.clone();
        let mut items: Vec<SharedString> = self
            .ai
            .models
            .iter()
            .map(|model| SharedString::from(model.clone()))
            .collect();
        if let Some(current) = &current
            && !items.iter().any(|item| item.as_ref() == current)
        {
            items.push(SharedString::from(current.clone()));
        }

        self.model_select.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(items), window, cx);
            if let Some(current) = current {
                state.set_selected_value(&SharedString::from(current), window, cx);
            } else {
                state.set_selected_index(None, window, cx);
            }
        });
    }

    /// Reconstruye los controles desde `self.settings` (reset).
    fn sync_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_index = index_of(&THEMES, &self.settings.ui.theme, 0);
        self.zoom_mode_index = index_of(&ZOOM_MODES, &self.settings.pdf.zoom_mode, 0);
        self.log_index = index_of(&LOG_LEVELS, &self.settings.advanced.log_level, 2);
        self.permission_index = permission_index(&self.settings.ai.permission_preset);
        self.mode_index = index_of(&MODES, &self.settings.ai.agent, 0);

        let values = [
            (self.accent_input.clone(), self.settings.ui.accent.clone()),
            (
                self.autosave_interval.clone(),
                self.settings.editor.autosave.interval_secs.to_string(),
            ),
            (
                self.font_family.clone(),
                self.settings.editor.font_family.clone().unwrap_or_default(),
            ),
            (
                self.font_size.clone(),
                format_float(self.settings.editor.font_size),
            ),
            (
                self.tab_size.clone(),
                self.settings.editor.tab_size.to_string(),
            ),
            (
                self.debounce.clone(),
                self.settings.latex.debounce_ms.to_string(),
            ),
            (
                self.timeout.clone(),
                self.settings.latex.timeout_secs.to_string(),
            ),
            (
                self.main_file.clone(),
                self.settings.latex.main_file.clone().unwrap_or_default(),
            ),
            (
                self.extra_args.clone(),
                self.settings.latex.extra_args.join(" "),
            ),
            (self.pdf_zoom.clone(), format_float(self.settings.pdf.zoom)),
            (
                self.pdf_cache.clone(),
                self.settings.pdf.max_cache_mb.to_string(),
            ),
            (
                self.ai_model.clone(),
                self.settings.ai.model.clone().unwrap_or_default(),
            ),
        ];
        for (input, value) in values {
            input.update(cx, |state, cx| state.set_value(value, window, cx));
        }

        if !self.ai.models.is_empty() {
            self.sync_model_select(window, cx);
        }
    }

    fn reset_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Restablecer preferencias no debe borrar los recientes ni el layout
        // que el usuario ya organizó.
        let defaults = Settings {
            project: self.original.project.clone(),
            ui: UiSettings {
                dock: self.original.ui.dock.clone(),
                ..UiSettings::default()
            },
            extra: self.original.extra.clone(),
            schema_version: self.original.schema_version,
            ..Settings::default()
        };
        self.settings = defaults;
        self.errors.clear();
        self.action_error = None;
        self.sync_controls(window, cx);
        theme::apply(&self.settings, cx);
        self.dirty = true;
        cx.notify();
    }

    fn confirm_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let form = form.clone();
            alert
                .title(rust_i18n::t!("ui.app.settings.reset_confirm_title").to_string())
                .description(rust_i18n::t!("ui.app.settings.reset_body").to_string())
                .confirm()
                .ok_text(rust_i18n::t!("ui.app.settings.reset_confirm").to_string())
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(rust_i18n::t!("ui.app.settings.cancel").to_string())
                .on_ok(move |_, window, cx| {
                    let _ = form.update(cx, |form, cx| form.reset_defaults(window, cx));
                    true
                })
        });
    }

    // --- render -------------------------------------------------------------

    fn render_error_summary(&self, cx: &App) -> Option<AnyElement> {
        let first = self.errors.first()?;
        Some(
            h_flex()
                .items_start()
                .gap_2()
                .child(
                    Icon::new(IconName::CircleAlert)
                        .xsmall()
                        .text_color(cx.theme().danger),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div().text_xs().text_color(cx.theme().danger).child(
                                rust_i18n::t!(
                                    "ui.app.settings.error_summary",
                                    count = self.errors.len()
                                )
                                .to_string(),
                            ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(first.message.clone()),
                        ),
                )
                .into_any_element(),
        )
    }

    fn field_error(&self, field: Field, cx: &App) -> Option<AnyElement> {
        self.errors
            .iter()
            .find(|error| error.field == field)
            .map(|error| {
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error.message.clone())
                    .into_any_element()
            })
    }

    fn input_with_error(
        &self,
        input: &Entity<InputState>,
        field: Field,
        width: f32,
        cx: &App,
    ) -> impl IntoElement {
        v_flex()
            .gap_1()
            .w(px(width))
            .child(Input::new(input).small())
            .children(self.field_error(field, cx))
    }

    fn input_plain(&self, input: &Entity<InputState>, width: f32) -> impl IntoElement {
        v_flex()
            .gap_1()
            .w(px(width))
            .child(Input::new(input).small())
    }

    fn render_nav(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .gap_1()
            .children(Section::ALL.iter().enumerate().map(|(index, section)| {
                let section = *section;
                Button::new(SharedString::from(format!("settings-nav-{index}")))
                    .ghost()
                    .selected(self.section == section)
                    .w_full()
                    .label(section.label())
                    .icon(section.icon())
                    .on_click(cx.listener(move |this, _, _, cx| this.set_section(section, cx)))
            }))
    }

    fn render_section(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            Section::Appearance => self.render_appearance(cx).into_any_element(),
            Section::Editor => self.render_editor(cx).into_any_element(),
            Section::Compilation => self.render_compilation(cx).into_any_element(),
            Section::Pdf => self.render_pdf(cx).into_any_element(),
            Section::Ai => self.render_ai(cx).into_any_element(),
            Section::ApiKeys => self
                .render_providers(
                    "ui.app.settings.sections.api_keys",
                    "ui.app.settings.api_keys_hint",
                    self.api_list.clone(),
                    cx,
                )
                .into_any_element(),
            Section::Subscriptions => self
                .render_providers(
                    "ui.app.settings.sections.subscriptions",
                    "ui.app.settings.subscriptions_hint",
                    self.subscription_list.clone(),
                    cx,
                )
                .into_any_element(),
            Section::Advanced => self.render_advanced(cx).into_any_element(),
        }
    }

    fn render_appearance(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme_labels = [
            rust_i18n::t!("ui.app.settings.theme_dark").to_string(),
            rust_i18n::t!("ui.app.settings.theme_light").to_string(),
            rust_i18n::t!("ui.app.settings.theme_system").to_string(),
        ];

        let swatches = h_flex()
            .items_center()
            .gap_2()
            .children(theme::ACCENT_PRESETS.iter().map(|preset| {
                let preset = *preset;
                let selected = self.settings.ui.accent.eq_ignore_ascii_case(preset.hex);
                let label = accent_label(preset.id);
                Button::new(SharedString::from(format!("settings-accent-{}", preset.id)))
                    .selected(selected)
                    .accessibility_label(label.clone())
                    .tooltip(label)
                    .child(div().size(px(16.)).rounded_full().bg(preset.color()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_accent(preset.hex, window, cx);
                    }))
            }));

        let accent_control = v_flex().gap_2().w(px(340.)).child(swatches).child(
            h_flex()
                .items_center()
                .gap_2()
                .child(self.input_with_error(&self.accent_input, Field::Accent, 150., cx))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("#RRGGBB"),
                ),
        );

        v_flex()
            .gap_1()
            .child(section_header(
                rust_i18n::t!("ui.app.settings.sections.appearance").to_string(),
                rust_i18n::t!("ui.app.settings.appearance_hint").to_string(),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.theme").to_string(),
                Some(rust_i18n::t!("ui.app.settings.theme_hint").to_string()),
                RadioGroup::horizontal("settings-theme")
                    .selected_index(Some(self.theme_index))
                    .children([
                        Radio::new("settings-theme-dark").label(theme_labels[0].clone()),
                        Radio::new("settings-theme-light").label(theme_labels[1].clone()),
                        Radio::new("settings-theme-system").label(theme_labels[2].clone()),
                    ])
                    .on_click(cx.listener(|this, index, _window, cx| {
                        this.theme_index = *index;
                        this.preview_theme(cx);
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.accent").to_string(),
                Some(rust_i18n::t!("ui.app.settings.accent_hint").to_string()),
                accent_control,
                cx,
            ))
            .child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("ui.app.settings.language_note").to_string()),
            )
    }

    fn render_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(section_header(
                rust_i18n::t!("ui.app.settings.sections.editor").to_string(),
                rust_i18n::t!("ui.app.settings.editor_hint").to_string(),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.font_family").to_string(),
                Some(rust_i18n::t!("ui.app.settings.font_family_hint").to_string()),
                self.input_plain(&self.font_family, TEXT_WIDTH),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.font_size").to_string(),
                Some(rust_i18n::t!("ui.app.settings.font_size_hint").to_string()),
                self.input_with_error(&self.font_size, Field::FontSize, NUMBER_WIDTH, cx),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.tab_size").to_string(),
                Some(rust_i18n::t!("ui.app.settings.tab_size_hint").to_string()),
                self.input_with_error(&self.tab_size, Field::TabSize, NUMBER_WIDTH, cx),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.soft_wrap").to_string(),
                Some(rust_i18n::t!("ui.app.settings.soft_wrap_hint").to_string()),
                Switch::new("settings-soft-wrap")
                    .checked(self.settings.editor.soft_wrap)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        this.settings.editor.soft_wrap = *checked;
                        this.mark_dirty(cx);
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.autosave").to_string(),
                Some(rust_i18n::t!("ui.app.settings.autosave_hint").to_string()),
                Switch::new("settings-autosave")
                    .checked(self.settings.editor.autosave.enabled)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        this.settings.editor.autosave.enabled = *checked;
                        this.mark_dirty(cx);
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.autosave_interval").to_string(),
                Some(rust_i18n::t!("ui.app.settings.autosave_interval_hint").to_string()),
                self.input_with_error(
                    &self.autosave_interval,
                    Field::AutosaveInterval,
                    NUMBER_WIDTH,
                    cx,
                ),
                cx,
            ))
            .child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("ui.app.settings.autosave_pending").to_string()),
            )
    }

    fn render_compilation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(section_header(
                rust_i18n::t!("ui.app.settings.sections.compilation").to_string(),
                rust_i18n::t!("ui.app.settings.compilation_hint").to_string(),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.auto_compile").to_string(),
                Some(rust_i18n::t!("ui.app.settings.auto_compile_hint").to_string()),
                Switch::new("settings-auto-compile")
                    .checked(self.settings.latex.auto_compile)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        this.settings.latex.auto_compile = *checked;
                        this.mark_dirty(cx);
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.sync_scroll").to_string(),
                Some(rust_i18n::t!("ui.app.settings.sync_scroll_hint").to_string()),
                Switch::new("settings-sync-scroll")
                    .checked(self.settings.latex.sync_scroll)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        this.settings.latex.sync_scroll = *checked;
                        this.mark_dirty(cx);
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.auto_install").to_string(),
                Some(rust_i18n::t!("ui.app.settings.auto_install_hint").to_string()),
                Switch::new("settings-auto-install")
                    .checked(self.settings.latex.auto_install_packages)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        this.settings.latex.auto_install_packages = *checked;
                        this.mark_dirty(cx);
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.clean_on_success").to_string(),
                None,
                Switch::new("settings-clean")
                    .checked(self.settings.latex.clean_on_success)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        this.settings.latex.clean_on_success = *checked;
                        this.mark_dirty(cx);
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.debounce").to_string(),
                Some(rust_i18n::t!("ui.app.settings.debounce_hint").to_string()),
                self.input_with_error(&self.debounce, Field::Debounce, NUMBER_WIDTH, cx),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.timeout").to_string(),
                Some(rust_i18n::t!("ui.app.settings.timeout_hint").to_string()),
                self.input_with_error(&self.timeout, Field::Timeout, NUMBER_WIDTH, cx),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.main_file").to_string(),
                Some(rust_i18n::t!("ui.app.settings.main_file_hint").to_string()),
                self.input_plain(&self.main_file, TEXT_WIDTH),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.extra_args").to_string(),
                Some(rust_i18n::t!("ui.app.settings.extra_args_hint").to_string()),
                self.input_with_error(&self.extra_args, Field::ExtraArgs, ARGS_WIDTH, cx),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.engine").to_string(),
                Some(rust_i18n::t!("ui.app.settings.engine_hint").to_string()),
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("TeX Live (texlive)"),
                cx,
            ))
    }

    fn render_pdf(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom_labels = [
            rust_i18n::t!("ui.app.settings.zoom_fit_width").to_string(),
            rust_i18n::t!("ui.app.settings.zoom_fit_page").to_string(),
            rust_i18n::t!("ui.app.settings.zoom_actual").to_string(),
            rust_i18n::t!("ui.app.settings.zoom_custom").to_string(),
        ];

        v_flex()
            .gap_1()
            .child(section_header(
                rust_i18n::t!("ui.app.settings.sections.pdf").to_string(),
                rust_i18n::t!("ui.app.settings.pdf_hint").to_string(),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.zoom_mode").to_string(),
                None,
                RadioGroup::vertical("settings-zoom-mode")
                    .selected_index(Some(self.zoom_mode_index))
                    .children(zoom_labels.into_iter().enumerate().map(|(index, label)| {
                        Radio::new(SharedString::from(format!("settings-zoom-mode-{index}")))
                            .label(label)
                    }))
                    .on_click(cx.listener(|this, index, _window, cx| {
                        this.zoom_mode_index = *index;
                        this.dirty = true;
                        cx.notify();
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.zoom").to_string(),
                Some(rust_i18n::t!("ui.app.settings.zoom_hint").to_string()),
                v_flex()
                    .gap_1()
                    .w(px(NUMBER_WIDTH))
                    .child(Input::new(&self.pdf_zoom).small())
                    .children(self.field_error(Field::PdfZoom, cx)),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.pdf_open_external").to_string(),
                Some(rust_i18n::t!("ui.app.settings.pdf_open_external_hint").to_string()),
                Switch::new("settings-pdf-external")
                    .checked(self.settings.pdf.open_external_on_double_click)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        this.settings.pdf.open_external_on_double_click = *checked;
                        this.mark_dirty(cx);
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.pdf_cache").to_string(),
                Some(rust_i18n::t!("ui.app.settings.pdf_cache_hint").to_string()),
                self.input_with_error(&self.pdf_cache, Field::PdfCache, NUMBER_WIDTH, cx),
                cx,
            ))
            .child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("ui.app.settings.pdf_pending").to_string()),
            )
    }

    fn render_ai(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let agent_available = self.ai.setup.as_ref().is_some_and(AiSetup::agent_exists);

        let status = if let Some(error) = &self.ai.setup_error {
            div()
                .max_w(px(380.))
                .text_xs()
                .text_color(cx.theme().danger)
                .child(error.clone())
                .into_any_element()
        } else if let Some(setup) = &self.ai.setup {
            let (color, label) = if agent_available {
                (
                    cx.theme().success,
                    rust_i18n::t!("ui.app.settings.ai_exe_found").to_string(),
                )
            } else {
                (
                    cx.theme().warning,
                    rust_i18n::t!("ui.app.settings.ai_exe_missing").to_string(),
                )
            };
            v_flex()
                .gap_1()
                .max_w(px(380.))
                .child(div().text_xs().text_color(color).child(label))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(setup.python_path().display().to_string()),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        // Resumen y accesos a las dos secciones de proveedores.
        let active = |kind: ProviderKind| {
            self.ai
                .statuses
                .iter()
                .filter(|status| status.spec.kind == kind && status.configured)
                .count()
        };
        let providers = setting_row(
            rust_i18n::t!("ui.app.settings.ai_providers").to_string(),
            Some(
                rust_i18n::t!(
                    "ui.app.settings.ai_providers_summary",
                    api = active(ProviderKind::Api),
                    subscriptions = active(ProviderKind::Subscription)
                )
                .to_string(),
            ),
            h_flex()
                .gap_2()
                .child(
                    Button::new("settings-ai-goto-api")
                        .small()
                        .outline()
                        .icon(IconName::Globe)
                        .label(rust_i18n::t!("ui.app.settings.sections.api_keys").to_string())
                        .on_click(
                            cx.listener(|this, _, _, cx| this.set_section(Section::ApiKeys, cx)),
                        ),
                )
                .child(
                    Button::new("settings-ai-goto-subscriptions")
                        .small()
                        .outline()
                        .icon(IconName::CircleUser)
                        .label(rust_i18n::t!("ui.app.settings.sections.subscriptions").to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_section(Section::Subscriptions, cx)
                        })),
                ),
            cx,
        );

        let model_control = if self.ai.models.is_empty() {
            div()
                .max_w(px(TEXT_WIDTH))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(rust_i18n::t!("ui.app.settings.ai_no_providers").to_string())
                .into_any_element()
        } else {
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div().w(px(TEXT_WIDTH)).child(
                        Select::new(&self.model_select)
                            .small()
                            .placeholder(
                                rust_i18n::t!("ui.app.settings.ai_model_placeholder").to_string(),
                            )
                            .menu_max_h(px(260.)),
                    ),
                )
                .child(
                    Button::new("settings-ai-probe")
                        .small()
                        .outline()
                        .label(rust_i18n::t!("ui.app.settings.ai_probe").to_string())
                        .icon(IconName::Play)
                        .loading(self.ai.probing)
                        .disabled(!agent_available || self.ai.probing)
                        .on_click(cx.listener(|this, _, _, cx| this.probe_ai(cx))),
                )
                .into_any_element()
        };

        let feedback = v_flex()
            .gap_1()
            .children(self.ai.error.as_ref().map(|error| {
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
            .children(self.ai.info.as_ref().map(|info| {
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(info.clone())
            }));

        let modes = [
            rust_i18n::t!("ui.app.settings.ai_mode_build").to_string(),
            rust_i18n::t!("ui.app.settings.ai_mode_fix").to_string(),
            rust_i18n::t!("ui.app.settings.ai_mode_ask").to_string(),
        ];

        v_flex()
            .gap_1()
            .child(section_header(
                rust_i18n::t!("ui.app.settings.sections.ai").to_string(),
                rust_i18n::t!("ui.app.settings.ai_hint").to_string(),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.ai_exe").to_string(),
                None,
                status,
                cx,
            ))
            .child(providers)
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.ai_model").to_string(),
                Some(rust_i18n::t!("ui.app.settings.ai_model_hint").to_string()),
                model_control,
                cx,
            ))
            .child(div().pt_1().child(feedback))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.ai_agent").to_string(),
                Some(rust_i18n::t!("ui.app.settings.ai_agent_hint").to_string()),
                v_flex()
                    .gap_1()
                    .child(
                        RadioGroup::horizontal("settings-ai-mode")
                            .selected_index(Some(self.mode_index))
                            .children(modes.into_iter().enumerate().map(|(index, label)| {
                                Radio::new(SharedString::from(format!("settings-ai-mode-{index}")))
                                    .label(label)
                            }))
                            .on_click(cx.listener(|this, index, _window, cx| {
                                this.mode_index = *index;
                                this.dirty = true;
                                cx.notify();
                            })),
                    )
                    .children(self.field_error(Field::AiAgent, cx)),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.ai_permissions").to_string(),
                Some(rust_i18n::t!("ui.app.settings.ai_permissions_hint").to_string()),
                RadioGroup::horizontal("settings-ai-permissions")
                    .selected_index(Some(self.permission_index))
                    .children([
                        Radio::new("settings-ai-permissions-ask")
                            .label(rust_i18n::t!("ui.app.settings.ai_permissions_ask").to_string()),
                        Radio::new("settings-ai-permissions-project").label(
                            rust_i18n::t!("ui.app.settings.ai_permissions_project").to_string(),
                        ),
                        Radio::new("settings-ai-permissions-all")
                            .label(rust_i18n::t!("ui.app.settings.ai_permissions_all").to_string()),
                    ])
                    .on_click(cx.listener(|this, index, _window, cx| {
                        this.permission_index = *index;
                        this.dirty = true;
                        cx.notify();
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.ai_auto_start").to_string(),
                Some(rust_i18n::t!("ui.app.settings.ai_auto_start_hint").to_string()),
                Switch::new("settings-ai-autostart")
                    .checked(self.settings.ai.auto_start)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        this.settings.ai.auto_start = *checked;
                        this.mark_dirty(cx);
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.ai_show_reasoning").to_string(),
                Some(rust_i18n::t!("ui.app.settings.ai_show_reasoning_hint").to_string()),
                Switch::new("settings-ai-reasoning")
                    .checked(self.settings.ai.show_reasoning)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        this.settings.ai.show_reasoning = *checked;
                        this.mark_dirty(cx);
                    })),
                cx,
            ))
            .child(
                h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .pt_2()
                    .child(
                        Button::new("settings-ai-open-agent")
                            .ghost()
                            .small()
                            .label(rust_i18n::t!("ui.app.settings.ai_open_agent").to_string())
                            .icon(IconName::FolderOpen)
                            .on_click(cx.listener(|this, _, _, cx| this.open_agent_folder(cx))),
                    )
                    .child(
                        Button::new("settings-ai-open-log")
                            .ghost()
                            .small()
                            .label(rust_i18n::t!("ui.app.settings.ai_open_log").to_string())
                            .icon(IconName::FileText)
                            .on_click(cx.listener(|this, _, _, cx| this.open_agent_log(cx))),
                    ),
            )
            .child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("ui.app.settings.ai_pending").to_string()),
            )
    }

    /// «Claves de API» o «Suscripciones»: cabecera + tarjetas de la familia.
    fn render_providers(
        &self,
        title: &str,
        hint: &str,
        list: Entity<ProviderList>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(section_header(
                rust_i18n::t!(title).to_string(),
                rust_i18n::t!(hint).to_string(),
                cx,
            ))
            .child(div().pt_2().child(list))
            .child(
                div()
                    .pt_3()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("ui.app.settings.providers.storage_note").to_string()),
            )
    }

    fn render_advanced(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let config_path = self
            .root_dir
            .as_ref()
            .map(|root| Settings::path(root).display().to_string())
            .unwrap_or_else(|| rust_i18n::t!("ui.app.settings.path_unknown").to_string());

        v_flex()
            .gap_1()
            .child(section_header(
                rust_i18n::t!("ui.app.settings.sections.advanced").to_string(),
                rust_i18n::t!("ui.app.settings.advanced_hint").to_string(),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.log_level").to_string(),
                Some(rust_i18n::t!("ui.app.settings.log_level_hint").to_string()),
                RadioGroup::horizontal("settings-log-level")
                    .selected_index(Some(self.log_index))
                    .children(LOG_LEVELS.iter().map(|level| {
                        Radio::new(SharedString::from(format!("settings-log-{level}")))
                            .label(level.to_string())
                    }))
                    .on_click(cx.listener(|this, index, _window, cx| {
                        this.log_index = *index;
                        this.dirty = true;
                        cx.notify();
                    })),
                cx,
            ))
            .child(setting_row(
                rust_i18n::t!("ui.app.settings.config_path").to_string(),
                Some(rust_i18n::t!("ui.app.settings.config_path_hint").to_string()),
                div()
                    .max_w(px(360.))
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(config_path),
                cx,
            ))
            .child(
                h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .pt_2()
                    .child(
                        Button::new("settings-open-config")
                            .outline()
                            .label(rust_i18n::t!("ui.app.settings.open_config_folder").to_string())
                            .icon(IconName::FolderOpen)
                            .on_click(cx.listener(|this, _, _, cx| this.open_config_folder(cx))),
                    )
                    .child(
                        Button::new("settings-open-agent-log")
                            .outline()
                            .label(rust_i18n::t!("ui.app.settings.open_agent_log").to_string())
                            .icon(IconName::FileText)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_agent_log(cx);
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .pt_4()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(rust_i18n::t!("ui.app.settings.reset_title").to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(rust_i18n::t!("ui.app.settings.reset_hint").to_string()),
                    )
                    .child(
                        Button::new("settings-reset")
                            .danger()
                            .label(rust_i18n::t!("ui.app.settings.reset").to_string())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.confirm_reset(window, cx)),
                            ),
                    ),
            )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dirty_hint = if self.dirty {
            rust_i18n::t!("ui.app.settings.dirty").to_string()
        } else {
            rust_i18n::t!("ui.app.settings.clean").to_string()
        };

        v_flex()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .children(self.render_error_summary(cx))
            .children(self.action_error.as_ref().map(|error| {
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
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .text_xs()
                            .text_color(if self.dirty {
                                cx.theme().primary
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(dirty_hint),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("settings-cancel")
                                    .outline()
                                    .label(rust_i18n::t!("ui.app.settings.cancel").to_string())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.cancel_preview(cx);
                                        window.close_dialog(cx);
                                    })),
                            )
                            .child(
                                Button::new("settings-save")
                                    .primary()
                                    .disabled(!self.dirty)
                                    .label(rust_i18n::t!("ui.app.settings.save").to_string())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if this.commit(window, cx) {
                                            window.close_dialog(cx);
                                        }
                                    })),
                            ),
                    ),
            )
    }
}

impl Render for SettingsForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let subtitle = rust_i18n::t!("ui.app.settings.subtitle").to_string();
        let content = self.render_section(cx);
        let nav = self.render_nav(cx);

        v_flex()
            .gap_3()
            .p_1()
            .h(px(540.))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .gap_4()
                    .child(
                        div()
                            .w(px(190.))
                            .h_full()
                            .flex_none()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .pr_2()
                            .child(nav),
                    )
                    .child(
                        div()
                            .id("settings-content")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scrollbar()
                            .pr_2()
                            .child(content),
                    ),
            )
            .child(self.render_footer(cx))
    }
}

/// Abre el diálogo de ajustes. La firma se conserva para el shell.
pub fn open_dialog(
    window: &mut Window,
    cx: &mut App,
    settings: Settings,
    root: WeakEntity<AppRoot>,
) {
    open_dialog_at(window, cx, settings, root, Section::Appearance);
}

/// Abre Ajustes directamente en la sección de IA (botón «Configurar IA» del
/// chat). Mantiene el resto del comportamiento de [`open_dialog`].
pub fn open_ai_dialog(
    window: &mut Window,
    cx: &mut App,
    settings: Settings,
    root: WeakEntity<AppRoot>,
) {
    open_dialog_at(window, cx, settings, root, Section::Ai);
}

/// Abre Ajustes en la sección de compilación (estado del motor LaTeX).
pub fn open_compilation_dialog(
    window: &mut Window,
    cx: &mut App,
    settings: Settings,
    root: WeakEntity<AppRoot>,
) {
    open_dialog_at(window, cx, settings, root, Section::Compilation);
}

/// Abre Ajustes en la sección indicada por nombre (`ai`, `api`, `subscriptions`; otro
/// valor, Apariencia). Lo usa el hook de QA `LSX_QA_OPEN_SETTINGS`.
pub fn open_named_dialog(
    window: &mut Window,
    cx: &mut App,
    settings: Settings,
    root: WeakEntity<AppRoot>,
    name: &str,
) {
    let section = match name {
        "ai" => Section::Ai,
        "api" => Section::ApiKeys,
        "subscriptions" => Section::Subscriptions,
        _ => Section::Appearance,
    };
    open_dialog_at(window, cx, settings, root, section);
}

fn open_dialog_at(
    window: &mut Window,
    cx: &mut App,
    settings: Settings,
    root: WeakEntity<AppRoot>,
    section: Section,
) {
    let form = cx.new(|cx| SettingsForm::new(window, cx, settings, root));
    form.update(cx, |form, cx| form.set_section(section, cx));
    let form_for_builder = form.clone();
    let form_for_close = form.clone();
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            form_for_builder.update(cx, |form, cx| form.focus_initial(window, cx));
        }
        let close_form = form_for_close.clone();
        dialog
            .title(rust_i18n::t!("ui.app.settings.title").to_string())
            .width(px(880.))
            .margin_top(px(48.))
            .on_close(move |_, _window, cx| {
                close_form.update(cx, |form, cx| form.cancel_preview(cx));
            })
            .child(form_for_builder.clone())
    });
}

/// Fila etiqueta + descripción + control.
fn setting_row(
    label: String,
    description: Option<String>,
    control: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_start()
        .justify_between()
        .gap_4()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(label),
                )
                .children(description.map(|description| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(description)
                })),
        )
        .child(div().flex_none().child(control))
}

/// Cabecera de sección.
fn section_header(title: String, description: String, cx: &App) -> impl IntoElement {
    v_flex()
        .gap_1()
        .pb_2()
        .child(
            div()
                .text_lg()
                .text_color(cx.theme().foreground)
                .child(title),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(description),
        )
}

/// Etiqueta traducida de un preset de acento.
fn accent_label(id: &str) -> String {
    match id {
        "blue" => rust_i18n::t!("ui.app.settings.accent_blue").to_string(),
        "violet" => rust_i18n::t!("ui.app.settings.accent_violet").to_string(),
        "green" => rust_i18n::t!("ui.app.settings.accent_green").to_string(),
        "amber" => rust_i18n::t!("ui.app.settings.accent_amber").to_string(),
        "red" => rust_i18n::t!("ui.app.settings.accent_red").to_string(),
        other => other.to_owned(),
    }
}

fn index_of(values: &[&str], value: &str, fallback: usize) -> usize {
    values
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(value))
        .unwrap_or(fallback)
}

/// Índice del preset de permisos; `auto` es el alias legado de `all`.
fn permission_index(value: &str) -> usize {
    match value.trim().to_ascii_lowercase().as_str() {
        "project" | "project-edits" | "edits" => 1,
        "all" | "auto" | "yolo" => 2,
        _ => 0,
    }
}

fn format_float(value: f32) -> String {
    format!("{value}")
}

/// Normaliza un acento a `#RRGGBB` en mayúsculas; `None` si no es válido.
fn normalize_accent(value: &str) -> Option<String> {
    let rgb = parse_accent(value.trim())?;
    Some(format!("#{rgb:06X}"))
}

fn parse_u32_field(
    value: &str,
    (min, max): (u32, u32),
    field: Field,
    errors: &mut Vec<FieldError>,
) -> Option<u32> {
    let value = value.trim();
    match value.parse::<u32>() {
        Ok(parsed) if (min..=max).contains(&parsed) => Some(parsed),
        Ok(_) => {
            errors.push(FieldError::new(
                field,
                rust_i18n::t!(
                    "ui.app.settings.error_range",
                    value = value,
                    min = min,
                    max = max
                ),
            ));
            None
        }
        Err(_) => {
            errors.push(FieldError::new(
                field,
                rust_i18n::t!("ui.app.settings.error_number", value = value),
            ));
            None
        }
    }
}

fn parse_u64_field(
    value: &str,
    (min, max): (u64, u64),
    field: Field,
    errors: &mut Vec<FieldError>,
) -> Option<u64> {
    let value = value.trim();
    match value.parse::<u64>() {
        Ok(parsed) if (min..=max).contains(&parsed) => Some(parsed),
        Ok(_) => {
            errors.push(FieldError::new(
                field,
                rust_i18n::t!(
                    "ui.app.settings.error_range",
                    value = value,
                    min = min,
                    max = max
                ),
            ));
            None
        }
        Err(_) => {
            errors.push(FieldError::new(
                field,
                rust_i18n::t!("ui.app.settings.error_number", value = value),
            ));
            None
        }
    }
}

fn parse_f32_field(
    value: &str,
    (min, max): (f32, f32),
    field: Field,
    errors: &mut Vec<FieldError>,
) -> Option<f32> {
    let value = value.trim();
    match value.parse::<f32>() {
        Ok(parsed) if parsed.is_finite() && (min..=max).contains(&parsed) => Some(parsed),
        Ok(_) => {
            errors.push(FieldError::new(
                field,
                rust_i18n::t!(
                    "ui.app.settings.error_range",
                    value = value,
                    min = min,
                    max = max
                ),
            ));
            None
        }
        Err(_) => {
            errors.push(FieldError::new(
                field,
                rust_i18n::t!("ui.app.settings.error_number", value = value),
            ));
            None
        }
    }
}

fn parse_extra_args(value: &str, errors: &mut Vec<FieldError>) -> Option<Vec<String>> {
    let value = value.trim();
    if value.is_empty() {
        return Some(Vec::new());
    }
    if value.contains('"') || value.contains('\'') {
        errors.push(FieldError::new(
            Field::ExtraArgs,
            rust_i18n::t!("ui.app.settings.error_extra_args"),
        ));
        return None;
    }
    Some(value.split_whitespace().map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_root(name: &str) -> (PortableRoot, std::path::PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "lsx-ui-settings-{}-{}-{n}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("crear raíz temporal");
        (PortableRoot::from_dir(dir.clone()), dir)
    }

    fn draft_with(changes: impl FnOnce(&mut SettingsDraft)) -> SettingsDraft {
        let mut draft = SettingsDraft::from_settings(&Settings::default());
        changes(&mut draft);
        draft
    }

    #[test]
    fn the_defaults_round_trip_through_the_draft() {
        let settings = Settings::default();
        let draft = SettingsDraft::from_settings(&settings);
        let mut applied = Settings::default();
        draft.apply_to(&mut applied).expect("defaults válidos");
        assert_eq!(applied, settings);
    }

    #[test]
    fn ranges_match_the_core_validation() {
        let draft = draft_with(|draft| {
            draft.debounce_ms = "10001".to_owned();
            draft.timeout_secs = "1".to_owned();
            draft.pdf_zoom = "9".to_owned();
            draft.pdf_max_cache_mb = "1".to_owned();
            draft.autosave_interval = "1".to_owned();
            draft.font_size = "100".to_owned();
            draft.tab_size = "0".to_owned();
            draft.ai_agent = "plan".to_owned();
        });
        let mut settings = Settings::default();
        let errors = draft.apply_to(&mut settings).expect_err("fuera de rango");
        let fields: Vec<Field> = errors.iter().map(|error| error.field).collect();
        assert!(fields.contains(&Field::Debounce));
        assert!(fields.contains(&Field::Timeout));
        assert!(fields.contains(&Field::PdfZoom));
        assert!(fields.contains(&Field::PdfCache));
        assert!(fields.contains(&Field::AutosaveInterval));
        assert!(fields.contains(&Field::AiAgent));
        assert!(fields.contains(&Field::FontSize));
        assert!(fields.contains(&Field::TabSize));
        // Nada se aplica si hay errores.
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn the_zoom_parser_rejects_non_finite_values() {
        let draft = draft_with(|draft| draft.pdf_zoom = "nan".to_owned());
        let errors = draft
            .apply_to(&mut Settings::default())
            .expect_err("nan no es válido");
        assert_eq!(errors[0].field, Field::PdfZoom);
    }

    #[test]
    fn empty_optional_fields_become_none_or_empty() {
        let draft = draft_with(|draft| {
            draft.main_file = "   ".to_owned();
            draft.extra_args = "  ".to_owned();
            draft.ai_model = String::new();
        });
        let mut settings = Settings::default();
        draft.apply_to(&mut settings).expect("opcionales vacíos");
        assert!(settings.latex.main_file.is_none());
        assert!(settings.latex.extra_args.is_empty());
        assert!(settings.ai.model.is_none());
    }

    #[test]
    fn extra_args_reject_quotes_and_split_on_whitespace() {
        let draft = draft_with(|draft| {
            draft.extra_args = "-shell-escape -synctex=1".to_owned();
        });
        let mut settings = Settings::default();
        draft.apply_to(&mut settings).expect("args válidos");
        assert_eq!(
            settings.latex.extra_args,
            vec!["-shell-escape", "-synctex=1"]
        );

        let draft = draft_with(|draft| draft.extra_args = "-jobname \"x\"".to_owned());
        let errors = draft
            .apply_to(&mut Settings::default())
            .expect_err("comillas no soportadas");
        assert_eq!(errors[0].field, Field::ExtraArgs);
    }

    #[test]
    fn the_agent_and_the_accent_are_validated() {
        let draft = draft_with(|draft| {
            draft.ai_agent = "  ".to_owned();
            draft.accent = "azul".to_owned();
        });
        let errors = draft
            .apply_to(&mut Settings::default())
            .expect_err("agente y acento inválidos");
        let fields: Vec<Field> = errors.iter().map(|error| error.field).collect();
        assert!(fields.contains(&Field::AiAgent));
        assert!(fields.contains(&Field::Accent));
    }

    #[test]
    fn a_valid_accent_is_normalized_to_uppercase() {
        let draft = draft_with(|draft| draft.accent = "#4c8dff".to_owned());
        let mut settings = Settings::default();
        draft.apply_to(&mut settings).expect("acento válido");
        assert_eq!(settings.ui.accent, "#4C8DFF");
    }

    #[test]
    fn system_theme_and_all_permission_presets_are_accepted() {
        for preset in ["ask", "project", "all"] {
            let draft = draft_with(|draft| {
                draft.theme = "system".to_owned();
                draft.ai_permission_preset = preset.to_owned();
            });
            let mut settings = Settings::default();
            draft.apply_to(&mut settings).expect("valores válidos");
            assert_eq!(settings.ui.theme, "system");
            assert_eq!(settings.ai.permission_preset, preset);
        }
    }

    #[test]
    fn every_enum_index_has_a_value() {
        assert_eq!(THEMES.len(), 3);
        assert_eq!(ZOOM_MODES.len(), 4);
        assert_eq!(LOG_LEVELS.len(), 5);
        assert_eq!(PERMISSION_PRESETS.len(), 3);
        assert_eq!(MODES, ["build", "latex-fix", "ask"]);
        assert!(PERMISSION_PRESETS.contains(&"ask"));
        assert!(PERMISSION_PRESETS.contains(&"project"));
        assert!(PERMISSION_PRESETS.contains(&"all"));
        assert!(ZOOM_MODES.contains(&"custom"));
    }

    /// Cubre la ruta de datos del diálogo: borrador validado → `Settings` →
    /// `save` atómico → recarga con `Settings::load` (lo que hace
    /// `AppRoot::apply_settings`).
    #[test]
    fn a_valid_draft_persists_the_new_surface_to_settings_json() {
        let (root, dir) = temp_root("persist");
        let draft = draft_with(|draft| {
            draft.theme = "light".to_owned();
            draft.accent = "#8B5CF6".to_owned();
            draft.ai_model = "anthropic/claude-sonnet-5-5".to_owned();
            draft.ai_agent = "latex-fix".to_owned();
            draft.ai_auto_start = false;
            draft.pdf_max_cache_mb = "512".to_owned();
            draft.main_file = "src/main.tex".to_owned();
            draft.font_family = "Cascadia Code".to_owned();
            draft.font_size = "15".to_owned();
            draft.tab_size = "4".to_owned();
            draft.soft_wrap = true;
        });

        let mut settings = Settings::default();
        draft.apply_to(&mut settings).expect("borrador válido");
        settings.save(&root).expect("guardar settings");

        let (loaded, warnings) = Settings::load(&root);
        assert!(warnings.is_empty(), "avisos inesperados: {warnings:?}");
        assert_eq!(loaded.ui.theme, "light");
        assert_eq!(loaded.ui.accent, "#8B5CF6");
        assert_eq!(
            loaded.ai.model.as_deref(),
            Some("anthropic/claude-sonnet-5-5")
        );
        assert_eq!(loaded.ai.agent, "latex-fix");
        assert!(!loaded.ai.auto_start);
        assert_eq!(loaded.pdf.max_cache_mb, 512);
        assert_eq!(loaded.latex.main_file.as_deref(), Some("src/main.tex"));
        assert_eq!(loaded.editor.font_family.as_deref(), Some("Cascadia Code"));
        assert_eq!(loaded.editor.font_size, 15.0);
        assert_eq!(loaded.editor.tab_size, 4);
        assert!(loaded.editor.soft_wrap);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
