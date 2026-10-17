//! Panel de Problemas (M1-D3): diagnósticos agrupados por archivo, filtros de
//! severidad/texto y callback «ir a línea».
//!
//! # Contrato con el shell (streams A/B)
//!
//! - El panel **no abre archivos ni llama servicios**: al pulsar una fila con
//!   archivo+línea emite [`ProblemsEvent::OpenSource`] (`cx.emit`). El shell se
//!   suscribe con `cx.subscribe(&panel, ...)` y decide cómo navegar.
//! - Los diagnósticos entran por [`ProblemsPanel::apply_diagnostics`] con su
//!   `generation` (la del `CompileQueue`); las generaciones viejas se descartan.
//!   Atajo: [`ProblemsPanel::apply_outcome`].
//! - El modelo ([`ProblemsModel`]) es Rust puro y se testea sin GPUI; el render
//!   solo lee estado y la lista se virtualiza con `v_virtual_list` (M2-9).
//!
//! # Filtros
//!
//! Severidad por interruptores (Errores/Avisos/Info; Info arranca apagado
//! porque los logs reales traen decenas de `LaTeX Font Info`/`hyperref Info`) y
//! filtro de texto por subcadena (mensaje, archivo o rótulo del tipo).

use std::rc::Rc;

use camino::Utf8PathBuf;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _,
    button::Button,
    dock::{BasePanel, DockArea, Panel as DockPanel, PanelEvent},
    h_flex,
    input::{Input, InputEvent, InputState},
    tooltip::Tooltip,
    v_flex, v_virtual_list,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString, Size,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, size,
};
use lsx_latex::{CompileOutcome, Diagnostic, DiagnosticSeverity};

/// Nombre estable del panel en el dock (persistencia de layout).
pub const PANEL_NAME: &str = "Problems";
/// Alto de una fila de diagnóstico (la lista virtualizada exige geometría fija).
const ROW_HEIGHT: Pixels = px(24.);
/// Alto de una cabecera de archivo.
const HEADER_HEIGHT: Pixels = px(22.);

/// Eventos que el panel emite al shell (el panel nunca navega por su cuenta).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemsEvent {
    /// El usuario pidió abrir `file:line` en el editor.
    OpenSource {
        file: Utf8PathBuf,
        line: u32,
        column: Option<u32>,
    },
}

/// Interruptores y texto del filtro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemsFilter {
    pub errors: bool,
    pub warnings: bool,
    pub info: bool,
    /// Subcadena a buscar (case-insensitive); vacío = sin filtro de texto.
    pub text: String,
}

impl Default for ProblemsFilter {
    fn default() -> Self {
        Self {
            errors: true,
            warnings: true,
            // Los logs reales traen decenas de Info de fuentes/paquetes; apagado
            // por defecto para que la lista abra con lo accionable.
            info: false,
            text: String::new(),
        }
    }
}

impl ProblemsFilter {
    /// ¿Pasa `diagnostic` los interruptores de severidad?
    pub fn allows_severity(&self, severity: DiagnosticSeverity) -> bool {
        match severity {
            DiagnosticSeverity::Error => self.errors,
            DiagnosticSeverity::Warning => self.warnings,
            DiagnosticSeverity::Info => self.info,
        }
    }
}

/// Fila aplanada del panel (cabecera de archivo o diagnóstico).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemsRow {
    Header {
        file: Option<Utf8PathBuf>,
        errors: usize,
        warnings: usize,
        infos: usize,
    },
    /// Índice en `ProblemsModel::diagnostics`.
    Item { index: usize },
}

/// Modelo puro del panel (sin GPUI): reducer `(estado, diagnósticos) → filas`.
#[derive(Debug, Default)]
pub struct ProblemsModel {
    generation: u64,
    diagnostics: Vec<Diagnostic>,
    filter: ProblemsFilter,
    rows: Vec<ProblemsRow>,
}

