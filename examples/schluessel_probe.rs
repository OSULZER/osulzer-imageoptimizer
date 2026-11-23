//! Prüft, ob der Schlüsselbund funktioniert: schreibt, liest und löscht einen Probe-Schlüssel.

use osulzer_imageoptimizer::ai::provider::Provider;
use osulzer_imageoptimizer::ai::secret::{load_key_for, store_key_for};

fn main() {
    let p = Provider::Custom;
    if load_key_for(p).is_some() {
        println!("Es gibt schon einen Schlüssel für {}, Probe übersprungen.", p.id());
        return;
    }
    store_key_for(p, Some("probe-123")).unwrap();
    println!("gelesen: {:?}", load_key_for(p));
    store_key_for(p, None).unwrap();
    println!("nach dem Löschen: {:?}", load_key_for(p));
}
