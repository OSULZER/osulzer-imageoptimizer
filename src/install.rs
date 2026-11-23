//! Installieren und Deinstallieren für den aktuellen Benutzer (ohne Admin-Rechte).

use std::path::{Path, PathBuf};

/// Eintrag unter Windows „Apps“ (aktueller Benutzer).
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\OSULZER.ImageOptimizer";
const EXE_NAME: &str = if cfg!(windows) { "osulzer-imageoptimizer.exe" } else { "osulzer-imageoptimizer" };
/// Name des App-Pakets unter macOS.
pub const MAC_APP: &str = "OSULZER ImageOptimizer.app";

fn local_app_data() -> PathBuf {
    directories::BaseDirs::new().map(|d| d.data_local_dir().to_path_buf()).unwrap_or_else(std::env::temp_dir)
}

/// Windows: `%LOCALAPPDATA%\Programs\OSULZER\ImageOptimizer`, macOS: `~/Applications`,
/// Linux: `~/.local/share/OSULZER/ImageOptimizer`.
pub fn install_dir() -> PathBuf {
    if cfg!(windows) {
        local_app_data().join("Programs").join("OSULZER").join("ImageOptimizer")
    } else if cfg!(target_os = "macos") {
        directories::BaseDirs::new().map(|d| d.home_dir().join("Applications")).unwrap_or_else(std::env::temp_dir)
    } else {
        local_app_data().join("OSULZER").join("ImageOptimizer")
    }
}

pub fn installed_exe() -> PathBuf {
    program_path(&install_dir())
}

/// Wo die Programmdatei in `dir` liegt (beim Mac im App-Paket).
fn program_path(dir: &Path) -> PathBuf {
    if cfg!(target_os = "macos") { dir.join(MAC_APP).join("Contents").join("MacOS").join(EXE_NAME) } else { dir.join(EXE_NAME) }
}

/// Das App-Paket, in dem `exe` liegt (`…/X.app/Contents/MacOS/exe`).
pub fn app_bundle_of(exe: &Path) -> Option<PathBuf> {
    let bundle = exe.parent()?.parent()?.parent()?;
    bundle.extension().is_some_and(|e| e == "app").then(|| bundle.to_path_buf())
}

/// Befehl im Terminal unter Linux (`~/.local/bin` liegt dort meist im Suchpfad).
fn linux_command_link() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join(".local").join("bin").join(EXE_NAME))
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

/// Soll beim Start das Installieren angeboten werden? `installed_exe`: wo die installierte Fassung läge.
pub fn should_offer(exe: &Path, installed: bool, declined: bool, installed_exe: &Path) -> bool {
    !installed && !declined && !same_path(exe, installed_exe)
}

fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
        norm(a) == norm(b)
    } else {
        a == b
    }
}

