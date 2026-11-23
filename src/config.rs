//! Konfigurationsdatei: eigene Vorgaben und der zuletzt benutzte Zustand.

use crate::ai::provider::{Provider, guess_provider, is_local_url};
use crate::i18n::{Lang, detect_system_lang};
use crate::settings::{Preset, Settings, Target, builtin_presets, builtin_presets_in};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub user_presets: Vec<Preset>,
    /// Die zuletzt eingestellten Werte.
    pub current: Settings,
    pub target: Target,
    /// Quellbilder nach dem Speichern in den Papierkorb legen (nur im Fenster).
    pub delete_sources: bool,
    pub last_preset: Option<String>,
    /// KI-Verbindung (ohne Schlüssel, der liegt in der Anmeldeinformationsverwaltung).
    pub ai: AiSettings,
    /// Oberflächensprache; `None` = beim Start aus Windows ermitteln.
    pub ui_lang: Option<Lang>,
    /// Sprache der KI-Texte und Dateinamen; `None` = wie die Oberfläche.
    pub ai_lang: Option<Lang>,
    /// Dateinamen-Regeln für alle Bilder.
    pub name_rules: crate::naming::NameRules,
    /// Beim Start nach Updates suchen (fragt GitHub).
    pub update_check: bool,
    /// Zeitpunkt der letzten Update-Prüfung (Unix-Sekunden).
    pub last_update_check: u64,
    /// „Nicht mehr fragen“ beim Installationsangebot.
    pub install_declined: bool,
}

/// KI-Verbindung: aktiver Anbieter und je Anbieter Adresse und Modell.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(from = "AiSettingsFile")]
pub struct AiSettings {
    pub provider: Provider,
    pub providers: BTreeMap<Provider, ProviderSettings>,
    /// Anbieter (bzw. `custom:<adresse>`), denen das Senden von Bildern erlaubt wurde.
    pub cloud_consent: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderSettings {
    pub base_url: String,
    pub model: String,
}

/// Dateiform, die auch das alte Format (`base_url`/`model` ohne Anbieter) liest.
#[derive(Default, Deserialize)]
#[serde(default)]
struct AiSettingsFile {
    provider: Option<Provider>,
    providers: BTreeMap<Provider, ProviderSettings>,
    cloud_consent: Vec<String>,
    base_url: Option<String>,
    model: Option<String>,
}

impl From<AiSettingsFile> for AiSettings {
    fn from(f: AiSettingsFile) -> Self {
        let mut ai = AiSettings { provider: f.provider.unwrap_or_default(), providers: f.providers, cloud_consent: f.cloud_consent };
        let legacy_url = f.base_url.unwrap_or_default();
        if f.provider.is_none() && !legacy_url.trim().is_empty() {
            ai.provider = guess_provider(&legacy_url);
            ai.providers.insert(ai.provider, ProviderSettings { base_url: legacy_url.trim().to_string(), model: f.model.unwrap_or_default() });
        }
        ai
    }
}

impl AiSettings {
    /// Werte des aktiven Anbieters; feste Adressen kommen immer aus der Anbieterliste.
    pub fn active(&self) -> ProviderSettings {
        let mut s = self.providers.get(&self.provider).cloned().unwrap_or_default();
        if !self.provider.url_editable() || s.base_url.trim().is_empty() {
            s.base_url = self.provider.default_url().to_string();
        }
        s
    }

    pub fn set_active(&mut self, s: ProviderSettings) {
        self.providers.insert(self.provider, s);
    }

    /// Wofür eine Zustimmung gilt: der Anbieter, beim eigenen Endpunkt die Adresse.
    /// `None`, wenn die Bilder den Rechner bzw. das lokale Netz nicht verlassen.
    pub fn consent_key(&self) -> Option<String> {
        // Bei einstellbarer Adresse (Ollama, LM Studio, eigener Endpunkt) entscheidet die Adresse.
        if self.provider.url_editable() {
            let url = self.active().base_url.trim().trim_end_matches('/').to_ascii_lowercase();
            return (!is_local_url(&url)).then(|| format!("{}:{url}", self.provider.id()));
        }
        Some(self.provider.id().to_string())
    }

    pub fn needs_consent(&self) -> bool {
        self.consent_key().is_some_and(|k| !self.cloud_consent.contains(&k))
    }

