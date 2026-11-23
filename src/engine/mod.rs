//! Bildverarbeitung: Einlesen, Drehen, Skalieren, Kodieren.

mod decode;
mod encode;
mod meta;
mod resize;
mod thumb;

pub use decode::{Decoded, Probe, decode, probe};
pub use encode::{Encoded, Note, PreviewResult, preview, process};
pub use meta::{ai_input_jpeg, embed_texts, xmp_packet};
pub use resize::{resize, target_size};
pub use thumb::thumbnail;

use crate::i18n::{t, tf};
use std::fmt;

#[derive(Debug)]
pub enum EngineError {
    Io(std::io::Error),
    Unsupported,
    Animated,
    Decode(String),
    Encode(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Io(e) => f.write_str(&tf("error.io", &[("e", e)])),
            EngineError::Unsupported => f.write_str(t("error.unsupported")),
            EngineError::Animated => f.write_str(t("error.animated")),
            EngineError::Decode(e) => f.write_str(&tf("error.decode", &[("e", e)])),
            EngineError::Encode(e) => f.write_str(&tf("error.encode", &[("e", e)])),
        }
    }
}

impl std::error::Error for EngineError {}
