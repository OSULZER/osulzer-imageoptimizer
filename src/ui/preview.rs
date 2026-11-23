//! Vorschau im Hintergrund: rechnet nur die jeweils neueste Anfrage, kurz nach der letzten Änderung.

use crate::engine::{PreviewResult, preview};
use crate::settings::Settings;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

/// Wartezeit nach der letzten Änderung, bevor gerechnet wird.
pub const DEBOUNCE: Duration = Duration::from_millis(250);
/// Größte Kantenlänge der Vorschaubilder.
pub const MAX_TEX: u32 = 4096;

struct Request {
    id: u64,
    path: PathBuf,
    settings: Settings,
}

pub type PreviewAnswer = (u64, Settings, Result<PreviewResult, String>);

pub struct PreviewWorker {
    requests: Sender<Request>,
    answers: Receiver<PreviewAnswer>,
}

impl PreviewWorker {
    /// `wake` wird nach jedem fertigen Ergebnis aufgerufen.
    pub fn new(wake: impl Fn() + Send + 'static) -> Self {
        let (requests, incoming) = mpsc::channel::<Request>();
        let (outgoing, answers) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(mut request) = incoming.recv() {
                // Weitere Änderungen abwarten; nur die letzte zählt.
                loop {
                    match incoming.recv_timeout(DEBOUNCE) {
                        Ok(newer) => request = newer,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                let result = std::panic::catch_unwind(|| preview(&request.path, &request.settings, MAX_TEX))
                    .unwrap_or_else(|_| Err(crate::engine::EngineError::Encode(crate::i18n::t("error.internal_image").into())))
                    .map_err(|e| e.to_string());
                if outgoing.send((request.id, request.settings, result)).is_err() {
                    return;
                }
                wake();
            }
        });
        PreviewWorker { requests, answers }
    }

    pub fn request(&mut self, id: u64, path: PathBuf, s: Settings) {
        let _ = self.requests.send(Request { id, path, settings: s });
    }

    pub fn poll(&mut self) -> Option<PreviewAnswer> {
        let mut newest = None;
        while let Ok(answer) = self.answers.try_recv() {
            newest = Some(answer);
        }
        newest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Format;
    use std::time::Instant;

    fn write_jpeg(dir: &std::path::Path) -> PathBuf {
        let p = dir.join("a.jpg");
        image::RgbImage::from_fn(300, 200, |x, y| image::Rgb([x as u8, y as u8, 90])).save(&p).unwrap();
        p
    }

    fn settings(quality: u8) -> Settings {
        Settings { max_edge: Some(150), format: Format::Jpeg, quality, max_kb: None }
    }

    fn wait(worker: &mut PreviewWorker) -> PreviewAnswer {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(answer) = worker.poll() {
                return answer;
            }
            assert!(Instant::now() < deadline, "keine Vorschau erhalten");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn only_the_newest_of_quick_requests_is_computed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_jpeg(dir.path());
        let mut worker = PreviewWorker::new(|| {});
        for q in [10, 20, 30, 40, 77] {
            worker.request(1, path.clone(), settings(q));
        }
        let (id, s, result) = wait(&mut worker);
        assert_eq!((id, s.quality), (1, 77));
        assert_eq!(result.unwrap().encoded.width, 150);
        std::thread::sleep(DEBOUNCE * 2);
        assert!(worker.poll().is_none(), "ältere Anfragen dürfen nicht nachkommen");
    }

    #[test]
    fn errors_arrive_as_text() {
        crate::i18n::set_lang(crate::i18n::Lang::De);
        let dir = tempfile::tempdir().unwrap();
        let mut worker = PreviewWorker::new(|| {});
        worker.request(7, dir.path().join("fehlt.jpg"), settings(80));
        let (id, _, result) = wait(&mut worker);
        assert_eq!(id, 7);
        assert!(result.err().unwrap().starts_with("Datei nicht lesbar"));
    }
}
