//! Paleta de comandos: catálogo único de comandos del shell.
//!
//! El componente es `gpui_kit::component::command::Command`. Este módulo define
//! **qué** comandos existen, cómo se etiquetan y su atajo; el shell
//! ([`crate::app::AppRoot`]) ejecuta el seleccionado por índice. Los items
//! mantienen un orden fijo, así que el `IndexPath` del `on_confirm` mapea 1:1
//! con [`PaletteCommand::ALL`].
//!
//! El atajo se muestra a la derecha de la etiqueta y se toma de
//! [`PaletteCommand::shortcut`], la misma tabla que pinta el diálogo de atajos
//! (F1). Así la paleta, el menú y la ayuda no divergen.

use gpui_kit::assets::IconName;
use gpui_kit::component::command::CommandItem;

/// Comando ejecutable desde la paleta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteCommand {
    Compile,
    SyncForward,
    FixWithAi,
    NewChat,
    QuickOpen,
    Save,
    SaveAll,
    CloseTab,
    ToggleTree,
    ToggleEditor,
    TogglePdf,
    ToggleChat,
    ToggleFocus,
    ResetLayout,
    OpenSettings,
    ConfigureAi,
    ReloadTree,
    OpenProject,
    NewProject,
    Shortcuts,
    About,
    Quit,
}

impl PaletteCommand {
    /// Orden fijo de la paleta (el índice del item = índice del array).
    pub const ALL: [PaletteCommand; 22] = [
        PaletteCommand::Compile,
        PaletteCommand::SyncForward,
        PaletteCommand::FixWithAi,
        PaletteCommand::NewChat,
        PaletteCommand::QuickOpen,
        PaletteCommand::Save,
        PaletteCommand::SaveAll,
        PaletteCommand::CloseTab,
        PaletteCommand::ToggleTree,
        PaletteCommand::ToggleEditor,
        PaletteCommand::TogglePdf,
        PaletteCommand::ToggleChat,
        PaletteCommand::ToggleFocus,
        PaletteCommand::ResetLayout,
        PaletteCommand::OpenSettings,
        PaletteCommand::ConfigureAi,
        PaletteCommand::ReloadTree,
        PaletteCommand::OpenProject,
        PaletteCommand::NewProject,
        PaletteCommand::Shortcuts,
        PaletteCommand::About,
        PaletteCommand::Quit,
    ];