    pub fn give_consent(&mut self) {
        if let Some(k) = self.consent_key()
            && !self.cloud_consent.contains(&k)
        {
            self.cloud_consent.push(k);
        }
    }

    pub fn is_configured(&self) -> bool {
        let s = self.active();
        !s.base_url.trim().is_empty() && !s.model.trim().is_empty()
    }
}

impl Default for Config {
    fn default() -> Self {
        let first = builtin_presets().remove(0);
        Config { user_presets: Vec::new(), current: first.settings, target: Target::WebSubfolder, delete_sources: false, last_preset: Some(first.name), ai: AiSettings::default(), ui_lang: None, ai_lang: None, name_rules: Default::default(), update_check: true, last_update_check: 0, install_declined: false }
    }
}

/// `%APPDATA%\OSULZER\ImageOptimizer` (bzw. das Gegenstück auf anderen Systemen).
pub fn config_dir() -> PathBuf {
    app_data().join("OSULZER").join("ImageOptimizer")
}

/// Ordner des früheren Bildoptimierers, nur noch für den Umzug.
pub fn legacy_config_dir() -> PathBuf {
    app_data().join("Bildoptimierer")
}

fn app_data() -> PathBuf {
    directories::BaseDirs::new().map(|d| d.config_dir().to_path_buf()).unwrap_or_else(std::env::temp_dir)
}

/// `config.json`; die Umgebungsvariable `OSULZER_IMAGEOPTIMIZER_CONFIG` kann eine andere Datei vorgeben
/// (für Skripte und Tests).
pub fn config_file() -> PathBuf {
    match std::env::var_os("OSULZER_IMAGEOPTIMIZER_CONFIG") {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => config_dir().join("config.json"),
    }
}

/// Lädt die Konfiguration. Fehlt die Datei oder ist sie defekt, gelten die Standardwerte;
/// eine defekte Datei wird als `config.json.bak` aufgehoben.
pub fn load_from(file: &Path) -> Config {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Config::default();
    };
    match serde_json::from_str(&text) {
        Ok(config) => config,
        Err(_) => {
            let mut bak = file.as_os_str().to_owned();
            bak.push(".bak");
            let _ = std::fs::rename(file, PathBuf::from(bak));
            Config::default()
        }
    }
}

/// Speichert atomar und legt den Ordner bei Bedarf an.
pub fn save_to(file: &Path, c: &Config) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(c).map_err(std::io::Error::other)?;
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, file)
}

impl Config {
    /// Mitgelieferte Vorgaben zuerst, dann die eigenen.
    pub fn all_presets(&self) -> Vec<Preset> {
        let mut all = builtin_presets();
        all.extend(self.user_presets.iter().cloned());
        all
    }

    /// Auch die Namen in den anderen Sprachen zählen, sonst gäbe es nach einem Sprachwechsel Doppelnamen.
    pub fn is_builtin(name: &str) -> bool {
        Lang::ALL.iter().any(|&l| builtin_presets_in(l).iter().any(|p| p.name == name))
    }

    /// Nur eigene Vorgaben lassen sich löschen; auch eine, die zufällig wie eine mitgelieferte heißt.
    pub fn can_delete(&self, name: &str) -> bool {
        self.user_presets.iter().any(|p| p.name == name)
    }

    pub fn ui_lang(&self) -> Lang {
        self.ui_lang.unwrap_or_else(detect_system_lang)
    }

    pub fn ai_lang(&self) -> Lang {
        self.ai_lang.unwrap_or_else(|| self.ui_lang())
    }