impl ProblemsModel {
    /// Generación de los diagnósticos visibles (0 = todavía sin compilación).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn filter(&self) -> &ProblemsFilter {
        &self.filter
    }

    /// Filas de la lista (cabeceras + diagnósticos) ya filtradas y ordenadas.
    pub fn rows(&self) -> &[ProblemsRow] {
        &self.rows
    }

    /// Reemplaza los diagnósticos de una generación.
    ///
    /// Devuelve `false` (y no toca nada) si la generación es más vieja que la
    /// visible: el panel descarta resultados obsoletos (§8 de INTERFACES).
    pub fn apply(&mut self, generation: u64, diagnostics: Vec<Diagnostic>) -> bool {
        if generation < self.generation {
            return false;
        }
        self.generation = generation;
        self.diagnostics = diagnostics;
        self.rebuild();
        true
    }

    /// Vacía el panel (p. ej. al cerrar el proyecto). Conserva la generación
    /// para que resultados viejos en vuelo sigan descartándose hasta subirla.
    pub fn clear(&mut self) -> bool {
        if self.diagnostics.is_empty() && self.rows.is_empty() {
            return false;
        }
        self.diagnostics.clear();
        self.rows.clear();
        true
    }

    pub fn set_filter(&mut self, filter: ProblemsFilter) -> bool {
        if self.filter == filter {
            return false;
        }
        self.filter = filter;
        self.rebuild();
        true
    }

    pub fn set_filter_text(&mut self, text: impl Into<String>) -> bool {
        let mut filter = self.filter.clone();
        filter.text = text.into();
        self.set_filter(filter)
    }

    /// Interruptor de una severidad; devuelve `true` si cambió algo.
    pub fn toggle_severity(&mut self, severity: DiagnosticSeverity) -> bool {
        let mut filter = self.filter.clone();
        match severity {
            DiagnosticSeverity::Error => filter.errors = !filter.errors,
            DiagnosticSeverity::Warning => filter.warnings = !filter.warnings,
            DiagnosticSeverity::Info => filter.info = !filter.info,
        }
        self.set_filter(filter)
    }

    /// Cuenta de diagnósticos por severidad (totales, sin filtrar).
    pub fn total_counts(&self) -> (usize, usize, usize) {
        let mut counts = (0, 0, 0);
        for diagnostic in &self.diagnostics {
            match diagnostic.severity {
                DiagnosticSeverity::Error => counts.0 += 1,
                DiagnosticSeverity::Warning => counts.1 += 1,
                DiagnosticSeverity::Info => counts.2 += 1,
            }
        }
        counts
    }

    /// ¿Pasa el diagnóstico todos los filtros?
    pub fn matches(&self, diagnostic: &Diagnostic) -> bool {
        if !self.filter.allows_severity(diagnostic.severity) {
            return false;
        }
        let needle = self.filter.text.trim();
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_lowercase();
        let haystacks = [
            diagnostic.message.to_lowercase(),
            diagnostic
                .file
                .as_deref()
                .map(|file| file.as_str().to_lowercase())
                .unwrap_or_default(),
            diagnostic.kind.label().to_lowercase(),
        ];
        haystacks.iter().any(|haystack| haystack.contains(&needle))
    }

    /// Recalcula `rows` (agrupación + orden determinista).
    fn rebuild(&mut self) {
        use std::collections::BTreeMap;

        let mut groups: BTreeMap<Option<Utf8PathBuf>, Vec<usize>> = BTreeMap::new();
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if self.matches(diagnostic) {
                groups
                    .entry(diagnostic.file.clone())
                    .or_default()
                    .push(index);
            }
        }
        let mut group_list: Vec<(Option<Utf8PathBuf>, Vec<usize>)> = groups.into_iter().collect();
        // Grupos con errores primero; `sin archivo` siempre al final.
        group_list.sort_by(|(file_a, indexes_a), (file_b, indexes_b)| {
            severity_rank(max_severity(&self.diagnostics, indexes_a))
                .cmp(&severity_rank(max_severity(&self.diagnostics, indexes_b)))
                .then_with(|| file_a.is_none().cmp(&file_b.is_none()))
                .then_with(|| file_a.cmp(file_b))
        });

        let mut rows = Vec::new();
        for (file, mut indexes) in group_list {
            indexes.sort_by(|&a, &b| {
                let (one, other) = (&self.diagnostics[a], &self.diagnostics[b]);
                one.line
                    .unwrap_or(u32::MAX)
                    .cmp(&other.line.unwrap_or(u32::MAX))
                    .then_with(|| severity_rank(one.severity).cmp(&severity_rank(other.severity)))
                    .then_with(|| one.message.cmp(&other.message))
            });
            let mut counts = (0usize, 0usize, 0usize);
            for &index in &indexes {
                match self.diagnostics[index].severity {
                    DiagnosticSeverity::Error => counts.0 += 1,
                    DiagnosticSeverity::Warning => counts.1 += 1,
                    DiagnosticSeverity::Info => counts.2 += 1,
                }
            }
            rows.push(ProblemsRow::Header {
                file,
                errors: counts.0,
                warnings: counts.1,
                infos: counts.2,
            });
            rows.extend(indexes.into_iter().map(|index| ProblemsRow::Item { index }));
        }
        self.rows = rows;
    }
}

