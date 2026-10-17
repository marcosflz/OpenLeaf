//! Utilidades de rutas internas del crate (normalización léxica).

use camino::{Utf8Path, Utf8PathBuf};

/// Normaliza léxicamente una ruta (elimina `.` y colapsa `..`), sin tocar el disco
/// y sin resolver symlinks. Compatible con rutas Windows (`C:\a\b`, `C:/a/b`, UNC).
///
/// Necesario porque SyncTeX devuelve rutas registradas como `C:/x/./main.tex` y los
/// logs de TeX usan `./main.tex`, mientras que el editor compara rutas normalizadas.
pub(crate) fn normalize(path: &Utf8Path) -> Utf8PathBuf {
    let (anchor, rest) = split_anchor(path.as_str());
    let mut stack: Vec<&str> = Vec::new();
    for part in rest.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                if stack.pop().is_none() && anchor.is_empty() {
                    stack.push("..");
                }
            }
            other => stack.push(other),
        }
    }
    let mut out = String::from(anchor);
    for part in stack {
        if !out.is_empty() && !out.ends_with(['/', '\\']) {
            out.push('/');
        }
        out.push_str(part);
    }
    if out.is_empty() {
        Utf8PathBuf::from(".")
    } else {
        Utf8PathBuf::from(out)
    }
}

/// Separa el ancla de la ruta (`C:\`, `C:`, `/`, `\\\\server\share` o "").
fn split_anchor(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    // Prefijo de disco: `C:` (la salida usa `/` como separador canónico).
    if bytes.len() >= 2 && bytes[1] == b':' {
        let has_sep = bytes.len() >= 3 && (bytes[2] == b'/' || bytes[2] == b'\\');
        let rest = if has_sep { 3 } else { 2 };
        return (&s[..2], &s[rest..]);
    }
    // UNC: `\\server\share\...` (deja el ancla como `\\server\share`)
    if s.starts_with("\\\\") || s.starts_with("//") {
        let mut seen = 0;
        let mut idx = 0;
        for (i, ch) in s.char_indices() {
            if ch == '/' || ch == '\\' {
                seen += 1;
                if seen == 4 {
                    idx = i + 1;
                    break;
                }
            }
        }
        if idx > 0 {
            return s.split_at(idx);
        }
    }
    // Raíz absoluta `/...`
    if s.starts_with('/') || s.starts_with('\\') {
        return s.split_at(1);
    }
    ("", s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_puntos_y_doble_punto() {
        assert_eq!(
            normalize(Utf8Path::new("C:/a/b/./main.tex")).as_str(),
            "C:/a/b/main.tex"
        );
        assert_eq!(
            normalize(Utf8Path::new("./src/../main.tex")).as_str(),
            "main.tex"
        );
        assert_eq!(
            normalize(Utf8Path::new("C:\\a\\..\\b\\main.tex")).as_str(),
            "C:/b/main.tex"
        );
        assert_eq!(
            normalize(Utf8Path::new("../fuera/x")).as_str(),
            "../fuera/x"
        );
    }
}
