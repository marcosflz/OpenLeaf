//! Diff de texto simple para el conflicto externo (M1-B2).
//!
//! "Ver diff" del modal de 3 opciones muestra un diff **por líneas** entre el
//! buffer y el contenido en disco. `similar` ya está pinneado en el workspace;
//! M2-12 podrá usar el componente `diff` del kit para los tool cards.

use similar::{ChangeTag, TextDiff};

/// Tope de líneas mostradas (documentos grandes no congelan el modal).
pub const MAX_DIFF_LINES: usize = 400;

/// Tipo de línea del diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Same,
    Removed,
    Added,
}

/// Una línea del diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    /// Texto sin el `\n` final.
    pub text: String,
}

/// Resumen del diff para el título del modal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffStats {
    pub added: usize,
    pub removed: usize,
}

impl DiffStats {
    pub fn summary(&self) -> String {
        format!("+{} −{}", self.added, self.removed)
    }
}

/// Diff por líneas entre `old` (buffer) y `new` (disco).
///
/// Devuelve como máximo [`MAX_DIFF_LINES`] entradas; si hay más, se recortan
/// las de contexto comunes del centro (los cambios se concentran al inicio y
/// al final del recorte). Las estadísticas cuentan el diff completo.
pub fn diff_lines(old: &str, new: &str) -> (Vec<DiffLine>, DiffStats) {
    let diff = TextDiff::from_lines(old, new);
    let mut lines = Vec::new();
    let mut stats = DiffStats::default();
    for change in diff.iter_all_changes() {
        let kind = match change.tag() {
            ChangeTag::Delete => {
                stats.removed += 1;
                DiffKind::Removed
            }
            ChangeTag::Insert => {
                stats.added += 1;
                DiffKind::Added
            }
            ChangeTag::Equal => DiffKind::Same,
        };
        lines.push(DiffLine {
            kind,
            text: change.value().trim_end_matches(['\r', '\n']).to_string(),
        });
    }
    if lines.len() > MAX_DIFF_LINES {
        lines = trim_context(&lines, MAX_DIFF_LINES);
    }
    (lines, stats)
}

/// Recorta conservando cambios: elimina líneas `Same` del centro hasta caber.
fn trim_context(lines: &[DiffLine], budget: usize) -> Vec<DiffLine> {
    let mut keep = vec![true; lines.len()];
    let mut excess = lines.len() - budget;
    let mut left = 0usize;
    let mut right = lines.len().saturating_sub(1);
    // Alterna desde ambos extremos hacia el centro, saltando cambios.
    while excess > 0 && left <= right {
        if lines[left].kind == DiffKind::Same {
            keep[left] = false;
            excess -= 1;
        }
        left += 1;
        if excess == 0 || left > right {
            break;
        }
        if lines[right].kind == DiffKind::Same {
            keep[right] = false;
            excess -= 1;
        }
        right = right.saturating_sub(1);
    }
    lines
        .iter()
        .zip(keep)
        .filter(|(_, keep)| *keep)
        .map(|(line, _)| line.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_lineas_agregadas_y_borradas() {
        let (lines, stats) = diff_lines("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(stats.added, 1);
        assert_eq!(stats.removed, 1);
        let kinds: Vec<DiffKind> = lines.iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            vec![
                DiffKind::Same,
                DiffKind::Removed,
                DiffKind::Added,
                DiffKind::Same
            ]
        );
        assert_eq!(lines[1].text, "b");
        assert_eq!(lines[2].text, "B");
    }

    #[test]
    fn textos_iguales_solo_same() {
        let (lines, stats) = diff_lines("igual\n", "igual\n");
        assert_eq!(stats, DiffStats::default());
        assert!(lines.iter().all(|l| l.kind == DiffKind::Same));
    }

    #[test]
    fn diff_grande_se_recorta_pero_conserva_cambios() {
        let old = (0..2000)
            .map(|i| format!("linea {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let new = old.replace("linea 1000", "linea 1000 CAMBIADA");
        let (lines, stats) = diff_lines(&old, &new);
        assert_eq!(stats.added, 1);
        assert_eq!(stats.removed, 1);
        assert!(lines.len() <= MAX_DIFF_LINES);
        assert!(
            lines.iter().any(|l| l.text.contains("CAMBIADA")),
            "el cambio debe sobrevivir al recorte"
        );
    }

    #[test]
    fn crlf_no_aparece_en_el_texto_mostrado() {
        let (lines, _) = diff_lines("a\r\nb\r\n", "a\r\nB\r\n");
        assert!(!lines.iter().any(|l| l.text.contains('\r')));
    }
}
