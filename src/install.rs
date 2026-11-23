//! Installieren und Deinstallieren für den aktuellen Benutzer (ohne Admin-Rechte).

use std::path::{Path, PathBuf};

/// Eintrag unter Windows „Apps“ (aktueller Benutzer).
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\OSULZER.ImageOptimizer";
const EXE_NAME: &str = "osulzer-imageoptimizer.exe";

fn local_app_data() -> PathBuf {
    directories::BaseDirs::new().map(|d| d.data_local_dir().to_path_buf()).unwrap_or_else(std::env::temp_dir)
}

/// `%LOCALAPPDATA%\Programs\OSULZER\ImageOptimizer`
pub fn install_dir() -> PathBuf {
    local_app_data().join("Programs").join("OSULZER").join("ImageOptimizer")
}

pub fn installed_exe() -> PathBuf {
    install_dir().join(EXE_NAME)
}

/// Ordner des früheren Bildoptimierers.
pub fn legacy_dir() -> PathBuf {
    local_app_data().join("Programs").join("Bildoptimierer")
}

/// Verknüpfung im Startmenü des Benutzers.
pub fn start_menu_shortcut() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join(r"Microsoft\Windows\Start Menu\Programs")
        .join("OSULZER ImageOptimizer.lnk")
}

/// Soll beim Start das Installieren angeboten werden?
pub fn should_offer(exe: &Path, installed: bool, declined: bool, install_dir: &Path) -> bool {
    !installed && !declined && !same_path(exe.parent().unwrap_or(Path::new("")), install_dir)
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

/// Ist das Programm installiert (Eintrag unter „Apps“ und Exe vorhanden)?
pub fn is_installed() -> bool {
    imp::key_exists(UNINSTALL_KEY) && installed_exe().is_file()
}

/// Uninstall-Eintrag unter `HKCU\<key_path>` schreiben.
pub fn write_uninstall_entry(key_path: &str, exe: &Path, version: &str) -> std::io::Result<()> {
    imp::write_uninstall_entry(key_path, exe, version)
}

pub fn remove_uninstall_entry(key_path: &str) -> std::io::Result<()> {
    imp::remove_key(key_path)
}

/// Löscht im Ordner des alten Bildoptimierers nur die eigenen Exe-Dateien, den Ordner nur, wenn er danach leer ist.
pub fn cleanup_legacy_dir(dir: &Path) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    for name in ["bildoptimierer.exe", EXE_NAME] {
        let file = dir.join(name);
        if file.is_file() && std::fs::remove_file(&file).is_ok() {
            removed.push(file);
        }
    }
    // remove_dir scheitert bei nicht leerem Ordner: genau das gewünschte Verhalten.
    let _ = std::fs::remove_dir(dir);
    removed
}

