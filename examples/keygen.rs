//! Erzeugt das Schlüsselpaar für signierte Updates. Einmalig auszuführen, siehe docs/RELEASE.md.
//! Schreibt den öffentlichen Schlüssel nach assets/update-public-key.txt und zeigt den privaten EINMAL an.

use osulzer_imageoptimizer::update::sign::{generate, is_real_key};

fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("update-public-key.txt");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if is_real_key(&current) {
        eprintln!("{} already contains a real key. Refusing to overwrite it:", path.display());
        eprintln!("installed copies could never update again. Delete the file yourself if you really want a new key.");
        std::process::exit(1);
    }
    let (private, public) = generate();
    std::fs::write(&path, format!("{public}\n")).expect("write public key");
    println!("Public key written to {} (commit this file).", path.display());
    println!();
    println!("PRIVATE KEY (shown only now):");
    println!("{private}");
    println!();
    println!("1. Save it in your password manager.");
    println!("2. Add it as GitHub secret UPDATE_SIGNING_KEY (repo Settings > Secrets and variables > Actions).");
    println!("If it is lost, installed copies cannot receive updates anymore.");
}