    /// Índice estable dentro de [`PaletteCommand::ALL`].
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|command| *command == self)
            .expect("todo comando está en ALL")
    }

    /// Comando correspondiente a la fila de un `IndexPath` (sección 0).
    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// Etiqueta i18n (sin el atajo).
    pub fn label(self) -> String {
        match self {
            PaletteCommand::Compile => rust_i18n::t!("ui.app.palette.compile"),
            PaletteCommand::SyncForward => rust_i18n::t!("ui.app.palette.sync_forward"),
            PaletteCommand::FixWithAi => rust_i18n::t!("ui.app.palette.fix_with_ai"),
            PaletteCommand::NewChat => rust_i18n::t!("ui.app.palette.new_chat"),
            PaletteCommand::QuickOpen => rust_i18n::t!("ui.app.palette.quick_open"),
            PaletteCommand::Save => rust_i18n::t!("ui.app.palette.save"),
            PaletteCommand::SaveAll => rust_i18n::t!("ui.app.palette.save_all"),
            PaletteCommand::CloseTab => rust_i18n::t!("ui.app.palette.close_tab"),
            PaletteCommand::ToggleTree => rust_i18n::t!("ui.app.palette.toggle_tree"),
            PaletteCommand::ToggleEditor => rust_i18n::t!("ui.app.palette.toggle_editor"),
            PaletteCommand::TogglePdf => rust_i18n::t!("ui.app.palette.toggle_pdf"),
            PaletteCommand::ToggleChat => rust_i18n::t!("ui.app.palette.toggle_chat"),
            PaletteCommand::ToggleFocus => rust_i18n::t!("ui.app.palette.focus"),
            PaletteCommand::ResetLayout => rust_i18n::t!("ui.app.palette.reset_layout"),
            PaletteCommand::OpenSettings => rust_i18n::t!("ui.app.palette.settings"),
            PaletteCommand::ConfigureAi => rust_i18n::t!("ui.app.palette.configure_ai"),
            PaletteCommand::ReloadTree => rust_i18n::t!("ui.app.palette.reload_tree"),
            PaletteCommand::OpenProject => rust_i18n::t!("ui.app.palette.open_project"),
            PaletteCommand::NewProject => rust_i18n::t!("ui.app.palette.new_project"),
            PaletteCommand::Shortcuts => rust_i18n::t!("ui.app.palette.shortcuts"),
            PaletteCommand::About => rust_i18n::t!("ui.app.palette.about"),
            PaletteCommand::Quit => rust_i18n::t!("ui.app.palette.quit"),
        }
        .to_string()
    }

    /// Atajo de teclado tal como se muestra (coincide con `register_keybindings`).
    pub const fn shortcut(self) -> Option<&'static str> {
        match self {
            PaletteCommand::Compile => Some("Ctrl+B"),
            PaletteCommand::SyncForward => Some("Ctrl+Alt+J"),
            PaletteCommand::FixWithAi => Some("Ctrl+Alt+F"),
            PaletteCommand::NewChat => Some("Ctrl+Alt+N"),
            PaletteCommand::QuickOpen => Some("Ctrl+P"),
            PaletteCommand::Save => Some("Ctrl+S"),
            PaletteCommand::SaveAll => Some("Ctrl+Alt+S"),
            PaletteCommand::CloseTab => Some("Ctrl+W"),
            PaletteCommand::ToggleTree => Some("Ctrl+Alt+1"),
            PaletteCommand::ToggleEditor => Some("Ctrl+Alt+2"),
            PaletteCommand::TogglePdf => Some("Ctrl+Alt+3"),
            PaletteCommand::ToggleChat => Some("Ctrl+Alt+4"),
            PaletteCommand::ToggleFocus => Some("Ctrl+Alt+M"),
            PaletteCommand::OpenSettings => Some("Ctrl+,"),
            PaletteCommand::Shortcuts => Some("F1"),
            PaletteCommand::Quit => Some("Ctrl+Q"),
            PaletteCommand::ResetLayout
            | PaletteCommand::ConfigureAi
            | PaletteCommand::ReloadTree
            | PaletteCommand::OpenProject
            | PaletteCommand::NewProject
            | PaletteCommand::About => None,
        }
    }

    /// Icono Lucide.
    pub fn icon(self) -> IconName {
        match self {
            PaletteCommand::Compile => IconName::Play,
            PaletteCommand::SyncForward => IconName::ArrowLeftRight,
            PaletteCommand::FixWithAi => IconName::Sparkles,
            PaletteCommand::NewChat => IconName::SquarePen,
            PaletteCommand::QuickOpen => IconName::Search,
            PaletteCommand::Save => IconName::Save,
            PaletteCommand::SaveAll => IconName::Save,
            PaletteCommand::CloseTab => IconName::X,
            PaletteCommand::ToggleTree => IconName::FolderClosed,
            PaletteCommand::ToggleEditor => IconName::FileCode,
            PaletteCommand::TogglePdf => IconName::FileType,
            PaletteCommand::ToggleChat => IconName::Bot,
            PaletteCommand::ToggleFocus => IconName::Maximize,
            PaletteCommand::ResetLayout => IconName::LayoutDashboard,
            PaletteCommand::OpenSettings => IconName::Settings,
            PaletteCommand::ConfigureAi => IconName::KeyRound,
            PaletteCommand::ReloadTree => IconName::RefreshCw,
            PaletteCommand::OpenProject => IconName::FolderOpen,
            PaletteCommand::NewProject => IconName::FolderPlus,
            PaletteCommand::Shortcuts => IconName::Keyboard,
            PaletteCommand::About => IconName::Info,
            PaletteCommand::Quit => IconName::LogOut,
        }
    }

    /// Términos extra de búsqueda (además de la etiqueta).
    pub const fn keywords(self) -> &'static [&'static str] {
        match self {
            PaletteCommand::Compile => &["build", "latexmk", "compilar"],
            PaletteCommand::SyncForward => &["synctex", "pdf", "ir", "posicion"],
            PaletteCommand::FixWithAi => &["ia", "errores", "arreglar", "fix", "reparar"],
            PaletteCommand::NewChat => &["ia", "chat", "conversacion", "sesion"],
            PaletteCommand::QuickOpen => &["abrir", "archivo", "buscar", "ir", "file"],
            PaletteCommand::Save => &["guardar", "save"],
            PaletteCommand::SaveAll => &["guardar", "todo", "save", "all"],
            PaletteCommand::CloseTab => &["cerrar", "pestaña", "tab"],
            PaletteCommand::ToggleTree => &["explorer", "arbol", "panel"],
            PaletteCommand::ToggleEditor => &["editor", "panel", "codigo"],
            PaletteCommand::TogglePdf => &["pdf", "visor", "panel"],
            PaletteCommand::ToggleChat => &["chat", "ia", "panel"],
            PaletteCommand::ToggleFocus => &["focus", "maximizar", "panel"],
            PaletteCommand::ResetLayout => &["layout", "paneles", "restablecer"],
            PaletteCommand::OpenSettings => &["settings", "config", "ajustes"],
            PaletteCommand::ConfigureAi => &["ia", "proveedor", "clave", "modelo", "api"],
            PaletteCommand::ReloadTree => &["refresh", "recargar", "arbol"],
            PaletteCommand::OpenProject => &["workspace", "carpeta", "proyecto"],
            PaletteCommand::NewProject => &["nuevo", "proyecto", "plantilla", "crear"],
            PaletteCommand::Shortcuts => &["atajos", "teclado", "ayuda"],
            PaletteCommand::About => &["version", "acerca"],
            PaletteCommand::Quit => &["salir", "cerrar", "exit"],
        }
    }

    /// Item del componente `command` con orden estable.
    pub fn item(self) -> CommandItem {
        let label = match self.shortcut() {
            Some(shortcut) => format!("{}    {shortcut}", self.label()),
            None => self.label(),
        };
        CommandItem::new()
            .label(label)
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
            assert!(
                !command.label().starts_with("ui."),
                "falta i18n en {command:?}"
            );
            assert!(!command.keywords().is_empty());
        }
    }

    #[test]
    fn shortcuts_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for command in PaletteCommand::ALL {
            if let Some(shortcut) = command.shortcut() {
                assert!(seen.insert(shortcut), "atajo repetido: {shortcut}");
            }
        }
    }

    #[test]
    fn items_keep_the_catalogue_order() {
        let items = PaletteCommand::items();
        assert_eq!(items.len(), PaletteCommand::ALL.len());
    }
}
