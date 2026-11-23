use osulzer_imageoptimizer::engine::Encoded;
use osulzer_imageoptimizer::job::{ConflictPolicy, JobError, SaveNote, Session, planned_path};
use osulzer_imageoptimizer::settings::{OutFormat, Target};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

fn enc(format: OutFormat, data: &[u8]) -> Encoded {
    Encoded { data: data.to_vec(), format, width: 10, height: 10, quality_used: Some(80), note: None, source_untouched: false }
}

fn no_trash(_: &Path) -> Result<(), String> {
    panic!("Papierkorb darf hier nicht benutzt werden")
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn planned_paths_for_all_targets() {
    let src = Path::new(r"C:\x\IMG 1.png");
    assert_eq!(planned_path(src, Target::WebSubfolder, OutFormat::Jpeg), PathBuf::from(r"C:\x\web\IMG 1.jpg"));
    assert_eq!(planned_path(src, Target::Suffix, OutFormat::Jpeg), PathBuf::from(r"C:\x\IMG 1-web.jpg"));
    assert_eq!(planned_path(src, Target::Replace, OutFormat::Jpeg), PathBuf::from(r"C:\x\IMG 1.jpg"));
    assert_eq!(planned_path(Path::new(r"C:\x\a.b.tiff"), Target::Suffix, OutFormat::Webp), PathBuf::from(r"C:\x\a.b-web.webp"));
}

#[test]
fn web_subfolder_is_created_and_original_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.jpg");
    fs::write(&src, b"original").unwrap();
    let mut s = Session::default();
    s.begin_run();
    let saved = s.save(&src, &enc(OutFormat::Jpeg, b"neu"), Target::WebSubfolder, ConflictPolicy::Skip, &no_trash).unwrap();
    assert_eq!(saved.path, Some(dir.path().join("web").join("a.jpg")));
    assert_eq!(saved.bytes, 3);
    assert_eq!(saved.note, None);
    assert_eq!(fs::read(dir.path().join("web/a.jpg")).unwrap(), b"neu");
    assert_eq!(fs::read(&src).unwrap(), b"original");
    assert_eq!(names(&dir.path().join("web")), vec!["a.jpg"]);
}

#[test]
fn two_sources_with_same_target_name_get_a_counter() {
    let dir = tempfile::tempdir().unwrap();
    let (png, jpg) = (dir.path().join("a.png"), dir.path().join("a.jpg"));
    fs::write(&png, b"p").unwrap();
    fs::write(&jpg, b"j").unwrap();
    let mut s = Session::default();
    s.begin_run();
    let first = s.save(&png, &enc(OutFormat::Jpeg, b"1"), Target::WebSubfolder, ConflictPolicy::Overwrite, &no_trash).unwrap();
    let second = s.save(&jpg, &enc(OutFormat::Jpeg, b"2"), Target::WebSubfolder, ConflictPolicy::Overwrite, &no_trash).unwrap();
    assert_eq!(first.path, Some(dir.path().join("web/a.jpg")));
    assert_eq!(second.path, Some(dir.path().join("web/a-1.jpg")));
    assert_eq!(fs::read(dir.path().join("web/a.jpg")).unwrap(), b"1");
    assert_eq!(fs::read(dir.path().join("web/a-1.jpg")).unwrap(), b"2");
}

fn with_foreign_file(policy: ConflictPolicy) -> (tempfile::TempDir, osulzer_imageoptimizer::job::Saved) {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.jpg");
    fs::write(&src, b"original").unwrap();
    fs::create_dir(dir.path().join("web")).unwrap();
    fs::write(dir.path().join("web/a.jpg"), b"fremd").unwrap();
    let mut s = Session::default();
    s.begin_run();
    assert!(s.has_foreign_conflict(&dir.path().join("web/a.jpg"), &src));
    let saved = s.save(&src, &enc(OutFormat::Jpeg, b"neu"), Target::WebSubfolder, policy, &no_trash).unwrap();
    (dir, saved)
}

