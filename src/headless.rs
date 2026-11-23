//! Bildoptimierung ohne Fenster: gemeinsamer Kern für Kommandozeile und MCP-Server.

use crate::engine::{embed_texts, probe, process};
use crate::i18n::{Lang, t};
use crate::job::{ConflictPolicy, Session, planned_path_named, system_trash};
use crate::naming::{NameRules, final_stem};
use crate::settings::{Settings, SrcFormat, Target};
use crate::state::message_for;
use crate::texts::ImageTexts;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Wohin die Ergebnisse geschrieben werden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    Target(Target),
    /// Eigener Ordner; vorhandene Dateien werden nie überschrieben.
    Folder(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Request {
    pub files: Vec<PathBuf>,
    pub settings: Settings,
    pub destination: Destination,
    pub rules: NameRules,
    /// Texte je Datei, Schlüssel = `path_key(datei)`; `file_stem` dient als neuer Name.
    pub texts: HashMap<String, ImageTexts>,
    pub lang: Lang,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileReport {
    pub source: PathBuf,
    pub output: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub message: Option<String>,
    pub error: Option<String>,
}

impl FileReport {
    fn failed(source: &Path, error: String) -> Self {
        FileReport {
            source: source.to_path_buf(),
            output: None,
            width: 0,
            height: 0,
            bytes_before: 0,
            bytes_after: 0,
            message: None,
            error: Some(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InfoReport {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub format: String,
}

/// Vergleichsschlüssel für Pfade: absolut, Backslashes, klein (Windows unterscheidet keine Groß-/Kleinschreibung).
pub fn path_key(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    abs.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// Texte, zu denen keine der Dateien passt (z. B. Tippfehler im Pfad).
pub fn unmatched_texts(req: &Request) -> Vec<String> {
    let files: std::collections::HashSet<String> = req.files.iter().map(|f| path_key(f)).collect();
    let mut keys: Vec<String> = req.texts.keys().filter(|k| !files.contains(*k)).cloned().collect();
    keys.sort();
    keys
}

fn format_name(f: SrcFormat) -> &'static str {
    match f {
        SrcFormat::Jpeg => "jpeg",
        SrcFormat::Png => "png",
        SrcFormat::Webp => "webp",
        SrcFormat::Bmp => "bmp",
        SrcFormat::Tiff => "tiff",
    }
}

/// Abmessungen, Größe und Format eines Bildes.
pub fn info(path: &Path) -> Result<InfoReport, String> {
    let p = probe(path).map_err(|e| e.to_string())?;
    Ok(InfoReport { path: path.to_path_buf(), width: p.width, height: p.height, bytes: p.bytes, format: format_name(p.format).into() })
}

/// Verarbeitet alle Dateien der Reihe nach; ein Fehler betrifft nur seine Datei.
pub fn optimize(req: &Request) -> Vec<FileReport> {
    // Doppelt angegebene Dateien nur einmal verarbeiten.
    let mut seen = std::collections::HashSet::new();
    let files: Vec<&PathBuf> = req.files.iter().filter(|f| seen.insert(path_key(f))).collect();
    // Laufnummern über alle lesbaren Dateien, wie in der Oberfläche.
    let readable: Vec<bool> = files.iter().map(|f| f.is_absolute() && probe(f).is_ok()).collect();
    let count = readable.iter().filter(|r| **r).count();
    let mut session = Session::default();
    session.protect_sources(files.iter().map(|f| (*f).clone()));
    let mut number = 0;
    files
        .into_iter()
        .zip(readable)
        .map(|(src, readable)| {
            if !src.is_absolute() {
                return FileReport::failed(src, t("headless.error.relative").into());
            }
            if readable {
                number += 1;
            }
            optimize_one(req, &mut session, src, (number, count)).unwrap_or_else(|e| FileReport::failed(src, e))
        })
        .collect()
}

fn optimize_one(req: &Request, session: &mut Session, src: &Path, number: (usize, usize)) -> Result<FileReport, String> {
    let bytes_before = std::fs::metadata(src).map_err(|e| e.to_string())?.len();
    let mut encoded = process(src, &req.settings).map_err(|e| e.to_string())?;
    let texts = req.texts.get(&path_key(src)).filter(|t| !t.is_empty());
    if let Some(texts) = texts {
        encoded.data = embed_texts(&encoded.data, encoded.format, texts, req.lang).map_err(|e| e.to_string())?;
        encoded.source_untouched = false;
    }
    let original = src.file_stem().unwrap_or_default().to_string_lossy();
    let ai = texts.map(|t| t.file_stem.as_str());
    let stem = final_stem(&original, ai, &req.rules, Some(number), req.lang);
    let (output, bytes_after, note) = match &req.destination {
        Destination::Target(target) => {
            let saved = session
                .save_named(src, &encoded, *target, ConflictPolicy::Rename, &system_trash, stem.as_deref())
                .map_err(|e| e.to_string())?;
            (saved.path, saved.bytes, saved.note)
        }
        Destination::Folder(dir) => {
            let name = planned_path_named(src, Target::WebSubfolder, encoded.format, stem.as_deref());
            let name = name.file_name().unwrap_or_default();
            let path = write_new(dir, &PathBuf::from(name), &encoded.data)?;
            (Some(path), encoded.data.len() as u64, None)
        }
    };
    Ok(FileReport {
        source: src.to_path_buf(),
        output,
        width: encoded.width,
        height: encoded.height,
        bytes_before,
        bytes_after,
        message: message_for(encoded.note, note),
        error: None,
    })
}

/// Schreibt `data` als `dir/name`; gibt es die Datei schon, als `name-1`, `name-2`, …
/// Über eine temporäre Datei, damit nie eine halbe Datei liegen bleibt.
fn write_new(dir: &Path, name: &Path, data: &[u8]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stem = name.file_stem().unwrap_or_default().to_string_lossy();
    let ext = name.extension().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{stem}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    let candidates = std::iter::once(dir.join(format!("{stem}.{ext}"))).chain((1u32..).map(|n| dir.join(format!("{stem}-{n}.{ext}"))));
    for target in candidates.take(10_000) {
        // Hard-Link schlägt fehl, wenn das Ziel existiert: so wird nie überschrieben.
        match std::fs::hard_link(&tmp, &target) {
            Ok(()) => {
                let _ = std::fs::remove_file(&tmp);
                return Ok(target);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            // Laufwerke ohne Hard-Links (z. B. FAT, manche Netzlaufwerke): vorher prüfen, dann umbenennen.
            Err(_) if !target.exists() => {
                return match std::fs::rename(&tmp, &target) {
                    Ok(()) => Ok(target),
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp);
                        Err(e.to_string())
                    }
                };
            }
            Err(_) => continue,
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(t("headless.error.no_free_name").into())
}
