//! Apertura rápida de archivos (Ctrl+P), como el «Go to File» de VS Code.
//!
//! Recorre el proyecto y devuelve los archivos que el shell sabe abrir (texto en
//! el editor, PDF en el visor). Omite carpetas ocultas (`.git`, `.lsx`…), los
//! artefactos auxiliares de LaTeX y cualquier cosa más allá de
//! [`MAX_FILES`] entradas, para que un proyecto enorme no bloquee la UI.

use std::path::Path;

use gpui_kit::component::command::CommandItem;

use crate::panels::tree::{file_icon, open_kind};

/// Tope de archivos listados.
pub const MAX_FILES: usize = 5_000;

/// Profundidad máxima del recorrido.
const MAX_DEPTH: usize = 12;

/// Carpetas que nunca se recorren (además de las ocultas).
const SKIP_DIRS: &[&str] = &["node_modules", "target", "__pycache__"];

/// Archivos abribles del proyecto, relativos con `/` y ordenados (primero los
/// de la raíz, luego por ruta).
pub fn project_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, "", 0, &mut out);
    out.sort_by(|a, b| {
        let depth = |path: &str| path.matches('/').count();
        depth(a)
            .cmp(&depth(b))
            .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
    });
    out
}

fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(&entry.path(), &rel, depth + 1, out);
            }
        } else if !lsx_core::fs::is_aux_artifact(&name) && open_kind(&name).is_some() {
            out.push(rel);
        }
    }
}

/// Items del componente `command`: nombre del archivo y su carpeta.
pub fn items(files: &[String]) -> Vec<CommandItem> {
    files
        .iter()
        .map(|rel| {
            let (dir, name) = match rel.rsplit_once('/') {
                Some((dir, name)) => (dir, name),
                None => ("", rel.as_str()),
            };
            let label = if dir.is_empty() {
                name.to_owned()
            } else {
                format!("{name}    {dir}")
            };
            CommandItem::new()
                .label(label)
                .icon(file_icon(name))
                .keywords([rel.clone()])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_openable_files_and_skips_hidden_and_aux() {
        let root = std::env::temp_dir().join(format!("lsx-quick-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (rel, body) in [
            ("main.tex", "x"),
            ("main.aux", "x"),
            ("main.pdf", "x"),
            ("refs.bib", "x"),
            ("cap/intro.tex", "x"),
            ("figuras/plot.png", "x"),
            (".git/config", "x"),
            ("node_modules/a.tex", "x"),
        ] {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let files = project_files(&root);
        assert_eq!(files, ["main.pdf", "main.tex", "refs.bib", "cap/intro.tex"]);
        assert_eq!(items(&files).len(), 4);
        let _ = std::fs::remove_dir_all(&root);
    }
}
