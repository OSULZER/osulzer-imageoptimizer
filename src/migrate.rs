//! Einmaliger Umzug einer Bildoptimierer-Installation.

use crate::ai::provider::Provider;
use crate::ai::secret;
use crate::config::{config_dir, legacy_config_dir};
use crate::i18n::{t, tf};
use crate::shell;
use std::path::Path;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MigrationReport {
    pub config_copied: bool,
    pub key_moved: bool,
    pub menu_moved: bool,
    pub errors: Vec<String>,
}

/// Kopiert `config.json` aus dem alten Ordner, wenn es im neuen noch keine gibt.
/// Der alte Ordner bleibt liegen. Gibt zurück, ob kopiert wurde.
pub fn migrate_config(old_dir: &Path, new_dir: &Path) -> Result<bool, String> {
    let (old, new) = (old_dir.join("config.json"), new_dir.join("config.json"));
    if new.exists() || !old.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(new_dir).map_err(|e| e.to_string())?;
    std::fs::copy(&old, &new).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Einmaliger Umzug von Einstellungen, Schlüssel und Kontextmenü. Bricht nie ab;
/// was nicht klappt, steht in `errors`.
pub fn run(exe: &Path) -> MigrationReport {
    let mut report = MigrationReport::default();
    match migrate_config(&legacy_config_dir(), &config_dir()) {
        Ok(copied) => report.config_copied = copied,
        Err(e) => report.errors.push(tf("migrate.error", &[("e", &e)])),
    }
    // Erst der Schlüssel des Bildoptimierers in den gemeinsamen Eintrag, dann zum aktiven Anbieter.
    match move_key(
        secret::load_shared_key(),
        secret::load_legacy_key(),
        |k| secret::store_shared_key(Some(k)),
        secret::load_shared_key,
        secret::delete_legacy_key,
    ) {
        Ok(moved) => report.key_moved = moved,
        Err(e) => report.errors.push(tf("migrate.error", &[("e", &e)])),
    }
    let provider = crate::config::load_from(&crate::config::config_file()).ai.provider;
    match migrate_shared_key_with(provider, secret::load_key_for, secret::load_shared_key, |p, k| secret::store_key_for(p, Some(k)), secret::delete_shared_key) {
        Ok(moved) => report.key_moved |= moved,
        Err(e) => report.errors.push(tf("migrate.error", &[("e", &e)])),
    }
    match move_menu(shell::legacy_registered(), || shell::register(exe, t("menu.label")), shell::remove_legacy) {
        Ok(moved) => report.menu_moved = moved,
        Err(e) => report.errors.push(tf("migrate.error", &[("e", &e)])),
    }
    // Ein vorhandener Eintrag bekommt den aktuellen Text (z. B. nach einem Update mit neuem Wortlaut).
    if let Err(e) = refresh_menu(shell::status(exe) == shell::Registration::Current, || shell::register(exe, t("menu.label"))) {
        report.errors.push(tf("migrate.error", &[("e", &e)]));
    }
    report
}

/// Schreibt den Kontextmenü-Eintrag neu, wenn er auf diese Exe zeigt. Gibt zurück, ob geschrieben wurde.
pub fn refresh_menu(registered_here: bool, register: impl FnOnce() -> std::io::Result<()>) -> std::io::Result<bool> {
    if !registered_here {
        return Ok(false);
    }
    register()?;
    Ok(true)
}

/// Übernimmt den alten Schlüssel, wenn es noch keinen neuen gibt. Der alte wird erst gelöscht,
/// wenn der neue nachweislich gespeichert ist.
pub fn move_key(
    new: Option<String>,
    old: Option<String>,
    store: impl FnOnce(&str) -> Result<(), String>,
    reload: impl FnOnce() -> Option<String>,
    delete_old: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    let (None, Some(old)) = (new, old) else { return Ok(false) };
    store(&old)?;
    if reload().as_deref() != Some(old.trim()) {
        return Err(t("migrate.key_not_stored").to_string());
    }
    delete_old()?;
    Ok(true)
}

/// Gibt den gemeinsamen Schlüssel aus der Zeit vor den Anbietern dem aktiven Anbieter.
pub fn migrate_shared_key_with(
    provider: Provider,
    load_for: impl Fn(Provider) -> Option<String>,
    load_shared: impl FnOnce() -> Option<String>,
    store_for: impl FnOnce(Provider, &str) -> Result<(), String>,
    delete_shared: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    move_key(load_for(provider), load_shared(), |k| store_for(provider, k), || load_for(provider), delete_shared)
}

/// Trägt erst den neuen Kontextmenü-Eintrag ein und entfernt danach den alten,
/// damit bei einem Fehler nicht beide fehlen.
pub fn move_menu(
    legacy: bool,
    register: impl FnOnce() -> std::io::Result<()>,
    remove_legacy: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<bool> {
    if !legacy {
        return Ok(false);
    }
    register()?;
    remove_legacy()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn key_is_moved_and_old_deleted_after_read_back() {
        let stored = RefCell::new(None::<String>);
        let deleted = RefCell::new(false);
        let moved = move_key(
            None,
            Some("geheim".into()),
            |k| {
                *stored.borrow_mut() = Some(k.to_string());
                Ok(())
            },
            || stored.borrow().clone(),
            || {
                *deleted.borrow_mut() = true;
                Ok(())
            },
        );
        assert_eq!(moved, Ok(true));
        assert!(*deleted.borrow());
    }

    #[test]
    fn old_key_stays_when_store_does_not_stick() {
        let deleted = RefCell::new(false);
        let moved = move_key(None, Some("geheim".into()), |_| Ok(()), || None, || {
            *deleted.borrow_mut() = true;
            Ok(())
        });
        assert!(moved.is_err());
        assert!(!*deleted.borrow(), "alter Schlüssel darf nicht gelöscht werden");
    }

    #[test]
    fn existing_new_key_is_never_overwritten() {
        let moved = move_key(Some("neu".into()), Some("alt".into()), |_| panic!("darf nicht schreiben"), || None, || panic!("darf nicht löschen"));
        assert_eq!(moved, Ok(false));
    }

    #[test]
    fn shared_key_goes_to_active_provider() {
        use crate::ai::provider::Provider;
        let stored = RefCell::new(None::<(Provider, String)>);
        let moved = migrate_shared_key_with(
            Provider::LmStudio,
            |_| stored.borrow().as_ref().map(|(_, k)| k.clone()),
            || Some("geheim".into()),
            |p, k| {
                *stored.borrow_mut() = Some((p, k.to_string()));
                Ok(())
            },
            || Ok(()),
        );
        assert_eq!(moved, Ok(true));
        assert_eq!(stored.borrow().clone(), Some((Provider::LmStudio, "geheim".to_string())));
    }

    #[test]
    fn menu_text_is_refreshed_only_for_own_entry() {
        let calls = RefCell::new(0);
        assert!(refresh_menu(true, || {
            *calls.borrow_mut() += 1;
            Ok(())
        })
        .unwrap());
        assert!(!refresh_menu(false, || panic!("fremden oder fehlenden Eintrag nicht anlegen")).unwrap());
        assert_eq!(*calls.borrow(), 1);
    }

    #[test]
    fn failed_register_keeps_legacy_menu_entry() {
        let removed = RefCell::new(false);
        let result = move_menu(true, || Err(std::io::Error::other("kaputt")), || {
            *removed.borrow_mut() = true;
            Ok(())
        });
        assert!(result.is_err());
        assert!(!*removed.borrow(), "alter Eintrag muss bleiben, bis der neue steht");
    }

    #[test]
    fn menu_is_moved_register_first() {
        let order = RefCell::new(Vec::new());
        let result = move_menu(true, || {
            order.borrow_mut().push("register");
            Ok(())
        }, || {
            order.borrow_mut().push("remove");
            Ok(())
        });
        assert!(result.unwrap());
        assert_eq!(*order.borrow(), ["register", "remove"]);
        assert!(!move_menu(false, || panic!(), || panic!()).unwrap());
    }

    #[test]
    fn migrate_config_copies_when_new_missing() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join("Bildoptimierer"), dir.path().join("OSULZER").join("ImageOptimizer"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("config.json"), "{\"a\":1}").unwrap();
        assert_eq!(migrate_config(&old, &new), Ok(true));
        assert_eq!(std::fs::read_to_string(new.join("config.json")).unwrap(), "{\"a\":1}");
        assert!(old.join("config.json").exists());
    }

    #[test]
    fn migrate_config_never_overwrites_new() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join("alt"), dir.path().join("neu"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("config.json"), "alt").unwrap();
        std::fs::write(new.join("config.json"), "neu").unwrap();
        assert_eq!(migrate_config(&old, &new), Ok(false));
        assert_eq!(std::fs::read_to_string(new.join("config.json")).unwrap(), "neu");
    }

    #[test]
    fn migrate_config_without_old_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let new = dir.path().join("neu");
        assert_eq!(migrate_config(&dir.path().join("alt"), &new), Ok(false));
        assert!(!new.exists());
    }

    #[test]
    fn migrate_config_twice_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join("alt"), dir.path().join("neu"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("config.json"), "alt").unwrap();
        assert_eq!(migrate_config(&old, &new), Ok(true));
        std::fs::write(new.join("config.json"), "geaendert").unwrap();
        assert_eq!(migrate_config(&old, &new), Ok(false));
        assert_eq!(std::fs::read_to_string(new.join("config.json")).unwrap(), "geaendert");
    }
}
