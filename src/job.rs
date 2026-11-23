//! Auftragslogik: Zielpfade, Namenskonflikte, sicheres Schreiben, Ersetzen mit Papierkorb.

use crate::engine::Encoded;
use crate::settings::{OutFormat, Target};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

/// Was mit einer bereits vorhandenen fremden Zieldatei geschieht.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictPolicy {
    Overwrite,
    Rename,
    Skip,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveNote {
    Skipped,
    NoRecycleBin,
    Unchanged,
    /// Original im Papierkorb, der Name war danach belegt: Ergebnis daneben gespeichert.
    SavedBeside,
}

#[derive(Debug)]
pub struct Saved {
    /// Wohin geschrieben wurde; `None`, wenn übersprungen.
    pub path: Option<PathBuf>,
    pub bytes: u64,
    pub note: Option<SaveNote>,
    /// Frühere eigene Ausgabe dieses Bildes, die nicht in den Papierkorb konnte.
    pub old_kept: Option<PathBuf>,
}

impl Saved {
    fn new(path: Option<PathBuf>, bytes: u64, note: Option<SaveNote>) -> Self {
        Saved { path, bytes, note, old_kept: None }
    }
}

#[derive(Debug)]
pub enum JobError {
    Io(std::io::Error),
    /// Original im Papierkorb, Ergebnis liegt noch unter diesem temporären Namen.
    Stranded(PathBuf, std::io::Error),
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobError::Io(e) => f.write_str(&crate::i18n::tf("job.error.io", &[("e", e)])),
            JobError::Stranded(tmp, e) => {
                f.write_str(&crate::i18n::tf("job.error.stranded", &[("e", e), ("path", &tmp.display())]))
            }
        }
    }
}

impl std::error::Error for JobError {}

/// Zielpfad ohne Rücksicht auf vorhandene Dateien.
pub fn planned_path(src: &Path, target: Target, fmt: OutFormat) -> PathBuf {
    planned_path_named(src, target, fmt, None)
}

/// Zielpfad mit optional neuem Dateinamen (ohne Endung). Weicht er vom Namen der Quelle ab,
/// entfällt bei „Daneben mit Zusatz“ das „-web“.
pub fn planned_path_named(src: &Path, target: Target, fmt: OutFormat, stem: Option<&str>) -> PathBuf {
    let dir = src.parent().unwrap_or(Path::new(""));
    let original = src.file_stem().unwrap_or_default().to_string_lossy();
    let new = stem.map(str::trim).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case(&original));
    let name = new.unwrap_or(&original);
    let ext = fmt.extension();
    match target {
        Target::WebSubfolder => dir.join("web").join(format!("{name}.{ext}")),
        Target::Suffix if new.is_some() => dir.join(format!("{name}.{ext}")),
        Target::Suffix => dir.join(format!("{name}-web.{ext}")),
        Target::Replace => dir.join(format!("{name}.{ext}")),
    }
}

/// Schlüssel für Pfadvergleiche; Windows unterscheidet keine Groß-/Kleinschreibung.
fn key(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// Hängt `-1`, `-2`, … vor die Endung, bis `blocked` den Pfad freigibt.
fn next_free(path: &Path, blocked: impl Fn(&Path) -> bool) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new(""));
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = path.extension().unwrap_or_default().to_string_lossy();
    (1u32..)
        .map(|n| dir.join(format!("{stem}-{n}.{ext}")))
        .find(|candidate| !blocked(candidate))
        .expect("a free counter always exists")
}

static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Name einer temporären Datei im Ordner des Ziels.
fn temp_path(path: &Path) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}.tmp-{}-{n}", std::process::id()))
}

/// Schreibt die Daten in eine temporäre Datei neben dem Ziel.
fn write_temp(path: &Path, data: &[u8]) -> std::io::Result<PathBuf> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = temp_path(path);
    if let Err(e) = std::fs::write(&tmp, data) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(tmp)
}

