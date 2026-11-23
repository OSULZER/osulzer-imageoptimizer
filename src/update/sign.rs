//! Ed25519-Signaturen für Release-Dateien. Der öffentliche Schlüssel ist in die Exe eingebaut.

/// Öffentlicher Schlüssel (Base64); `PLACEHOLDER`, solange noch kein echter erzeugt wurde.
pub const PUBLIC_KEY: &str = include_str!("../../assets/update-public-key.txt");

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

fn key_bytes(b64: &str) -> Option<[u8; 32]> {
    STANDARD.decode(b64.trim()).ok()?.try_into().ok()
}

/// Ist das ein gültiger öffentlicher Schlüssel (und nicht der Platzhalter)?
pub fn is_real_key(public_key_b64: &str) -> bool {
    key_bytes(public_key_b64).is_some_and(|k| VerifyingKey::from_bytes(&k).is_ok())
}

pub fn has_real_key() -> bool {
    is_real_key(PUBLIC_KEY)
}

/// Prüft eine Signatur; jedes falsche Format gilt als ungültig.
pub fn verify(data: &[u8], signature_b64: &str, public_key_b64: &str) -> bool {
    let Some(key) = key_bytes(public_key_b64).and_then(|k| VerifyingKey::from_bytes(&k).ok()) else { return false };
    let Some(sig) = STANDARD.decode(signature_b64.trim()).ok().and_then(|s| <[u8; 64]>::try_from(s).ok()) else { return false };
    key.verify_strict(data, &Signature::from_bytes(&sig)).is_ok()
}

pub fn sign(data: &[u8], private_key_b64: &str) -> Result<String, String> {
    let key = key_bytes(private_key_b64).ok_or("private key must be 32 bytes, Base64")?;
    Ok(STANDARD.encode(SigningKey::from_bytes(&key).sign(data).to_bytes()))
}

/// Neues Schlüsselpaar (privat, öffentlich), Base64.
pub fn generate() -> (String, String) {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("system random generator");
    let key = SigningKey::from_bytes(&seed);
    (STANDARD.encode(key.to_bytes()), STANDARD.encode(key.verifying_key().to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_verifies() {
        let (private, public) = generate();
        let sig = sign(b"hallo", &private).unwrap();
        assert!(verify(b"hallo", &sig, &public));
    }

    #[test]
    fn tampered_data_fails() {
        let (private, public) = generate();
        let sig = sign(b"hallo", &private).unwrap();
        assert!(!verify(b"hallO", &sig, &public));
    }

    #[test]
    fn wrong_key_fails() {
        let (private, _) = generate();
        let (_, other) = generate();
        let sig = sign(b"x", &private).unwrap();
        assert!(!verify(b"x", &sig, &other));
    }

    #[test]
    fn garbage_signature_fails() {
        let (_, public) = generate();
        assert!(!verify(b"x", "kein base64!", &public));
        assert!(!verify(b"x", "AAAA", &public));
        assert!(!verify(b"x", "AAAA", "PLACEHOLDER"));
        assert!(sign(b"x", "falsch").is_err());
    }

    #[test]
    fn placeholder_is_not_a_real_key() {
        assert!(!is_real_key("PLACEHOLDER"));
        assert!(!is_real_key(""));
        assert!(is_real_key(&generate().1));
    }
}
