mod common;

use osulzer_imageoptimizer::update::check::{Release, download_verified, due, fetch_latest, newer_release, parse_version, signed_message};
use osulzer_imageoptimizer::update::sign::{generate, sign};
use osulzer_imageoptimizer::update::swap::{cleanup_old, swap, swap_with};

fn release_json(tag: &str, with_sig: bool) -> String {
    let mut assets = vec![serde_json::json!({ "name": "osulzer-imageoptimizer.exe", "browser_download_url": "https://x/exe" })];
    if with_sig {
        assets.push(serde_json::json!({ "name": "osulzer-imageoptimizer.exe.sig", "browser_download_url": "https://x/sig" }));
    }
    serde_json::json!({ "tag_name": tag, "html_url": "https://github.com/OSULZER/osulzer-imageoptimizer/releases/tag/x", "assets": assets }).to_string()
}

#[test]
fn parse_version_rules() {
    assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
    assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
    assert_eq!(parse_version("v1.2"), None);
    assert_eq!(parse_version("v1.2.3-beta"), None);
    assert_eq!(parse_version("x"), None);
}

#[test]
fn only_newer_releases_count() {
    assert!(newer_release(&release_json("v1.0.0", true), "1.0.0").is_none());
    assert!(newer_release(&release_json("v0.9.9", true), "1.0.0").is_none());
    assert!(newer_release(&release_json("v1.1.0-beta", true), "1.0.0").is_none());
    assert!(newer_release(&release_json("v1.0.1", false), "1.0.0").is_none(), "ohne Signatur kein Update");
    let r = newer_release(&release_json("v1.0.1", true), "1.0.0").unwrap();
    assert_eq!((r.version, r.exe_url.as_str(), r.sig_url.as_str()), ((1, 0, 1), "https://x/exe", "https://x/sig"));
}

#[test]
fn due_after_a_day() {
    assert!(due(0, 100_000));
    assert!(!due(100_000, 100_000 + 3600));
    assert!(due(100_000, 100_000 + 86_400));
}

fn served(exe: &'static str, sig: String) -> String {
    common::fake_server(move |r| if r.path.ends_with("/sig") { (200, sig.clone()) } else { (200, exe.to_string()) })
}

fn release(base: &str) -> Release {
    Release { version: (9, 0, 0), tag: "v9.0.0".into(), notes_url: String::new(), exe_url: format!("{base}/exe"), sig_url: format!("{base}/sig") }
}

#[test]
fn download_rejects_bad_signature() {
    let (_, public) = generate();
    let (other_private, _) = generate();
    let base = served("NEUE-EXE", sign(&signed_message("v9.0.0", b"NEUE-EXE"), &other_private).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("neu.exe");
    assert!(download_verified(&release(&base), &public, &dest).is_err());
    assert!(!dest.exists());
}

#[test]
fn download_accepts_good_signature() {
    let (private, public) = generate();
    let base = served("NEUE-EXE", sign(&signed_message("v9.0.0", b"NEUE-EXE"), &private).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("neu.exe");
    download_verified(&release(&base), &public, &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"NEUE-EXE");
}

#[test]
fn fetch_latest_reads_the_api() {
    let base = common::fake_server(|r| {
        assert!(r.header("user-agent").is_some_and(|u| u.starts_with("osulzer-imageoptimizer/")));
        (200, release_json("v2.0.0", true))
    });
    let r = fetch_latest(&base, "1.0.0").unwrap().unwrap();
    assert_eq!(r.tag, "v2.0.0");
    let none = common::fake_server(|_| (404, "{}".into()));
    assert_eq!(fetch_latest(&none, "1.0.0").unwrap(), None);
}

#[test]
fn swap_replaces_and_keeps_old() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("app.exe");
    let new = dir.path().join("app.exe.new");
    std::fs::write(&exe, b"ALT").unwrap();
    std::fs::write(&new, b"NEU").unwrap();
    swap(&exe, &new).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"NEU");
    assert_eq!(std::fs::read(dir.path().join("app.exe.old")).unwrap(), b"ALT");
    assert!(!new.exists());
    cleanup_old(&exe);
    assert!(!dir.path().join("app.exe.old").exists());
}

#[test]
fn swap_failure_restores() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("app.exe");
    std::fs::write(&exe, b"ALT").unwrap();
    assert!(swap(&exe, &dir.path().join("fehlt.exe")).is_err());
    assert_eq!(std::fs::read(&exe).unwrap(), b"ALT");
    assert!(!dir.path().join("app.exe.old").exists());
}

#[test]
fn old_signed_exe_under_new_tag_is_rejected() {
    // Echte Signatur der alten Version, aber als „v9.0.0“ veröffentlicht: Rollback-Versuch.
    let (private, public) = generate();
    let base = served("ALTE-EXE", sign(&signed_message("v1.0.0", b"ALTE-EXE"), &private).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("neu.exe");
    assert!(download_verified(&release(&base), &public, &dest).is_err());
    assert!(!dest.exists());
}

#[test]
fn second_rename_failure_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("app.exe");
    let new = dir.path().join("app.exe.new");
    std::fs::write(&exe, b"ALT").unwrap();
    std::fs::write(&new, b"NEU").unwrap();
    let calls = std::cell::Cell::new(0);
    let result = swap_with(&exe, &new, |from, to| {
        calls.set(calls.get() + 1);
        if calls.get() == 2 { Err(std::io::Error::other("gesperrt")) } else { std::fs::rename(from, to) }
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read(&exe).unwrap(), b"ALT", "Original wieder an seinem Platz");
    assert!(!dir.path().join("app.exe.old").exists());
}
