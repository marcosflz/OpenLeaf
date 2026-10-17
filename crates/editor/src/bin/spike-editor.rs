//! Binario del spike M0-S2 en el crate `lsx-editor` (alternativo al de
//! `latex-studio`, útil si el wiring de `app/` aún no depende de este crate).
//!
//! `cargo run -p lsx-editor --bin spike-editor [-- --check]`

fn main() {
    lsx_editor::spike::run_spike();
}