/// Schreibt erst vollständig in eine temporäre Datei und benennt sie dann um.
fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = write_temp(path, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Merkt sich, was in dieser Sitzung geschrieben und im laufenden Lauf vergeben wurde.
#[derive(Default)]
pub struct Session {
    /// Geschriebene Datei → Quelle, aus der sie entstand.
    own: HashMap<String, String>,
    taken: HashSet<String>,
    /// Quelle → zuletzt geschriebene Ausgabe.
    last_output: HashMap<String, PathBuf>,
    /// Quelldateien der Liste: werden nie überschrieben, auch nicht mit „Überschreiben“.
    protected: HashSet<String>,
}

impl Session {
    /// Beginnt einen neuen Lauf: die im Lauf vergebenen Namen werden vergessen.
    pub fn begin_run(&mut self) {
        self.taken.clear();
    }

    /// Merkt sich die Quelldateien der Liste, damit kein Ergebnis über sie geschrieben wird.
    pub fn protect_sources(&mut self, sources: impl IntoIterator<Item = PathBuf>) {
        self.protected = sources.into_iter().map(|p| key(&p)).collect();
    }

    /// Darf auch mit „Überschreiben“ nie ersetzt werden: eine andere Quelle der Liste oder das
    /// Ergebnis eines anderen Bildes aus dieser Sitzung.
    fn untouchable(&self, p: &Path, src: &Path) -> bool {
        let k = key(p);
        if k == key(src) {
            return false;
        }
        self.protected.contains(&k) || self.own.get(&k).is_some_and(|owner| *owner != key(src))
    }

    /// Die Datei existiert und ist nicht in dieser Sitzung aus genau dieser Quelle entstanden.
    pub fn has_foreign_conflict(&self, planned: &Path, src: &Path) -> bool {
        planned.exists() && self.own.get(&key(planned)) != Some(&key(src))
    }

    /// Der Pfad ist in diesem Lauf schon vergeben oder gehört einer fremden Datei.
    fn blocked(&self, p: &Path, src: &Path) -> bool {
        self.taken.contains(&key(p)) || self.has_foreign_conflict(p, src)
    }

    fn remember(&mut self, p: &Path, src: &Path) {
        self.taken.insert(key(p));
        self.own.insert(key(p), key(src));
    }

    /// Speichern für „Unterordner web“ und „Daneben mit Zusatz“.
    fn save_beside(
        &mut self,
        src: &Path,
        enc: &Encoded,
        target: Target,
        policy: ConflictPolicy,
        stem: Option<&str>,
    ) -> Result<Saved, JobError> {
        let mut path = planned_path_named(src, target, enc.format, stem);
        if self.taken.contains(&key(&path)) {
            path = next_free(&path, |p| self.blocked(p, src) || self.untouchable(p, src));
        } else if self.has_foreign_conflict(&path, src) {
            match policy {
                ConflictPolicy::Overwrite if !self.untouchable(&path, src) => {}
                ConflictPolicy::Overwrite | ConflictPolicy::Rename => {
                    path = next_free(&path, |p| self.blocked(p, src) || self.untouchable(p, src))
                }
                ConflictPolicy::Skip => return Ok(Saved::new(None, 0, Some(SaveNote::Skipped))),
            }
        }
        write_atomic(&path, &enc.data).map_err(JobError::Io)?;
        self.remember(&path, src);
        Ok(Saved::new(Some(path), enc.data.len() as u64, None))
    }

    /// „Original ersetzen“: erst fertig schreiben, dann Original in den Papierkorb, dann umbenennen.
    fn save_replacing(
        &mut self,
        src: &Path,
        enc: &Encoded,
        trash: &dyn Fn(&Path) -> Result<(), String>,
        stem: Option<&str>,
    ) -> Result<Saved, JobError> {
        let mut path = planned_path_named(src, Target::Replace, enc.format, stem);
        if enc.source_untouched && key(&path) == key(src) {
            return Ok(Saved::new(Some(src.to_path_buf()), enc.data.len() as u64, Some(SaveNote::Unchanged)));
        }
        // Bei neuer Endung darf keine fremde Datei überschrieben werden.
        if key(&path) != key(src) && self.blocked(&path, src) {
            path = next_free(&path, |p| self.blocked(p, src));
        }
        let tmp = write_temp(&path, &enc.data).map_err(JobError::Io)?;
        if trash(src).is_err() {
            // Kein Papierkorb: nie endgültig löschen, Ergebnis daneben ablegen.
            let _ = std::fs::remove_file(&tmp);
            // Bei neuem Namen liegt das Ergebnis ohnehin neben dem Original (ohne „-web“).
            let mut saved = self.save_beside(src, enc, Target::Suffix, ConflictPolicy::Rename, stem)?;
            saved.note = Some(SaveNote::NoRecycleBin);
            return Ok(saved);
        }
        let bytes = enc.data.len() as u64;
        if std::fs::rename(&tmp, &path).is_ok() {
            self.remember(&path, src);
            return Ok(Saved::new(Some(path), bytes, None));
        }
        // Das Original ist schon im Papierkorb: das Ergebnis darf nicht verloren gehen.
        let beside = planned_path_named(src, Target::Suffix, enc.format, stem);
        let beside = if self.blocked(&beside, src) { next_free(&beside, |p| self.blocked(p, src)) } else { beside };
        match std::fs::rename(&tmp, &beside) {
            Ok(()) => {
                self.remember(&beside, src);
                Ok(Saved::new(Some(beside), bytes, Some(SaveNote::SavedBeside)))
            }
            Err(e) => Err(JobError::Stranded(tmp, e)),
        }
    }

    pub fn save(
        &mut self,
        src: &Path,
        enc: &Encoded,
        target: Target,
        policy: ConflictPolicy,
        trash: &dyn Fn(&Path) -> Result<(), String>,
    ) -> Result<Saved, JobError> {
        self.save_named(src, enc, target, policy, trash, None)
    }

    /// Wie `save`, mit optional neuem Dateinamen. Landet das Ergebnis unter einem anderen Namen als
    /// beim letzten Speichern dieses Bildes, wandert die frühere eigene Ausgabe in den Papierkorb.
    pub fn save_named(
        &mut self,
        src: &Path,
        enc: &Encoded,
        target: Target,
        policy: ConflictPolicy,
        trash: &dyn Fn(&Path) -> Result<(), String>,
        stem: Option<&str>,
    ) -> Result<Saved, JobError> {
        let mut saved = match target {
            Target::Replace => self.save_replacing(src, enc, trash, stem)?,
            Target::WebSubfolder | Target::Suffix => self.save_beside(src, enc, target, policy, stem)?,
        };
        if let Some(new) = &saved.path {
            // Nur bei neuem Namen aus den Texten; ohne KI bleiben frühere Fassungen wie bisher liegen.
            if stem.is_some_and(|s| !s.trim().is_empty()) {
                saved.old_kept = self.retire_previous_output(src, new, trash);
            }
            self.last_output.insert(key(src), new.clone());
        }
        Ok(saved)
    }

    /// Frühere Ausgabe derselben Quelle in den Papierkorb, wenn sie noch dieser Quelle gehört.
    /// Gibt den Pfad zurück, wenn sie bleiben musste.
    fn retire_previous_output(&mut self, src: &Path, new: &Path, trash: &dyn Fn(&Path) -> Result<(), String>) -> Option<PathBuf> {
        let previous = self.last_output.get(&key(src))?.clone();
        let still_ours = self.own.get(&key(&previous)) == Some(&key(src));
        if key(&previous) == key(new) || key(&previous) == key(src) || !still_ours || !previous.exists() {
            return None;
        }
        match trash(&previous) {
            Ok(()) => {
                self.own.remove(&key(&previous));
                None
            }
            Err(_) => Some(previous),
        }
    }
}

/// Verschiebt eine Datei in den Papierkorb des Systems.
pub fn system_trash(p: &Path) -> Result<(), String> {
    trash::delete(p).map_err(|e| e.to_string())
}