/// Ist das Programm installiert (unter Windows: Eintrag unter „Apps“ und Exe vorhanden)?
pub fn is_installed() -> bool {
    (!cfg!(windows) || imp::key_exists(UNINSTALL_KEY)) && installed_exe().is_file()
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

/// Kopiert das Programm nach `dir` (falls es nicht schon dort läuft) und schreibt den Uninstall-Eintrag.
/// Unter macOS entsteht dort das App-Paket.
pub fn install_to(exe: &Path, dir: &Path, key_path: &str) -> Result<PathBuf, String> {
    let target = program_path(dir);
    std::fs::create_dir_all(target.parent().unwrap_or(dir)).map_err(|e| e.to_string())?;
    if !same_path(exe, &target) {
        copy_program(exe, &target).map_err(|e| e.to_string())?;
        if cfg!(target_os = "macos")
            && let (Some(src), Some(dst)) = (app_bundle_of(exe), app_bundle_of(&target))
        {
            // Symbol aus dem heruntergeladenen Paket übernehmen.
            let icon = Path::new("Contents").join("Resources").join("AppIcon.icns");
            if src.join(&icon).is_file() {
                let _ = std::fs::create_dir_all(dst.join("Contents").join("Resources"));
                let _ = std::fs::copy(src.join(&icon), dst.join(&icon));
            }
        }
    }
    write_uninstall_entry(key_path, &target, env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Kopiert die Programmdatei. Unter macOS und Linux über eine Zwischendatei, weil sich eine laufende
/// Programmdatei dort nicht überschreiben, wohl aber ersetzen lässt.
fn copy_program(exe: &Path, target: &Path) -> std::io::Result<()> {
    if cfg!(windows) {
        return std::fs::copy(exe, target).map(|_| ());
    }
    let mut part = target.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    std::fs::copy(exe, &part)?;
    crate::update::check::make_executable(&part)?;
    std::fs::rename(&part, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })
}

/// Installiert für den aktuellen Benutzer: Kopie, Rechtsklick-Eintrag, Startmenü bzw. Anwendungsmenü,
/// unter Windows der „Apps“-Eintrag; räumt den alten Bildoptimierer-Ordner auf. Gibt das installierte
/// Programm zurück.
pub fn install(exe: &Path) -> Result<PathBuf, String> {
    let installed = install_to(exe, &install_dir(), UNINSTALL_KEY)?;
    crate::shell::register(&installed, crate::i18n::t("menu.label")).map_err(|e| e.to_string())?;
    let _ = create_shortcut(&installed);
    if cfg!(windows) {
        cleanup_legacy_dir(&legacy_dir());
    }
    Ok(installed)
}

#[cfg(windows)]
fn create_shortcut(exe: &Path) -> Result<(), String> {
    let lnk = start_menu_shortcut();
    if let Some(dir) = lnk.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut link = mslnk::ShellLink::new(exe).map_err(|e| e.to_string())?;
    link.set_working_dir(exe.parent().map(|p| p.to_string_lossy().into_owned()));
    link.create_lnk(&lnk).map_err(|e| e.to_string())
}

/// macOS: das Paket in `~/Applications` erscheint von selbst im Launchpad. Linux: den Menüeintrag schreibt
/// `shell::register`; hier nur der Befehl im Terminal.
#[cfg(not(windows))]
fn create_shortcut(exe: &Path) -> Result<(), String> {
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(link) = linux_command_link() {
        if let Some(dir) = link.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Nur einen eigenen Verweis ersetzen, nie eine fremde Datei.
        if std::fs::read_link(&link).is_ok() {
            let _ = std::fs::remove_file(&link);
        }
        if !link.exists() {
            std::os::unix::fs::symlink(exe, &link).map_err(|e| e.to_string())?;
        }
    }
    let _ = exe;
    Ok(())
}

/// Entfernt das installierte Programm: unter Windows kurz nach Programmende (die laufende Exe ist gesperrt),
/// unter macOS und Linux sofort.
fn remove_program() {
    let exe = installed_exe();
    if cfg!(windows) {
        let _ = delete_command(&exe, 3).spawn();
    } else if let Some(bundle) = app_bundle_of(&exe).filter(|_| cfg!(target_os = "macos")) {
        let _ = std::fs::remove_dir_all(bundle);
    } else {
        if let Some(link) = linux_command_link()
            && std::fs::read_link(&link).is_ok_and(|target| target == exe)
        {
            let _ = std::fs::remove_file(link);
        }
        let _ = std::fs::remove_file(&exe);
        if let Some(dir) = exe.parent() {
            let _ = std::fs::remove_dir(dir);
            let _ = dir.parent().map(std::fs::remove_dir);
        }
    }
}

/// Entfernt Rechtsklick-Eintrag, Verknüpfung und „Apps“-Eintrag; auf Wunsch auch Einstellungen und
/// API-Schlüssel. Danach das Programm selbst.
pub fn uninstall(remove_settings: bool) -> Result<(), String> {
    let _ = crate::shell::unregister();
    if cfg!(windows) {
        let _ = std::fs::remove_file(start_menu_shortcut());
    }
    remove_uninstall_entry(UNINSTALL_KEY).map_err(|e| e.to_string())?;
    if remove_settings {
        let _ = std::fs::remove_dir_all(crate::config::config_dir());
        for p in crate::ai::provider::Provider::ALL {
            let _ = crate::ai::secret::store_key_for(p, None);
        }
        let _ = crate::ai::secret::delete_shared_key();
        let _ = crate::ai::secret::delete_legacy_key();
    }
    // Das installierte Programm löschen – auch wenn „uninstall“ aus einer anderen Kopie aufgerufen wurde.
    remove_program();
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
/// Installieren noch) und die Versionsangabe (Windows „Apps“, macOS `Info.plist`) nach einem Update nachziehen.
pub fn after_start(exe: &Path) {
    if !same_path(exe, &installed_exe()) || !is_installed() {
        return;
    }
    if cfg!(windows) {
        cleanup_legacy_dir(&legacy_dir());
    }
    let _ = write_uninstall_entry(UNINSTALL_KEY, exe, env!("CARGO_PKG_VERSION"));
}

/// Fragt, ob auch die Einstellungen gelöscht werden, und deinstalliert dann.
/// `None` = abgebrochen, `Some(Ok)` = deinstalliert.
pub fn uninstall_with_question() -> Option<Result<(), String>> {
    let remove_settings = imp::ask_yes_no_cancel(crate::i18n::t("install.uninstall_title"), crate::i18n::t("install.uninstall_question"))?;
    let result = uninstall(remove_settings);
    if let Err(e) = &result {
        imp::show_error(crate::i18n::t("install.uninstall_title"), e);
    }
    Some(result)
}

/// `uninstall` von der Kommandozeile bzw. aus Windows „Apps“.
pub fn uninstall_interactive() -> u8 {
    match uninstall_with_question() {
        Some(Err(_)) => 1,
        _ => 0,
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
    /// macOS: die Versionsangabe im `Info.plist` des App-Pakets. Linux: nichts zu tun.
    pub fn write_uninstall_entry(_: &str, exe: &Path, version: &str) -> std::io::Result<()> {
        if cfg!(target_os = "macos")
            && let Some(bundle) = app_bundle_of(exe)
        {
            return std::fs::write(bundle.join("Contents").join("Info.plist"), crate::desktop::mac_info_plist(version));
        }
        Ok(())
    }
    pub fn remove_key(_: &str) -> std::io::Result<()> {
        Ok(())
    }
    /// `Some(true)` = Ja, `Some(false)` = Nein, `None` = Abbrechen.
    pub fn ask_yes_no_cancel(title: &str, text: &str) -> Option<bool> {
        match rfd::MessageDialog::new()
            .set_title(title)
            .set_description(text)
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show()
        {
            rfd::MessageDialogResult::Yes => Some(true),
            rfd::MessageDialogResult::No => Some(false),
            _ => None,
        }
    }
    pub fn show_error(title: &str, text: &str) {
        let _ = rfd::MessageDialog::new().set_title(title).set_description(text).set_level(rfd::MessageLevel::Error).show();
    }
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
        let installed = &dir.join(EXE_NAME);
        assert!(should_offer(elsewhere, false, false, installed));
        assert!(!should_offer(installed, false, false, installed), "läuft schon aus dem Installationsordner");
        assert!(!should_offer(&Path::new(r"c:\users\X\appdata\local\programs\osulzer\imageoptimizer").join(EXE_NAME), false, false, installed));
        assert!(!should_offer(elsewhere, true, false, installed), "schon installiert");
        assert!(!should_offer(elsewhere, false, true, installed), "abgelehnt");
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
