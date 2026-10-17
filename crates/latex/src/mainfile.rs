//! Detección del archivo principal (`main.tex`) de un proyecto (M1-D5).
//!
//! Heurística documentada, en orden (gana la primera que acierta):
//!
//! 1. **Settings**: `latex.mainFile` si existe en disco (el usuario manda).
//! 2. **Archivo abierto con `\documentclass`**: lo que el editor tiene
//!    delante suele ser el documento que se edita.
//! 3. **Raíz del grafo de `\input`**: entre los `.tex` de la raíz del proyecto
//!    que llevan `\documentclass`, se descartan los que son `\input`/`\include`
//!    de otro candidato. Si queda uno, es el maestro; si quedan varios, se
//!    prefiere el que incluye al archivo abierto.
//! 4. **Único `.tex` con `\documentclass`**: búsqueda recursiva acotada
//!    (profundidad 4, sin directorios ocultos ni `out`/`build`/`target`); si
//!    hay exactamente uno, es el principal.
//! 5. Si nada es concluyente → `None` (la UI pide elegir).
//!
//! El escaneo de contenido está acotado a [`MAX_SCAN_BYTES`] por archivo y las
//! líneas comentadas con `%` no cuentan para `\documentclass`.

use std::collections::BTreeSet;

use camino::{Utf8Path, Utf8PathBuf};

use crate::engine::EngineError;
use crate::pathutil::normalize;

/// Tope de lectura por archivo al buscar marcadores.
pub const MAX_SCAN_BYTES: u64 = 1024 * 1024;
/// Profundidad de la búsqueda recursiva del punto 4.
pub const MAX_DEPTH: usize = 4;

/// Detecta el `main.tex` de `project` (ver doc del módulo).
///
/// `configured` es `latex.mainFile` (relativo a `project` o absoluto) y
/// `open_file` el archivo abierto en el editor (absoluto, o relativo a
/// `project`). Devuelve rutas absolutas normalizadas.
pub fn detect_main(
    project: &Utf8Path,
    configured: Option<&Utf8Path>,
    open_file: Option<&Utf8Path>,
) -> Result<Option<Utf8PathBuf>, EngineError> {
    let project = absolute_project(project)?;
    // 1. Settings.
    if let Some(configured) = configured {
        let path = resolve(&project, configured);
        if path.is_file() {
            return Ok(Some(normalize(&path)));
        }
    }
    let open_abs = open_file.map(|path| {
        let resolved = resolve(&project, path);
        normalize(&resolved)
    });
    // 2. Archivo abierto con `\documentclass`.
    if let Some(open) = &open_abs
        && open.is_file()
        && has_documentclass(open)
    {
        return Ok(Some(open.clone()));
    }
    // 3. Raíz del grafo de \input entre los .tex de la raíz.
    let root_tex = tex_files_in(&project, false);
    let candidates: Vec<Utf8PathBuf> = root_tex
        .iter()
        .filter(|path| has_documentclass(path))
        .cloned()
        .collect();
    if candidates.len() == 1 {
        return Ok(candidates.into_iter().next());
    }
    if candidates.len() > 1 {
        let children = input_children(&candidates);
        let masters: Vec<&Utf8PathBuf> = candidates
            .iter()
            .filter(|path| !children.contains(normalize(path).as_str()))
            .collect();
        if let [master] = masters.as_slice() {
            return Ok(Some((*master).clone()));
        }
        if let Some(open) = &open_abs
            && let Some(master) = masters.iter().find(|master| {
                parse_input_targets(master).iter().any(|target| {
                    normalize(&master.parent().unwrap_or(&project).join(target)) == *open
                })
            })
        {
            return Ok(Some((*master).clone()));
        }
    }
    // 4. Único .tex con \documentclass en el subárbol.
    let recursive: Vec<Utf8PathBuf> = tex_files_in(&project, true)
        .into_iter()
        .filter(|path| has_documentclass(path))
        .collect();
    if let [only] = recursive.as_slice() {
        return Ok(Some(only.clone()));
    }
    Ok(None)
}

/// `.tex` de la raíz (`recursive = false`) o del subárbol acotado.
fn tex_files_in(project: &Utf8Path, recursive: bool) -> Vec<Utf8PathBuf> {
    let mut out = Vec::new();
    if recursive {
        collect_recursive(project, 0, &mut out);
    } else {
        collect_dir(project, &mut out);
    }
    out.sort();
    out
}

