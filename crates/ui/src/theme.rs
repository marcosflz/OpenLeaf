//! Tema del shell (M1-A3 / P2-UI-2): temas dark/light/system y acento.
//!
//! Reglas:
//! - El look aprobado del explorer (M0.5) no se toca: filas de 22 px, hover
//!   `foreground` al 7 % y selección con el acento al 24 % ([`tokens`]).
//! - El acento sale de `settings.ui.accent` (`#RRGGBB`); si el valor no es
//!   válido se usa [`tokens::ACCENT_HEX`].
//! - `system` consulta la apariencia real del SO con
//!   [`Theme::sync_system_appearance`]; no cae a dark.
//! - Los colores se leen siempre por token (`cx.theme().…`, trait
//!   `ActiveTheme`) para que el tema claro no obligue a tocar los paneles.
//! - `ui.reducedMotion` se proyecta en `App::set_reduce_motion`, que es lo que
//!   consultan las animaciones propias del kit (diálogos, spinners).

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, rgb};
use lsx_core::config::Settings;

/// Tokens y métricas visuales aprobadas en el pase de diseño M0.5.
pub mod tokens {
    /// Acento del producto (`#4C8DFF`).
    pub const ACCENT_HEX: &str = "#4C8DFF";
    /// Acento como entero `0x00RRGGBB` (para `rgb()`).
    pub const ACCENT_RGB: u32 = 0x004C_8DFF;

    /// Alto de fila del árbol (VS Code Explorer ≈ 22 px lógicos).
    pub const TREE_ROW_HEIGHT: f32 = 22.;
    /// Tamaño de fuente de las filas del árbol (≈13 px).
    pub const TREE_FONT_SIZE: f32 = 13.;
    /// Sangría extra por nivel de profundidad (≈10 px).
    pub const TREE_INDENT_STEP: f32 = 10.;
    /// Sangría base del primer nivel.
    pub const TREE_INDENT_BASE: f32 = 4.;
    /// Lado del icono de archivo/carpeta y del hueco de chevron.
    pub const TREE_ICON_SIZE: f32 = 16.;
    /// Alto del encabezado de sección y barras compactas.
    pub const BAR_HEIGHT: f32 = 28.;
    /// Alto de la barra de estado/toolbar.
    pub const CHROME_HEIGHT: f32 = 22.;
    /// Alto de la toolbar del shell.
    pub const TOOLBAR_HEIGHT: f32 = 36.;
    /// Alto del panel de salida del explorador.
    pub const OUTPUT_HEIGHT: f32 = 170.;
    /// Acento con el que se pinta la fila seleccionada (24 % en dark).
    pub const LIST_ACTIVE_ALPHA: f32 = 0.24;
    /// Hover de fila no seleccionada (7 % del foreground).
    pub const LIST_HOVER_ALPHA: f32 = 0.07;
}

/// Preset de acento ofrecido en Ajustes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccentPreset {
    /// Identificador estable para el `ElementId` y la i18n.
    pub id: &'static str,
    /// Color `#RRGGBB`; es el único sitio con hex fuera de [`tokens`].
    pub hex: &'static str,
}

impl AccentPreset {
    /// Color listo para pintar un swatch (los hex viven en esta capa).
    pub fn color(self) -> Hsla {
        accent_from_hex(self.hex).unwrap_or_else(default_accent)
    }
}

/// Presets de acento: el azul es el token aprobado y el resto usa tonos
/// medio-oscuros para no perder contraste sobre fondos claros.
pub const ACCENT_PRESETS: [AccentPreset; 5] = [
    AccentPreset {
        id: "blue",
        hex: "#4C8DFF",
    },
    AccentPreset {
        id: "violet",
        hex: "#7C3AED",
    },
    AccentPreset {
        id: "green",
        hex: "#1F7A33",
    },
    AccentPreset {
        id: "amber",
        hex: "#B45309",
    },
    AccentPreset {
        id: "red",
        hex: "#C81E1E",
    },
];

/// Tema oscuro por defecto (antes de cargar `Settings`).
pub fn init(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
}

/// Aplica el tema que dictan los ajustes: modo + acento + tokens de listas.
pub fn apply(settings: &Settings, cx: &mut App) {
    // `reducedMotion` no tiene animaciones propias en `lsx-ui`, pero el kit sí
    // anima diálogos y spinners: proyectarlo aquí evita exponer un campo muerto
    // y respeta la preferencia del usuario aunque solo edite settings.json.
    cx.set_reduce_motion(settings.ui.reduced_motion);
    apply_mode(&settings.ui.theme, cx);
    apply_accent(&settings.ui.accent, cx);
}

/// Cambia solo el modo del tema (`dark`/`light`/`system`).
///
/// En `system` se consulta la apariencia real del SO; si el tema es
/// desconocido se mantiene el fallback histórico a dark.
pub fn apply_mode(theme: &str, cx: &mut App) {
    match parse_mode(theme) {
        Some(mode) => Theme::change(mode, None, cx),
        None => Theme::sync_system_appearance(None, cx),
    }
}

