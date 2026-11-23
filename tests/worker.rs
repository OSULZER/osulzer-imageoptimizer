mod common;

use osulzer_imageoptimizer::job::{ConflictPolicy, Session};
use osulzer_imageoptimizer::settings::{Format, Settings, Target};
use osulzer_imageoptimizer::ui::worker::{RunEvent, RunItem, start_run};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn settings() -> Settings {
    Settings { max_edge: Some(100), format: Format::Jpeg, quality: 80, max_kb: None }
}

fn item(id: u64, path: PathBuf) -> RunItem {
    RunItem { id, path, settings: settings(), texts: None, lang: osulzer_imageoptimizer::i18n::Lang::De, stem: None, delete_source: false }
}

/// Sammelt alle Ereignisse bis `AllDone`.
fn collect(events: std::sync::mpsc::Receiver<RunEvent>) -> (Vec<u64>, HashMap<u64, Result<osulzer_imageoptimizer::state::Outcome, String>>) {
    let (mut started, mut finished) = (Vec::new(), HashMap::new());
    loop {
        match events.recv_timeout(Duration::from_secs(60)).expect("Lauf muss mit AllDone enden") {
            RunEvent::Started(id) => started.push(id),
            RunEvent::Finished(id, result) => {
                finished.insert(id, result);
            }
            RunEvent::AllDone => return (started, finished),
        }
    }
}

#[test]
fn broken_image_does_not_stop_the_run() {
    osulzer_imageoptimizer::i18n::set_lang(osulzer_imageoptimizer::i18n::Lang::De);
    let dir = tempfile::tempdir().unwrap();
    let mut items: Vec<RunItem> =
        (0..5).map(|i| item(i, common::write_jpeg(dir.path(), &format!("bild{i}.jpg"), 400, 300))).collect();
    let broken = dir.path().join("kaputt.jpg");
    std::fs::write(&broken, b"kein Bild").unwrap();
    items.push(item(5, broken));

    let session = Arc::new(Mutex::new(Session::default()));
    let handle = start_run(items, Target::WebSubfolder, ConflictPolicy::Overwrite, session, || {});
    let (started, finished) = collect(handle.events);

    assert_eq!(started.len(), 6);
    assert_eq!(finished.len(), 6);
    for i in 0..5 {
        let outcome = finished[&i].as_ref().unwrap();
        assert_eq!((outcome.width, outcome.height), (100, 75));
        assert_eq!(outcome.path, Some(dir.path().join("web").join(format!("bild{i}.jpg"))));
        assert_eq!(outcome.bytes, std::fs::metadata(outcome.path.as_ref().unwrap()).unwrap().len());
    }
    assert_eq!(finished[&5].as_ref().unwrap_err(), "Kein unterstütztes Bildformat");
    assert_eq!(std::fs::read_dir(dir.path().join("web")).unwrap().count(), 5);
}

#[test]
fn blocked_target_folder_fails_only_that_image() {
    osulzer_imageoptimizer::i18n::set_lang(osulzer_imageoptimizer::i18n::Lang::De);
    let good = tempfile::tempdir().unwrap();
    let blocked = tempfile::tempdir().unwrap();
    std::fs::write(blocked.path().join("web"), b"Datei statt Ordner").unwrap();
    let items = vec![
        item(1, common::write_jpeg(good.path(), "a.jpg", 400, 300)),
        item(2, common::write_jpeg(blocked.path(), "b.jpg", 400, 300)),
        item(3, common::write_jpeg(good.path(), "c.jpg", 400, 300)),
    ];
    let session = Arc::new(Mutex::new(Session::default()));
    let handle = start_run(items, Target::WebSubfolder, ConflictPolicy::Overwrite, session, || {});
    let (_, finished) = collect(handle.events);
    assert!(finished[&1].is_ok());
    assert!(finished[&3].is_ok());
    assert!(finished[&2].as_ref().unwrap_err().starts_with("Ziel nicht beschreibbar"));
}

#[test]
fn cancelled_run_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let items: Vec<RunItem> = (0..4).map(|i| item(i, common::write_jpeg(dir.path(), &format!("b{i}.jpg"), 400, 300))).collect();
    let session = Arc::new(Mutex::new(Session::default()));
    // Die Sperre hält den Lauf an, bis der Abbruch gesetzt ist.
    let guard = session.lock().unwrap();
    let handle = start_run(items, Target::WebSubfolder, ConflictPolicy::Overwrite, session.clone(), || {});
    handle.cancel.store(true, Ordering::SeqCst);
    drop(guard);
    let (started, finished) = collect(handle.events);
    assert!(started.is_empty());
    assert!(finished.is_empty());
    assert!(!dir.path().join("web").exists());
}