#[test]
fn foreign_file_overwrite() {
    let (dir, saved) = with_foreign_file(ConflictPolicy::Overwrite);
    assert_eq!(saved.path, Some(dir.path().join("web/a.jpg")));
    assert_eq!(fs::read(dir.path().join("web/a.jpg")).unwrap(), b"neu");
}

#[test]
fn foreign_file_rename() {
    let (dir, saved) = with_foreign_file(ConflictPolicy::Rename);
    assert_eq!(saved.path, Some(dir.path().join("web/a-1.jpg")));
    assert_eq!(fs::read(dir.path().join("web/a.jpg")).unwrap(), b"fremd");
    assert_eq!(fs::read(dir.path().join("web/a-1.jpg")).unwrap(), b"neu");
}

#[test]
fn foreign_file_skip() {
    let (dir, saved) = with_foreign_file(ConflictPolicy::Skip);
    assert_eq!(saved.path, None);
    assert_eq!(saved.note, Some(SaveNote::Skipped));
    assert_eq!(fs::read(dir.path().join("web/a.jpg")).unwrap(), b"fremd");
    assert_eq!(names(&dir.path().join("web")), vec!["a.jpg"]);
}

#[test]
fn own_files_are_overwritten_in_a_second_run() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.jpg");
    fs::write(&src, b"original").unwrap();
    let mut s = Session::default();
    s.begin_run();
    s.save(&src, &enc(OutFormat::Jpeg, b"erster"), Target::WebSubfolder, ConflictPolicy::Skip, &no_trash).unwrap();
    s.begin_run();
    assert!(!s.has_foreign_conflict(&dir.path().join("web/a.jpg"), &src));
    let saved = s.save(&src, &enc(OutFormat::Jpeg, b"zweiter"), Target::WebSubfolder, ConflictPolicy::Skip, &no_trash).unwrap();
    assert_eq!(saved.path, Some(dir.path().join("web/a.jpg")));
    assert_eq!(fs::read(dir.path().join("web/a.jpg")).unwrap(), b"zweiter");
    assert_eq!(names(&dir.path().join("web")), vec!["a.jpg"]);
}

/// „Papierkorb“ für Tests: verschiebt in einen Ordner und merkt sich die Aufrufe.
fn fake_trash(bin: PathBuf, calls: &RefCell<Vec<PathBuf>>) -> impl Fn(&Path) -> Result<(), String> + '_ {
    move |p: &Path| {
        calls.borrow_mut().push(p.to_path_buf());
        fs::create_dir_all(&bin).unwrap();
        fs::rename(p, bin.join(p.file_name().unwrap())).map_err(|e| e.to_string())
    }
}

#[test]
fn replace_same_extension_moves_original_to_trash() {
    let dir = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.jpg");
    fs::write(&src, b"original").unwrap();
    let calls = RefCell::new(Vec::new());
    let trash = fake_trash(bin.path().to_path_buf(), &calls);
    let mut s = Session::default();
    s.begin_run();
    let saved = s.save(&src, &enc(OutFormat::Jpeg, b"neu"), Target::Replace, ConflictPolicy::Skip, &trash).unwrap();
    assert_eq!(saved.path, Some(src.clone()));
    assert_eq!(saved.note, None);
    assert_eq!(*calls.borrow(), vec![src.clone()]);
    assert_eq!(fs::read(&src).unwrap(), b"neu");
    assert_eq!(fs::read(bin.path().join("a.jpg")).unwrap(), b"original");
    assert_eq!(names(dir.path()), vec!["a.jpg"]);
}

#[test]
fn replace_with_format_change_uses_new_extension() {
    let dir = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.png");
    fs::write(&src, b"original").unwrap();
    let calls = RefCell::new(Vec::new());
    let trash = fake_trash(bin.path().to_path_buf(), &calls);
    let mut s = Session::default();
    s.begin_run();
    let saved = s.save(&src, &enc(OutFormat::Jpeg, b"neu"), Target::Replace, ConflictPolicy::Skip, &trash).unwrap();
    assert_eq!(saved.path, Some(dir.path().join("a.jpg")));
    assert_eq!(names(dir.path()), vec!["a.jpg"]);
    assert_eq!(fs::read(bin.path().join("a.png")).unwrap(), b"original");
}