/// Mapea `ui.theme` a modo. `None` significa «seguir al sistema».
///
/// Es una función pura para poder testear el contrato sin `App`.
fn parse_mode(theme: &str) -> Option<ThemeMode> {
    match theme.to_ascii_lowercase().as_str() {
        "light" => Some(ThemeMode::Light),
        "system" => None,
        _ => Some(ThemeMode::Dark),
    }
}

/// Aplica un acento `#RRGGBB` a los tokens de interacción.
///
/// Los alphas se ajustan por modo: el mismo 35 % sobre blanco resulta más
/// pesado que sobre negro, así que el tema claro usa tintes más suaves.
pub fn apply_accent(accent_hex: &str, cx: &mut App) {
    let accent = accent_from_hex(accent_hex).unwrap_or_else(default_accent);
    Theme::update(cx, |theme| {
        let foreground = theme.foreground;
        let dark = theme.is_dark();
        let selection_alpha = if dark { 0.35 } else { 0.22 };
        let active_alpha = if dark {
            tokens::LIST_ACTIVE_ALPHA
        } else {
            0.16
        };
        let hover_alpha = if dark { tokens::LIST_HOVER_ALPHA } else { 0.06 };

        theme.accent = accent;
        // Familia primaria: botones primarios, foco y selección con el acento
        // del producto (pulido M1-A3; el resto de tokens queda como el kit).
        theme.primary = accent;
        theme.button_primary = accent;
        theme.primary_hover = accent.opacity(0.92);
        theme.button_primary_hover = accent.opacity(0.92);
        theme.primary_active = accent.opacity(0.85);
        theme.button_primary_active = accent.opacity(0.85);
        theme.ring = accent;
        theme.selection = accent.opacity(selection_alpha);
        // Fila seleccionada: tinte del acento (VS Code usa #04395E en dark).
        theme.list_active = accent.opacity(active_alpha);
        // Navegación lateral (Ajustes): el acento también marca la sección.
        theme.sidebar_accent = accent.opacity(if dark { 0.20 } else { 0.14 });
        theme.sidebar_accent_foreground = foreground;
        // Hover sutil sobre filas no seleccionadas.
        theme.tokens.list_hover = foreground.opacity(hover_alpha).into();
    });
}

/// Acento por defecto del producto.
pub fn default_accent() -> Hsla {
    Hsla::from(rgb(tokens::ACCENT_RGB))
}

/// Convierte `#RRGGBB` en color; `None` si no es válido.
pub fn accent_from_hex(value: &str) -> Option<Hsla> {
    let rgb_value = lsx_core::config::parse_accent(value)?;
    Some(Hsla::from(rgb(rgb_value)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::{ActiveTheme as _, ThemeMode};
    use lsx_core::config::UiSettings;

    #[test]
    fn accent_parsing_matches_the_core_parser() {
        assert!(accent_from_hex("#4C8DFF").is_some());
        assert!(accent_from_hex("azul").is_none());
        assert!(accent_from_hex("#12345").is_none());
    }

    #[test]
    fn the_mode_mapping_handles_system_and_keeps_dark_as_the_fallback() {
        assert_eq!(parse_mode("light"), Some(ThemeMode::Light));
        assert_eq!(parse_mode("LIGHT"), Some(ThemeMode::Light));
        assert_eq!(parse_mode("dark"), Some(ThemeMode::Dark));
        assert_eq!(parse_mode("system"), None);
        assert_eq!(parse_mode("SYSTEM"), None);
        assert_eq!(parse_mode("cualquiera"), Some(ThemeMode::Dark));
    }

    #[gpui_kit::test]
    fn system_theme_reads_the_platform_appearance(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let settings = Settings {
                ui: UiSettings {
                    theme: "system".to_owned(),
                    ..UiSettings::default()
                },
                ..Settings::default()
            };
            apply(&settings, cx);
            let expected = ThemeMode::from(cx.window_appearance());
            assert_eq!(cx.theme().mode, expected);
        });
    }

    #[test]
    fn every_accent_preset_is_a_valid_hex_and_ids_are_unique() {
        let mut ids = std::collections::BTreeSet::new();
        for preset in ACCENT_PRESETS {
            assert!(
                lsx_core::config::parse_accent(preset.hex).is_some(),
                "preset {} no es #RRGGBB",
                preset.id
            );
            assert!(ids.insert(preset.id), "id repetido: {}", preset.id);
        }
        assert_eq!(ACCENT_PRESETS[0].hex, tokens::ACCENT_HEX);
    }

    #[test]
    fn the_accent_token_is_the_approved_one() {
        assert_eq!(tokens::ACCENT_HEX, "#4C8DFF");
        assert_eq!(tokens::ACCENT_RGB, 0x004C_8DFF);
    }
}
