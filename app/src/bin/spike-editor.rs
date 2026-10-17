//! Binario del spike M0-S2: editor del kit + resaltado LaTeX tree-sitter.
//!
//! `cargo run -p latex-studio --bin spike-editor [-- --check]`
//!
//! La vista vive en `lsx_editor::spike`; aquí solo se enlaza con el binario de
//! la app (ver `crates/editor/src/spike.rs`).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(linker_messages)]

fn main() {
    lsx_editor::spike::run_spike();
}