#[test]
fn replace_never_overwrites_a_foreign_file_with_the_new_name() {
    let dir = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.png");
    fs::write(&src, b"original").unwrap();
    fs::write(dir.path().join("a.jpg"), b"fremd").unwrap();
    let calls = RefCell::new(Vec::new());
    let trash = fake_trash(bin.path().to_path_buf(), &calls);
    let mut s = Session::default();
    s.begin_run();
    let saved = s.save(&src, &enc(OutFormat::Jpeg, b"neu"), Target::Replace, ConflictPolicy::Overwrite, &trash).unwrap();
    assert_eq!(saved.path, Some(dir.path().join("a-1.jpg")));
    assert_eq!(fs::read(dir.path().join("a.jpg")).unwrap(), b"fremd");
    assert_eq!(names(dir.path()), vec!["a-1.jpg", "a.jpg"]);
}

#[test]
fn replace_without_recycle_bin_keeps_original() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.jpg");
    fs::write(&src, b"original").unwrap();
    let failing = |_: &Path| Err("kein Papierkorb".to_string());
    let mut s = Session::default();
    s.begin_run();
    let saved = s.save(&src, &enc(OutFormat::Jpeg, b"neu"), Target::Replace, ConflictPolicy::Skip, &failing).unwrap();
    assert_eq!(saved.note, Some(SaveNote::NoRecycleBin));
    assert_eq!(saved.path, Some(dir.path().join("a-web.jpg")));
    assert_eq!(fs::read(&src).unwrap(), b"original");
    assert_eq!(fs::read(dir.path().join("a-web.jpg")).unwrap(), b"neu");
    assert_eq!(names(dir.path()), vec!["a-web.jpg", "a.jpg"]);
}

#[test]
fn replace_leaves_untouched_source_alone() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.jpg");
    fs::write(&src, b"original").unwrap();
    let mut e = enc(OutFormat::Jpeg, b"original");
    e.source_untouched = true;
    let mut s = Session::default();
    s.begin_run();
    let saved = s.save(&src, &e, Target::Replace, ConflictPolicy::Skip, &no_trash).unwrap();
    assert_eq!(saved.note, Some(SaveNote::Unchanged));
    assert_eq!(saved.path, Some(src.clone()));
    assert_eq!(names(dir.path()), vec!["a.jpg"]);
}

#[test]
fn unusable_target_folder_is_an_error_without_leftovers() {
    osulzer_imageoptimizer::i18n::set_lang(osulzer_imageoptimizer::i18n::Lang::De);
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.jpg");
    fs::write(&src, b"original").unwrap();
    fs::write(dir.path().join("web"), b"ich bin eine Datei").unwrap();
    let mut s = Session::default();
    s.begin_run();
    let r = s.save(&src, &enc(OutFormat::Jpeg, b"neu"), Target::WebSubfolder, ConflictPolicy::Overwrite, &no_trash);
    assert!(matches!(r, Err(JobError::Io(_))));
    assert!(r.unwrap_err().to_string().starts_with("Ziel nicht beschreibbar"));
    assert_eq!(names(dir.path()), vec!["a.jpg", "web"]);
}

#[test]
fn output_of_another_source_from_an_earlier_run_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let (jpg, tif) = (dir.path().join("a.jpg"), dir.path().join("a.tif"));
    fs::write(&jpg, b"j").unwrap();
    fs::write(&tif, b"t").unwrap();
    let mut s = Session::default();
    s.begin_run();
    s.save(&jpg, &enc(OutFormat::Jpeg, b"vom jpg"), Target::WebSubfolder, ConflictPolicy::Rename, &no_trash).unwrap();
    s.begin_run();
    assert!(s.has_foreign_conflict(&dir.path().join("web/a.jpg"), &tif));
    let saved = s.save(&tif, &enc(OutFormat::Jpeg, b"vom tif"), Target::WebSubfolder, ConflictPolicy::Rename, &no_trash).unwrap();
    assert_eq!(saved.path, Some(dir.path().join("web/a-1.jpg")));
    assert_eq!(fs::read(dir.path().join("web/a.jpg")).unwrap(), b"vom jpg");
}