fn collect_dir(dir: &Utf8Path, out: &mut Vec<Utf8PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = dir.join(&name);
        if path.is_file() && has_tex_extension(&path) {
            out.push(path);
        }
    }
}

fn collect_recursive(dir: &Utf8Path, depth: usize, out: &mut Vec<Utf8PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if is_ignored_dir(&name) {
            continue;
        }
        let path = dir.join(&name);
        if path.is_file() {
            if has_tex_extension(&path) {
                out.push(path);
            }
        } else if path.is_dir() {
            collect_recursive(&path, depth + 1, out);
        }
    }
}

/// Directorios que nunca contienen el `main.tex` de fuentes.
fn is_ignored_dir(name: &str) -> bool {
    name.starts_with('.')
        || name.eq_ignore_ascii_case("out")
        || name.eq_ignore_ascii_case("build")
        || name.eq_ignore_ascii_case("target")
        || name.eq_ignore_ascii_case("node_modules")
}

fn has_tex_extension(path: &Utf8Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("tex"))
}

/// `\documentclass` en una línea no comentada.
fn has_documentclass(path: &Utf8Path) -> bool {
    let Some(text) = read_capped(path) else {
        return false;
    };
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        !trimmed.starts_with('%') && trimmed.contains("\\documentclass")
    })
}

/// Rutas normalizadas de los archivos que son `\input`/`\include` de algún candidato.
fn input_children(candidates: &[Utf8PathBuf]) -> BTreeSet<String> {
    let mut children = BTreeSet::new();
    for candidate in candidates {
        for target in parse_input_targets(candidate) {
            let parent = candidate.parent().unwrap_or(Utf8Path::new("."));
            children.insert(normalize(&parent.join(&target)).as_str().to_string());
        }
    }
    children
}

/// Objetivos de `\input{...}`, `\input ...` y `\include{...}` de un `.tex`.
fn parse_input_targets(path: &Utf8Path) -> Vec<String> {
    let Some(text) = read_capped(path) else {
        return Vec::new();
    };
    let mut targets = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('%') {
            continue;
        }
        for command in ["\\input", "\\include"] {
            let mut rest = line;
            while let Some(idx) = rest.find(command) {
                let after_command = &rest[idx + command.len()..];
                // `\inputencoding` no es `\input`: debe seguir `{` o espacio.
                if !after_command.is_empty() && !after_command.starts_with(['{', ' ', '\t', '\\']) {
                    rest = after_command;
                    continue;
                }
                let after = after_command.trim_start();
                let target = if let Some(inner) = after.strip_prefix('{') {
                    match inner.find('}') {
                        Some(end) => inner[..end].trim().to_string(),
                        None => String::new(),
                    }
                } else {
                    after
                        .chars()
                        .take_while(|c| !c.is_whitespace() && *c != '%')
                        .collect()
                };
                if !target.is_empty() {
                    let target = target.trim().trim_end_matches(".tex").to_string() + ".tex";
                    targets.push(target);
                }
                rest = after;
            }
        }
    }
    targets
}

fn read_capped(path: &Utf8Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let capped = if bytes.len() as u64 > MAX_SCAN_BYTES {
        &bytes[..MAX_SCAN_BYTES as usize]
    } else {
        &bytes[..]
    };
    Some(String::from_utf8_lossy(capped).into_owned())
}

fn resolve(project: &Utf8Path, path: &Utf8Path) -> Utf8PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        project.join(path)
    }
}