    /// Speichert eine eigene Vorgabe; ein vorhandener eigener Name wird ersetzt.
    pub fn save_preset(&mut self, name: &str, s: &Settings) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(crate::i18n::t("preset.error.empty").into());
        }
        if Self::is_builtin(name) {
            return Err(crate::i18n::tf("preset.error.builtin", &[("name", &name)]));
        }
        match self.user_presets.iter_mut().find(|p| p.name == name) {
            Some(existing) => existing.settings = s.clone(),
            None => self.user_presets.push(Preset { name: name.to_string(), settings: s.clone() }),
        }
        Ok(())
    }

    /// Löscht eine eigene Vorgabe. Mitgelieferte lassen sich nicht löschen.
    pub fn delete_preset(&mut self, name: &str) -> bool {
        let before = self.user_presets.len();
        self.user_presets.retain(|p| p.name != name);
        self.user_presets.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Format;

    fn custom() -> Settings {
        Settings { max_edge: Some(1000), format: Format::Webp, quality: 70, max_kb: Some(300) }
    }

    #[test]
    fn legacy_ai_settings_become_provider_entry() {
        use crate::ai::provider::Provider;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.json");
        std::fs::write(&file, r#"{"ai":{"base_url":"http://localhost:1234/v1","model":"qwen/qwen3.8-27b"}}"#).unwrap();
        let c = load_from(&file);
        assert_eq!(c.ai.provider, Provider::LmStudio);
        assert_eq!(c.ai.active().model, "qwen/qwen3.8-27b");
        assert_eq!(c.ai.active().base_url, "http://localhost:1234/v1");
        std::fs::write(&file, r#"{"ai":{"base_url":"https://ki.example.com/v1","model":"m"}}"#).unwrap();
        let c = load_from(&file);
        assert_eq!(c.ai.provider, Provider::Custom);
        assert_eq!(c.ai.active().base_url, "https://ki.example.com/v1");
        assert!(c.ai.is_configured());
    }

    #[test]
    fn fixed_url_cannot_be_overridden() {
        use crate::ai::provider::Provider;
        let mut ai = AiSettings { provider: Provider::OpenAi, ..AiSettings::default() };
        ai.set_active(ProviderSettings { base_url: "https://evil.example/v1".into(), model: "x".into() });
        assert_eq!(ai.active().base_url, "https://api.openai.com/v1");
        assert_eq!(ai.active().model, "x");
    }

    #[test]
    fn switching_provider_keeps_each_entry() {
        use crate::ai::provider::Provider;
        let mut ai = AiSettings { provider: Provider::LmStudio, ..AiSettings::default() };
        ai.set_active(ProviderSettings { base_url: "http://localhost:1234/v1".into(), model: "qwen".into() });
        ai.provider = Provider::Anthropic;
        assert_eq!(ai.active().model, "");
        assert!(!ai.is_configured());
        ai.set_active(ProviderSettings { base_url: String::new(), model: "claude-haiku-5-5".into() });
        ai.provider = Provider::LmStudio;
        assert_eq!(ai.active().model, "qwen");
    }

    #[test]
    fn ai_settings_roundtrip_without_legacy_fields() {
        use crate::ai::provider::Provider;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.json");
        let mut c = Config::default();
        c.ai.provider = Provider::Gemini;
        c.ai.set_active(ProviderSettings { base_url: String::new(), model: "g".into() });
        save_to(&file, &c).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\"provider\": \"gemini\""), "{text}");
        assert_eq!(load_from(&file), c);
    }

    #[test]
    fn local_providers_need_no_consent() {
        use crate::ai::provider::Provider;
        for p in [Provider::Ollama, Provider::LmStudio] {
            let ai = AiSettings { provider: p, ..AiSettings::default() };
            assert!(!ai.needs_consent(), "{p:?}");
        }
    }

    #[test]
    fn cloud_provider_needs_consent_once() {
        use crate::ai::provider::Provider;
        let mut ai = AiSettings { provider: Provider::Anthropic, ..AiSettings::default() };
        assert!(ai.needs_consent());
        ai.give_consent();
        assert!(!ai.needs_consent());
        ai.provider = Provider::OpenAi;
        assert!(ai.needs_consent());
    }

    #[test]
    fn ollama_or_lmstudio_with_internet_address_needs_consent() {
        use crate::ai::provider::Provider;
        for p in [Provider::Ollama, Provider::LmStudio] {
            let mut ai = AiSettings { provider: p, ..AiSettings::default() };
            assert!(!ai.needs_consent(), "{p:?} lokal");
            ai.set_active(ProviderSettings { base_url: "https://ki-tunnel.example.com/v1".into(), model: "m".into() });
            assert!(ai.needs_consent(), "{p:?} im Internet");
            ai.give_consent();
            assert!(!ai.needs_consent());
        }
    }

    #[test]
    fn custom_url_change_to_internet_asks_again() {
        use crate::ai::provider::Provider;
        let mut ai = AiSettings { provider: Provider::Custom, ..AiSettings::default() };
        let set = |ai: &mut AiSettings, url: &str| ai.set_active(ProviderSettings { base_url: url.into(), model: "m".into() });
        set(&mut ai, "http://192.168.1.5/v1");
        assert!(!ai.needs_consent());
        set(&mut ai, "https://ki.example.com/v1");
        assert!(ai.needs_consent());
        ai.give_consent();
        assert!(!ai.needs_consent());
        set(&mut ai, "https://andere.example.com/v1");
        assert!(ai.needs_consent());
    }

    #[test]
    fn migrated_preset_named_like_builtin_can_be_deleted() {
        let mut c = Config::default();
        c.user_presets.push(Preset { name: "Thumbnail".into(), settings: custom() });
        assert!(c.can_delete("Thumbnail"));
        assert!(!c.can_delete("Web groß"));
        assert!(c.delete_preset("Thumbnail"));
    }

    #[test]
    fn old_config_without_lang_fields_loads() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.json");
        std::fs::write(&file, r#"{"last_preset":"Web groß","target":"WebSubfolder"}"#).unwrap();
        let c = load_from(&file);
        assert_eq!((c.ui_lang, c.ai_lang), (None, None));
        assert_eq!(c.last_preset.as_deref(), Some("Web groß"));
    }

    #[test]
    fn ai_lang_defaults_to_ui_lang() {
        use crate::i18n::Lang;
        let c = Config { ui_lang: Some(Lang::De), ..Config::default() };
        assert_eq!(c.ai_lang(), Lang::De);
        let c = Config { ui_lang: Some(Lang::De), ai_lang: Some(Lang::En), ..Config::default() };
        assert_eq!((c.ui_lang(), c.ai_lang()), (Lang::De, Lang::En));
    }

    #[test]
    fn builtin_name_of_other_language_is_rejected() {
        let mut c = Config::default();
        for name in ["Web large", "Web groß", "Thumbnail", "Vorschaubild"] {
            assert!(c.save_preset(name, &custom()).is_err(), "{name}");
        }
    }

    #[test]
    fn default_starts_with_web_gross_into_web_subfolder() {
        crate::i18n::set_lang(crate::i18n::Lang::De);
        let c = Config::default();
        assert_eq!(c.current, builtin_presets()[0].settings);
        assert_eq!(c.target, Target::WebSubfolder);
        assert_eq!(c.last_preset.as_deref(), Some("Web groß"));
    }

    #[test]
    fn roundtrip_creates_folder_and_keeps_everything() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("neu").join("config.json");
        let mut c = Config::default();
        c.save_preset("Kunde Müller", &custom()).unwrap();
        c.current = custom();
        c.target = Target::Suffix;
        c.last_preset = None;
        save_to(&file, &c).unwrap();
        assert_eq!(load_from(&file), c);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_from(&dir.path().join("config.json")), Config::default());
    }

    #[test]
    fn broken_file_gives_defaults_and_is_kept_as_bak() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.json");
        std::fs::write(&file, "{kaputt").unwrap();
        assert_eq!(load_from(&file), Config::default());
        assert_eq!(std::fs::read_to_string(dir.path().join("config.json.bak")).unwrap(), "{kaputt");
    }

    #[test]
    fn builtin_names_and_empty_names_are_rejected() {
        let mut c = Config::default();
        assert!(c.save_preset("Web groß", &custom()).is_err());
        assert!(c.save_preset("   ", &custom()).is_err());
        assert!(c.user_presets.is_empty());
    }

    #[test]
    fn saving_same_name_twice_replaces() {
        let mut c = Config::default();
        c.save_preset("Meins", &custom()).unwrap();
        let mut other = custom();
        other.quality = 55;
        c.save_preset(" Meins ", &other).unwrap();
        assert_eq!(c.user_presets.len(), 1);
        assert_eq!(c.user_presets[0].name, "Meins");
        assert_eq!(c.user_presets[0].settings.quality, 55);
    }

    #[test]
    fn all_presets_lists_builtins_first() {
        crate::i18n::set_lang(crate::i18n::Lang::De);
        let mut c = Config::default();
        c.save_preset("Meins", &custom()).unwrap();
        let names: Vec<_> = c.all_presets().into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["Web groß", "Web mittel", "Vorschaubild", "Meins"]);
    }

    #[test]
    fn only_own_presets_can_be_deleted() {
        let mut c = Config::default();
        c.save_preset("Meins", &custom()).unwrap();
        assert!(!c.delete_preset("Web groß"));
        assert!(c.delete_preset("Meins"));
        assert!(!c.delete_preset("Meins"));
        assert_eq!(c.all_presets().len(), 3);
    }
}