#[test]
fn wake_is_called_for_every_event() {
    let dir = tempfile::tempdir().unwrap();
    let items = vec![item(1, common::write_jpeg(dir.path(), "a.jpg", 200, 100))];
    let session = Arc::new(Mutex::new(Session::default()));
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = wakes.clone();
    let handle = start_run(items, Target::WebSubfolder, ConflictPolicy::Overwrite, session, move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    collect(handle.events);
    // Started, Finished, AllDone
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(wakes.load(Ordering::SeqCst), 3);
}

#[test]
fn texts_rename_the_output_and_end_up_in_the_xmp() {
    let dir = tempfile::tempdir().unwrap();
    let mut it = item(1, common::write_jpeg(dir.path(), "IMG_1.jpg", 400, 300));
    it.texts = Some(osulzer_imageoptimizer::texts::ImageTexts {
        file_stem: "kachelofen-gruen".into(),
        title: "Grüner Kachelofen".into(),
        alt: "Ein grüner Kachelofen".into(),
        description: "Beschreibung".into(),
    });
    // Den endgültigen Namen berechnet die Oberfläche (Namensregeln bzw. KI-Name) und gibt ihn mit.
    it.stem = Some("kachelofen-gruen".into());
    let session = Arc::new(Mutex::new(Session::default()));
    let handle = start_run(vec![it], Target::WebSubfolder, ConflictPolicy::Overwrite, session, || {});
    let (_, finished) = collect(handle.events);
    let outcome = finished[&1].as_ref().unwrap();
    let out = dir.path().join("web").join("kachelofen-gruen.jpg");
    assert_eq!(outcome.path, Some(out.clone()));
    let data = std::fs::read(&out).unwrap();
    let text = String::from_utf8_lossy(&data);
    assert!(text.contains("Ein grüner Kachelofen"));
    assert!(text.contains("Iptc4xmpCore:AltTextAccessibility"));
    assert!(!dir.path().join("web").join("IMG_1.jpg").exists());
}

#[test]
fn stem_from_rules_is_used_for_saving() {
    let dir = tempfile::tempdir().unwrap();
    let mut it = item(1, common::write_jpeg(dir.path(), "IMG_1.jpg", 400, 300));
    it.stem = Some("kunde-01".into());
    let session = Arc::new(Mutex::new(Session::default()));
    let handle = start_run(vec![it], Target::WebSubfolder, ConflictPolicy::Overwrite, session, || {});
    let (_, finished) = collect(handle.events);
    let outcome = finished[&1].as_ref().unwrap();
    assert_eq!(outcome.path.as_deref(), Some(dir.path().join("web").join("kunde-01.jpg").as_path()));
}

#[test]
fn source_goes_to_the_trash_only_when_asked_and_saved_elsewhere() {
    use osulzer_imageoptimizer::ui::worker::retire_source;
    use std::cell::RefCell;
    use std::path::Path;
    let src = PathBuf::from("C:/x/a.png");
    let out = PathBuf::from("C:/x/web/a.webp");
    let trashed = RefCell::new(Vec::new());
    let trash = |p: &Path| {
        trashed.borrow_mut().push(p.to_path_buf());
        Ok(())
    };
    let mut it = item(1, src.clone());
    // Nicht gewünscht.
    assert_eq!(retire_source(&it, Target::WebSubfolder, Some(&out), &trash), None);
    it.delete_source = true;
    // Übersprungen: kein Ergebnis, die Quelle bleibt.
    assert_eq!(retire_source(&it, Target::WebSubfolder, None, &trash), None);
    // „Original ersetzen“ räumt selbst auf.
    assert_eq!(retire_source(&it, Target::Replace, Some(&out), &trash), None);
    // Ergebnis ist die Quelle selbst.
    assert_eq!(retire_source(&it, Target::Suffix, Some(&src), &trash), None);
    assert!(trashed.borrow().is_empty());
    assert_eq!(retire_source(&it, Target::WebSubfolder, Some(&out), &trash), Some(true));
    assert_eq!(*trashed.borrow(), vec![src]);
    // Kein Papierkorb: die Quelle bleibt und das wird gemeldet.
    assert_eq!(retire_source(&it, Target::WebSubfolder, Some(&out), &|_| Err("x".into())), Some(false));
}
