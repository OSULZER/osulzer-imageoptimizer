//! Bettet unter Windows Programmsymbol und Versionsangaben in die `.exe` ein.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("FileDescription", "OSULZER ImageOptimizer")
            .set("ProductName", "OSULZER ImageOptimizer")
            .set("OriginalFilename", "osulzer-imageoptimizer.exe");
        if let Err(e) = res.compile() {
            // Ohne Ressourcen-Compiler bauen, nur eben ohne Symbol.
            println!("cargo:warning=Programmsymbol nicht eingebettet: {e}");
        }
    }
}
