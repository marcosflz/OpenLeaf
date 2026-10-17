//! Layout estable y serializable del shell de 4 hojas (spike M0-S1).
//!
//! Este módulo es **puro Rust** (sin GPUI): es la forma canónica que
//! `config/settings.json → ui.dock` usará en M1, según `INTERFACES.md` §7:
//!
//! ```json
//! {
//!   "version": 1,
//!   "panels": [
//!     { "id": "tree", "kind": "tree", "width_fraction": 0.18, "collapsed": false },
//!     { "id": "editor", "kind": "editor", "width_fraction": 0.46, "collapsed": false },
//!     { "id": "pdf", "kind": "pdf", "width_fraction": 0.21, "collapsed": false },
//!     { "id": "chat", "kind": "chat", "width_fraction": 0.15, "collapsed": false }
//!   ]
//! }
//! ```
//!
//! `focus` (modo focus de M1/A1) mantiene la forma
//! `{ "hidden": [...], "previous_fractions": [...] }`; el spike solo la fija y
//! la hace round-trip.
//!
//! Decisiones del spike:
//! - `id` identifica la **instancia** estable de la hoja (`"tree"`); `kind` el
//!   tipo de panel. Un futuro layout con dos hojas del mismo tipo podrá usar
//!   `id` distinto sin romper el esquema.
//! - `width_fraction` es una proporción relativa (no hace falta que sume 1).
//! - `collapsed` mapea a `Panel::visible(false)` del dock del kit: la hoja
//!   libera su slot (ver `shell.rs`).

use serde::{Deserialize, Serialize};

/// Versión del esquema que este build escribe en el JSON.
pub const SCHEMA_VERSION: u32 = 1;

/// Fracciones por defecto de `[árbol | editor | pdf | chat]`.
const DEFAULT_FRACTIONS: [f32; 4] = [0.18, 0.46, 0.21, 0.15];

/// Hoja del shell, en orden canónico de izquierda a derecha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotKind {
    Tree,
    Editor,
    Pdf,
    Chat,
}

impl SlotKind {
    /// Orden canónico del layout `[árbol | editor | pdf | chat]`.
    pub const ALL: [SlotKind; 4] = [
        SlotKind::Tree,
        SlotKind::Editor,
        SlotKind::Pdf,
        SlotKind::Chat,
    ];

    /// Posición de la hoja dentro del split.
    pub const fn index(self) -> usize {
        match self {
            SlotKind::Tree => 0,
            SlotKind::Editor => 1,
            SlotKind::Pdf => 2,
            SlotKind::Chat => 3,
        }
    }

    /// Identidad estable de la hoja en el JSON (`id`).
    pub const fn id(self) -> &'static str {
        match self {
            SlotKind::Tree => "tree",
            SlotKind::Editor => "editor",
            SlotKind::Pdf => "pdf",
            SlotKind::Chat => "chat",
        }
    }

    /// Nombre estable con el que el kit persiste el panel (`Panel::panel_name`).
    ///
    /// Una vez elegido no se cambia: `DockArea::load` reconstruye el panel
    /// buscándolo en `PanelRegistry` por este nombre.
    pub const fn panel_name(self) -> &'static str {
        match self {
            SlotKind::Tree => "Spike.Tree",
            SlotKind::Editor => "Spike.Editor",
            SlotKind::Pdf => "Spike.Pdf",
            SlotKind::Chat => "Spike.Chat",
        }
    }

    /// Rótulo visible de la hoja.
    pub const fn label(self) -> &'static str {
        match self {
            SlotKind::Tree => "ÁRBOL",
            SlotKind::Editor => "EDITOR",
            SlotKind::Pdf => "PDF",
            SlotKind::Chat => "CHAT",
        }
    }

    /// Fracción de ancho por defecto de la hoja.
    pub const fn default_fraction(self) -> f32 {
        DEFAULT_FRACTIONS[self.index()]
    }

    /// Texto placeholder del contenido del spike (`"1 · ÁRBOL"`).
    pub fn placeholder(self) -> String {
        format!("{} · {}", self.index() + 1, self.label())
    }

    /// Resuelve una hoja a partir de su `id` de layout.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }

    /// Resuelve una hoja a partir del `panel_name` persistido por el kit.
    pub fn from_panel_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.panel_name() == name)
    }
}

