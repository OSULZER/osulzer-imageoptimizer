//! Signiert eine Release-Exe für einen Tag mit dem privaten Schlüssel aus UPDATE_SIGNING_KEY und schreibt
//! <datei>.sig. Signiert wird Tag + Exe (siehe `signed_message`). Wird vom Release-Workflow aufgerufen.

use osulzer_imageoptimizer::update::check::signed_message;
use osulzer_imageoptimizer::update::sign::{PUBLIC_KEY, sign, verify};

fn main() {
    let mut args = std::env::args().skip(1);
    let (file, tag) = match (args.next(), args.next()) {
        (Some(f), Some(t)) => (f, t),
        _ => panic!("usage: sign <file> <tag, e.g. v1.0.1>"),
    };
    let key = std::env::var("UPDATE_SIGNING_KEY").expect("UPDATE_SIGNING_KEY is not set");
    let message = signed_message(&tag, &std::fs::read(&file).expect("read file"));
    let signature = sign(&message, &key).expect("sign");
    // Passt der private Schlüssel zum eingebauten öffentlichen? Sonst wären alle Updates ungültig.
    assert!(verify(&message, &signature, PUBLIC_KEY), "UPDATE_SIGNING_KEY does not match assets/update-public-key.txt");
    std::fs::write(format!("{file}.sig"), format!("{signature}\n")).expect("write signature");
    println!("{file}.sig written for {tag}");
}
