//! Eintrag im Explorer-Kontextmenü. Der einzige plattformspezifische Teil des Programms.

use std::path::{Path, PathBuf};

pub const EXTENSIONS: [&str; 7] = [".jpg", ".jpeg", ".png", ".webp", ".bmp", ".tif", ".tiff"];

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Registration {
    /// Kein Eintrag vorhanden.
    None,
    /// Der Eintrag zeigt auf diese `.exe`.
    Current,
    /// Der Eintrag zeigt auf eine andere `.exe`.
    Stale(PathBuf),
}

pub fn status(exe: &Path) -> Registration {
    imp::status(imp::BASE, exe)
}

/// `label` ist der Text im Kontextmenü (in der Oberflächensprache).
pub fn register(exe: &Path, label: &str) -> std::io::Result<()> {
    imp::register(imp::BASE, exe, label)
}

/// Gibt es noch den Eintrag des alten Bildoptimierers?
pub fn legacy_registered() -> bool {
    imp::legacy_registered(imp::BASE)
}

/// Entfernt den Eintrag des alten Bildoptimierers.
pub fn remove_legacy() -> std::io::Result<()> {
    imp::remove_legacy(imp::BASE)
}

pub fn unregister() -> std::io::Result<()> {
    imp::unregister(imp::BASE)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    pub const BASE: &str = r"Software\Classes\SystemFileAssociations";

    const VERB: &str = "OsulzerImageOptimizer";
    const LEGACY_VERB: &str = "Bildoptimierer";

    fn verb_key(base: &str, ext: &str) -> String {
        format!(r"{base}\{ext}\shell\{VERB}")
    }

    fn legacy_key(base: &str, ext: &str) -> String {
        format!(r"{base}\{ext}\shell\{LEGACY_VERB}")
    }

    pub fn legacy_registered(base: &str) -> bool {
        EXTENSIONS.iter().any(|ext| hkcu().open_subkey(legacy_key(base, ext)).is_ok())
    }

    pub fn remove_legacy(base: &str) -> std::io::Result<()> {
        delete_all(base, legacy_key)
    }

    fn delete_all(base: &str, key: fn(&str, &str) -> String) -> std::io::Result<()> {
        for ext in EXTENSIONS {
            match hkcu().delete_subkey_all(key(base, ext)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    /// Liest den eingetragenen Pfad aus dem Befehl `"<exe>" "%1"` der ersten Endung.
    pub fn status(base: &str, exe: &Path) -> Registration {
        let command: Option<String> = hkcu()
            .open_subkey(format!(r"{}\command", verb_key(base, EXTENSIONS[0])))
            .and_then(|k| k.get_value(""))
            .ok();
        let Some(command) = command else {
            return Registration::None;
        };
        let registered = command.split('"').nth(1).unwrap_or_default();
        if registered.eq_ignore_ascii_case(&exe.to_string_lossy()) {
            Registration::Current
        } else {
            Registration::Stale(PathBuf::from(registered))
        }
    }

    pub fn register(base: &str, exe: &Path, label: &str) -> std::io::Result<()> {
        let exe = exe.to_string_lossy();
        for ext in EXTENSIONS {
            let (verb, _) = hkcu().create_subkey(verb_key(base, ext))?;
            verb.set_value("", &label)?;
            // Ohne Anführungszeichen und Index: so zeigt der Explorer das Symbol zuverlässig.
            verb.set_value("Icon", &exe.as_ref())?;
            // Ohne diesen Wert blendet der Explorer den Eintrag bei mehr als 15 Dateien aus.
            verb.set_value("MultiSelectModel", &"Player")?;
            let (command, _) = verb.create_subkey("command")?;
            command.set_value("", &format!(r#""{exe}" "%1""#))?;
        }
        Ok(())
    }

    /// Entfernt nur den eigenen Schlüssel; fremde Einträge bleiben unberührt.
    pub fn unregister(base: &str) -> std::io::Result<()> {
        delete_all(base, verb_key)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;

        /// Eigener Registry-Zweig je Test, der am Ende wieder verschwindet.
        struct TestBase(String);

        impl TestBase {
            fn new(tag: &str) -> Self {
                TestBase(format!(r"Software\OsulzerImageOptimizerTest\{}-{tag}", std::process::id()))
            }
        }

        impl Drop for TestBase {
            fn drop(&mut self) {
                let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.0);
                // Gelingt nur, wenn kein anderer Test mehr darunter arbeitet.
                let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey(r"Software\OsulzerImageOptimizerTest");
            }
        }

        fn read(base: &str, sub: &str, value: &str) -> String {
            RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!(r"{base}\{sub}")).unwrap().get_value(value).unwrap()
        }

        #[test]
        fn register_writes_all_extensions() {
            let base = TestBase::new("register");
            let exe = Path::new(r"C:\Pfad mit Leer\b.exe");
            assert_eq!(status(&base.0, exe), Registration::None);
            register(&base.0, exe, "Mein Text").unwrap();
            assert_eq!(status(&base.0, exe), Registration::Current);
            assert_eq!(EXTENSIONS.len(), 7);
            for ext in EXTENSIONS {
                let verb = format!(r"{ext}\shell\OsulzerImageOptimizer");
                assert_eq!(read(&base.0, &verb, ""), "Mein Text");
                assert_eq!(read(&base.0, &verb, "Icon"), r"C:\Pfad mit Leer\b.exe");
                assert_eq!(read(&base.0, &verb, "MultiSelectModel"), "Player");
                assert_eq!(read(&base.0, &format!(r"{verb}\command"), ""), r#""C:\Pfad mit Leer\b.exe" "%1""#);
            }
        }

        #[test]
        fn other_exe_location_is_stale() {
            let base = TestBase::new("stale");
            register(&base.0, Path::new(r"C:\alt\b.exe"), "x").unwrap();
            assert_eq!(status(&base.0, Path::new(r"C:\neu\b.exe")), Registration::Stale(PathBuf::from(r"C:\alt\b.exe")));
            assert_eq!(status(&base.0, Path::new(r"c:\ALT\b.exe")), Registration::Current);
        }

        #[test]
        fn unregister_removes_only_our_key() {
            let base = TestBase::new("unregister");
            let exe = Path::new(r"C:\x\b.exe");
            register(&base.0, exe, "x").unwrap();
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            hkcu.create_subkey(format!(r"{}\.jpg\shell\Fremd", base.0)).unwrap();
            unregister(&base.0).unwrap();
            assert_eq!(status(&base.0, exe), Registration::None);
            assert!(hkcu.open_subkey(format!(r"{}\.jpg\shell\Fremd", base.0)).is_ok());
            assert!(hkcu.open_subkey(format!(r"{}\.png\shell\OsulzerImageOptimizer", base.0)).is_err());
            unregister(&base.0).unwrap();
        }

        #[test]
        fn legacy_entry_is_detected_and_removed() {
            let base = TestBase::new("legacy");
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            assert!(!legacy_registered(&base.0));
            for ext in EXTENSIONS {
                hkcu.create_subkey(format!(r"{}\{ext}\shell\Bildoptimierer\command", base.0)).unwrap();
            }
            hkcu.create_subkey(format!(r"{}\.jpg\shell\Fremd", base.0)).unwrap();
            assert!(legacy_registered(&base.0));
            remove_legacy(&base.0).unwrap();
            assert!(!legacy_registered(&base.0));
            for ext in EXTENSIONS {
                assert!(hkcu.open_subkey(format!(r"{}\{ext}\shell\Bildoptimierer", base.0)).is_err());
            }
            assert!(hkcu.open_subkey(format!(r"{}\.jpg\shell\Fremd", base.0)).is_ok());
            remove_legacy(&base.0).unwrap();
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub const BASE: &str = "";

    pub fn status(_base: &str, _exe: &Path) -> Registration {
        Registration::None
    }

    pub fn register(_base: &str, _exe: &Path, _label: &str) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }

    pub fn legacy_registered(_base: &str) -> bool {
        false
    }

    pub fn remove_legacy(_base: &str) -> std::io::Result<()> {
        Ok(())
    }

    pub fn unregister(_base: &str) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

/// Zeigt einen Ordner im Dateimanager des Systems.
pub fn open_folder(dir: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(dir).spawn();
}
