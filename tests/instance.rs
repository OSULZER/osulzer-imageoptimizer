use osulzer_imageoptimizer::instance::{Role, acquire};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

static N: AtomicU32 = AtomicU32::new(0);

fn unique_name() -> String {
    format!("bildoptimierer-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

#[test]
fn second_instance_hands_its_paths_to_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    let Role::Primary(rx) = acquire(dir.path(), &name, &[]).unwrap() else {
        panic!("erste Instanz muss primär sein");
    };
    let paths = vec![PathBuf::from(r"C:\Fotos\Müller & Söhne\bild 1.jpg"), PathBuf::from(r"\\nas\freigabe\ä.png")];
    assert!(matches!(acquire(dir.path(), &name, &paths).unwrap(), Role::Secondary));
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), paths);
}

#[test]
fn empty_handover_arrives_as_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    let Role::Primary(rx) = acquire(dir.path(), &name, &[]).unwrap() else {
        panic!("erste Instanz muss primär sein");
    };
    assert!(matches!(acquire(dir.path(), &name, &[]).unwrap(), Role::Secondary));
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), Vec::<PathBuf>::new());
}

#[test]
fn twenty_simultaneous_starts_end_up_in_one_instance() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    let handles: Vec<_> = (0..20)
        .map(|i| {
            let (dir, name) = (dir.path().to_path_buf(), name.clone());
            std::thread::spawn(move || {
                let own = PathBuf::from(format!(r"C:\bilder\bild {i}.jpg"));
                (own.clone(), acquire(&dir, &name, &[own]).unwrap())
            })
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut receivers = Vec::new();
    for h in handles {
        let (own, role) = h.join().unwrap();
        if let Role::Primary(rx) = role {
            seen.insert(own);
            receivers.push(rx);
        }
    }
    assert_eq!(receivers.len(), 1, "genau eine primäre Instanz");
    while seen.len() < 20 {
        let batch = receivers[0].recv_timeout(Duration::from_secs(5)).expect("alle Pfade müssen ankommen");
        seen.extend(batch);
    }
    let expected: BTreeSet<_> = (0..20).map(|i| PathBuf::from(format!(r"C:\bilder\bild {i}.jpg"))).collect();
    assert_eq!(seen, expected);
}