/// Kopiert die Exe nach `dir` (falls sie nicht schon dort läuft) und schreibt den Uninstall-Eintrag.
pub fn install_to(exe: &Path, dir: &Path, key_path: &str) -> Result<PathBuf, String> {
    let target = dir.join(EXE_NAME);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if !same_path(exe, &target) {
        std::fs::copy(exe, &target).map_err(|e| e.to_string())?;
    }
    write_uninstall_entry(key_path, &target, env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Installiert für den aktuellen Benutzer: Kopie, Kontextmenü, Startmenü, „Apps“-Eintrag; räumt den alten
/// Bildoptimierer-Ordner auf. Gibt die installierte Exe zurück.
pub fn install(exe: &Path) -> Result<PathBuf, String> {
    let installed = install_to(exe, &install_dir(), UNINSTALL_KEY)?;
    crate::shell::register(&installed, crate::i18n::t("menu.label")).map_err(|e| e.to_string())?;
    let _ = create_shortcut(&installed);
    cleanup_legacy_dir(&legacy_dir());
    Ok(installed)
}

fn create_shortcut(exe: &Path) -> Result<(), String> {
    let lnk = start_menu_shortcut();
    if let Some(dir) = lnk.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut link = mslnk::ShellLink::new(exe).map_err(|e| e.to_string())?;
    link.set_working_dir(exe.parent().map(|p| p.to_string_lossy().into_owned()));
    link.create_lnk(&lnk).map_err(|e| e.to_string())
}

/// Entfernt Kontextmenü, Verknüpfung und „Apps“-Eintrag; auf Wunsch auch Einstellungen und API-Schlüssel.
/// Die Exe selbst wird kurz nach Programmende gelöscht.
pub fn uninstall(remove_settings: bool) -> Result<(), String> {
    let _ = crate::shell::unregister();
    let _ = std::fs::remove_file(start_menu_shortcut());
    remove_uninstall_entry(UNINSTALL_KEY).map_err(|e| e.to_string())?;
    if remove_settings {
        let _ = std::fs::remove_dir_all(crate::config::config_dir());
        for p in crate::ai::provider::Provider::ALL {
            let _ = crate::ai::secret::store_key_for(p, None);
        }
        let _ = crate::ai::secret::delete_shared_key();
        let _ = crate::ai::secret::delete_legacy_key();
    }
    // Die installierte Exe löschen – auch wenn „uninstall“ aus einer anderen Kopie aufgerufen wurde.
    let _ = delete_command(&installed_exe(), 3).spawn();
    Ok(())
}

/// Befehl, der `file` nach `delay_secs` Sekunden löscht (zweiter Versuch nach weiteren 5 s, falls noch
/// gesperrt) und danach den Ordner, falls er leer ist.
pub fn delete_command(file: &Path, delay_secs: u32) -> std::process::Command {
    let dir = file.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    let f = file.display();
    let wait = |s: u32| format!("ping 127.0.0.1 -n {} > nul", s + 1);
    let script = format!(
        // Das `if` in eigenen Klammern, sonst gehört der Rest der Zeile zum `if` und `rmdir` liefe nie.
        "{} & del /f /q \"{f}\" 2> nul & (if exist \"{f}\" ({} & del /f /q \"{f}\")) & rmdir \"{dir}\" 2> nul",
        wait(delay_secs),
        wait(5)
    );
    let mut cmd = std::process::Command::new("cmd");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // raw_arg: sonst maskiert Rust die Anführungszeichen als \", was cmd nicht versteht.
        cmd.raw_arg(format!("/c \"{script}\"")).creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    cmd.arg(script);
    cmd
}

/// Beim Start aus dem Installationsordner: alten Bildoptimierer-Ordner aufräumen (die alte Exe lief beim
/// Installieren noch) und die Versionsangabe unter „Apps“ nach einem Update nachziehen.
pub fn after_start(exe: &Path) {
    if !same_path(exe, &installed_exe()) || !is_installed() {
        return;
    }
    cleanup_legacy_dir(&legacy_dir());
    let _ = write_uninstall_entry(UNINSTALL_KEY, exe, env!("CARGO_PKG_VERSION"));
}

/// `uninstall` aus „Apps“: fragt per Windows-Meldungsfenster, ob auch die Einstellungen gelöscht werden.
pub fn uninstall_interactive() -> u8 {
    match imp::ask_yes_no_cancel(crate::i18n::t("install.uninstall_title"), crate::i18n::t("install.uninstall_question")) {
        Some(remove_settings) => match uninstall(remove_settings) {
            Ok(()) => 0,
            Err(e) => {
                imp::show_error(crate::i18n::t("install.uninstall_title"), &e);
                1
            }
        },
        None => 0,
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    pub fn key_exists(key_path: &str) -> bool {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey(key_path).is_ok()
    }

    pub fn write_uninstall_entry(key_path: &str, exe: &Path, version: &str) -> std::io::Result<()> {
        let (k, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(key_path)?;
        let dir = exe.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        k.set_value("DisplayName", &"OSULZER ImageOptimizer")?;
        k.set_value("DisplayVersion", &version)?;
        k.set_value("Publisher", &"OSULZER")?;
        k.set_value("DisplayIcon", &exe.to_string_lossy().as_ref())?;
        k.set_value("InstallLocation", &dir)?;
        k.set_value("UninstallString", &format!("\"{}\" uninstall", exe.display()))?;
        k.set_value("NoModify", &1u32)?;
        k.set_value("NoRepair", &1u32)?;
        Ok(())
    }

    pub fn remove_key(key_path: &str) -> std::io::Result<()> {
        match RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(key_path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// `Some(true)` = Ja, `Some(false)` = Nein, `None` = Abbrechen.
    pub fn ask_yes_no_cancel(title: &str, text: &str) -> Option<bool> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IDNO, IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_YESNOCANCEL, MessageBoxW};
        let (t, m) = (wide(title), wide(text));
        // SAFETY: nullterminierte UTF-16-Zeichenketten, die bis zum Ende des Aufrufs leben.
        match unsafe { MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_YESNOCANCEL | MB_ICONQUESTION | MB_DEFBUTTON2) } {
            IDYES => Some(true),
            IDNO => Some(false),
            _ => None,
        }
    }

    pub fn show_error(title: &str, text: &str) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let (t, m) = (wide(title), wide(text));
        // SAFETY: wie oben.
        unsafe { MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONERROR) };
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn key_exists(_: &str) -> bool {
        false
    }
    pub fn write_uninstall_entry(_: &str, _: &Path, _: &str) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    pub fn remove_key(_: &str) -> std::io::Result<()> {
        Ok(())
    }
    pub fn ask_yes_no_cancel(_: &str, _: &str) -> Option<bool> {
        None
    }
    pub fn show_error(_: &str, _: &str) {}
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    struct TestKey(String);
    impl TestKey {
        fn new(tag: &str) -> Self {
            TestKey(format!(r"Software\OsulzerImageOptimizerTest\uninstall-{}-{tag}", std::process::id()))
        }
    }
    impl Drop for TestKey {
        fn drop(&mut self) {
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.0);
        }
    }

    #[test]
    fn offer_rules() {
        let dir = Path::new(r"C:\Users\x\AppData\Local\Programs\OSULZER\ImageOptimizer");
        let elsewhere = Path::new(r"C:\Users\x\Downloads\osulzer-imageoptimizer.exe");
        assert!(should_offer(elsewhere, false, false, dir));
        assert!(!should_offer(&dir.join(EXE_NAME), false, false, dir), "läuft schon aus dem Installationsordner");
        assert!(!should_offer(&Path::new(r"c:\users\X\appdata\local\programs\osulzer\imageoptimizer").join(EXE_NAME), false, false, dir));
        assert!(!should_offer(elsewhere, true, false, dir), "schon installiert");
        assert!(!should_offer(elsewhere, false, true, dir), "abgelehnt");
    }

    #[test]
    fn uninstall_entry_roundtrip() {
        let key = TestKey::new("entry");
        let exe = Path::new(r"C:\Pfad mit Leer\osulzer-imageoptimizer.exe");
        write_uninstall_entry(&key.0, exe, "1.2.3").unwrap();
        let k = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&key.0).unwrap();
        let get = |n: &str| -> String { k.get_value(n).unwrap() };
        assert_eq!(get("DisplayName"), "OSULZER ImageOptimizer");
        assert_eq!(get("DisplayVersion"), "1.2.3");
        assert_eq!(get("Publisher"), "OSULZER");
        assert_eq!(get("UninstallString"), r#""C:\Pfad mit Leer\osulzer-imageoptimizer.exe" uninstall"#);
        assert_eq!(get("InstallLocation"), r"C:\Pfad mit Leer");
        assert_eq!(k.get_value::<u32, _>("NoModify").unwrap(), 1);
        remove_uninstall_entry(&key.0).unwrap();
        assert!(RegKey::predef(HKEY_CURRENT_USER).open_subkey(&key.0).is_err());
        remove_uninstall_entry(&key.0).unwrap();
    }

    #[test]
    fn legacy_cleanup_only_removes_our_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Bildoptimierer");
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["bildoptimierer.exe", EXE_NAME, "notiz.txt"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        assert_eq!(cleanup_legacy_dir(&dir).len(), 2);
        assert!(dir.join("notiz.txt").exists());
        std::fs::remove_file(dir.join("notiz.txt")).unwrap();
        std::fs::write(dir.join(EXE_NAME), b"x").unwrap();
        cleanup_legacy_dir(&dir);
        assert!(!dir.exists(), "leerer Ordner wird entfernt");
        assert!(cleanup_legacy_dir(&tmp.path().join("gibt-es-nicht")).is_empty());
    }

    #[test]
    fn self_delete_removes_file_and_folder_even_with_spaces_and_ampersand() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Ordner mit & Zeichen");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("app test.exe");
        std::fs::write(&file, b"x").unwrap();
        let status = delete_command(&file, 0).status().unwrap();
        assert!(status.success(), "{status:?}");
        assert!(!file.exists());
        assert!(!dir.exists());
    }

    #[test]
    fn install_copies_once() {
        let tmp = tempfile::tempdir().unwrap();
        let key = TestKey::new("install");
        let src = tmp.path().join("Downloads").join(EXE_NAME);
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"EXE").unwrap();
        let dir = tmp.path().join("Programs").join("OSULZER").join("ImageOptimizer");
        let installed = install_to(&src, &dir, &key.0).unwrap();
        assert_eq!(installed, dir.join(EXE_NAME));
        assert_eq!(std::fs::read(&installed).unwrap(), b"EXE");
        // Zweiter Aufruf aus dem Installationsordner selbst: kein Fehler, nichts kaputt.
        assert_eq!(install_to(&installed, &dir, &key.0).unwrap(), installed);
        assert_eq!(std::fs::read(&installed).unwrap(), b"EXE");
    }
}
