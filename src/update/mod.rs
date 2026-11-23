//! Updates über GitHub: Signatur, Prüfung auf neue Versionen, Austausch der Exe.

pub mod sign;
pub mod check;
pub mod swap;

/// Umgebungsvariable, über die ein Neustart die Prozess-ID der alten Instanz erfährt.
pub const WAIT_PID_ENV: &str = "OSULZER_IMAGEOPTIMIZER_WAIT_PID";

/// Startet die (neue) Exe mit diesen Dateien; sie wartet, bis dieser Prozess beendet ist.
pub fn restart(exe: &std::path::Path, files: &[std::path::PathBuf]) -> std::io::Result<()> {
    std::process::Command::new(exe).args(files).env(WAIT_PID_ENV, std::process::id().to_string()).spawn().map(|_| ())
}

/// Wartet höchstens 15 s auf das Ende der in `WAIT_PID_ENV` genannten Instanz.
pub fn wait_for_previous_instance() {
    let Some(pid) = std::env::var(WAIT_PID_ENV).ok().and_then(|p| p.parse::<u32>().ok()) else { return };
    // SAFETY: OpenProcess/WaitForSingleObject/CloseHandle mit eigenem Handle; ungültige PID liefert null.
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !handle.is_null() {
            WaitForSingleObject(handle, 15_000);
            CloseHandle(handle);
        }
    }
    // `kill -0` prüft nur, ob der Prozess noch lebt; gibt es den Befehl nicht, wird nicht gewartet.
    #[cfg(not(windows))]
    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let alive = || {
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        while std::time::Instant::now() < deadline && alive() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// Aktuelle Zeit in Unix-Sekunden.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
