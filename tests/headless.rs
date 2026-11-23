mod common;

use osulzer_imageoptimizer::headless::{Destination, Request, info, optimize, path_key, unmatched_texts};
use osulzer_imageoptimizer::i18n::Lang;
use osulzer_imageoptimizer::naming::NameRules;
use osulzer_imageoptimizer::settings::{Target, builtin_presets_in, preset_by_key};
use osulzer_imageoptimizer::texts::ImageTexts;
use std::collections::HashMap;
use std::path::PathBuf;

fn request(files: Vec<PathBuf>, destination: Destination) -> Request {
    Request {
        files,
        settings: preset_by_key("web-medium").unwrap().settings,
        destination,
        rules: NameRules::default(),
        texts: HashMap::new(),
        lang: Lang::De,
    }
}

#[test]
fn optimizes_into_web_folder() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "foto.jpg", 2400, 1600);
    let reports = optimize(&request(vec![src.clone()], Destination::Target(Target::WebSubfolder)));
    assert_eq!(reports.len(), 1);
    let r = &reports[0];
    assert_eq!(r.error, None);
    assert_eq!(r.output.as_deref(), Some(dir.path().join("web").join("foto.jpg").as_path()));
    assert_eq!((r.width, r.height), (1200, 800));
    assert!(r.bytes_after > 0 && r.bytes_before == std::fs::metadata(&src).unwrap().len());
    assert!(src.exists());
}

#[test]
fn uniform_name_numbers_files() {
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<_> = (1..=3).map(|i| common::write_jpeg(dir.path(), &format!("IMG_{i}.jpg"), 300, 200)).collect();
    let mut req = request(files, Destination::Target(Target::WebSubfolder));
    req.rules = NameRules { uniform: "Kunde".into(), ..Default::default() };
    let names: Vec<_> = optimize(&req).iter().map(|r| r.output.clone().unwrap().file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["kunde-01.jpg", "kunde-02.jpg", "kunde-03.jpg"]);
}

#[test]
fn same_final_name_gets_counter() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::write_jpeg(dir.path(), "a-x.jpg", 300, 200);
    let b = common::write_jpeg(dir.path(), "b-x.jpg", 300, 200);
    let mut req = request(vec![a, b], Destination::Target(Target::WebSubfolder));
    // „a-“ und „b-“ entfernen: beide heißen danach „x“.
    req.rules = NameRules { search: "a-".into(), prefix: String::new(), ..Default::default() };
    let reports = optimize(&req);
    let first = reports[0].output.clone().unwrap();
    assert_eq!(first.file_name().unwrap(), "x.jpg");
    let mut req2 = request(vec![dir.path().join("b-x.jpg")], Destination::Target(Target::WebSubfolder));
    req2.rules = NameRules { search: "b-".into(), ..Default::default() };
    let second = optimize(&req2)[0].output.clone().unwrap();
    assert_eq!(second.file_name().unwrap(), "x-1.jpg", "vorhandene Datei wird nicht überschrieben");
}

#[test]
fn existing_file_in_folder_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "foto.jpg", 300, 200);
    let out = dir.path().join("ausgabe");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("foto.jpg"), b"alt").unwrap();
    let reports = optimize(&request(vec![src], Destination::Folder(out.clone())));
    assert_eq!(reports[0].output.as_deref(), Some(out.join("foto-1.jpg").as_path()));
    assert_eq!(std::fs::read(out.join("foto.jpg")).unwrap(), b"alt");
}

#[test]
fn texts_are_embedded_and_rename() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "IMG_9.jpg", 300, 200);
    let mut req = request(vec![src.clone()], Destination::Target(Target::WebSubfolder));
    req.texts.insert(
        path_key(&src),
        ImageTexts { file_stem: "gruener-ofen".into(), title: "Grüner Ofen".into(), alt: "Ein grüner Kachelofen".into(), description: String::new() },
    );
    let out = optimize(&req)[0].output.clone().unwrap();
    assert_eq!(out.file_name().unwrap(), "gruener-ofen.jpg");
    let bytes = std::fs::read(out).unwrap();
    assert!(bytes.windows("Ein grüner Kachelofen".len()).any(|w| w == "Ein grüner Kachelofen".as_bytes()));
}