#[test]
fn replace_in_a_later_run_never_overwrites_another_sources_result() {
    let dir = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let (jpg, png) = (dir.path().join("a.jpg"), dir.path().join("a.png"));
    fs::write(&jpg, b"j").unwrap();
    fs::write(&png, b"p").unwrap();
    let calls = RefCell::new(Vec::new());
    let trash = fake_trash(bin.path().to_path_buf(), &calls);
    let mut s = Session::default();
    s.begin_run();
    s.save(&jpg, &enc(OutFormat::Jpeg, b"vom jpg"), Target::Replace, ConflictPolicy::Rename, &trash).unwrap();
    s.begin_run();
    let saved = s.save(&png, &enc(OutFormat::Jpeg, b"vom png"), Target::Replace, ConflictPolicy::Rename, &trash).unwrap();
    assert_eq!(saved.path, Some(dir.path().join("a-1.jpg")));
    assert_eq!(fs::read(&jpg).unwrap(), b"vom jpg");
}

#[test]
fn replace_keeps_the_result_when_the_final_rename_fails() {
    let dir = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.jpg");
    fs::write(&src, b"original").unwrap();
    // Nach dem Papierkorb taucht unter dem Zielnamen etwas auf, das sich nicht ersetzen lässt.
    let blocker = |p: &Path| -> Result<(), String> {
        fs::rename(p, bin.path().join("a.jpg")).unwrap();
        fs::create_dir(p).unwrap();
        Ok(())
    };
    let mut s = Session::default();
    s.begin_run();
    let saved = s.save(&src, &enc(OutFormat::Jpeg, b"neu"), Target::Replace, ConflictPolicy::Skip, &blocker).unwrap();
    assert_eq!(saved.note, Some(SaveNote::SavedBeside));
    assert_eq!(saved.path, Some(dir.path().join("a-web.jpg")));
    assert_eq!(fs::read(dir.path().join("a-web.jpg")).unwrap(), b"neu");
    assert_eq!(names(dir.path()), vec!["a-web.jpg", "a.jpg"]);
}

// ---- Benannte Ziele (KI-Dateiname) ----

use osulzer_imageoptimizer::job::planned_path_named;

#[test]
fn named_paths_for_all_targets() {
    let src = Path::new(r"C:\x\IMG 1.png");
    let stem = Some("kachelofen-gruen");
    assert_eq!(planned_path_named(src, Target::WebSubfolder, OutFormat::Jpeg, stem), PathBuf::from(r"C:\x\web\kachelofen-gruen.jpg"));
    assert_eq!(planned_path_named(src, Target::Suffix, OutFormat::Jpeg, stem), PathBuf::from(r"C:\x\kachelofen-gruen.jpg"));
    assert_eq!(planned_path_named(src, Target::Replace, OutFormat::Webp, stem), PathBuf::from(r"C:\x\kachelofen-gruen.webp"));
    // Gleicher Name wie die Quelle oder leer: wie bisher.
    assert_eq!(planned_path_named(src, Target::Suffix, OutFormat::Jpeg, Some("IMG 1")), PathBuf::from(r"C:\x\IMG 1-web.jpg"));
    assert_eq!(planned_path_named(src, Target::Suffix, OutFormat::Jpeg, Some("")), PathBuf::from(r"C:\x\IMG 1-web.jpg"));
    assert_eq!(planned_path_named(src, Target::Suffix, OutFormat::Jpeg, None), PathBuf::from(r"C:\x\IMG 1-web.jpg"));
}

