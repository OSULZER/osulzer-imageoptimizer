//! KI-Lauf im Hintergrund: beschreibt die Bilder mit höchstens `AI_PARALLEL` gleichzeitigen Anfragen.

use crate::ai::{AiConfig, describe};
use crate::engine::ai_input_jpeg;
use crate::texts::ImageTexts;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

pub enum AiEvent {
    Started(u64),
    Finished(u64, Result<ImageTexts, String>),
    AllDone,
}

pub struct AiHandle {
    pub events: Receiver<AiEvent>,
    /// Auf `true` setzen, um noch nicht begonnene Bilder auszulassen.
    pub cancel: Arc<AtomicBool>,
    /// Bilder, die inzwischen aus der Liste entfernt wurden; sie werden übersprungen.
    pub skip: SkipSet,
}

pub type SkipSet = Arc<Mutex<HashSet<u64>>>;

impl AiHandle {
    /// Dieses Bild nicht mehr an die KI schicken (falls es noch nicht begonnen hat).
    pub fn skip(&self, id: u64) {
        self.skip.lock().unwrap_or_else(|p| p.into_inner()).insert(id);
    }
}

/// Lokale Modelle bearbeiten Anfragen meist nacheinander; mehr bringt kaum Tempo.
pub const AI_PARALLEL: usize = 2;

pub fn start_ai_run(
    items: Vec<(u64, PathBuf)>,
    cfg: AiConfig,
    hint: String,
    wake: impl Fn() + Send + Sync + 'static,
) -> AiHandle {
    start_ai_run_with_cancel(items, cfg, hint, Arc::new(AtomicBool::new(false)), SkipSet::default(), wake)
}

/// Wie `start_ai_run`, mit vorgegebenem Abbruch-Schalter.
pub fn start_ai_run_with_cancel(
    items: Vec<(u64, PathBuf)>,
    cfg: AiConfig,
    hint: String,
    cancel: Arc<AtomicBool>,
    skip: SkipSet,
    wake: impl Fn() + Send + Sync + 'static,
) -> AiHandle {
    let (tx, events) = mpsc::channel();
    let queue = Arc::new(Mutex::new(items.into_iter()));
    let wake = Arc::new(wake);
    let (cfg, hint) = (Arc::new(cfg), Arc::new(hint));
    let workers: Vec<_> = (0..AI_PARALLEL)
        .map(|_| {
            let (queue, tx, wake, cancel, cfg, hint, skip) =
                (queue.clone(), tx.clone(), wake.clone(), cancel.clone(), cfg.clone(), hint.clone(), skip.clone());
            std::thread::spawn(move || {
                loop {
                    if cancel.load(Ordering::SeqCst) {
                        return;
                    }
                    let next = queue.lock().unwrap_or_else(|p| p.into_inner()).next();
                    let Some((id, path)) = next else { return };
                    if skip.lock().unwrap_or_else(|p| p.into_inner()).contains(&id) {
                        continue;
                    }
                    let _ = tx.send(AiEvent::Started(id));
                    wake();
                    let result = std::panic::catch_unwind(|| {
                        let jpeg = ai_input_jpeg(&path).map_err(|e| e.to_string())?;
                        describe(&cfg, &jpeg, &hint).map_err(|e| e.to_string())
                    })
                    .unwrap_or_else(|_| Err(crate::i18n::t("ai.error.internal").to_string()));
                    let _ = tx.send(AiEvent::Finished(id, result));
                    wake();
                }
            })
        })
        .collect();
    let cancel_out = cancel.clone();
    std::thread::spawn(move || {
        for worker in workers {
            let _ = worker.join();
        }
        let _ = tx.send(AiEvent::AllDone);
        wake();
    });
    AiHandle { events, cancel: cancel_out, skip }
}
