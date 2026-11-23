//! Dateinamen-Regeln: einheitlicher Name mit Laufnummer, Präfix, Suffix, Suchen und Ersetzen.

use crate::i18n::Lang;
use crate::texts::{MAX_STEM, slugify_max};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NameRules {
    /// Gleicher Name für alle Bilder, mit Laufnummer.
    pub uniform: String,
    pub prefix: String,
    pub suffix: String,
    pub search: String,
    pub replace: String,
}

impl NameRules {
    /// Wirkt irgendeine Regel? „Ersetzen“ allein wirkt nicht.
    pub fn is_active(&self) -> bool {
        [&self.uniform, &self.prefix, &self.suffix, &self.search].iter().any(|f| !f.trim().is_empty())
    }
}

/// Ersetzt alle Vorkommen von `search`, ohne auf Groß- und Kleinschreibung zu achten.
pub fn replace_ignore_case(text: &str, search: &str, with: &str) -> String {
    let needle: Vec<char> = search.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<String> = chars.iter().map(|c| c.to_lowercase().collect()).collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    'outer: while i < chars.len() {
        // Ab Zeichen i so viele Zeichen sammeln, bis die Kleinbuchstaben die Länge des Gesuchten erreichen.
        let mut got: Vec<char> = Vec::new();
        let mut j = i;
        while got.len() < needle.len() && j < chars.len() {
            got.extend(lower[j].chars());
            j += 1;
        }
        if got == needle {
            out.push_str(with);
            i = j;
            continue 'outer;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Endgültiger Dateiname ohne Endung. `number` = (Nummer, Anzahl) bei einheitlichem Namen.
/// `None`, wenn der Name bleiben soll, wie er ist (keine Regel, kein KI-Name, oder leeres Ergebnis).
pub fn final_stem(original: &str, ai: Option<&str>, rules: &NameRules, number: Option<(usize, usize)>, lang: Lang) -> Option<String> {
    let ai = ai.map(str::trim).filter(|a| !a.is_empty());
    if !rules.is_active() && ai.is_none() {
        return None;
    }
    let uniform = rules.uniform.trim();
    let base = if !uniform.is_empty() { uniform } else { ai.unwrap_or(original) };
    let replaced = if rules.search.trim().is_empty() { base.to_string() } else { replace_ignore_case(base, &rules.search, &rules.replace) };
    // Mit Leerzeichen verbunden, damit `slugify` Bindestriche setzt.
    let joined = format!("{} {} {}", rules.prefix.trim(), replaced, rules.suffix.trim());
    let tail = match number {
        Some((n, count)) if !uniform.is_empty() => {
            let width = count.to_string().len().max(2);
            format!("-{n:0width$}")
        }
        _ => String::new(),
    };
    let name = slugify_max(&joined, lang, MAX_STEM - tail.len());
    (!name.is_empty()).then(|| format!("{name}{tail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(f: impl FnOnce(&mut NameRules)) -> NameRules {
        let mut r = NameRules::default();
        f(&mut r);
        r
    }

    #[test]
    fn no_rules_no_ai_keeps_original() {
        assert_eq!(final_stem("DSC_0042", None, &NameRules::default(), None, Lang::De), None);
    }

    #[test]
    fn ai_name_alone_is_used() {
        assert_eq!(final_stem("DSC_0042", Some("gruener-ofen"), &NameRules::default(), None, Lang::De).as_deref(), Some("gruener-ofen"));
    }

    #[test]
    fn prefix_search_replace_example() {
        let r = rules(|r| {
            r.prefix = "Mueller".into();
            r.search = "DSC_".into();
        });
        assert_eq!(final_stem("DSC_0042", None, &r, None, Lang::De).as_deref(), Some("mueller-0042"));
    }

    #[test]
    fn suffix_is_appended() {
        let r = rules(|r| r.suffix = "web klein".into());
        assert_eq!(final_stem("Foto", None, &r, None, Lang::De).as_deref(), Some("foto-web-klein"));
    }

    #[test]
    fn uniform_name_with_numbers() {
        let r = rules(|r| r.uniform = "Badsanierung Müller".into());
        assert_eq!(final_stem("x", None, &r, Some((3, 12)), Lang::De).as_deref(), Some("badsanierung-mueller-03"));
        assert_eq!(final_stem("x", None, &r, Some((7, 120)), Lang::De).as_deref(), Some("badsanierung-mueller-007"));
    }

    #[test]
    fn uniform_wins_over_ai_name() {
        let r = rules(|r| r.uniform = "kunde".into());
        assert_eq!(final_stem("x", Some("ofen"), &r, Some((1, 2)), Lang::De).as_deref(), Some("kunde-01"));
    }

    #[test]
    fn number_is_never_cut() {
        let r = rules(|r| r.uniform = "a".repeat(80));
        let stem = final_stem("x", None, &r, Some((1, 5)), Lang::De).unwrap();
        assert_eq!(stem.len(), 60);
        assert!(stem.ends_with("-01"), "{stem}");
    }

    #[test]
    fn search_ignores_case_and_umlauts() {
        assert_eq!(replace_ignore_case("MÜLLER_01", "müller", "Kunde"), "Kunde_01");
        assert_eq!(replace_ignore_case("a-B-a-b", "b", "x"), "a-x-a-x");
        assert_eq!(replace_ignore_case("abc", "", "x"), "abc");
    }

    #[test]
    fn empty_result_falls_back_to_original() {
        let r = rules(|r| r.search = "DSC_0042".into());
        assert_eq!(final_stem("DSC_0042", None, &r, None, Lang::De), None);
    }

    #[test]
    fn english_slug() {
        let r = rules(|r| r.prefix = "Küche".into());
        assert!(final_stem("a", None, &r, None, Lang::En).unwrap().starts_with("kuche-"));
    }

    #[test]
    fn replace_alone_is_not_active() {
        assert!(!rules(|r| r.replace = "x".into()).is_active());
        assert!(!NameRules::default().is_active());
        assert!(rules(|r| r.search = "x".into()).is_active());
        assert!(rules(|r| r.uniform = " k ".into()).is_active());
    }
}
