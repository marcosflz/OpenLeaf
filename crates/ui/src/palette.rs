//! Paleta de comandos (M1-A5) — catálogo de comandos del shell.
//!
//! El componente de palette es `gpui_kit::component::command::Command`
//! (estado en [`gpui_kit::component::command::CommandState`]); este módulo
//! solo define **qué** comandos existen y cómo se etiquetan. El shell
//! ([`crate::app::AppRoot`]) construye el `Command` y ejecuta la acción
//! seleccionada por índice (los items mantienen un orden fijo, así que el
//! `IndexPath` del `on_confirm` mapea 1:1 con [`PaletteCommand::ALL`]).

use gpui_kit::assets::IconName;
use gpui_kit::component::command::CommandItem;

/// Comando ejecutable desde la paleta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteCommand {
    /// Compilar el proyecto (placeholder M1-D).
    Compile,
    /// Mostrar/ocultar el explorador.
    ToggleTree,
    /// Mostrar/ocultar el editor.
    ToggleEditor,
    /// Mostrar/ocultar el PDF.
    TogglePdf,
    /// Mostrar/ocultar el chat IA.
    ToggleChat,
    /// Maximizar/restaurar el panel activo.
    ToggleFocus,
    /// Abrir los ajustes.
    OpenSettings,
    /// Recargar el árbol del proyecto.
    ReloadTree,
    /// Abrir otra carpeta de proyecto.
    OpenProject,
    /// Probar el modal de permiso (demo M1-A4).
    PermissionDemo,
    /// Crear un proyecto nuevo desde plantilla (M1-F4).
    NewProject,
}

impl PaletteCommand {
    /// Orden fijo de la paleta (el índice del item = índice del array).
    pub const ALL: [PaletteCommand; 11] = [
        PaletteCommand::Compile,
        PaletteCommand::ToggleTree,
        PaletteCommand::ToggleEditor,
        PaletteCommand::TogglePdf,
        PaletteCommand::ToggleChat,
        PaletteCommand::ToggleFocus,
        PaletteCommand::OpenSettings,
        PaletteCommand::ReloadTree,
        PaletteCommand::OpenProject,
        PaletteCommand::PermissionDemo,
        PaletteCommand::NewProject,
    ];

    /// Índice estable dentro de [`PaletteCommand::ALL`].
    pub const fn index(self) -> usize {
        match self {
            PaletteCommand::Compile => 0,
            PaletteCommand::ToggleTree => 1,
            PaletteCommand::ToggleEditor => 2,
            PaletteCommand::TogglePdf => 3,
            PaletteCommand::ToggleChat => 4,
            PaletteCommand::ToggleFocus => 5,
            PaletteCommand::OpenSettings => 6,
            PaletteCommand::ReloadTree => 7,
            PaletteCommand::OpenProject => 8,
            PaletteCommand::PermissionDemo => 9,
            PaletteCommand::NewProject => 10,
        }
    }

    /// Comando correspondiente a la fila de un `IndexPath` (sección 0).
    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// Etiqueta i18n.
    pub fn label(self) -> String {
        match self {
            PaletteCommand::Compile => rust_i18n::t!("ui.app.palette.compile"),
            PaletteCommand::ToggleTree => rust_i18n::t!("ui.app.palette.toggle_tree"),
            PaletteCommand::ToggleEditor => rust_i18n::t!("ui.app.palette.toggle_editor"),
            PaletteCommand::TogglePdf => rust_i18n::t!("ui.app.palette.toggle_pdf"),
            PaletteCommand::ToggleChat => rust_i18n::t!("ui.app.palette.toggle_chat"),
            PaletteCommand::ToggleFocus => rust_i18n::t!("ui.app.palette.focus"),
            PaletteCommand::OpenSettings => rust_i18n::t!("ui.app.palette.settings"),
            PaletteCommand::ReloadTree => rust_i18n::t!("ui.app.palette.reload_tree"),
            PaletteCommand::OpenProject => rust_i18n::t!("ui.app.palette.open_project"),
            PaletteCommand::PermissionDemo => rust_i18n::t!("ui.app.palette.permission_demo"),
            PaletteCommand::NewProject => rust_i18n::t!("ui.app.palette.new_project"),
        }
        .to_string()
    }

    /// Icono Lucide.
    pub fn icon(self) -> IconName {
        match self {
            PaletteCommand::Compile => IconName::Play,
            PaletteCommand::ToggleTree => IconName::FolderClosed,
            PaletteCommand::ToggleEditor => IconName::FileCode,
            PaletteCommand::TogglePdf => IconName::FileType,
            PaletteCommand::ToggleChat => IconName::Bot,
            PaletteCommand::ToggleFocus => IconName::Maximize,
            PaletteCommand::OpenSettings => IconName::Settings,
            PaletteCommand::ReloadTree => IconName::RefreshCw,
            PaletteCommand::OpenProject => IconName::FolderOpen,
            PaletteCommand::PermissionDemo => IconName::ShieldCheck,
            PaletteCommand::NewProject => IconName::FolderPlus,
        }
    }

    /// Términos extra de búsqueda (además de la etiqueta).
    pub const fn keywords(self) -> &'static [&'static str] {
        match self {
            PaletteCommand::Compile => &["build", "latexmk", "compilar"],
            PaletteCommand::ToggleTree => &["explorer", "arbol", "panel"],
            PaletteCommand::ToggleEditor => &["editor", "panel", "codigo"],
            PaletteCommand::TogglePdf => &["pdf", "visor", "panel"],
            PaletteCommand::ToggleChat => &["chat", "ia", "panel"],
            PaletteCommand::ToggleFocus => &["focus", "maximizar", "panel"],
            PaletteCommand::OpenSettings => &["settings", "config", "ajustes"],
            PaletteCommand::ReloadTree => &["refresh", "recargar", "arbol"],
            PaletteCommand::OpenProject => &["workspace", "carpeta", "proyecto"],
            PaletteCommand::PermissionDemo => &["permiso", "modal", "demo"],
            PaletteCommand::NewProject => &["nuevo", "proyecto", "plantilla", "crear"],
        }
    }

    /// Item del componente `command` con orden estable.
    pub fn item(self) -> CommandItem {
        CommandItem::new()
            .label(self.label())
            .icon(self.icon())
            .keywords(self.keywords().iter().copied())
    }

    /// Catálogo completo como items.
    pub fn items() -> Vec<CommandItem> {
        Self::ALL.into_iter().map(Self::item).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_mapping_is_bijective() {
        for (position, command) in PaletteCommand::ALL.into_iter().enumerate() {
            assert_eq!(command.index(), position);
            assert_eq!(PaletteCommand::from_index(position), Some(command));
        }
        assert_eq!(PaletteCommand::from_index(PaletteCommand::ALL.len()), None);
    }

    #[test]
    fn every_command_has_a_label_and_keywords() {
        for command in PaletteCommand::ALL {
            assert!(!command.label().is_empty(), "etiqueta vacía en {command:?}");
            assert!(!command.keywords().is_empty());
        }
    }

    #[test]
    fn items_keep_the_catalogue_order() {
        let items = PaletteCommand::items();
        assert_eq!(items.len(), PaletteCommand::ALL.len());
    }
}