/// Estado serializado de una hoja.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SlotState {
    /// Identidad estable de la instancia (`"tree"`…).
    pub id: String,
    /// Tipo de panel de la hoja.
    pub kind: SlotKind,
    /// Ancho relativo dentro del split; solo se usan las proporciones.
    pub width_fraction: f32,
    /// Si la hoja está colapsada (oculta y sin slot en el dock).
    pub collapsed: bool,
}

/// Estado del modo focus (M1/A1): hojas ocultas y anchos previos.
///
/// El spike fija la forma serializada para que M1 no cambie el esquema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FocusMode {
    /// Hojas ocultas por el modo focus, en orden canónico.
    pub hidden: Vec<String>,
    /// Fracciones de ancho previas a entrar en foco.
    pub previous_fractions: Vec<f32>,
}

/// Layout completo de las 4 hojas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SpikeLayout {
    /// Versión del esquema; ver [`SCHEMA_VERSION`].
    pub version: u32,
    /// Estado por hoja, en el orden en que se aplican al split.
    pub panels: Vec<SlotState>,
    /// Modo focus; `None` mientras no se active (M1/A1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<FocusMode>,
}

impl Default for SpikeLayout {
    fn default() -> Self {
        Self::four_columns()
    }
}

impl SpikeLayout {
    /// Layout por defecto `[árbol | editor | pdf | chat]`, todo visible.
    pub fn four_columns() -> Self {
        Self {
            version: SCHEMA_VERSION,
            panels: SlotKind::ALL
                .into_iter()
                .map(|kind| SlotState {
                    id: kind.id().to_owned(),
                    kind,
                    width_fraction: kind.default_fraction(),
                    collapsed: false,
                })
                .collect(),
            focus: None,
        }
    }

    /// Construye un layout a partir de hojas observadas en el dock.
    ///
    /// `observed` puede venir incompleto o con fracciones no utilizables
    /// (`0`, `NaN`): las hojas ausentes o inválidas toman sus valores por
    /// defecto. Las hojas extra no contempladas se ignoran.
    pub fn from_observed(observed: &[(SlotKind, f32, bool)]) -> Self {
        let panels = SlotKind::ALL
            .into_iter()
            .map(|kind| {
                let found = observed.iter().find(|(observed, _, _)| *observed == kind);
                match found {
                    Some((_, fraction, collapsed)) if fraction.is_finite() && *fraction > 0.0 => {
                        SlotState {
                            id: kind.id().to_owned(),
                            kind,
                            width_fraction: *fraction,
                            collapsed: *collapsed,
                        }
                    }
                    _ => SlotState {
                        id: kind.id().to_owned(),
                        kind,
                        width_fraction: kind.default_fraction(),
                        collapsed: found.is_some_and(|(_, _, collapsed)| *collapsed),
                    },
                }
            })
            .collect();
        Self {
            version: SCHEMA_VERSION,
            panels,
            focus: None,
        }
    }

    /// Serializa a JSON (formato estable del spike).
    pub fn to_json(&self) -> Result<String, SpikeLayoutError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Parsea y valida un JSON de layout.
    ///
    /// Reglas: la versión no puede ser más nueva que [`SCHEMA_VERSION`]; las
    /// fracciones deben ser finitas y no negativas; las hojas ausentes o con
    /// fracción cero toman sus valores por defecto en el orden canónico.
    pub fn from_json(json: &str) -> Result<Self, SpikeLayoutError> {
        let parsed: Self = serde_json::from_str(json)?;
        parsed.validated()
    }

    fn validated(mut self) -> Result<Self, SpikeLayoutError> {
        if self.version > SCHEMA_VERSION {
            return Err(SpikeLayoutError::UnsupportedVersion(self.version));
        }
        for slot in &self.panels {
            if !slot.width_fraction.is_finite() || slot.width_fraction < 0.0 {
                return Err(SpikeLayoutError::InvalidFraction(slot.id.clone()));
            }
        }
        let observed: Vec<(SlotKind, f32, bool)> = SlotKind::ALL
            .into_iter()
            .filter_map(|kind| {
                let slot = self
                    .panels
                    .iter()
                    .find(|slot| slot.kind == kind && slot.width_fraction.is_finite())?;
                Some((kind, slot.width_fraction, slot.collapsed))
            })
            .collect();
        self.panels = SpikeLayout::from_observed(&observed).panels;
        Ok(self)
    }

