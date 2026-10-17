//! `lsx-latex` — motor LaTeX, cola de compilación, parser de log y SyncTeX.
//!
//! Responsabilidades (ver `docs/plan/INTERFACES.md` §3):
//! - `LatexEngine` (Trait) + backend TeX Live (`tex/bin/windows/latexmk.exe`)
//! - `CompileQueue` (single-flight, coalescing, cancelación, job objects)
//! - `parse_log` → `Diagnostic[]` (fixtures en `fixtures/logs/`)
//! - `Synctex` (envoltorio de `synctex.exe`: forward/inverse)
//! - auto-install de paquetes vía `tlmgr`
//!
//! Regla dura: sin gpui; produce datos puros y consume comandos.
//!
//! Estado: P0 — stub. Se implementa en M0-S5 (spike) y M1 (stream D).