#[test]
fn two_images_with_the_same_ai_name_get_a_counter() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.jpg"), dir.path().join("b.jpg"));
    fs::write(&a, b"a").unwrap();
    fs::write(&b, b"b").unwrap();
    let mut s = Session::default();
    s.begin_run();
    let first = s.save_named(&a, &enc(OutFormat::Jpeg, b"1"), Target::WebSubfolder, ConflictPolicy::Rename, &no_trash, Some("ofen")).unwrap();
    let second = s.save_named(&b, &enc(OutFormat::Jpeg, b"2"), Target::WebSubfolder, ConflictPolicy::Rename, &no_trash, Some("ofen")).unwrap();
    assert_eq!(first.path, Some(dir.path().join("web/ofen.jpg")));
    assert_eq!(second.path, Some(dir.path().join("web/ofen-1.jpg")));
}

#[test]
fn renaming_a_saved_image_moves_the_old_output_to_the_trash() {
    let dir = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let src = dir.path().join("IMG_1.jpg");
    fs::write(&src, b"original").unwrap();
    let calls = RefCell::new(Vec::new());
    let trash = fake_trash(bin.path().to_path_buf(), &calls);
    let mut s = Session::default();
    s.begin_run();
    s.save_named(&src, &enc(OutFormat::Jpeg, b"v1"), Target::WebSubfolder, ConflictPolicy::Rename, &trash, Some("kachelofen")).unwrap();
    s.begin_run();
    let saved = s
        .save_named(&src, &enc(OutFormat::Jpeg, b"v2"), Target::WebSubfolder, ConflictPolicy::Rename, &trash, Some("kachelofen-gruen"))
        .unwrap();
    assert_eq!(saved.path, Some(dir.path().join("web/kachelofen-gruen.jpg")));
    assert_eq!(saved.old_kept, None);
    assert_eq!(*calls.borrow(), vec![dir.path().join("web/kachelofen.jpg")]);
    assert_eq!(names(&dir.path().join("web")), vec!["kachelofen-gruen.jpg"]);
    assert_eq!(fs::read(&src).unwrap(), b"original");
}

#[test]
fn old_output_stays_and_is_reported_when_the_trash_fails() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("IMG_1.jpg");
    fs::write(&src, b"original").unwrap();
    let failing = |_: &Path| Err("kein Papierkorb".to_string());
    let mut s = Session::default();
    s.begin_run();
    s.save_named(&src, &enc(OutFormat::Jpeg, b"v1"), Target::WebSubfolder, ConflictPolicy::Rename, &failing, Some("alt")).unwrap();
    s.begin_run();
    let saved = s.save_named(&src, &enc(OutFormat::Jpeg, b"v2"), Target::WebSubfolder, ConflictPolicy::Rename, &failing, Some("neu")).unwrap();
    assert_eq!(saved.old_kept, Some(dir.path().join("web/alt.jpg")));
    assert_eq!(names(&dir.path().join("web")), vec!["alt.jpg", "neu.jpg"]);
}

#[test]
fn foreign_files_and_other_sources_outputs_are_never_trashed() {
    let dir = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.jpg"), dir.path().join("b.jpg"));
    fs::write(&a, b"a").unwrap();
    fs::write(&b, b"b").unwrap();
    let calls = RefCell::new(Vec::new());
    let trash = fake_trash(bin.path().to_path_buf(), &calls);
    let mut s = Session::default();
    s.begin_run();
    s.save_named(&a, &enc(OutFormat::Jpeg, b"a1"), Target::WebSubfolder, ConflictPolicy::Rename, &trash, Some("x")).unwrap();
    s.save_named(&b, &enc(OutFormat::Jpeg, b"b1"), Target::WebSubfolder, ConflictPolicy::Rename, &trash, Some("y")).unwrap();
    // Dieselbe Quelle ohne Namensänderung: nichts wandert in den Papierkorb.
    s.begin_run();
    s.save_named(&a, &enc(OutFormat::Jpeg, b"a2"), Target::WebSubfolder, ConflictPolicy::Rename, &trash, Some("x")).unwrap();
    assert!(calls.borrow().is_empty());
    assert_eq!(names(&dir.path().join("web")), vec!["x.jpg", "y.jpg"]);
}

