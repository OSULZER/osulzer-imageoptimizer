//! Austausch der laufenden Exe gegen die neue Fassung. Windows erlaubt, eine laufende Exe
//! umzubenennen, aber nicht, sie zu überschreiben.

use std::path::{Path, PathBuf};

fn old_path(exe: &Path) -> PathBuf {
    let mut p = exe.as_os_str().to_owned();
    p.push(".old");
    PathBuf::from(p)
}

/// `exe` → `exe.old`, `new_file` → `exe`. Scheitert der zweite Schritt, wird der erste rückgängig gemacht.
pub fn swap(exe: &Path, new_file: &Path) -> Result<(), String> {
    swap_with(exe, new_file, |from, to| std::fs::rename(from, to))
}

/// Wie `swap`, mit austauschbarem Umbenennen (für Tests des Rückwegs).
pub fn swap_with(exe: &Path, new_file: &Path, rename: impl Fn(&Path, &Path) -> std::io::Result<()>) -> Result<(), String> {
    if !new_file.is_file() {
        return Err(crate::i18n::t("update.error.missing").into());
    }
    let old = old_path(exe);
    let _ = std::fs::remove_file(&old);
    rename(exe, &old).map_err(|e| e.to_string())?;
    if let Err(e) = rename(new_file, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(e.to_string());
    }
    Ok(())
}

/// Reste eines früheren Updates entfernen (beim Start).
pub fn cleanup_old(exe: &Path) {
    let _ = std::fs::remove_file(old_path(exe));
}
