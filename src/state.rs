//! Die Bilderliste des Fensters: Einträge, Zustände, eigene Werte.

use std::collections::HashMap;
use crate::i18n::{fmt_decimal, t, tf};
use crate::engine::{EngineError, Note, Probe};
use crate::job::SaveNote;
use crate::settings::Settings;
use crate::texts::ImageTexts;
use std::path::{Path, PathBuf};

/// Ergebnis eines verarbeiteten Bildes.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// Wohin geschrieben wurde; `None`, wenn übersprungen.
    pub path: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub message: Option<String>,
    /// Das Original wurde ersetzt (liegt im Papierkorb).
    pub replaced: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Pending,
    Running,
    Done(Outcome),
    Failed(String),
}

pub struct Item {
    pub id: u64,
    pub path: PathBuf,
    /// Kennzahlen der Quelle oder der Grund, warum sie nicht lesbar ist.
    pub probe: Result<Probe, String>,
    /// Eigene Werte für dieses Bild; `None` = es gilt die Vorgabe.
    pub own: Option<Settings>,
    pub status: Status,
    /// Original ersetzt: das Bild ist endgültig fertig und wird nicht erneut verarbeitet.
    pub replaced: bool,
    /// Dateiname und Texte für Web und Metadaten; `None` = wie bisher speichern.
    pub texts: Option<ImageTexts>,
    /// Texte wurden von Hand bearbeitet; ein KI-Lauf überschreibt sie nicht.
    pub texts_edited: bool,
    pub ai: AiStatus,
    key: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AiStatus {
    Idle,
    Waiting,
    Running,
    Failed(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub count: usize,
    pub source_bytes: u64,
    /// Summe der Ergebnisse aller fertigen Bilder.
    pub result_bytes: u64,
    /// Summe der Quellgrößen genau dieser fertigen Bilder.
    pub done_source_bytes: u64,
    pub done: usize,
    pub failed: usize,
}

#[derive(Default)]
pub struct Batch {
    pub items: Vec<Item>,
    next_id: u64,
}

/// Vergleichsschlüssel: derselbe Dateiort ergibt denselben Schlüssel.
fn key_of(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    resolved.to_string_lossy().replace('/', "\\").to_lowercase()
}

impl Batch {
    /// Hängt ein Bild an. `None`, wenn dieselbe Datei schon in der Liste steht.
    pub fn add(&mut self, path: PathBuf, probe: Result<Probe, EngineError>) -> Option<u64> {
        let key = key_of(&path);
        if self.items.iter().any(|i| i.key == key) {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let probe = probe.map_err(|e| e.to_string());
        self.items.push(Item {
            id,
            path,
            probe,
            own: None,
            status: Status::Pending,
            replaced: false,
            texts: None,
            texts_edited: false,
            ai: AiStatus::Idle,
            key,
        });
        Some(id)
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|i| i.id != id);
    }

    /// Leert die Liste. Die Nummern laufen weiter, damit alte Vorschauen nicht passen.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Trägt das Ergebnis eines Laufs ein.
    pub fn finish(&mut self, id: u64, result: Result<Outcome, String>) {
        if let Some(item) = self.get_mut(id) {
            item.status = match result {
                Ok(outcome) => {
                    item.replaced |= outcome.replaced;
                    Status::Done(outcome)
                }
                Err(text) => Status::Failed(text),
            };
        }
    }

    /// Das Ziel hat sich geändert: alle Bilder werden wieder „offen“.
    pub fn target_changed(&mut self) {
        self.items.iter_mut().for_each(reopen);
    }

    /// Andere Namensregeln: fertige Bilder müssen unter dem neuen Namen neu geschrieben werden.
    pub fn rules_changed(&mut self) {
        self.items.iter_mut().for_each(reopen);
    }

    /// Dateiname mit Endung, den die Liste als „neuer Name“ zeigt: genau der Name, unter dem gespeichert
    /// wird (bzw. wurde). Nur für Bilder, deren Name sich durch Regeln oder KI ändert.
    pub fn preview_names(
        &self,
        default: &Settings,
        target: crate::settings::Target,
        rules: &crate::naming::NameRules,
        lang: crate::i18n::Lang,
    ) -> HashMap<u64, String> {
        let stems = self.final_stems(rules, lang);
        self.items
            .iter()
            .filter_map(|item| {
                let stem = stems.get(&item.id).cloned().flatten()?;
                if let Status::Done(Outcome { path: Some(saved), .. }) = &item.status {
                    return Some((item.id, saved.file_name()?.to_string_lossy().into_owned()));
                }
                let probe = item.probe.as_ref().ok()?;
                let settings = self.effective(item.id, default);
                let format = crate::settings::resolve_format(settings.format, probe.format, probe.has_alpha);
                let planned = crate::job::planned_path_named(&item.path, target, format, Some(&stem));
                Some((item.id, planned.file_name()?.to_string_lossy().into_owned()))
            })
            .collect()
    }

    /// Endgültiger Dateiname (ohne Endung) je Bild; `None` = Name bleibt wie bisher.
    /// Laufnummern zählen über alle lesbaren Bilder in Listenreihenfolge, damit sie stabil bleiben.
    pub fn final_stems(&self, rules: &crate::naming::NameRules, lang: crate::i18n::Lang) -> HashMap<u64, Option<String>> {
        let count = self.items.iter().filter(|i| i.probe.is_ok()).count();
        let mut n = 0;
        self.items
            .iter()
            .map(|item| {
                if item.probe.is_err() {
                    return (item.id, None);
                }
                n += 1;
                let original = item.path.file_stem().unwrap_or_default().to_string_lossy();
                let ai = item.texts.as_ref().map(|t| t.file_stem.as_str());
                (item.id, crate::naming::final_stem(&original, ai, rules, Some((n, count)), lang))
            })
            .collect()
    }

    /// Für jedes Bild, das „Optimieren“ verarbeiten würde: (id, Quelle, geplanter Zielpfad),
    /// mit dem Dateinamen aus den Texten, falls vorhanden.
    pub fn planned_outputs(
        &self,
        default: &Settings,
        target: crate::settings::Target,
        rules: &crate::naming::NameRules,
        lang: crate::i18n::Lang,
    ) -> Vec<(u64, PathBuf, PathBuf)> {
        let stems = self.final_stems(rules, lang);
        self.runnable()
            .into_iter()
            .filter_map(|id| {
                let item = self.get(id)?;
                let probe = item.probe.as_ref().ok()?;
                let settings = self.effective(id, default);
                let format = crate::settings::resolve_format(settings.format, probe.format, probe.has_alpha);
                let stem = stems.get(&id).cloned().flatten();
                let planned = crate::job::planned_path_named(&item.path, target, format, stem.as_deref());
                Some((id, item.path.clone(), planned))
            })
            .collect()
    }

    /// Bilder, die ein KI-Lauf beschreiben würde.
    pub fn ai_candidates(&self) -> Vec<u64> {
        self.items.iter().filter(|i| i.probe.is_ok() && !i.texts_edited && !i.replaced).map(|i| i.id).collect()
    }

    pub fn set_ai_status(&mut self, id: u64, status: AiStatus) {
        if let Some(item) = self.get_mut(id) {
            item.ai = status;
        }
    }

    /// Ergebnis der KI eintragen. Von Hand bearbeitete Texte bleiben unberührt.
    pub fn ai_finished(&mut self, id: u64, result: Result<ImageTexts, String>) {
        let Some(item) = self.get_mut(id) else { return };
        match result {
            Ok(texts) => {
                item.ai = AiStatus::Idle;
                if !item.texts_edited && item.texts.as_ref() != Some(&texts) {
                    item.texts = Some(texts);
                    reopen(item);
                }
            }
            Err(text) => item.ai = AiStatus::Failed(text),
        }
    }

    /// Texte von Hand setzen (`Some`) oder verwerfen (`None`).
    pub fn edit_texts(&mut self, id: u64, texts: Option<ImageTexts>) {
        let Some(item) = self.get_mut(id) else { return };
        let edited = texts.is_some();
        if item.texts != texts {
            item.texts = texts;
            reopen(item);
        }
        item.texts_edited = edited;
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// Die für dieses Bild geltenden Werte: die eigenen, sonst die Vorgabe.
    pub fn effective<'a>(&'a self, id: u64, default: &'a Settings) -> &'a Settings {
        self.get(id).and_then(|i| i.own.as_ref()).unwrap_or(default)
    }

    /// Setzt oder entfernt eigene Werte. Ändert sich dadurch etwas, wird das Bild wieder „offen“.
    pub fn set_own(&mut self, id: u64, own: Option<Settings>) {
        if let Some(item) = self.get_mut(id)
            && item.own != own
        {
            item.own = own;
            reopen(item);
        }
    }

    /// Die Vorgabe hat sich geändert: alle Bilder ohne eigene Werte werden wieder „offen“.
    pub fn defaults_changed(&mut self) {
        self.items.iter_mut().filter(|i| i.own.is_none()).for_each(reopen);
    }

    /// Bilder, die „Optimieren“ verarbeiten würde.
    pub fn runnable(&self) -> Vec<u64> {
        self.items
            .iter()
            .filter(|i| i.probe.is_ok() && !i.replaced && matches!(i.status, Status::Pending | Status::Failed(_)))
            .map(|i| i.id)
            .collect()
    }

    pub fn totals(&self) -> Totals {
        let mut t = Totals { count: self.items.len(), ..Totals::default() };
        for item in &self.items {
            let source = item.probe.as_ref().map(|p| p.bytes).unwrap_or(0);
            t.source_bytes += source;
            match &item.status {
                Status::Done(outcome) => {
                    t.done += 1;
                    t.result_bytes += outcome.bytes;
                    t.done_source_bytes += source;
                }
                Status::Failed(_) => t.failed += 1,
                Status::Pending | Status::Running => {}
            }
        }
        t
    }
}

/// Setzt ein Bild wieder auf „offen“, außer es läuft gerade oder sein Original wurde ersetzt.
fn reopen(item: &mut Item) {
    if item.status != Status::Running && !item.replaced {
        item.status = Status::Pending;
    }
}

/// Hinweistext zu einem Ergebnis.
pub fn message_for(note: Option<Note>, save: Option<SaveNote>) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    match note {
        Some(Note::TargetSizeMissed) => parts.push(t("msg.target_size_missed")),
        // „unverändert“ unten sagt es bereits.
        Some(Note::AlreadyOptimal) if save != Some(SaveNote::Unchanged) => parts.push(t("msg.already_optimal")),
        _ => {}
    }
    match save {
        Some(SaveNote::Skipped) => parts.push(t("msg.skipped")),
        Some(SaveNote::NoRecycleBin) => parts.push(t("msg.no_recycle_bin")),
        Some(SaveNote::Unchanged) => parts.push(t("msg.unchanged")),
        Some(SaveNote::SavedBeside) => parts.push(t("msg.saved_beside")),
        None => {}
    }
    if parts.is_empty() { None } else { Some(parts.join("; ")) }
}

/// Hinweis, wenn eine frühere Ausgabe nicht in den Papierkorb konnte.
pub fn old_file_kept_message(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    tf("msg.old_file_kept", &[("name", &name)])
}

/// „412 KB“, „10,2 MB“ (Dezimalzeichen nach Sprache).
pub fn format_bytes(b: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if b >= MB {
        format!("{} MB", fmt_decimal(b as f64 / MB as f64, 1))
    } else if b == 0 {
        "0 KB".to_string()
    } else {
        format!("{} KB", ((b as f64 / 1024.0).round() as u64).max(1))
    }
}

/// Ersparnis in Prozent, z. B. „−96 %“; bei größerem Ergebnis „+3 %“.
pub fn format_saving(source: u64, result: u64) -> String {
    if source == 0 {
        return String::new();
    }
    let mut percent = ((result as f64 - source as f64) * 100.0 / source as f64).round() as i64;
    // Eine Datei mit Inhalt ist nie „−100 %“.
    if percent == -100 && result > 0 {
        percent = -99;
    }
    match percent {
        0 => "±0 %".to_string(),
        p if p < 0 => format!("−{} %", -p),
        p => format!("+{p} %"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Format, SrcFormat};

    fn probe(bytes: u64) -> Result<Probe, EngineError> {
        Ok(Probe { width: 100, height: 50, bytes, format: SrcFormat::Jpeg, has_alpha: false })
    }

    fn default_settings() -> Settings {
        Settings { max_edge: Some(1920), format: Format::Jpeg, quality: 80, max_kb: None }
    }

    fn done(bytes: u64) -> Status {
        Status::Done(Outcome { path: None, width: 1, height: 1, bytes, message: None, replaced: false })
    }

    #[test]
    fn same_file_is_added_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Bild.jpg");
        std::fs::write(&file, b"x").unwrap();
        let mut b = Batch::default();
        assert!(b.add(file.clone(), probe(1)).is_some());
        assert_eq!(b.add(file.clone(), probe(1)), None);
        assert_eq!(b.add(dir.path().join("BILD.JPG"), probe(1)), None);
        assert_eq!(b.add(dir.path().join(".").join("Bild.jpg"), probe(1)), None);
        assert_eq!(b.items.len(), 1);
    }

    #[test]
    fn different_files_get_different_ids() {
        let mut b = Batch::default();
        let a = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        let c = b.add(PathBuf::from(r"C:\x\b.jpg"), probe(1)).unwrap();
        assert_ne!(a, c);
        b.remove(a);
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].id, c);
        // Nach dem Entfernen darf dieselbe Datei wieder hinzukommen.
        assert!(b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).is_some());
    }

    #[test]
    fn unreadable_file_is_listed_but_not_runnable() {
        crate::i18n::set_lang(crate::i18n::Lang::De);
        let mut b = Batch::default();
        let ok = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        let bad = b.add(PathBuf::from(r"C:\x\b.txt"), Err(EngineError::Unsupported)).unwrap();
        assert_eq!(b.get(bad).unwrap().probe.as_ref().unwrap_err(), "Kein unterstütztes Bildformat");
        assert_eq!(b.runnable(), vec![ok]);
    }

    #[test]
    fn runnable_contains_pending_and_failed_only() {
        let mut b = Batch::default();
        let ids: Vec<u64> = (0..4).map(|i| b.add(PathBuf::from(format!(r"C:\x\{i}.jpg")), probe(1)).unwrap()).collect();
        b.get_mut(ids[1]).unwrap().status = Status::Running;
        b.get_mut(ids[2]).unwrap().status = done(1);
        b.get_mut(ids[3]).unwrap().status = Status::Failed("x".into());
        assert_eq!(b.runnable(), vec![ids[0], ids[3]]);
    }

    #[test]
    fn changed_defaults_reopen_only_images_without_own_values() {
        let mut b = Batch::default();
        let plain = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        let own = b.add(PathBuf::from(r"C:\x\b.jpg"), probe(1)).unwrap();
        b.set_own(own, Some(default_settings()));
        b.get_mut(plain).unwrap().status = done(1);
        b.get_mut(own).unwrap().status = done(1);
        b.defaults_changed();
        assert_eq!(b.get(plain).unwrap().status, Status::Pending);
        assert_eq!(b.get(own).unwrap().status, done(1));
    }

    #[test]
    fn own_values_override_and_reopen() {
        let mut b = Batch::default();
        let id = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        let default = default_settings();
        assert_eq!(b.effective(id, &default), &default);

        b.get_mut(id).unwrap().status = done(1);
        let mut own = default_settings();
        own.quality = 60;
        b.set_own(id, Some(own.clone()));
        assert_eq!(b.effective(id, &default), &own);
        assert_eq!(b.get(id).unwrap().status, Status::Pending);

        // Gleicher Wert noch einmal: nichts ändert sich, das Ergebnis bleibt stehen.
        b.get_mut(id).unwrap().status = done(1);
        b.set_own(id, Some(own));
        assert_eq!(b.get(id).unwrap().status, done(1));

        b.set_own(id, None);
        assert_eq!(b.effective(id, &default), &default);
        assert_eq!(b.get(id).unwrap().status, Status::Pending);
    }

    #[test]
    fn totals_sum_sources_and_finished_results() {
        let mut b = Batch::default();
        let a = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1000)).unwrap();
        let c = b.add(PathBuf::from(r"C:\x\b.jpg"), probe(3000)).unwrap();
        b.add(PathBuf::from(r"C:\x\c.txt"), Err(EngineError::Unsupported)).unwrap();
        b.get_mut(a).unwrap().status = done(100);
        b.get_mut(c).unwrap().status = Status::Failed("x".into());
        let t = b.totals();
        assert_eq!(t, Totals { count: 3, source_bytes: 4000, result_bytes: 100, done_source_bytes: 1000, done: 1, failed: 1 });
    }

    #[test]
    fn messages_are_german() {
        crate::i18n::set_lang(crate::i18n::Lang::De);
        assert_eq!(message_for(None, None), None);
        assert_eq!(message_for(Some(Note::TargetSizeMissed), None).unwrap(), "Zielgröße nicht erreicht");
        assert_eq!(message_for(Some(Note::AlreadyOptimal), None).unwrap(), "bereits optimal");
        assert_eq!(message_for(None, Some(SaveNote::Skipped)).unwrap(), "übersprungen (Datei vorhanden)");
        assert_eq!(
            message_for(None, Some(SaveNote::NoRecycleBin)).unwrap(),
            "Kein Papierkorb auf diesem Laufwerk, Original behalten"
        );
        assert_eq!(message_for(Some(Note::AlreadyOptimal), Some(SaveNote::Unchanged)).unwrap(), "bereits optimal, unverändert");
        assert_eq!(
            message_for(Some(Note::TargetSizeMissed), Some(SaveNote::NoRecycleBin)).unwrap(),
            "Zielgröße nicht erreicht; Kein Papierkorb auf diesem Laufwerk, Original behalten"
        );
    }

    #[test]
    fn bytes_are_formatted_with_decimal_comma() {
        crate::i18n::set_lang(crate::i18n::Lang::De);
        assert_eq!(format_bytes(412_000), "402 KB");
        assert_eq!(format_bytes(10_695_475), "10,2 MB");
        assert_eq!(format_bytes(900), "1 KB");
        assert_eq!(format_bytes(1_048_576), "1,0 MB");
    }

    #[test]
    fn savings_are_formatted_as_percent() {
        assert_eq!(format_saving(10_000, 400), "−96 %");
        assert_eq!(format_saving(1000, 1030), "+3 %");
        assert_eq!(format_saving(1000, 1000), "±0 %");
        assert_eq!(format_saving(0, 10), "");
    }

    fn ai_texts(alt: &str) -> ImageTexts {
        ImageTexts { file_stem: "ofen".into(), title: "Ofen".into(), alt: alt.into(), description: "Ein Ofen.".into() }
    }

    #[test]
    fn ai_candidates_skip_unreadable_edited_and_replaced_images() {
        let mut b = Batch::default();
        let plain = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        let edited = b.add(PathBuf::from(r"C:\x\b.jpg"), probe(1)).unwrap();
        let replaced = b.add(PathBuf::from(r"C:\x\c.jpg"), probe(1)).unwrap();
        b.add(PathBuf::from(r"C:\x\d.txt"), Err(EngineError::Unsupported)).unwrap();
        b.edit_texts(edited, Some(ai_texts("von Hand")));
        b.finish(replaced, Ok(replaced_outcome()));
        assert_eq!(b.ai_candidates(), vec![plain]);
    }

    #[test]
    fn ai_result_sets_texts_and_reopens_a_finished_image() {
        let mut b = Batch::default();
        let id = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        b.get_mut(id).unwrap().status = done(1);
        b.set_ai_status(id, AiStatus::Running);
        b.ai_finished(id, Ok(ai_texts("KI")));
        let item = b.get(id).unwrap();
        assert_eq!(item.texts.as_ref().unwrap().alt, "KI");
        assert!(!item.texts_edited);
        assert_eq!(item.ai, AiStatus::Idle);
        assert_eq!(item.status, Status::Pending);
    }

    #[test]
    fn ai_result_never_overwrites_hand_edited_texts() {
        let mut b = Batch::default();
        let id = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        b.set_ai_status(id, AiStatus::Running);
        b.edit_texts(id, Some(ai_texts("von Hand")));
        b.ai_finished(id, Ok(ai_texts("KI")));
        let item = b.get(id).unwrap();
        assert_eq!(item.texts.as_ref().unwrap().alt, "von Hand");
        assert!(item.texts_edited);
        assert_eq!(item.ai, AiStatus::Idle);
    }

    #[test]
    fn discarding_texts_makes_the_image_a_candidate_again() {
        let mut b = Batch::default();
        let id = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        b.edit_texts(id, Some(ai_texts("von Hand")));
        assert!(b.ai_candidates().is_empty());
        b.get_mut(id).unwrap().status = done(1);
        b.edit_texts(id, None);
        assert_eq!(b.get(id).unwrap().texts, None);
        assert_eq!(b.get(id).unwrap().status, Status::Pending);
        assert_eq!(b.ai_candidates(), vec![id]);
    }

    #[test]
    fn setting_the_same_texts_again_does_not_reopen() {
        let mut b = Batch::default();
        let id = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        b.edit_texts(id, Some(ai_texts("x")));
        b.get_mut(id).unwrap().status = done(1);
        b.edit_texts(id, Some(ai_texts("x")));
        assert!(matches!(b.get(id).unwrap().status, Status::Done(_)));
    }

    #[test]
    fn ai_errors_are_kept_and_texts_stay() {
        let mut b = Batch::default();
        let id = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        b.ai_finished(id, Ok(ai_texts("erste")));
        b.ai_finished(id, Err("KI nicht erreichbar".into()));
        let item = b.get(id).unwrap();
        assert_eq!(item.ai, AiStatus::Failed("KI nicht erreichbar".into()));
        assert_eq!(item.texts.as_ref().unwrap().alt, "erste");
    }

    #[test]
    fn planned_outputs_use_the_ai_file_name() {
        use crate::settings::Target;
        let mut b = Batch::default();
        let plain = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        let named = b.add(PathBuf::from(r"C:\x\b.png"), probe(1)).unwrap();
        b.edit_texts(named, Some(ai_texts("x")));
        let planned = b.planned_outputs(&default_settings(), Target::Suffix, &crate::naming::NameRules::default(), crate::i18n::Lang::De);
        assert_eq!(
            planned,
            vec![
                (plain, PathBuf::from(r"C:\x\a.jpg"), PathBuf::from(r"C:\x\a-web.jpg")),
                (named, PathBuf::from(r"C:\x\b.png"), PathBuf::from(r"C:\x\ofen.jpg")),
            ]
        );
    }

    fn uniform(name: &str) -> crate::naming::NameRules {
        crate::naming::NameRules { uniform: name.into(), ..Default::default() }
    }

    #[test]
    fn uniform_names_are_numbered_in_list_order_skipping_unreadable() {
        use crate::i18n::Lang;
        let mut b = Batch::default();
        let a = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        let bad = b.add(PathBuf::from(r"C:\x\b.txt"), Err(EngineError::Unsupported)).unwrap();
        let c = b.add(PathBuf::from(r"C:\x\c.jpg"), probe(1)).unwrap();
        let stems = b.final_stems(&uniform("kunde"), Lang::De);
        assert_eq!(stems[&a].as_deref(), Some("kunde-01"));
        assert_eq!(stems[&bad], None);
        assert_eq!(stems[&c].as_deref(), Some("kunde-02"));
    }

    #[test]
    fn added_image_gets_next_number() {
        use crate::i18n::Lang;
        let mut b = Batch::default();
        let a = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        let c = b.add(PathBuf::from(r"C:\x\c.jpg"), probe(1)).unwrap();
        b.get_mut(a).unwrap().status = done(1);
        let d = b.add(PathBuf::from(r"C:\x\d.jpg"), probe(1)).unwrap();
        let stems = b.final_stems(&uniform("kunde"), Lang::De);
        assert_eq!((stems[&a].as_deref(), stems[&c].as_deref(), stems[&d].as_deref()), (Some("kunde-01"), Some("kunde-02"), Some("kunde-03")));
    }

    #[test]
    fn planned_outputs_use_rules() {
        use crate::i18n::Lang;
        use crate::settings::Target;
        let mut b = Batch::default();
        let a = b.add(PathBuf::from(r"C:\x\IMG_2001.jpg"), probe(1)).unwrap();
        let rules = crate::naming::NameRules { prefix: "Mueller".into(), search: "IMG_".into(), ..Default::default() };
        let planned = b.planned_outputs(&default_settings(), Target::Suffix, &rules, Lang::De);
        assert_eq!(planned, vec![(a, PathBuf::from(r"C:\x\IMG_2001.jpg"), PathBuf::from(r"C:\x\mueller-2001.jpg"))]);
    }

    #[test]
    fn preview_shows_the_real_file_name() {
        use crate::i18n::Lang;
        use crate::settings::Target;
        let mut b = Batch::default();
        let a = b.add(PathBuf::from(r"C:\x\foto.jpg"), probe(1)).unwrap();
        let rules = crate::naming::NameRules { search: "xyz".into(), ..Default::default() };
        let names = b.preview_names(&default_settings(), Target::Suffix, &rules, Lang::De);
        assert_eq!(names[&a], "foto-web.jpg", "unveränderter Name bekommt -web wie beim Speichern");
        let rules = crate::naming::NameRules { prefix: "k".into(), ..Default::default() };
        assert_eq!(b.preview_names(&default_settings(), Target::Suffix, &rules, Lang::De)[&a], "k-foto.jpg");
        assert!(b.preview_names(&default_settings(), Target::Suffix, &Default::default(), Lang::De).is_empty());
    }

    #[test]
    fn done_items_preview_their_saved_name() {
        use crate::i18n::Lang;
        use crate::settings::Target;
        let mut b = Batch::default();
        let a = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        b.get_mut(a).unwrap().status = Status::Done(Outcome {
            path: Some(PathBuf::from(r"C:\x\web\kunde-02.jpg")),
            width: 1,
            height: 1,
            bytes: 1,
            message: None,
            replaced: false,
        });
        let rules = crate::naming::NameRules { uniform: "kunde".into(), ..Default::default() };
        assert_eq!(b.preview_names(&default_settings(), Target::WebSubfolder, &rules, Lang::De)[&a], "kunde-02.jpg");
    }

    #[test]
    fn changing_rules_reopens_done_items() {
        let mut b = Batch::default();
        let a = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        b.get_mut(a).unwrap().status = done(1);
        b.rules_changed();
        assert_eq!(b.get(a).unwrap().status, Status::Pending);
    }

    #[test]
    fn kept_old_file_is_named_in_the_message() {
        crate::i18n::set_lang(crate::i18n::Lang::De);
        assert_eq!(
            old_file_kept_message(Path::new(r"C:\x\web\kachelofen.jpg")),
            "alte Datei kachelofen.jpg liegt noch im Zielordner"
        );
    }

    #[test]
    fn ids_are_not_reused_after_clearing() {
        let mut b = Batch::default();
        let first = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        b.clear();
        assert!(b.items.is_empty());
        let second = b.add(PathBuf::from(r"C:\x\b.jpg"), probe(1)).unwrap();
        assert_ne!(first, second);
    }

    fn replaced_outcome() -> Outcome {
        Outcome { path: Some(PathBuf::from(r"C:\x\a.jpg")), width: 1, height: 1, bytes: 5, message: None, replaced: true }
    }

    #[test]
    fn replaced_images_stay_final() {
        let mut b = Batch::default();
        let replaced = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        let plain = b.add(PathBuf::from(r"C:\x\b.jpg"), probe(1)).unwrap();
        b.finish(replaced, Ok(replaced_outcome()));
        b.finish(plain, Ok(Outcome { replaced: false, ..replaced_outcome() }));
        b.defaults_changed();
        b.target_changed();
        b.set_own(replaced, Some(default_settings()));
        assert!(matches!(b.get(replaced).unwrap().status, Status::Done(_)));
        assert_eq!(b.get(plain).unwrap().status, Status::Pending);
        assert_eq!(b.runnable(), vec![plain]);
    }

    #[test]
    fn finish_records_errors() {
        let mut b = Batch::default();
        let id = b.add(PathBuf::from(r"C:\x\a.jpg"), probe(1)).unwrap();
        b.finish(id, Err("kaputt".into()));
        assert_eq!(b.get(id).unwrap().status, Status::Failed("kaputt".into()));
    }

    #[test]
    fn a_non_empty_result_never_shows_minus_100_percent() {
        assert_eq!(format_saving(20_900_000, 33_353), "−99 %");
        assert_eq!(format_saving(1000, 0), "−100 %");
    }
}