#[test]
fn broken_file_reports_error_and_others_continue() {
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("kaputt.jpg");
    std::fs::write(&broken, b"kein bild").unwrap();
    let ok = common::write_jpeg(dir.path(), "ok.jpg", 300, 200);
    let reports = optimize(&request(vec![broken, ok], Destination::Target(Target::WebSubfolder)));
    assert!(reports[0].error.is_some() && reports[0].output.is_none());
    assert!(reports[1].error.is_none() && reports[1].output.is_some());
}

#[test]
fn relative_path_is_an_error() {
    let reports = optimize(&request(vec![PathBuf::from("relativ.jpg")], Destination::Target(Target::WebSubfolder)));
    assert!(reports[0].error.is_some());
}

#[test]
fn preset_keys_resolve() {
    let all = builtin_presets_in(Lang::En);
    assert_eq!(preset_by_key("web-large").unwrap().settings, all[0].settings);
    assert_eq!(preset_by_key("web-medium").unwrap().settings, all[1].settings);
    assert_eq!(preset_by_key("thumbnail").unwrap().settings, all[2].settings);
    assert!(preset_by_key("gibt-es-nicht").is_none());
}

#[test]
fn info_reports_dimensions() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "a.jpg", 320, 240);
    let i = info(&src).unwrap();
    assert_eq!((i.width, i.height, i.format.as_str()), (320, 240, "jpeg"));
}

#[test]
fn two_images_with_same_final_name_in_one_run() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::write_jpeg(dir.path(), "a.jpg", 300, 200);
    let b = common::write_png(dir.path(), "A.png", 300, 200);
    let mut req = request(vec![a, b], Destination::Target(Target::WebSubfolder));
    req.settings.format = osulzer_imageoptimizer::settings::Format::Jpeg;
    req.rules = NameRules { prefix: "k".into(), ..Default::default() };
    let names: Vec<_> = optimize(&req).iter().map(|r| r.output.clone().unwrap().file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["k-a.jpg", "k-a-1.jpg"]);
}

#[test]
fn texts_match_regardless_of_case_and_slashes() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "IMG_0413.jpg", 300, 200);
    let other_spelling = src.to_string_lossy().to_lowercase().replace('\\', "/").replace(".jpg", ".JPG");
    let mut req = request(vec![src.clone()], Destination::Target(Target::WebSubfolder));
    req.texts.insert(path_key(std::path::Path::new(&other_spelling)), ImageTexts { file_stem: "ofen".into(), alt: "Ofen".into(), ..Default::default() });
    assert!(unmatched_texts(&req).is_empty());
    assert_eq!(optimize(&req)[0].output.clone().unwrap().file_name().unwrap(), "ofen.jpg");
}

#[test]
fn texts_without_matching_file_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "a.jpg", 300, 200);
    let mut req = request(vec![src], Destination::Target(Target::WebSubfolder));
    req.texts.insert(path_key(&dir.path().join("gibt-es-nicht.jpg")), ImageTexts { alt: "x".into(), ..Default::default() });
    assert_eq!(unmatched_texts(&req).len(), 1);
}

#[test]
fn duplicate_input_is_processed_once() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "a.jpg", 300, 200);
    let mut req = request(vec![src.clone(), src.clone()], Destination::Target(Target::WebSubfolder));
    req.rules = NameRules { uniform: "Kunde".into(), ..Default::default() };
    let reports = optimize(&req);
    assert_eq!(reports.len(), 1);
    assert!(reports[0].output.as_ref().unwrap().exists());
}