    /// Estado de una hoja, si el layout la contempla.
    pub fn slot(&self, kind: SlotKind) -> Option<&SlotState> {
        self.panels.iter().find(|slot| slot.kind == kind)
    }

    /// Si la hoja está colapsada (ausente = no colapsada).
    pub fn is_collapsed(&self, kind: SlotKind) -> bool {
        self.slot(kind).is_some_and(|slot| slot.collapsed)
    }

    /// Anchos en píxeles para un ancho total dado, en orden canónico.
    pub fn sizes_px(&self, total_width: f32) -> Vec<f32> {
        SlotKind::ALL
            .into_iter()
            .map(|kind| {
                let fraction = self.slot(kind).map_or(0.0, |slot| slot.width_fraction);
                (fraction * total_width).max(0.0)
            })
            .collect()
    }
}

/// Errores al leer/escribir el layout del spike.
#[derive(Debug, thiserror::Error)]
pub enum SpikeLayoutError {
    /// JSON ilegible o con campos obligatorios ausentes.
    #[error("JSON de layout inválido: {0}")]
    Json(#[from] serde_json::Error),
    /// El archivo lo escribió una versión más nueva del esquema.
    #[error("versión de layout no soportada: {0}")]
    UnsupportedVersion(u32),
    /// Una fracción no es finita o es negativa.
    #[error("fracción de ancho inválida en la hoja «{0}»")]
    InvalidFraction(String),
    /// Error de disco al guardar o leer el archivo de demo.
    #[error("error de E/S del layout: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_layout_serializes_the_pinned_shape() {
        let layout = SpikeLayout::four_columns();
        let value = serde_json::to_value(&layout).expect("serializa");

        assert_eq!(value["version"], json!(1));
        let panels = value["panels"].as_array().expect("panels es array");
        assert_eq!(panels.len(), 4);
        let ids: Vec<&str> = panels.iter().map(|p| p["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["tree", "editor", "pdf", "chat"]);
        let kinds: Vec<&str> = panels.iter().map(|p| p["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["tree", "editor", "pdf", "chat"]);
        for panel in panels {
            assert_eq!(panel["collapsed"], json!(false));
            assert!(panel["width_fraction"].as_f64().unwrap() > 0.0);
        }
        assert!(value.get("focus").is_none(), "focus se omite si es None");
    }

    #[test]
    fn a_layout_round_trips_through_json_exactly() {
        let layout = SpikeLayout::four_columns();
        let json = layout.to_json().expect("serializa");
        let restored = SpikeLayout::from_json(&json).expect("parsea");
        assert_eq!(restored, layout);
    }

    #[test]
    fn fractions_and_order_survive_a_round_trip() {
        let mut layout = SpikeLayout::four_columns();
        layout.panels[0].width_fraction = 0.05;
        layout.panels[1].width_fraction = 0.7;
        layout.panels[2].width_fraction = 0.1;
        layout.panels[3].width_fraction = 0.15;
        layout.panels[3].collapsed = true;

        let restored = SpikeLayout::from_json(&layout.to_json().unwrap()).unwrap();
        assert_eq!(restored, layout);
        assert!(restored.is_collapsed(SlotKind::Chat));
        assert!(!restored.is_collapsed(SlotKind::Tree));
    }

    #[test]
    fn missing_slots_are_filled_with_defaults_in_canonical_order() {
        let json = json!({
            "version": 1,
            "panels": [
                { "id": "editor", "kind": "editor", "width_fraction": 0.8, "collapsed": true }
            ]
        })
        .to_string();

        let layout = SpikeLayout::from_json(&json).unwrap();
        assert_eq!(layout.panels.len(), 4);
        let order: Vec<SlotKind> = layout.panels.iter().map(|slot| slot.kind).collect();
        assert_eq!(order, SlotKind::ALL);
        assert_eq!(layout.slot(SlotKind::Editor).unwrap().width_fraction, 0.8);
        assert!(layout.is_collapsed(SlotKind::Editor));
        assert_eq!(
            layout.slot(SlotKind::Tree).unwrap().width_fraction,
            SlotKind::Tree.default_fraction()
        );
    }

    #[test]
    fn focus_mode_round_trips_with_its_pinned_shape() {
        let mut layout = SpikeLayout::four_columns();
        layout.focus = Some(FocusMode {
            hidden: vec!["tree".to_owned(), "chat".to_owned()],
            previous_fractions: vec![0.18, 0.15],
        });

        let value = serde_json::to_value(&layout).unwrap();
        assert_eq!(value["focus"]["hidden"], json!(["tree", "chat"]));
        assert_eq!(
            value["focus"]["previous_fractions"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let restored = SpikeLayout::from_json(&layout.to_json().unwrap()).unwrap();
        assert_eq!(restored, layout);
    }

    #[test]
    fn a_newer_schema_version_is_rejected() {
        let json = json!({ "version": 99, "panels": [] }).to_string();
        let err = SpikeLayout::from_json(&json).expect_err("versión 99 debe fallar");
        assert!(matches!(err, SpikeLayoutError::UnsupportedVersion(99)));
    }

    #[test]
    fn invalid_fractions_are_rejected() {
        let json = json!({
            "version": 1,
            "panels": [
                { "id": "tree", "kind": "tree", "width_fraction": -0.2, "collapsed": false }
            ]
        })
        .to_string();
        let err = SpikeLayout::from_json(&json).expect_err("fracción negativa debe fallar");
        assert!(matches!(err, SpikeLayoutError::InvalidFraction(id) if id == "tree"));
    }

    #[test]
    fn garbage_json_is_an_error_not_a_panic() {
        assert!(SpikeLayout::from_json("no soy json").is_err());
        assert!(SpikeLayout::from_json("{}").is_err());
    }

    #[test]
    fn missing_or_zero_fractions_fall_back_to_defaults() {
        // Una hoja presente con fracción cero equivale a "sin información":
        // toma su default (no invalida el layout).
        let json = json!({
            "version": 1,
            "panels": [
                { "id": "tree", "kind": "tree", "width_fraction": 0.0, "collapsed": false }
            ]
        })
        .to_string();
        let layout = SpikeLayout::from_json(&json).expect("se completa con defaults");
        assert_eq!(
            layout.slot(SlotKind::Tree).unwrap().width_fraction,
            SlotKind::Tree.default_fraction()
        );

        // Y un layout con todas las fracciones en cero también vuelve a los
        // defaults: nunca se guarda un dock sin anchos.
        let mut all_zero = SpikeLayout::four_columns();
        for slot in &mut all_zero.panels {
            slot.width_fraction = 0.0;
        }
        let sanitized = serde_json::to_string(&all_zero)
            .map(|json| SpikeLayout::from_json(&json).expect("vuelve a defaults"))
            .expect("serializa");
        assert!(
            sanitized
                .panels
                .iter()
                .all(|slot| slot.width_fraction > 0.0)
        );
    }

    #[test]
    fn sizes_follow_fractions_in_canonical_order() {
        let layout = SpikeLayout::four_columns();
        let sizes = layout.sizes_px(1_000.0);
        assert_eq!(sizes.len(), 4);
        assert_eq!(sizes[0], 180.0);
        assert!((sizes.iter().sum::<f32>() - 1_000.0).abs() < 0.5);
    }

    #[test]
    fn slot_lookup_by_id_and_panel_name() {
        assert_eq!(SlotKind::from_id("pdf"), Some(SlotKind::Pdf));
        assert_eq!(SlotKind::from_id("nope"), None);
        assert_eq!(
            SlotKind::from_panel_name("Spike.Chat"),
            Some(SlotKind::Chat)
        );
        assert_eq!(SlotKind::from_panel_name("other"), None);
        assert_eq!(SlotKind::Chat.placeholder(), "4 · CHAT");
    }
}