fn severity_rank(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 2,
    }
}

fn max_severity(diagnostics: &[Diagnostic], indexes: &[usize]) -> DiagnosticSeverity {
    indexes
        .iter()
        .map(|&index| diagnostics[index].severity)
        .min_by_key(|severity| severity_rank(*severity))
        .unwrap_or(DiagnosticSeverity::Info)
}

/// Panel de Problemas (entidad GPUI; el estado vive en [`ProblemsModel`]).
pub struct ProblemsPanel {
    focus: FocusHandle,
    model: ProblemsModel,
    /// Tamaños por fila que exige `v_virtual_list`.
    row_sizes: Rc<Vec<Size<Pixels>>>,
    text_filter: Entity<InputState>,
    _filter_events: Subscription,
    visible: bool,
    dock_area: Option<WeakEntity<DockArea>>,
}

impl ProblemsPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filtrar problemas…"));
        let filter_events = cx.subscribe(&text_filter, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let text = this.text_filter.read(cx).value().to_string();
                if this.model.set_filter_text(text) {
                    this.refresh_row_sizes();
                    cx.notify();
                }
            }
        });
        Self {
            focus: cx.focus_handle(),
            model: ProblemsModel::default(),
            row_sizes: Rc::new(Vec::new()),
            text_filter,
            _filter_events: filter_events,
            visible: true,
            dock_area: None,
        }
    }

    /// Registra el dock para poder repintarlo al colapsar la hoja (patrón M0-S1).
    pub fn with_dock_area(mut self, dock_area: WeakEntity<DockArea>) -> Self {
        self.dock_area = Some(dock_area);
        self
    }

    /// Modelo puro (diagnósticos, filtros y filas).
    pub fn model(&self) -> &ProblemsModel {
        &self.model
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.model.diagnostics()
    }

    /// Entrada del reducer: reemplaza los diagnósticos de `generation`.
    pub fn apply_diagnostics(
        &mut self,
        generation: u64,
        diagnostics: Vec<Diagnostic>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.model.apply(generation, diagnostics) {
            return false;
        }
        self.refresh_row_sizes();
        cx.notify();
        true
    }

    /// Conveniencia para el wiring del shell: aplica un `CompileOutcome`.
    pub fn apply_outcome(
        &mut self,
        generation: u64,
        outcome: &CompileOutcome,
        cx: &mut Context<Self>,
    ) -> bool {
        self.apply_diagnostics(generation, outcome.diagnostics.clone(), cx)
    }

    /// Vacía la lista (p. ej. al cerrar el proyecto).
    pub fn clear(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.model.clear() {
            return false;
        }
        self.refresh_row_sizes();
        cx.notify();
        true
    }

    /// Activa/desactiva una severidad (los interruptores de la toolbar).
    pub fn toggle_severity(&mut self, severity: DiagnosticSeverity, cx: &mut Context<Self>) {
        if self.model.toggle_severity(severity) {
            self.refresh_row_sizes();
            cx.notify();
        }
    }

    /// Si la hoja está actualmente en pantalla.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Muestra u oculta la hoja; si cambia, avisa al dock y repinta.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if let Some(area) = &self.dock_area
            && let Err(err) = area.update(cx, |_, cx| cx.notify())
        {
            // El dock puede no existir todavía durante la construcción o en
            // tests puros; no es un error accionable para el usuario.
            tracing::debug!(error = %err, "dock no disponible");
        }
        cx.notify();
    }

    /// Invierte la visibilidad de la hoja.
    pub fn toggle_visible(&mut self, cx: &mut Context<Self>) {
        let next = !self.visible;
        self.set_visible(next, cx);
    }

    fn refresh_row_sizes(&mut self) {
        self.row_sizes = Rc::new(
            self.model
                .rows()
                .iter()
                .map(|row| match row {
                    // Para listas verticales `v_virtual_list` solo usa el alto.
                    ProblemsRow::Header { .. } => size(px(0.), HEADER_HEIGHT),
                    ProblemsRow::Item { .. } => size(px(0.), ROW_HEIGHT),
                })
                .collect(),
        );
    }

    /// «Ir a línea»: emite el evento y no navega por su cuenta.
    fn open_row(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(diagnostic) = self.model.diagnostics().get(index) else {
            return;
        };
        let (Some(file), Some(line)) = (diagnostic.file.clone(), diagnostic.line) else {
            return;
        };
        cx.emit(ProblemsEvent::OpenSource {
            file,
            line,
            column: diagnostic.column,
        });
    }

    fn severity_button(
        &self,
        severity: DiagnosticSeverity,
        label: &str,
        count: usize,
        color: gpui_kit::Hsla,
        cx: &Context<Self>,
    ) -> Button {
        let selected = self.model.filter().allows_severity(severity);
        Button::new(SharedString::from(format!(
            "problems-filter-{}",
            severity_rank(severity)
        )))
        .label(SharedString::from(format!("{label} {count}")))
        .text_color(color)
        .xsmall()
        .tab_stop(false)
        .selected(selected)
        .tooltip(if selected {
            format!("Ocultar: {label}")
        } else {
            format!("Mostrar: {label}")
        })
        .on_click(cx.listener(move |this, _, _window, cx| this.toggle_severity(severity, cx)))
    }

    fn collapse_button(&self, cx: &Context<Self>) -> Button {
        let icon = if self.visible {
            gpui_kit::component::IconName::EyeOff
        } else {
            gpui_kit::component::IconName::Eye
        };
        Button::new("problems-collapse")
            .icon(icon)
            .xsmall()
            .tab_stop(false)
            .tooltip(if self.visible {
                "Colapsar «Problemas»"
            } else {
                "Mostrar «Problemas»"
            })
            .on_click(cx.listener(|this, _, _window, cx| this.toggle_visible(cx)))
    }

    fn render_toolbar(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let (errors, warnings, infos) = self.model.total_counts();
        h_flex()
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.text_filter).small()),
            )
            .child(self.severity_button(
                DiagnosticSeverity::Error,
                "Errores",
                errors,
                cx.theme().danger,
                cx,
            ))
            .child(self.severity_button(
                DiagnosticSeverity::Warning,
                "Avisos",
                warnings,
                cx.theme().warning,
                cx,
            ))
            .child(self.severity_button(
                DiagnosticSeverity::Info,
                "Info",
                infos,
                cx.theme().info,
                cx,
            ))
    }

    fn render_empty(&self, message: &str, cx: &Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(SharedString::from(message.to_string()))
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        if self.model.diagnostics().is_empty() {
            return div()
                .flex_1()
                .min_h_0()
                .child(self.render_empty("Sin problemas por ahora", cx));
        }
        if self.model.rows().is_empty() {
            return div()
                .flex_1()
                .min_h_0()
                .child(self.render_empty("Ningún problema coincide con los filtros", cx));
        }
        let row_sizes = self.row_sizes.clone();
        div().flex_1().min_h_0().child(
            v_virtual_list(
                cx.entity(),
                "problems-list",
                row_sizes,
                |panel, visible_range, _window, cx| {
                    visible_range
                        .map(|index| panel.render_row(index, cx))
                        .collect::<Vec<_>>()
                },
            )
            .size_full(),
        )
    }

    fn render_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let Some(row) = self.model.rows().get(index) else {
            return div().into_any_element();
        };
        match row {
            ProblemsRow::Header {
                file,
                errors,
                warnings,
                infos,
            } => self.render_header(index, file.as_deref(), (*errors, *warnings, *infos), cx),
            ProblemsRow::Item { index } => self.render_item(*index, cx),
        }
    }

    fn render_header(
        &self,
        row_index: usize,
        file: Option<&camino::Utf8Path>,
        counts: (usize, usize, usize),
        cx: &Context<Self>,
    ) -> AnyElement {
        let name = file
            .and_then(|file| file.file_name())
            .unwrap_or("Sin archivo");
        let mut summary: Vec<String> = Vec::new();
        if counts.0 > 0 {
            summary.push(format_count(counts.0, "error", "errores"));
        }
        if counts.1 > 0 {
            summary.push(format_count(counts.1, "advertencia", "advertencias"));
        }
        if counts.2 > 0 {
            summary.push(format_count(counts.2, "info", "info"));
        }
        let full_path = file.map(|file| file.as_str().to_string());
        let header = h_flex()
            .id(("problems-header", row_index))
            .h(HEADER_HEIGHT)
            .w_full()
            .gap_2()
            .px_2()
            .bg(cx.theme().muted)
            .text_sm()
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .child(div().flex_1().min_w_0().truncate().child(name.to_string()))
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(cx.theme().muted_foreground)
                    .child(summary.join(" · ")),
            );
        match full_path {
            Some(path) => header
                .tooltip(move |window, cx| {
                    Tooltip::new(SharedString::from(path.clone())).build(window, cx)
                })
                .into_any_element(),
            None => header.into_any_element(),
        }
    }

    fn render_item(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let Some(diagnostic) = self.model.diagnostics().get(index) else {
            return div().into_any_element();
        };
        let severity_color = match diagnostic.severity {
            DiagnosticSeverity::Error => cx.theme().danger,
            DiagnosticSeverity::Warning => cx.theme().warning,
            DiagnosticSeverity::Info => cx.theme().info,
        };
        let location = match (diagnostic.line, diagnostic.column) {
            (Some(line), Some(column)) => format!("{line}:{column}"),
            (Some(line), None) => line.to_string(),
            (None, _) => String::new(),
        };
        let navigable = diagnostic.file.is_some() && diagnostic.line.is_some();
        let tooltip_text = format!(
            "{} — {}\n{}",
            diagnostic.severity.label(),
            diagnostic.kind.label(),
            diagnostic.message
        );
        let row = h_flex()
            .id(("problems-row", index))
            .h(ROW_HEIGHT)
            .w_full()
            .gap_2()
            .px_2()
            .text_sm()
            .when(navigable, |this| this.cursor_pointer())
            .hover(|style| style.bg(cx.theme().secondary))
            .on_click(cx.listener(move |this, _, _window, cx| {
                if navigable {
                    this.open_row(index, cx);
                }
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .size_2()
                    .rounded_full()
                    .bg(severity_color),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(diagnostic.message.clone()),
            )
            .when(!location.is_empty(), |this| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(location),
                )
            });
        row.tooltip(move |window, cx| {
            Tooltip::new(SharedString::from(tooltip_text.clone())).build(window, cx)
        })
        .into_any_element()
    }
}

