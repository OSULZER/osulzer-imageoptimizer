//! Einstellungen, Vorgaben und die Wahl des Ausgabeformats.

use serde::{Deserialize, Serialize};

/// Vom Benutzer gewähltes Format.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Format {
    Original,
    Jpeg,
    Png,
    Webp,
}

/// Tatsächlich geschriebenes Format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutFormat {
    Jpeg,
    Png,
    Webp,
}

impl OutFormat {
    pub fn extension(self) -> &'static str {
        match self {
            OutFormat::Jpeg => "jpg",
            OutFormat::Png => "png",
            OutFormat::Webp => "webp",
        }
    }
}

/// Am Dateiinhalt erkanntes Quellformat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SrcFormat {
    Jpeg,
    Png,
    Webp,
    Bmp,
    Tiff,
}

/// Wohin die Ergebnisse geschrieben werden.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Target {
    WebSubfolder,
    Suffix,
    Replace,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Settings {
    /// Längste Kante in Pixeln; `None` lässt die Abmessungen unverändert.
    pub max_edge: Option<u32>,
    pub format: Format,
    /// 1–100. Bei PNG ist 100 verlustfrei, darunter werden die Farben reduziert.
    pub quality: u8,
    /// Obergrenze der Dateigröße in KB.
    pub max_kb: Option<u32>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub settings: Settings,
}

/// Unter diese Qualität senkt die Zielgrößen-Suche nie.
pub const MIN_QUALITY: u8 = 40;

/// Mitgelieferte Vorgaben mit Namen in der aktuellen Oberflächensprache.
pub fn builtin_presets() -> Vec<Preset> {
    builtin_presets_in(crate::i18n::lang())
}

pub fn builtin_presets_in(lang: crate::i18n::Lang) -> Vec<Preset> {
    let preset = |key: &'static str, edge: u32, quality: u8| Preset {
        name: crate::i18n::t_in(lang, key).to_string(),
        settings: Settings { max_edge: Some(edge), format: Format::Jpeg, quality, max_kb: None },
    };
    vec![preset("preset.web_large", 1920, 80), preset("preset.web_medium", 1200, 80), preset("preset.thumbnail", 600, 75)]
}

/// Mitgelieferte Vorgabe über ihren festen Schlüssel (für Kommandozeile und MCP).
pub fn preset_by_key(key: &str) -> Option<Preset> {
    let index = ["web-large", "web-medium", "thumbnail"].iter().position(|k| k.eq_ignore_ascii_case(key.trim()))?;
    builtin_presets().into_iter().nth(index)
}

pub fn resolve_format(f: Format, src: SrcFormat, has_alpha: bool) -> OutFormat {
    match (f, src) {
        (Format::Jpeg, _) => OutFormat::Jpeg,
        (Format::Png, _) => OutFormat::Png,
        (Format::Webp, _) => OutFormat::Webp,
        (Format::Original, SrcFormat::Jpeg) => OutFormat::Jpeg,
        (Format::Original, SrcFormat::Png) => OutFormat::Png,
        (Format::Original, SrcFormat::Webp) => OutFormat::Webp,
        // BMP und TIFF sind keine Webformate.
        (Format::Original, SrcFormat::Bmp | SrcFormat::Tiff) => {
            if has_alpha {
                OutFormat::Png
            } else {
                OutFormat::Jpeg
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_names_follow_language() {
        use crate::i18n::Lang;
        let names = |l| builtin_presets_in(l).into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(names(Lang::En), ["Web large", "Web medium", "Thumbnail"]);
        assert_eq!(names(Lang::De), ["Web groß", "Web mittel", "Vorschaubild"]);
    }

    #[test]
    fn builtin_presets_match_spec() {
        let p = builtin_presets_in(crate::i18n::Lang::De);
        let got: Vec<_> = p
            .iter()
            .map(|p| (p.name.as_str(), p.settings.max_edge, p.settings.format, p.settings.quality, p.settings.max_kb))
            .collect();
        assert_eq!(
            got,
            vec![
                ("Web groß", Some(1920), Format::Jpeg, 80, None),
                ("Web mittel", Some(1200), Format::Jpeg, 80, None),
                ("Vorschaubild", Some(600), Format::Jpeg, 75, None),
            ]
        );
    }

    #[test]
    fn explicit_format_always_wins() {
        for src in [SrcFormat::Jpeg, SrcFormat::Png, SrcFormat::Webp, SrcFormat::Bmp, SrcFormat::Tiff] {
            for alpha in [false, true] {
                assert_eq!(resolve_format(Format::Jpeg, src, alpha), OutFormat::Jpeg);
                assert_eq!(resolve_format(Format::Png, src, alpha), OutFormat::Png);
                assert_eq!(resolve_format(Format::Webp, src, alpha), OutFormat::Webp);
            }
        }
    }

    #[test]
    fn original_keeps_web_formats() {
        for alpha in [false, true] {
            assert_eq!(resolve_format(Format::Original, SrcFormat::Jpeg, alpha), OutFormat::Jpeg);
            assert_eq!(resolve_format(Format::Original, SrcFormat::Png, alpha), OutFormat::Png);
            assert_eq!(resolve_format(Format::Original, SrcFormat::Webp, alpha), OutFormat::Webp);
        }
    }

    #[test]
    fn original_turns_bmp_and_tiff_into_web_formats() {
        for src in [SrcFormat::Bmp, SrcFormat::Tiff] {
            assert_eq!(resolve_format(Format::Original, src, false), OutFormat::Jpeg);
            assert_eq!(resolve_format(Format::Original, src, true), OutFormat::Png);
        }
    }
}
