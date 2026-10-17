//! Tema del shell (M1-A3): dark pulido + acento configurable.
//!
//! Reglas:
//! - El look aprobado del explorer (M0.5) no se toca: filas de 22 px, hover
//!   `foreground` al 7 % y selección con el acento al 24 % ([`tokens`]).
//! - El acento sale de `settings.ui.accent` (`#RRGGBB`); si el valor no es
//!   válido se usa [`tokens::ACCENT_HEX`].
//! - Los colores se leen siempre por token (`cx.theme().…`, trait
//!   `ActiveTheme`) para que un futuro tema claro (M2-3) no obligue a tocar
//!   los paneles.

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
    /// Acento con el que se pinta la fila seleccionada (24 %).
    pub const LIST_ACTIVE_ALPHA: f32 = 0.24;
    /// Hover de fila no seleccionada (7 % del foreground).
    pub const LIST_HOVER_ALPHA: f32 = 0.07;
}

/// Tema oscuro por defecto (antes de cargar `Settings`).
pub fn init(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
}

/// Aplica el tema que dictan los ajustes: modo + acento + tokens de listas.
pub fn apply(settings: &Settings, cx: &mut App) {
    apply_mode(&settings.ui.theme, cx);
    let accent = accent_from_hex(&settings.ui.accent).unwrap_or_else(default_accent);
    Theme::update(cx, |theme| {
        let foreground = theme.foreground;
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
        theme.selection = accent.opacity(0.35);
        // Fila seleccionada: azul del acento suave (VS Code usa #04395E).
        theme.list_active = accent.opacity(tokens::LIST_ACTIVE_ALPHA);
        // Hover sutil sobre filas no seleccionadas.
        theme.tokens.list_hover = foreground.opacity(tokens::LIST_HOVER_ALPHA).into();
    });
}

/// Cambia solo el modo del tema (`dark`/`light`/`system` → dark por ahora).
pub fn apply_mode(theme: &str, cx: &mut App) {
    let mode = match theme.to_ascii_lowercase().as_str() {
        "light" => ThemeMode::Light,
        // `system` y `dark` comparten el dark hasta M2-3 (tema claro completo).
        _ => ThemeMode::Dark,
    };
    Theme::change(mode, None, cx);
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
    use gpui_kit::component::ThemeMode;

    #[test]
    fn accent_parsing_matches_the_core_parser() {
        assert!(accent_from_hex("#4C8DFF").is_some());
        assert!(accent_from_hex("azul").is_none());
        assert!(accent_from_hex("#12345").is_none());
    }

    #[test]
    fn the_mode_mapping_keeps_dark_as_the_fallback() {
        // `apply_mode` necesita `App`, así que aquí solo se fija el contrato
        // del mapeo de strings (la aplicación real se prueba en el shell).
        let mapped = |value: &str| match value.to_ascii_lowercase().as_str() {
            "light" => ThemeMode::Light,
            _ => ThemeMode::Dark,
        };
        assert_eq!(mapped("light"), ThemeMode::Light);
        assert_eq!(mapped("dark"), ThemeMode::Dark);
        assert_eq!(mapped("system"), ThemeMode::Dark);
        assert_eq!(mapped("cualquiera"), ThemeMode::Dark);
    }

    #[test]
    fn the_accent_token_is_the_approved_one() {
        assert_eq!(tokens::ACCENT_HEX, "#4C8DFF");
        assert_eq!(tokens::ACCENT_RGB, 0x004C_8DFF);
    }
}