fn absolute_project(project: &Utf8Path) -> Result<Utf8PathBuf, EngineError> {
    let abs = if project.is_absolute() {
        project.to_owned()
    } else {
        let cwd = Utf8PathBuf::from_path_buf(std::env::current_dir()?).map_err(|p| {
            EngineError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("ruta no UTF-8: {}", p.display()),
            ))
        })?;
        cwd.join(project)
    };
    if !abs.is_dir() {
        return Err(EngineError::ProjectNotFound(abs));
    }
    Ok(abs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Proyecto de prueba en `%TEMP%` (se borra al soltar).
    struct TempProject(Utf8PathBuf);

    impl TempProject {
        fn new(name: &str) -> Self {
            let base = Utf8PathBuf::from_path_buf(std::env::temp_dir())
                .expect("temp UTF-8")
                .join(format!("lsx-m1d5-mainfile-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).expect("crear temp");
            Self(base)
        }

        fn path(&self) -> &Utf8Path {
            &self.0
        }

        fn write(&self, rel: &str, content: &str) -> Utf8PathBuf {
            let path = self.0.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("crear dirs");
            }
            std::fs::write(&path, content).expect("escribir");
            path
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const DOC: &str = "\\documentclass{article}\n\\begin{document}Hola\\end{document}\n";

    #[test]
    fn settings_gana_a_todo() {
        let project = TempProject::new("settings");
        project.write("main.tex", DOC);
        let otro = project.write("otro.tex", DOC);
        let picked =
            detect_main(project.path(), Some(Utf8Path::new("otro.tex")), None).expect("detección");
        assert_eq!(picked, Some(normalize(&otro)));
    }

    #[test]
    fn archivo_abierto_con_documentclass_gana_a_la_heuristica() {
        let project = TempProject::new("abierto");
        project.write("main.tex", DOC);
        let abierto = project.write("cv.tex", DOC);
        let picked = detect_main(project.path(), None, Some(&abierto)).expect("detección");
        assert_eq!(picked, Some(normalize(&abierto)));
    }

    #[test]
    fn archivo_abierto_sin_documentclass_no_gana() {
        let project = TempProject::new("abierto-hijo");
        let main = project.write("main.tex", DOC);
        let hijo = project.write("secciones/uno.tex", "Solo texto.\n");
        let picked = detect_main(project.path(), None, Some(&hijo)).expect("detección");
        assert_eq!(picked, Some(normalize(&main)));
    }

    #[test]
    fn unico_tex_con_documentclass_en_la_raiz() {
        let project = TempProject::new("unico-raiz");
        let main = project.write("main.tex", DOC);
        project.write("notas.txt", "no soy tex");
        let picked = detect_main(project.path(), None, None).expect("detección");
        assert_eq!(picked, Some(normalize(&main)));
    }

    #[test]
    fn grafo_de_input_desambigua_dos_candidatos() {
        let project = TempProject::new("grafo");
        // capitulos.tex es candidato porque tiene \documentclass propio, pero
        // reporte.tex lo incluye: reporte.tex es el maestro.
        let maestro = project.write(
            "reporte.tex",
            "\\documentclass{report}\n\\begin{document}\n\\input{capitulos}\n\\end{document}\n",
        );
        project.write(
            "capitulos.tex",
            "\\documentclass{report}\n\\begin{document}cap\\end{document}\n",
        );
        let picked = detect_main(project.path(), None, None).expect("detección");
        assert_eq!(picked, Some(normalize(&maestro)));
    }

    #[test]
    fn sin_candidatos_en_raiz_encuentra_el_unico_recursivo() {
        let project = TempProject::new("recursivo");
        let main = project.write("src/main.tex", DOC);
        project.write("out/main.tex", DOC); // ignorado por directorio
        project.write("src/notas.tex", "sin clase\n");
        let picked = detect_main(project.path(), None, None).expect("detección");
        assert_eq!(picked, Some(normalize(&main)));
    }

    #[test]
    fn ambiguo_devuelve_none() {
        let project = TempProject::new("ambiguo");
        project.write("uno.tex", DOC);
        project.write("dos.tex", DOC);
        let picked = detect_main(project.path(), None, None).expect("detección");
        assert_eq!(picked, None);
    }

    #[test]
    fn comentario_con_documentclass_no_cuenta() {
        let project = TempProject::new("comentario");
        project.write("borrador.tex", "% \\documentclass{article}\nSolo texto.\n");
        let main = project.write("main.tex", DOC);
        let picked = detect_main(project.path(), None, None).expect("detección");
        assert_eq!(picked, Some(normalize(&main)));
    }

    #[test]
    fn proyecto_inexistente_es_error() {
        let err = detect_main(Utf8Path::new("C:/no-existe-para-nada-lsx"), None, None);
        assert!(matches!(err, Err(EngineError::ProjectNotFound(_))));
    }
}
