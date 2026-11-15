//! Recursos de Windows del ejecutable: icono (`installer/latex-studio.ico`) y
//! metadatos de versión que muestra el Explorador.
//!
//! Si el compilador de recursos (`rc.exe` del Windows SDK) no está disponible,
//! el build sigue y solo avisa: el exe queda sin icono, pero no se rompe.

fn main() {
    println!("cargo:rerun-if-changed=../installer/latex-studio.ico");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../installer/latex-studio.ico")
        .set("ProductName", "LaTeX Studio")
        .set("FileDescription", "LaTeX Studio")
        .set("CompanyName", "LaTeX Studio");
    if let Err(err) = res.compile() {
        println!("cargo:warning=sin icono embebido (no se pudo compilar el recurso): {err}");
    }
}
