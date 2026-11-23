//! Prüft den Schlüssel-Umzug am echten Schlüsselspeicher: legt einen Probe-Schlüssel unter dem alten
//! Namen an, zieht ihn über den gemeinsamen Eintrag zum Anbieter um und räumt danach alles wieder auf.
//! Vorhandene echte Schlüssel bleiben unberührt.

use osulzer_imageoptimizer::ai::provider::Provider;
use osulzer_imageoptimizer::ai::secret::*;
use osulzer_imageoptimizer::migrate::{migrate_shared_key_with, move_key};

fn main() {
    let p = Provider::Custom;
    if load_key_for(p).is_some() || load_shared_key().is_some() || load_legacy_key().is_some() {
        println!("Es gibt schon einen Schlüssel, Probe übersprungen.");
        return;
    }
    keyring::Entry::new("Bildoptimierer KI", "api-key").unwrap().set_password("probe-123").unwrap();
    let first = move_key(load_shared_key(), load_legacy_key(), |k| store_shared_key(Some(k)), load_shared_key, delete_legacy_key);
    let second = migrate_shared_key_with(p, load_key_for, load_shared_key, |p, k| store_key_for(p, Some(k)), delete_shared_key);
    println!("Schritt 1: {first:?}, Schritt 2: {second:?}");
    println!("Anbieter: {:?}, gemeinsam: {:?}, alt: {:?}", load_key_for(p), load_shared_key(), load_legacy_key());
    let ok = first == Ok(true)
        && second == Ok(true)
        && load_key_for(p).as_deref() == Some("probe-123")
        && load_shared_key().is_none()
        && load_legacy_key().is_none();
    store_key_for(p, None).unwrap();
    println!("aufgeräumt: {:?} {:?} {:?}", load_key_for(p), load_shared_key(), load_legacy_key());
    println!("{}", if ok { "ERGEBNIS: OK" } else { "ERGEBNIS: FEHLER" });
}
