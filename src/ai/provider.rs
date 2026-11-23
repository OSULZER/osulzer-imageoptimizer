//! Die KI-Anbieter, die das Programm kennt: Adresse, Protokoll, lokal oder Cloud, Links.

use crate::i18n::t;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize)]
pub enum Provider {
    #[default]
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "openrouter")]
    OpenRouter,
    #[serde(rename = "mistral")]
    Mistral,
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "lmstudio")]
    LmStudio,
    #[serde(rename = "custom")]
    Custom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protocol {
    OpenAiCompatible,
    Anthropic,
}

impl Provider {
    pub const ALL: [Provider; 8] = [
        Provider::OpenAi,
        Provider::Anthropic,
        Provider::Gemini,
        Provider::OpenRouter,
        Provider::Mistral,
        Provider::Ollama,
        Provider::LmStudio,
        Provider::Custom,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Provider::OpenAi => "openai",
            Provider::Anthropic => "anthropic",
            Provider::Gemini => "gemini",
            Provider::OpenRouter => "openrouter",
            Provider::Mistral => "mistral",
            Provider::Ollama => "ollama",
            Provider::LmStudio => "lmstudio",
            Provider::Custom => "custom",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Provider::OpenAi => "OpenAI",
            Provider::Anthropic => "Anthropic (Claude)",
            Provider::Gemini => "Google Gemini",
            Provider::OpenRouter => "OpenRouter",
            Provider::Mistral => "Mistral",
            Provider::Ollama => t("provider.ollama"),
            Provider::LmStudio => t("provider.lmstudio"),
            Provider::Custom => t("provider.custom"),
        }
    }

    pub fn default_url(self) -> &'static str {
        match self {
            Provider::OpenAi => "https://api.openai.com/v1",
            Provider::Anthropic => "https://api.anthropic.com/v1",
            Provider::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
            Provider::OpenRouter => "https://openrouter.ai/api/v1",
            Provider::Mistral => "https://api.mistral.ai/v1",
            Provider::Ollama => "http://localhost:11434/v1",
            Provider::LmStudio => "http://localhost:1234/v1",
            Provider::Custom => "",
        }
    }

    pub fn protocol(self) -> Protocol {
        if self == Provider::Anthropic { Protocol::Anthropic } else { Protocol::OpenAiCompatible }
    }

    pub fn url_editable(self) -> bool {
        matches!(self, Provider::Ollama | Provider::LmStudio | Provider::Custom)
    }

    pub fn needs_key(self) -> bool {
        !matches!(self, Provider::Ollama | Provider::LmStudio | Provider::Custom)
    }

    /// Läuft auf dem eigenen Rechner (der eigene Endpunkt hängt von der Adresse ab).
    pub fn is_local(self) -> bool {
        matches!(self, Provider::Ollama | Provider::LmStudio)
    }

    pub fn key_url(self) -> Option<&'static str> {
        match self {
            Provider::OpenAi => Some("https://platform.openai.com/api-keys"),
            Provider::Anthropic => Some("https://console.anthropic.com/settings/keys"),
            Provider::Gemini => Some("https://aistudio.google.com/apikey"),
            Provider::OpenRouter => Some("https://openrouter.ai/settings/keys"),
            Provider::Mistral => Some("https://console.mistral.ai/api-keys"),
            Provider::Ollama => Some("https://ollama.com/download"),
            Provider::LmStudio => Some("https://lmstudio.ai"),
            Provider::Custom => None,
        }
    }

    pub fn pricing_url(self) -> Option<&'static str> {
        match self {
            Provider::OpenAi => Some("https://openai.com/api/pricing"),
            Provider::Anthropic => Some("https://www.anthropic.com/pricing"),
            Provider::Gemini => Some("https://ai.google.dev/gemini-api/docs/pricing"),
            Provider::OpenRouter => Some("https://openrouter.ai/models"),
            Provider::Mistral => Some("https://mistral.ai/pricing"),
            Provider::Ollama | Provider::LmStudio | Provider::Custom => None,
        }
    }

    /// i18n-Schlüssel der groben Kosteneinordnung.
    pub fn cost_tier(self) -> Option<&'static str> {
        match self {
            Provider::OpenAi | Provider::Anthropic | Provider::Mistral => Some("cost.low"),
            Provider::Gemini => Some("cost.very_low"),
            Provider::OpenRouter => Some("cost.depends"),
            Provider::Ollama | Provider::LmStudio => Some("cost.free"),
            Provider::Custom => None,
        }
    }

    /// i18n-Schlüssel der Anleitung.
    pub fn guide_key(self) -> &'static str {
        match self {
            Provider::OpenAi => "guide.openai",
            Provider::Anthropic => "guide.anthropic",
            Provider::Gemini => "guide.gemini",
            Provider::OpenRouter => "guide.openrouter",
            Provider::Mistral => "guide.mistral",
            Provider::Ollama => "guide.ollama",
            Provider::LmStudio => "guide.lmstudio",
            Provider::Custom => "guide.custom",
        }
    }
}

