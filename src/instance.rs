//! Einzelinstanz: Die zuerst gestartete Instanz sammelt die Dateien aller weiteren Aufrufe.

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream};
use std::fs::{OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub enum Role {
    /// Diese Instanz öffnet das Fenster und empfängt die Pfade späterer Aufrufe.
    Primary(Receiver<Vec<PathBuf>>),
    /// Die Pfade wurden an die laufende Instanz übergeben.
    Secondary,
}

/// `dir`: Ordner für die Sperrdatei, `name`: Name des lokalen Sockets.
/// Die primäre Instanz hält die Sperre bis zum Prozessende.
pub fn acquire(dir: &Path, name: &str, paths: &[PathBuf]) -> std::io::Result<Role> {
    std::fs::create_dir_all(dir)?;
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("instance.lock"))?;
    match lock.try_lock() {
        Ok(()) => {
            // Wir halten die Sperre, also läuft keine andere primäre Instanz: Ein Socket, den ein
            // abgestürzter Prozess unter macOS in /tmp zurückgelassen hat, darf ersetzt werden.
            let listener = ListenerOptions::new().name(socket_name(name)?).try_overwrite(true).create_sync()?;
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                // Die Sperrdatei bleibt offen, solange dieser Thread lebt, also bis zum Prozessende.
                let _lock = lock;
                for conn in listener.incoming() {
                    let Ok(mut conn) = conn else { continue };
                    let mut text = String::new();
                    // Auch bei einem Lesefehler weitergeben: Das Fenster soll nach vorne kommen, und
                    // vollständig gelesene Zeilen gehen nicht verloren.
                    let _ = conn.read_to_string(&mut text);
                    let complete = &text[..text.rfind('\n').map_or(0, |i| i + 1)];
                    let batch: Vec<PathBuf> = complete.lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect();
                    if tx.send(batch).is_err() {
                        break;
                    }
                }
            });
            Ok(Role::Primary(rx))
        }
        Err(TryLockError::WouldBlock) => {
            hand_over(name, paths)?;
            Ok(Role::Secondary)
        }
        Err(TryLockError::Error(e)) => Err(e),
    }
}

fn socket_name(name: &str) -> std::io::Result<interprocess::local_socket::Name<'_>> {
    name.to_ns_name::<GenericNamespaced>()
}

/// Wie lange eine weitere Instanz auf die primäre wartet, falls diese gerade erst startet.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

fn hand_over(name: &str, paths: &[PathBuf]) -> std::io::Result<()> {
    // Immer mindestens ein Zeilenende: Eine Verbindung ganz ohne Daten meldet Windows beim Empfänger
    // mitunter als Fehler statt als leere Nachricht, dann käme das Fenster nicht nach vorne.
    let mut message: String = paths.iter().map(|p| format!("{}\n", p.to_string_lossy())).collect();
    message.push('\n');
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match Stream::connect(socket_name(name)?) {
            Ok(mut stream) => {
                stream.write_all(message.as_bytes())?;
                return stream.flush();
            }
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}