/// «3 errores», «1 error»…
fn format_count(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

impl BasePanel for ProblemsPanel {
    fn panel_name(&self) -> &'static str {
        PANEL_NAME
    }

    fn visible(&self, _cx: &App) -> bool {
        self.visible
    }

    /// El panel es fijo: no se cierra, solo se colapsa (I-7).
    fn closable(&self, _cx: &App) -> bool {
        false
    }
}

impl DockPanel for ProblemsPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(SharedString::from("Problemas"))
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Problemas"
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        Some(vec![self.collapse_button(cx)])
    }
}

impl EventEmitter<PanelEvent> for ProblemsPanel {}
impl EventEmitter<ProblemsEvent> for ProblemsPanel {}

impl Focusable for ProblemsPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ProblemsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = self.render_list(cx);
        div()
            .id("problems-panel")
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .track_focus(&self.focus)
            .child(self.render_toolbar(cx))
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsx_latex::DiagnosticKind;

    fn diag(
        severity: DiagnosticSeverity,
        kind: DiagnosticKind,
        file: Option<&str>,
        line: Option<u32>,
        message: &str,
    ) -> Diagnostic {
        Diagnostic {
            severity,
            kind,
            file: file.map(Utf8PathBuf::from),
            line,
            column: None,
            message: message.to_string(),
            raw_block: message.to_string(),
        }
    }

    fn sample() -> Vec<Diagnostic> {
        vec![
            diag(
                DiagnosticSeverity::Warning,
                DiagnosticKind::Overfull,
                Some("main.tex"),
                Some(37),
                "Overfull \\hbox en el párrafo",
            ),
            diag(
                DiagnosticSeverity::Error,
                DiagnosticKind::LatexError,
                Some("main.tex"),
                Some(4),
                "Undefined control sequence",
            ),
            diag(
                DiagnosticSeverity::Info,
                DiagnosticKind::Info,
                None,
                None,
                "Package hyperref Info: Hyper figures OFF",
            ),
            diag(
                DiagnosticSeverity::Error,
                DiagnosticKind::MissingPackage {
                    package: "lastpage".to_string(),
                },
                Some("capitulos/uno.tex"),
                Some(1),
                "File `lastpage.sty' not found",
            ),
        ]
    }

    #[test]
    fn agrupa_por_archivo_con_errores_primero() {
        let mut model = ProblemsModel::default();
        assert!(model.apply(1, sample()));
        // Con Info visible para poder comprobar que el grupo «sin archivo» va último.
        assert!(model.toggle_severity(DiagnosticSeverity::Info));
        let rows = model.rows();
        // Orden de grupos: severidad máxima, `sin archivo` al final y, a igualdad,
        // ruta ascendente (capitulos/uno.tex < main.tex).
        assert_eq!(
            rows.first(),
            Some(&ProblemsRow::Header {
                file: Some(Utf8PathBuf::from("capitulos/uno.tex")),
                errors: 1,
                warnings: 0,
                infos: 0,
            })
        );
        let files: Vec<Option<Utf8PathBuf>> = rows
            .iter()
            .filter_map(|row| match row {
                ProblemsRow::Header { file, .. } => Some(file.clone()),
                ProblemsRow::Item { .. } => None,
            })
            .collect();
        assert_eq!(
            files,
            vec![
                Some(Utf8PathBuf::from("capitulos/uno.tex")),
                Some(Utf8PathBuf::from("main.tex")),
                None,
            ]
        );
        // Dentro del grupo, por línea ascendente.
        let lines: Vec<Option<u32>> = rows
            .iter()
            .filter_map(|row| match row {
                ProblemsRow::Item { index } => Some(model.diagnostics()[*index].line),
                ProblemsRow::Header { .. } => None,
            })
            .collect();
        assert_eq!(lines, vec![Some(1), Some(4), Some(37), None]);
    }

    #[test]
    fn el_filtro_de_severidad_oculta_y_muestra() {
        let mut model = ProblemsModel::default();
        model.apply(1, sample());
        // Defaults: errores y advertencias visibles; Info apagada.
        assert!(model.matches(&sample()[0]), "warning visible");
        assert!(model.matches(&sample()[1]), "error visible");
        assert!(!model.matches(&sample()[2]), "info oculta por defecto");
        // Encender Info la muestra.
        assert!(model.toggle_severity(DiagnosticSeverity::Info));
        assert!(model.matches(&sample()[2]));
        assert!(model.set_filter_text("hyperref"));
        assert!(model.rows().iter().any(|row| matches!(
            row,
            ProblemsRow::Item { index }
                if model.diagnostics()[*index].message.contains("hyperref")
        )));
        assert!(model.set_filter_text(""));
        // Apagar Errores los quita de la lista.
        assert!(model.toggle_severity(DiagnosticSeverity::Error));
        assert!(!model.matches(&sample()[1]));
        assert!(!model.matches(&sample()[3]));
        assert!(
            model
                .rows()
                .iter()
                .all(|row| !matches!(row, ProblemsRow::Header { errors, .. } if *errors > 0)),
            "{:#?}",
            model.rows()
        );
    }

    #[test]
    fn el_filtro_de_texto_busca_en_mensaje_archivo_y_tipo() {
        let mut model = ProblemsModel::default();
        model.apply(1, sample());
        assert!(model.set_filter_text("undefined"));
        assert_eq!(
            model
                .rows()
                .iter()
                .filter(|row| matches!(row, ProblemsRow::Item { .. }))
                .count(),
            1
        );
        // Por archivo.
        assert!(model.set_filter_text("capitulos"));
        assert!(model.rows().iter().any(|row| matches!(
            row,
            ProblemsRow::Item { index }
                if model.diagnostics()[*index].kind.label() == "Falta un paquete"
        )));
        // Por rótulo del tipo (case-insensitive).
        assert!(model.set_filter_text("FALTA UN PAQUETE"));
        assert!(
            model
                .rows()
                .iter()
                .filter(|row| matches!(row, ProblemsRow::Item { .. }))
                .count()
                == 1
        );
        // Limpiar el texto restaura el resto de severidades visibles.
        assert!(model.set_filter_text(""));
        assert_eq!(
            model
                .rows()
                .iter()
                .filter(|row| matches!(row, ProblemsRow::Item { .. }))
                .count(),
            3
        );
    }

    #[test]
    fn generaciones_viejas_se_descartan() {
        let mut model = ProblemsModel::default();
        assert!(model.apply(7, sample()));
        assert_eq!(model.generation(), 7);
        assert!(!model.apply(6, Vec::new()));
        assert_eq!(model.diagnostics().len(), 4, "los datos viejos no entran");
        // Repetir la misma generación sí reemplaza (p. ej. reintento).
        assert!(model.apply(7, Vec::new()));
        assert!(model.diagnostics().is_empty());
        assert!(!model.clear(), "ya estaba vacío");
    }

    #[test]
    fn conteos_totales_por_severidad() {
        let mut model = ProblemsModel::default();
        model.apply(1, sample());
        assert_eq!(model.total_counts(), (2, 1, 1));
    }
}
