//! Texte der Oberfläche in mehreren Sprachen. Eine Tabelle pro Sprache, Englisch ist der Rückfall.

mod de;
mod en;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    De,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::De];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::De => "Deutsch",
        }
    }

    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Lang::En => en::EN,
            Lang::De => de::DE,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(l: Lang) {
    CURRENT.store(l as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::De,
        _ => Lang::En,
    }
}

pub fn t(key: &'static str) -> &'static str {
    t_in(lang(), key)
}

pub fn t_in(l: Lang, key: &'static str) -> &'static str {
    let find = |l: Lang| l.table().iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    find(l).or_else(|| find(Lang::En)).unwrap_or(key)
}

pub fn tf(key: &'static str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    tf_in(lang(), key, args)
}

pub fn tf_in(l: Lang, key: &'static str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = t_in(l, key).to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

/// Zahl mit `decimals` Nachkommastellen; Deutsch mit Komma, Englisch mit Punkt.
pub fn fmt_decimal(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if lang() == Lang::De { text.replace('.', ",") } else { text }
}

/// Deutsch, wenn die Windows-Oberfläche deutsch ist, sonst Englisch.
pub fn detect_system_lang() -> Lang {
    match sys_locale::get_locale() {
        Some(l) if l.to_ascii_lowercase().starts_with("de") => Lang::De,
        _ => Lang::En,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashSet};

    fn table(l: Lang) -> &'static [(&'static str, &'static str)] {
        l.table()
    }

    fn placeholders(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            let Some(len) = rest[start..].find('}') else { break };
            out.insert(rest[start + 1..start + len].to_string());
            rest = &rest[start + len + 1..];
        }
        out
    }

    #[test]
    fn every_key_exists_in_both_languages() {
        let en: BTreeSet<_> = table(Lang::En).iter().map(|(k, _)| *k).collect();
        let de: BTreeSet<_> = table(Lang::De).iter().map(|(k, _)| *k).collect();
        assert_eq!(en.difference(&de).collect::<Vec<_>>(), Vec::<&&str>::new(), "nur Englisch");
        assert_eq!(de.difference(&en).collect::<Vec<_>>(), Vec::<&&str>::new(), "nur Deutsch");
    }

    #[test]
    fn no_duplicate_keys() {
        for l in Lang::ALL {
            let mut seen = HashSet::new();
            for (k, _) in table(l) {
                assert!(seen.insert(*k), "doppelt in {l:?}: {k}");
            }
        }
    }

    #[test]
    fn placeholders_match_between_languages() {
        for (k, en) in table(Lang::En) {
            if let Some((_, de)) = table(Lang::De).iter().find(|(dk, _)| dk == k) {
                assert_eq!(placeholders(en), placeholders(de), "Platzhalter bei {k}");
            }
        }
    }

    #[test]
    fn fallback_to_english_then_key() {
        assert_eq!(t_in(Lang::De, "app.title"), "OSULZER ImageOptimizer");
        assert_eq!(t_in(Lang::De, "does.not.exist"), "does.not.exist");
    }

    #[test]
    fn tf_replaces_placeholders() {
        assert_eq!(tf_in(Lang::De, "ui.progress", &[("done", &1), ("total", &3)]), "1 von 3");
        assert_eq!(tf_in(Lang::En, "ui.progress", &[("done", &1), ("total", &3)]), "1 of 3");
        assert_eq!(t_in(Lang::De, "menu.label"), "ImageOptimizer");
    }

    #[test]
    fn codes_and_names() {
        assert_eq!((Lang::En.code(), Lang::De.code()), ("en", "de"));
        assert_eq!((Lang::En.native_name(), Lang::De.native_name()), ("English", "Deutsch"));
    }

    /// Findet `t("…")`, `tf("…"`, `t_in(x, "…")`, `tf_in(x, "…"` im Quelltext und prüft die Schlüssel.
    #[test]
    fn every_key_used_in_source_exists() {
        let en: HashSet<_> = table(Lang::En).iter().map(|(k, _)| *k).collect();
        let mut missing = Vec::new();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|n| n != "i18n") {
                        stack.push(path);
                    }
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for key in used_keys(&text) {
                    if !en.contains(key.as_str()) {
                        missing.push(format!("{}: {key}", path.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "fehlende Schlüssel:\n{}", missing.join("\n"));
    }

    fn used_keys(text: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for call in ["t(\"", "tf(\"", "t_in(", "tf_in("] {
            let mut rest = text;
            while let Some(pos) = rest.find(call) {
                let before = rest[..pos].chars().last();
                rest = &rest[pos + call.len()..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let arg = if call.ends_with('"') {
                    rest
                } else {
                    match rest.find(", \"") {
                        Some(i) if !rest[..i].contains(')') => &rest[i + 3..],
                        _ => continue,
                    }
                };
                if let Some(end) = arg.find('"') {
                    keys.push(arg[..end].to_string());
                }
            }
        }
        keys
    }
}
