mod common;

use osulzer_imageoptimizer::ai::AiConfig;
use osulzer_imageoptimizer::ui::ai_worker::{AI_PARALLEL, AiEvent, start_ai_run, start_ai_run_with_cancel};
use common::{chat_reply, fake_server};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const GOOD: &str = r#"{"dateiname":"ofen","titel":"Ofen","alt_text":"Ein Ofen","beschreibung":"Ein Ofen."}"#;

fn collect(events: std::sync::mpsc::Receiver<AiEvent>) -> HashMap<u64, Result<String, String>> {
    let mut finished = HashMap::new();
    loop {
        match events.recv_timeout(Duration::from_secs(60)).expect("KI-Lauf muss mit AllDone enden") {
            AiEvent::Started(_) => {}
            AiEvent::Finished(id, r) => {
                finished.insert(id, r.map(|t| t.alt));
            }
            AiEvent::AllDone => return finished,
        }
    }
}

#[test]
fn describes_images_in_parallel_and_reports_broken_ones() {
    let dir = tempfile::tempdir().unwrap();
    let (now, max) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (n, m) = (now.clone(), max.clone());
    let base = fake_server(move |_| {
        let current = n.fetch_add(1, Ordering::SeqCst) + 1;
        m.fetch_max(current, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        n.fetch_sub(1, Ordering::SeqCst);
        (200, chat_reply(GOOD))
    });
    let mut items: Vec<(u64, std::path::PathBuf)> =
        (0..4).map(|i| (i, common::write_jpeg(dir.path(), &format!("b{i}.jpg"), 300, 200))).collect();
    let broken = dir.path().join("kaputt.jpg");
    std::fs::write(&broken, "kein Bild").unwrap();
    items.push((9, broken));
    let cfg = AiConfig { base_url: base, model: "m".into(), api_key: None, lang: osulzer_imageoptimizer::i18n::Lang::De, protocol: osulzer_imageoptimizer::ai::provider::Protocol::OpenAiCompatible };
    let handle = start_ai_run(items, cfg, "Hinweis".into(), || {});
    let finished = collect(handle.events);
    assert_eq!(finished.len(), 5);
    for i in 0..4 {
        assert_eq!(finished[&i].as_deref(), Ok("Ein Ofen"));
    }
    assert!(finished[&9].is_err());
    assert!(max.load(Ordering::SeqCst) <= AI_PARALLEL, "zu viele gleichzeitige Anfragen");
    assert!(max.load(Ordering::SeqCst) >= 2, "Anfragen liefen nicht parallel");
}

#[test]
fn cancel_before_start_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let base = fake_server(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        (200, chat_reply(GOOD))
    });
    let items = vec![(1, common::write_jpeg(dir.path(), "a.jpg", 300, 200))];
    let cfg = AiConfig { base_url: base, model: "m".into(), api_key: None, lang: osulzer_imageoptimizer::i18n::Lang::De, protocol: osulzer_imageoptimizer::ai::provider::Protocol::OpenAiCompatible };
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let handle = start_ai_run_with_cancel(items, cfg, String::new(), cancel, Default::default(), || {});
    let finished = collect(handle.events);
    assert!(finished.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn removed_images_are_not_sent_to_the_ai() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let base = fake_server(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        (200, chat_reply(GOOD))
    });
    let items: Vec<(u64, std::path::PathBuf)> =
        (0..3).map(|i| (i, common::write_jpeg(dir.path(), &format!("b{i}.jpg"), 300, 200))).collect();
    let cfg = AiConfig { base_url: base, model: "m".into(), api_key: None, lang: osulzer_imageoptimizer::i18n::Lang::De, protocol: osulzer_imageoptimizer::ai::provider::Protocol::OpenAiCompatible };
    let skip: Arc<std::sync::Mutex<std::collections::HashSet<u64>>> = Default::default();
    skip.lock().unwrap().extend([0, 2]);
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handle = start_ai_run_with_cancel(items, cfg, String::new(), cancel, skip, || {});
    let finished = collect(handle.events);
    assert_eq!(finished.keys().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
