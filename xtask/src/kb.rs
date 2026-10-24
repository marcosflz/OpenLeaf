//! `xtask kb check` — validador mínimo del OKF (`docs/kb`).
//!
//! No pretende ser un parser de Markdown/YAML: comprueba lo que el contrato
//! (`docs/kb/schema.md`) exige para que una nota sea recuperable y no se
//! convierta en deuda: frontmatter obligatorio, `id` coherente con la ruta,
//! estado permitido, fecha, enlaces relativos existentes y changelog vivo.
//!
//! Uso:
//! ```powershell
//! cargo run -p xtask -- kb check
//! ```

use std::fs;
use std::path::{Path, PathBuf};

const REQUIRED: [&str; 8] = [
    "id:",
    "title:",
    "status:",
    "updated:",
    "source:",
    "tags:",
    "related:",
    "verified:",
];
const STATUSES: [&str; 4] = ["stable", "draft", "deprecated", "planned"];

/// Ejecuta `kb check` (o `kb` sin subcomando).
pub fn run(args: &[String]) {
    if args.first().map(String::as_str) != Some("check") {
        println!("uso: cargo run -p xtask -- kb check");
        return;
    }
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let kb = root.join("docs").join("kb");
    if !kb.is_dir() {
        eprintln!("no existe {}", kb.display());
        std::process::exit(1);
    }

    let mut errors = Vec::new();
    let mut notes = 0usize;
    visit(&kb, &kb, &mut notes, &mut errors);

    let changelog = kb.join("changes").join("CHANGELOG.md");
    match fs::read_to_string(&changelog) {
        Ok(text) if text.contains("## CHG-") => {}
        Ok(_) => errors.push(format!(
            "{}: no contiene ninguna entrada `## CHG-`",
            changelog.display()
        )),
        Err(error) => errors.push(format!("{}: {error}", changelog.display())),
    }

    if errors.is_empty() {
        println!("OK: {notes} notas del OKF válidas (frontmatter, ids y enlaces).");
    } else {
        for error in &errors {
            eprintln!("- {error}");
        }
        eprintln!("\n{} problema(s) en el OKF.", errors.len());
        std::process::exit(1);
    }
}

fn visit(base: &Path, dir: &Path, notes: &mut usize, errors: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        errors.push(format!("{}: no se puede leer", dir.display()));
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            visit(base, &path, notes, errors);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if matches!(name, "README.md" | "index.md" | "schema.md")
            || path
                .components()
                .any(|part| part.as_os_str() == "_templates")
        {
            continue;
        }
        *notes += 1;
        check_note(base, &path, errors);
    }
}

fn check_note(base: &Path, path: &Path, errors: &mut Vec<String>) {
    let Ok(text) = fs::read_to_string(path) else {
        errors.push(format!("{}: no se puede leer", path.display()));
        return;
    };
    let Some(front) = frontmatter(&text) else {
        errors.push(format!(
            "{}: falta el bloque frontmatter `---`",
            path.display()
        ));
        return;
    };
    for key in REQUIRED {
        if !front.lines().any(|line| line.trim_start().starts_with(key)) {
            errors.push(format!("{}: falta `{key}`", path.display()));
        }
    }

    let expected = path
        .strip_prefix(base)
        .ok()
        .map(|rel| {
            let rel = rel.with_extension("");
            let mut parts: Vec<String> = rel
                .components()
                .filter_map(|part| part.as_os_str().to_str())
                .map(|part| part.to_ascii_lowercase())
                .collect();
            // El prefijo numérico es de orden de lectura, no parte del id:
            // `01-crates.md` → `crates`, `PLAN-0.2.md` → `plan-0.2`.
            if let Some(last) = parts.last_mut()
                && let Some((prefix, rest)) = last.split_once('-')
                && !prefix.is_empty()
                && prefix.chars().all(|ch| ch.is_ascii_digit())
            {
                *last = rest.to_owned();
            }
            format!("kb/{}", parts.join("/"))
        })
        .unwrap_or_default();
    let id = value(front, "id:");
    if let Some(id) = id.as_deref()
        && id != expected
    {
        errors.push(format!(
            "{}: id `{id}` no coincide con la ruta (`{expected}`)",
            path.display()
        ));
    }

    if let Some(status) = value(front, "status:")
        && !STATUSES.contains(&status.as_str())
    {
        errors.push(format!(
            "{}: estado `{status}` no permitido (usa stable/draft/deprecated/planned)",
            path.display()
        ));
    }

    if let Some(updated) = value(front, "updated:")
        && !looks_like_date(&updated)
    {
        errors.push(format!(
            "{}: `updated: {updated}` no tiene forma YYYY-MM-DD",
            path.display()
        ));
    }

    for link in relative_links(&text) {
        let target = path
            .parent()
            .unwrap_or(base)
            .join(link.trim_start_matches("./"));
        if !target.exists() {
            errors.push(format!("{}: enlace roto `{link}`", path.display()));
        }
    }
}

fn frontmatter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

fn value(front: &str, key: &str) -> Option<String> {
    front.lines().find_map(|line| {
        line.trim()
            .strip_prefix(key)
            .map(|value| value.trim().trim_matches('"').to_owned())
    })
}

fn looks_like_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(ix, byte)| matches!(ix, 4 | 7) || byte.is_ascii_digit())
}

/// Enlaces Markdown relativos, sin URLs ni anclas.
fn relative_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = rest[..end].split('#').next().unwrap_or("").trim();
        if !target.is_empty()
            && !target.starts_with("http://")
            && !target.starts_with("https://")
            && !target.starts_with("mailto:")
            && !target.starts_with('#')
        {
            links.push(target.to_owned());
        }
        rest = &rest[end + 1..];
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_have_the_expected_shape() {
        assert!(looks_like_date("2026-10-24"));
        assert!(!looks_like_date("24/10/2026"));
        assert!(!looks_like_date("2026-1-4"));
    }

    #[test]
    fn links_skip_urls_and_anchors() {
        let links = relative_links("[a](../x.md) [b](https://x) [c](#y) [d](z.md#k)");
        assert_eq!(links, ["../x.md", "z.md"]);
    }

    #[test]
    fn frontmatter_is_found_only_between_markers() {
        assert_eq!(frontmatter("---\nid: x\n---\nbody"), Some("\nid: x"));
        assert!(frontmatter("# no frontmatter").is_none());
    }
}
