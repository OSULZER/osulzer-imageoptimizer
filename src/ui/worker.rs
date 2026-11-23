//! Stapellauf im Hintergrund: verarbeitet die Bilder parallel und meldet den Fortschritt.

use crate::engine::{embed_texts, process};
use crate::job::{ConflictPolicy, SaveNote, Session, system_trash};
use crate::settings::{Settings, Target};
use crate::state::{Outcome, message_for, old_file_kept_message};
use crate::texts::ImageTexts;
use rayon::prelude::*;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard};

pub struct RunItem {
    pub id: u64,
    pub path: PathBuf,
    pub settings: Settings,
    /// Neuer Dateiname und Texte für die Metadaten.
    pub texts: Option<ImageTexts>,
    /// Sprache der Texte (für das XMP).
    pub lang: crate::i18n::Lang,
    /// Endgültiger Dateiname ohne Endung aus Namensregeln bzw. KI; `None` = wie bisher.
    pub stem: Option<String>,
    /// Quelldatei nach dem Speichern in den Papierkorb legen.
    pub delete_source: bool,
}

/// Legt die Quelle in den Papierkorb, wenn das gewünscht ist und das Ergebnis woanders liegt.
/// `None`: nichts zu tun. `Some(false)`: Papierkorb nicht möglich, die Quelle bleibt.
pub fn retire_source(
    item: &RunItem,
    target: Target,
    saved: Option<&std::path::Path>,
    trash: &dyn Fn(&std::path::Path) -> Result<(), String>,
) -> Option<bool> {
    // „Original ersetzen“ räumt die Quelle selbst weg; ohne Ergebnis (übersprungen) bleibt sie.
    let saved = saved?;
    if !item.delete_source || target == Target::Replace || crate::headless::path_key(saved) == crate::headless::path_key(&item.path) {
        return None;
    }
    Some(trash(&item.path).is_ok())
}

pub enum RunEvent {
    Started(u64),
    Finished(u64, Result<Outcome, String>),
    AllDone,
}

pub struct RunHandle {
    pub events: Receiver<RunEvent>,
    /// Auf `true` setzen, um noch nicht begonnene Bilder auszulassen.
    pub cancel: Arc<AtomicBool>,
}

/// Startet den Lauf. `wake` wird nach jedem Ereignis aufgerufen, damit das Fenster neu zeichnet.
pub fn start_run(
    items: Vec<RunItem>,
    target: Target,
    policy: ConflictPolicy,
    session: Arc<Mutex<Session>>,
    wake: impl Fn() + Send + Sync + 'static,
) -> RunHandle {
    let (tx, events) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancelled = cancel.clone();
    std::thread::spawn(move || {
        let emit = |event: RunEvent| {
            let _ = tx.send(event);
            wake();
        };
        lock(&session).begin_run();
        let work = || {
            items.par_iter().for_each(|item| {
                if cancelled.load(Ordering::SeqCst) {
                    return;
                }
                emit(RunEvent::Started(item.id));
                // Ein Absturz in einem Bild darf den Lauf nicht beenden.
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| run_one(item, target, policy, &session)))
                    .unwrap_or_else(|_| Err(crate::i18n::t("error.internal_image").to_string()));
                emit(RunEvent::Finished(item.id, result));
            });
        };
        // Eigener, begrenzter Pool: jedes Bild belegt während der Verarbeitung viel Speicher.
        match rayon::ThreadPoolBuilder::new().num_threads(worker_threads()).build() {
            Ok(pool) => pool.install(work),
            Err(_) => work(),
        }
        emit(RunEvent::AllDone);
    });
    RunHandle { events, cancel }
}

/// Höchstens so viele Bilder gleichzeitig, auch auf Rechnern mit sehr vielen Kernen.
const MAX_PARALLEL: usize = 8;

fn worker_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(MAX_PARALLEL)
}

fn lock(session: &Mutex<Session>) -> MutexGuard<'_, Session> {
    session.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn run_one(item: &RunItem, target: Target, policy: ConflictPolicy, session: &Mutex<Session>) -> Result<Outcome, String> {
    let mut encoded = process(&item.path, &item.settings).map_err(|e| e.to_string())?;
    let texts = item.texts.as_ref().filter(|t| !t.is_empty());
    if let Some(texts) = texts {
        encoded.data = embed_texts(&encoded.data, encoded.format, texts, item.lang).map_err(|e| e.to_string())?;
        encoded.source_untouched = false;
    }
    let stem = item.stem.as_deref().filter(|s| !s.is_empty());
    let saved = lock(session)
        .save_named(&item.path, &encoded, target, policy, &system_trash, stem)
        .map_err(|e| e.to_string())?;
    let mut message = message_for(encoded.note, saved.note);
    if let Some(old) = &saved.old_kept {
        let kept = old_file_kept_message(old);
        message = Some(match message {
            Some(m) => format!("{m}; {kept}"),
            None => kept,
        });
    }
    let source_gone = retire_source(item, target, saved.path.as_deref(), &system_trash);
    if source_gone == Some(false) {
        let kept = crate::i18n::t("msg.no_recycle_bin").to_string();
        message = Some(match message {
            Some(m) => format!("{m}; {kept}"),
            None => kept,
        });
    }
    Ok(Outcome {
        path: saved.path,
        width: encoded.width,
        height: encoded.height,
        bytes: saved.bytes,
        message,
        replaced: source_gone == Some(true)
            || (target == Target::Replace && matches!(saved.note, None | Some(SaveNote::SavedBeside))),
    })
}