#[test]
fn replace_with_a_new_name_trashes_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let src = dir.path().join("IMG_1.jpg");
    fs::write(&src, b"original").unwrap();
    let calls = RefCell::new(Vec::new());
    let trash = fake_trash(bin.path().to_path_buf(), &calls);
    let mut s = Session::default();
    s.begin_run();
    let saved = s.save_named(&src, &enc(OutFormat::Jpeg, b"neu"), Target::Replace, ConflictPolicy::Rename, &trash, Some("kachelofen")).unwrap();
    assert_eq!(saved.path, Some(dir.path().join("kachelofen.jpg")));
    assert_eq!(*calls.borrow(), vec![src.clone()]);
    assert_eq!(names(dir.path()), vec!["kachelofen.jpg"]);
}

#[test]
fn replace_with_a_new_name_but_no_trash_keeps_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("IMG_1.jpg");
    fs::write(&src, b"original").unwrap();
    let failing = |_: &Path| Err("kein Papierkorb".to_string());
    let mut s = Session::default();
    s.begin_run();
    let saved = s.save_named(&src, &enc(OutFormat::Jpeg, b"neu"), Target::Replace, ConflictPolicy::Rename, &failing, Some("kachelofen")).unwrap();
    assert_eq!(saved.note, Some(SaveNote::NoRecycleBin));
    assert_eq!(saved.path, Some(dir.path().join("kachelofen.jpg")));
    assert_eq!(names(dir.path()), vec!["IMG_1.jpg", "kachelofen.jpg"]);
}

// ---- Fixes aus der Abschlussprüfung ----

#[test]
fn overwrite_never_hits_a_file_that_is_a_source_in_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("IMG_1.jpg"), dir.path().join("kachelofen.jpg"));
    fs::write(&a, b"a").unwrap();
    fs::write(&b, b"eigenes Kundenfoto").unwrap();
    let mut s = Session::default();
    s.protect_sources([a.clone(), b.clone()]);
    s.begin_run();
    let saved = s.save_named(&a, &enc(OutFormat::Jpeg, b"neu"), Target::Suffix, ConflictPolicy::Overwrite, &no_trash, Some("kachelofen")).unwrap();
    assert_eq!(saved.path, Some(dir.path().join("kachelofen-1.jpg")));
    assert_eq!(fs::read(&b).unwrap(), b"eigenes Kundenfoto");
}

#[test]
fn overwrite_never_hits_another_images_output_from_an_earlier_run() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.jpg"), dir.path().join("b.jpg"));
    fs::write(&a, b"a").unwrap();
    fs::write(&b, b"b").unwrap();
    let mut s = Session::default();
    s.begin_run();
    s.save_named(&a, &enc(OutFormat::Jpeg, b"von a"), Target::WebSubfolder, ConflictPolicy::Overwrite, &no_trash, Some("ofen")).unwrap();
    s.begin_run();
    assert!(s.has_foreign_conflict(&dir.path().join("web/ofen.jpg"), &b));
    let saved = s.save_named(&b, &enc(OutFormat::Jpeg, b"von b"), Target::WebSubfolder, ConflictPolicy::Overwrite, &no_trash, Some("ofen")).unwrap();
    assert_eq!(saved.path, Some(dir.path().join("web/ofen-1.jpg")));
    assert_eq!(fs::read(dir.path().join("web/ofen.jpg")).unwrap(), b"von a");
}

#[test]
fn without_a_new_name_earlier_outputs_are_never_trashed() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.jpg");
    fs::write(&src, b"original").unwrap();
    let mut s = Session::default();
    s.begin_run();
    s.save(&src, &enc(OutFormat::Jpeg, b"jpg"), Target::WebSubfolder, ConflictPolicy::Skip, &no_trash).unwrap();
    s.begin_run();
    // Formatwechsel ohne KI-Namen: beide Fassungen bleiben (`no_trash` würde sonst auslösen).
    s.save(&src, &enc(OutFormat::Webp, b"webp"), Target::WebSubfolder, ConflictPolicy::Skip, &no_trash).unwrap();
    assert_eq!(names(&dir.path().join("web")), vec!["a.jpg", "a.webp"]);
}
