//! Die Kommandozeile als echter Prozess: Ausgabe, JSON und Exit-Codes.

mod common;

use std::path::Path;
use std::process::{Command, Output};

const EXE: &str = env!("CARGO_BIN_EXE_osulzer-imageoptimizer");

/// Startet das Programm mit eigener (leerer oder vorgegebener) Konfiguration.
fn run(config: &Path, args: &[&str]) -> Output {
    Command::new(EXE).args(args).env("OSULZER_IMAGEOPTIMIZER_CONFIG", config).output().unwrap()
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("kein JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)))
}

#[test]
fn optimize_json_reports_each_file() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::write_jpeg(dir.path(), "a.jpg", 2000, 1500);
    let b = common::write_jpeg(dir.path(), "b.jpg", 800, 600);
    let cfg = dir.path().join("config.json");
    let out = run(&cfg, &["optimize", a.to_str().unwrap(), b.to_str().unwrap(), "--name", "Kunde", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let v = json(&out);
    let list = v.as_array().unwrap();
    assert_eq!(list.len(), 2);
    for (i, item) in list.iter().enumerate() {
        let output = item["output"].as_str().unwrap();
        assert!(Path::new(output).exists());
        assert!(output.ends_with(&format!("kunde-0{}.jpg", i + 1)), "{output}");
        assert!(item["error"].is_null());
    }
    assert!(!cfg.exists(), "die Kommandozeile schreibt keine Konfiguration");
}

#[test]
fn failure_gives_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("kaputt.jpg");
    std::fs::write(&broken, b"x").unwrap();
    let out = run(&dir.path().join("c.json"), &["optimize", broken.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn bad_usage_gives_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(&dir.path().join("c.json"), &["optimize", "--quality", "500", "a.jpg"]);
    assert_eq!(out.status.code(), Some(2));
    let out = run(&dir.path().join("c.json"), &["optimize", "--preset", "gibt-es-nicht", "a.jpg"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn presets_lists_keys() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(&dir.path().join("c.json"), &["presets", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("web-large") && text.contains("thumbnail"), "{text}");
}

#[test]
fn info_reports_dimensions() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::write_jpeg(dir.path(), "a.jpg", 320, 240);
    let out = run(&dir.path().join("c.json"), &["info", a.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let v = json(&out);
    assert_eq!((v[0]["width"].as_u64(), v[0]["height"].as_u64()), (Some(320), Some(240)));
}

#[test]
fn describe_with_cloud_provider_without_consent_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.json");
    std::fs::write(&cfg, r#"{"ui_lang":"de","ai":{"provider":"anthropic","providers":{"anthropic":{"base_url":"","model":"claude-haiku-5-5"}}}}"#).unwrap();
    let a = common::write_jpeg(dir.path(), "a.jpg", 320, 240);
    let out = run(&cfg, &["describe", a.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("zustimmen") && err.contains("KI-Beschreibung"), "{err}");
}

#[test]
fn describe_uses_local_provider() {
    let base = common::fake_server(|_| {
        (200, common::chat_reply(r#"{"dateiname":"Grüner Ofen","titel":"Ofen","alt_text":"Ein Ofen","beschreibung":"B."}"#))
    });
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.json");
    let config = serde_json::json!({ "ui_lang": "de", "ai_lang": "de", "ai": { "provider": "custom", "providers": { "custom": { "base_url": base, "model": "m" } } } });
    std::fs::write(&cfg, config.to_string()).unwrap();
    let a = common::write_jpeg(dir.path(), "a.jpg", 320, 240);
    let out = run(&cfg, &["describe", a.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let v = json(&out);
    assert_eq!(v["file_stem"], "gruener-ofen");
    assert_eq!(v["alt"], "Ein Ofen");
}
