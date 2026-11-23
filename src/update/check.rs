//! Neue Version bei GitHub suchen und die signierte Exe herunterladen.

use super::sign::verify;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

pub type Version = (u32, u32, u32);

/// Adresse der neuesten Veröffentlichung.
pub const LATEST_URL: &str = "https://api.github.com/repos/OSULZER/osulzer-imageoptimizer/releases/latest";
const EXE_NAME: &str = "osulzer-imageoptimizer.exe";
/// Höchstens so oft fragt das Programm nach Updates.
const INTERVAL_SECS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    pub notes_url: String,
    pub exe_url: String,
    pub sig_url: String,
}

/// `v1.2.3` oder `1.2.3`; Vorab-Versionen (`-beta`) und Unvollständiges zählen nicht.
pub fn parse_version(tag: &str) -> Option<Version> {
    let parts: Vec<u32> = tag.trim().trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    match parts[..] {
        [a, b, c] => Some((a, b, c)),
        _ => None,
    }
}

/// Die Veröffentlichung, falls sie neuer ist als `current` und Exe samt Signatur enthält.
pub fn newer_release(json: &str, current: &str) -> Option<Release> {
    let v: Value = serde_json::from_str(json).ok()?;
    let tag = v["tag_name"].as_str()?.to_string();
    let version = parse_version(&tag)?;
    if version <= parse_version(current)? {
        return None;
    }
    let asset = |name: &str| {
        v["assets"].as_array()?.iter().find(|a| a["name"] == name).and_then(|a| a["browser_download_url"].as_str()).map(String::from)
    };
    Some(Release {
        version,
        notes_url: v["html_url"].as_str().unwrap_or_default().to_string(),
        exe_url: asset(EXE_NAME)?,
        sig_url: asset(&format!("{EXE_NAME}.sig"))?,
        tag,
    })
}

/// Ist die letzte Prüfung mindestens einen Tag her?
pub fn due(last_check: u64, now: u64) -> bool {
    now.saturating_sub(last_check) >= INTERVAL_SECS
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).http_status_as_error(false).build().into()
}

fn user_agent() -> String {
    format!("osulzer-imageoptimizer/{}", env!("CARGO_PKG_VERSION"))
}

/// Fragt GitHub nach der neuesten Veröffentlichung. `Ok(None)`: keine neuere (oder noch keine).
pub fn fetch_latest(api_url: &str, current: &str) -> Result<Option<Release>, String> {
    let mut response = agent(Duration::from_secs(15))
        .get(api_url)
        .header("User-Agent", &user_agent())
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    match status {
        200 => Ok(newer_release(&text, current)),
        404 => Ok(None),
        other => Err(format!("HTTP {other}")),
    }
}

fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut response = agent(Duration::from_secs(300)).get(url).header("User-Agent", &user_agent()).call().map_err(|e| e.to_string())?;
    if response.status().as_u16() != 200 {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    response.body_mut().with_config().limit(200 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())
}

/// Was signiert wird: Tag und Exe. So lässt sich eine ältere, echt signierte Exe nicht als neue Version
/// ausgeben (Rollback).
pub fn signed_message(tag: &str, exe: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(tag.len() + 1 + exe.len());
    m.extend_from_slice(tag.trim().as_bytes());
    m.push(b'\n');
    m.extend_from_slice(exe);
    m
}

/// Lädt Exe und Signatur und schreibt die Exe nur bei gültiger Signatur nach `dest`.
pub fn download_verified(r: &Release, public_key: &str, dest: &Path) -> Result<(), String> {
    let exe = get_bytes(&r.exe_url)?;
    let sig = String::from_utf8(get_bytes(&r.sig_url)?).map_err(|e| e.to_string())?;
    if !verify(&signed_message(&r.tag, &exe), &sig, public_key) {
        return Err(crate::i18n::t("update.error.signature").into());
    }
    let mut tmp = dest.as_os_str().to_owned();
    tmp.push(".part");
    std::fs::write(&tmp, &exe).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    std::fs::rename(&tmp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}
