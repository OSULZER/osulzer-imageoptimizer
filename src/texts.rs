//! Texte zu einem Bild (Dateiname, Titel, Alt-Text, Beschreibung) und ihre Regeln.

use crate::i18n::Lang;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct ImageTexts {
    /// Dateiname ohne Endung, schon bereinigt.
    pub file_stem: String,
    pub title: String,
    pub alt: String,
    pub description: String,
}

pub const MAX_STEM: usize = 60;
pub const MAX_TITLE: usize = 70;
pub const MAX_ALT: usize = 150;
pub const MAX_DESCRIPTION: usize = 300;

/// Macht aus einem Text einen Dateinamen: klein, `a–z`, `0–9`, Bindestriche, Umlaute ausgeschrieben,
/// höchstens `MAX_STEM` Zeichen, gekürzt an einer Wortgrenze.
pub fn slugify(s: &str) -> String {
    slugify_in(s, Lang::De)
}

/// Wie `slugify`; Deutsch schreibt Umlaute aus (ä → ae), andere Sprachen nehmen den Grundbuchstaben (ä → a).
pub fn slugify_in(s: &str, lang: Lang) -> String {
    slugify_max(s, lang, MAX_STEM)
}

/// Wie `slugify_in`, mit eigener Höchstlänge.
pub fn slugify_max(s: &str, lang: Lang, max: usize) -> String {
    let german = lang == Lang::De;
    let mut ascii = String::with_capacity(s.len());
    for c in s.to_lowercase().chars() {
        match c {
            'a'..='z' | '0'..='9' => ascii.push(c),
            'ä' if german => ascii.push_str("ae"),
            'ö' if german => ascii.push_str("oe"),
            'ü' if german => ascii.push_str("ue"),
            'ä' => ascii.push('a'),
            'ö' => ascii.push('o'),
            'ü' => ascii.push('u'),
            'ß' => ascii.push_str("ss"),
            'à' | 'á' | 'â' | 'ã' | 'å' => ascii.push('a'),
            'è' | 'é' | 'ê' | 'ë' => ascii.push('e'),
            'ì' | 'í' | 'î' | 'ï' => ascii.push('i'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ø' => ascii.push('o'),
            'ù' | 'ú' | 'û' => ascii.push('u'),
            'ç' => ascii.push('c'),
            'ñ' => ascii.push('n'),
            _ => ascii.push(' '),
        }
    }
    let words: Vec<&str> = ascii.split_whitespace().collect();
    let mut out = String::new();
    for word in words {
        let needed = if out.is_empty() { word.len() } else { out.len() + 1 + word.len() };
        if needed > max {
            if out.is_empty() {
                out.push_str(&word[..max]);
            }
            break;
        }
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(word);
    }
    out
}

/// Trimmt, ersetzt Zeilenumbrüche durch Leerzeichen und kürzt an einer Wortgrenze auf `max` Zeichen.
pub fn clamp_chars(s: &str, max: usize) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= max {
        return joined;
    }
    let cut: String = joined.chars().take(max).collect();
    // Folgt direkt ein Leerzeichen, endet `cut` schon an einer Wortgrenze.
    if joined.chars().nth(max) == Some(' ') {
        return cut;
    }
    match cut.rfind(' ') {
        Some(space) if space > 0 => cut[..space].trim_end().to_string(),
        _ => cut,
    }
}

impl ImageTexts {
    /// Wendet alle Regeln an.
    pub fn normalized(&self) -> ImageTexts {
        self.normalized_in(Lang::De)
    }

    /// Wie `normalized`, mit den Dateinamen-Regeln der Sprache.
    pub fn normalized_in(&self, lang: Lang) -> ImageTexts {
        ImageTexts {
            file_stem: slugify_in(&self.file_stem, lang),
            title: clamp_chars(&self.title, MAX_TITLE),
            alt: clamp_chars(&self.alt, MAX_ALT),
            description: clamp_chars(&self.description, MAX_DESCRIPTION),
        }
    }

    pub fn is_empty(&self) -> bool {
        [&self.file_stem, &self.title, &self.alt, &self.description].iter().all(|f| f.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    #[test]
    fn slug_follows_language() {
        assert_eq!(slugify_in("Küche Öfen Straße", Lang::En), "kuche-ofen-strasse");
        assert_eq!(slugify_in("Küche Öfen Straße", Lang::De), "kueche-oefen-strasse");
        assert_eq!(slugify_in("Café", Lang::En), "cafe");
    }

    #[test]
    fn slug_lowercases_and_writes_out_umlauts() {
        assert_eq!(slugify("Grüner Kachelofen im Wohnzimmer"), "gruener-kachelofen-im-wohnzimmer");
        assert_eq!(slugify("Straße & Größe"), "strasse-groesse");
        assert_eq!(slugify("ÄÖÜ äöü"), "aeoeue-aeoeue");
        assert_eq!(slugify("Café Ölbaum"), "cafe-oelbaum");
    }

    #[test]
    fn slug_never_contains_path_or_extension_characters() {
        let s = slugify("../ETC/Passwd.JPG");
        assert_eq!(s, "etc-passwd-jpg");
        assert!(!s.contains(['/', '\\', '.', ' ']));
    }

    #[test]
    fn slug_collapses_and_trims_dashes() {
        assert_eq!(slugify("--a__b  c--"), "a-b-c");
        assert_eq!(slugify("!!!"), "");
        assert_eq!(slugify("   "), "");
    }

    #[test]
    fn slug_is_cut_at_a_word_boundary() {
        let long = "kachelofen mit gruenen kacheln und sitzbank in einem hellen wohnzimmer in der steiermark";
        let s = slugify(long);
        assert!(s.chars().count() <= MAX_STEM, "{s}");
        assert!(!s.ends_with('-'));
        assert!(long.replace(' ', "-").starts_with(&s), "nicht mitten im Wort gekürzt: {s}");
        // Ein einziges überlanges Wort wird hart gekürzt.
        assert_eq!(slugify(&"a".repeat(90)).len(), MAX_STEM);
    }

    #[test]
    fn clamp_replaces_line_breaks_and_cuts_by_characters() {
        assert_eq!(clamp_chars("  a\nb\r\nc  ", 10), "a b c");
        let umlauts = "Größe ".repeat(40);
        let c = clamp_chars(&umlauts, MAX_ALT);
        assert!(c.chars().count() <= MAX_ALT);
        assert!(c.ends_with("Größe"));
    }

    #[test]
    fn normalized_applies_all_rules() {
        let t = ImageTexts {
            file_stem: "Grüner Ofen.JPG".into(),
            title: "t".repeat(100),
            alt: " Alt\nText ".into(),
            description: "d".repeat(400),
        }
        .normalized();
        assert_eq!(t.file_stem, "gruener-ofen-jpg");
        assert_eq!(t.title.chars().count(), MAX_TITLE);
        assert_eq!(t.alt, "Alt Text");
        assert_eq!(t.description.chars().count(), MAX_DESCRIPTION);
    }

    #[test]
    fn empty_means_all_fields_blank() {
        assert!(ImageTexts::default().is_empty());
        assert!(ImageTexts { title: "  ".into(), ..Default::default() }.is_empty());
        assert!(!ImageTexts { alt: "x".into(), ..Default::default() }.is_empty());
    }
}
