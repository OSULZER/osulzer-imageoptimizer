//! API-Schlüssel in der Windows-Anmeldeinformationsverwaltung (nie in `config.json`), einer je Anbieter.

use super::provider::Provider;

const SERVICE: &str = "OSULZER ImageOptimizer KI";
const LEGACY_SERVICE: &str = "Bildoptimierer KI";
/// Gemeinsamer Eintrag aus der Zeit vor den Anbietern (nur noch für den Umzug).
const SHARED_USER: &str = "api-key";

fn load(service: &str, user: &str) -> Option<String> {
    keyring::Entry::new(service, user).ok()?.get_password().ok().filter(|k| !k.is_empty())
}

fn delete(service: &str, user: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(service, user).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn store(service: &str, user: &str, key: Option<&str>) -> Result<(), String> {
    match key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => keyring::Entry::new(service, user).and_then(|e| e.set_password(k)).map_err(|e| e.to_string()),
        None => delete(service, user),
    }
}

pub fn load_key_for(p: Provider) -> Option<String> {
    load(SERVICE, p.id())
}

/// Speichert den Schlüssel des Anbieters; `None` oder leer löscht ihn.
pub fn store_key_for(p: Provider, key: Option<&str>) -> Result<(), String> {
    store(SERVICE, p.id(), key)
}

pub fn load_shared_key() -> Option<String> {
    load(SERVICE, SHARED_USER)
}

pub fn store_shared_key(key: Option<&str>) -> Result<(), String> {
    store(SERVICE, SHARED_USER, key)
}

pub fn delete_shared_key() -> Result<(), String> {
    delete(SERVICE, SHARED_USER)
}

/// Schlüssel des früheren Bildoptimierers, nur noch für den Umzug.
pub fn load_legacy_key() -> Option<String> {
    load(LEGACY_SERVICE, SHARED_USER)
}

pub fn delete_legacy_key() -> Result<(), String> {
    delete(LEGACY_SERVICE, SHARED_USER)
}
