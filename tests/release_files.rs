//! Dateien, die jede Veröffentlichung braucht.

use std::path::Path;

fn read(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn license_and_notice_exist() {
    let license = read("LICENSE");
    assert!(license.contains("Apache License") && license.contains("Version 2.0"));
    assert!(license.contains("Commons Clause"));
    let notice = read("NOTICE");
    assert!(notice.contains("OSULZER") && notice.contains("Independent JPEG Group"));
}

#[test]
fn readme_is_english_and_mentions_license() {
    let readme = read("README.md");
    assert!(readme.contains("Commons Clause"));
    assert!(readme.contains("not licensed"), "Hinweis, dass der Name OSULZER nicht mitlizenziert ist");
    assert!(readme.contains("Install"));
    assert!(read("README.de.md").contains("Kontextmenü"));
}

#[test]
fn version_is_plain_semver() {
    let parts: Vec<_> = env!("CARGO_PKG_VERSION").split('.').collect();
    assert_eq!(parts.len(), 3);
    assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()), "keine Vorab-Version");
}