/// Host und Port einer Adresse wie `http://host:port/pfad`.
fn host_port(url: &str) -> (String, Option<u16>) {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit_once('@').map_or(authority, |(_, a)| a);
    if let Some(v6) = authority.strip_prefix('[') {
        let (host, after) = v6.split_once(']').unwrap_or((v6, ""));
        return (host.to_ascii_lowercase(), after.strip_prefix(':').and_then(|p| p.parse().ok()));
    }
    match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_ascii_lowercase(), p.parse().ok()),
        None => (authority.to_ascii_lowercase(), None),
    }
}

/// Adresse auf diesem Rechner oder im lokalen Netz.
pub fn is_local_url(url: &str) -> bool {
    let (host, _) = host_port(url);
    if host == "localhost" || host == "::1" || host.ends_with(".local") || host.ends_with(".localhost") {
        return true;
    }
    let Ok(ip) = host.parse::<std::net::Ipv4Addr>() else { return false };
    ip.is_loopback() || ip.is_private()
}

/// Ordnet eine gespeicherte Adresse einem Anbieter zu.
pub fn guess_provider(url: &str) -> Provider {
    let norm = |u: &str| u.trim().trim_end_matches('/').to_ascii_lowercase();
    if let Some(p) = Provider::ALL.iter().find(|p| p.default_url() != "" && norm(p.default_url()) == norm(url)) {
        return *p;
    }
    if is_local_url(url) {
        match host_port(url).1 {
            Some(11434) => return Provider::Ollama,
            Some(1234) => return Provider::LmStudio,
            _ => {}
        }
    }
    Provider::Custom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_has_url_and_links() {
        for p in Provider::ALL {
            match p {
                Provider::Custom => assert_eq!(p.default_url(), ""),
                p if p.is_local() => assert!(p.default_url().starts_with("http://localhost"), "{p:?}"),
                p => {
                    assert!(p.default_url().starts_with("https://"), "{p:?}");
                    assert!(p.key_url().is_some() && p.pricing_url().is_some(), "{p:?}");
                    assert!(p.needs_key(), "{p:?}");
                }
            }
        }
        assert_eq!(Provider::Anthropic.protocol(), Protocol::Anthropic);
        assert_eq!(Provider::Gemini.protocol(), Protocol::OpenAiCompatible);
    }

    #[test]
    fn local_urls_are_recognized() {
        for url in ["http://localhost:1234/v1", "http://127.0.0.1:8080", "http://192.168.1.20:1234/v1", "http://172.20.1.1/v1", "http://10.0.0.2/v1", "http://nas.local/v1", "http://[::1]:11434/v1"] {
            assert!(is_local_url(url), "{url}");
        }
        for url in ["https://api.openai.com/v1", "http://172.32.0.1", "http://8.8.8.8", "https://localhost.example.com/v1", ""] {
            assert!(!is_local_url(url), "{url}");
        }
    }

    #[test]
    fn guess_provider_maps_known_urls() {
        assert_eq!(guess_provider("http://localhost:1234/v1"), Provider::LmStudio);
        assert_eq!(guess_provider("http://192.168.1.9:1234/v1"), Provider::LmStudio);
        assert_eq!(guess_provider("http://localhost:11434/v1/"), Provider::Ollama);
        assert_eq!(guess_provider("https://api.openai.com/v1"), Provider::OpenAi);
        assert_eq!(guess_provider("https://openrouter.ai/api/v1/"), Provider::OpenRouter);
        assert_eq!(guess_provider("https://ki.example.com/v1"), Provider::Custom);
        assert_eq!(guess_provider("http://localhost:8000/v1"), Provider::Custom);
    }

    #[test]
    fn every_provider_has_a_guide_in_both_languages() {
        use crate::i18n::{Lang, t_in};
        for p in Provider::ALL {
            for key in std::iter::once(p.guide_key()).chain(p.cost_tier()) {
                for l in Lang::ALL {
                    let text = t_in(l, key);
                    assert!(text != key && !text.trim().is_empty(), "{key} fehlt in {l:?}");
                }
            }
        }
    }

    #[test]
    fn ids_are_unique_and_serialized() {
        let ids: std::collections::HashSet<_> = Provider::ALL.iter().map(|p| p.id()).collect();
        assert_eq!(ids.len(), 8);
        for p in Provider::ALL {
            assert_eq!(serde_json::to_string(&p).unwrap(), format!("\"{}\"", p.id()));
        }
    }
}
