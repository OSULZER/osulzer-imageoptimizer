//! Dateien für die Einbindung in macOS und Linux: Rechtsklick-Einträge, Anwendungsmenü, App-Paket.
//! Nur Text erzeugen und lesen; geschrieben wird in `shell` und `install`. So lässt sich alles auf jedem
//! System testen.

use std::path::{Path, PathBuf};

/// Eine zu schreibende Datei, relativ zu ihrem Basisordner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopFile {
    pub path: PathBuf,
    pub content: Vec<u8>,
    /// Ausführrecht setzen (Skripte, Dolphin-Dienstmenüs).
    pub executable: bool,
}

/// Kennung für Dateinamen und Symbol unter Linux.
pub const LINUX_ID: &str = "osulzer-imageoptimizer";
/// Bildtypen, für die das Programm im Dateimanager erscheint.
pub const LINUX_MIME: &str = "image/jpeg;image/png;image/webp;image/bmp;image/tiff;";
/// Name der Schnellaktion unter macOS.
pub const MAC_WORKFLOW: &str = "ImageOptimizer.workflow";
/// Zeile im Mac-Skript, aus der sich der eingetragene Programmpfad zurücklesen lässt.
const MAC_MARKER: &str = "OSULZER_EXE=";

const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");
const MAC_INFO_PLIST: &str = include_str!("../assets/macos/Info.plist");

/// Pfad in einfachen Anführungszeichen für `sh`.
pub fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// Umkehrung von `sh_quote` für den Anfang von `s`; `None`, wenn das nicht so aussieht.
pub fn sh_unquote(s: &str) -> Option<PathBuf> {
    let mut out = String::new();
    let mut rest = s.strip_prefix('\'')?;
    loop {
        let end = rest.find('\'')?;
        out.push_str(&rest[..end]);
        rest = &rest[end + 1..];
        match rest.strip_prefix(r"\''") {
            Some(more) => {
                out.push('\'');
                rest = more;
            }
            None => return Some(PathBuf::from(out)),
        }
    }
}

/// Programmpfad als Argument in `Exec=` (Desktop Entry Specification): in Anführungszeichen mit maskierten
/// `"` `` ` `` `$` `\`, danach `\` und `%` für das Dateiformat verdoppelt.
pub fn exec_arg(path: &Path) -> String {
    let mut quoted = String::from("\"");
    for c in path.to_string_lossy().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted.replace('\\', r"\\").replace('%', "%%")
}

/// Liest den Programmpfad aus dem Wert von `Exec=` zurück.
pub fn parse_exec(value: &str) -> Option<PathBuf> {
    // Ebene der Datei: `\\` → `\`, `%%` → `%`.
    let mut unescaped = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some('\\')) | ('%', Some('%')) => {
                unescaped.push(c);
                chars.next();
            }
            _ => unescaped.push(c),
        }
    }
    // Ebene des Befehls: Pfad in Anführungszeichen mit `\`-Maskierung.
    let mut chars = unescaped.strip_prefix('"')?.chars();
    let mut out = String::new();
    loop {
        match chars.next()? {
            '\\' => out.push(chars.next()?),
            '"' => return Some(PathBuf::from(out)),
            c => out.push(c),
        }
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn text(path: impl Into<PathBuf>, content: String, executable: bool) -> DesktopFile {
    DesktopFile { path: path.into(), content: content.into_bytes(), executable }
}

/// Linux-Dateien relativ zu `$XDG_DATA_HOME` (meist `~/.local/share`):
/// Anwendungsmenü und „Öffnen mit“ für alle Dateimanager, dazu Rechtsklick-Einträge für Dolphin (KDE),
/// Nautilus (GNOME) und Nemo (Cinnamon).
pub fn linux_files(exe: &Path, label: &str) -> Vec<DesktopFile> {
    let id = LINUX_ID;
    let mime = LINUX_MIME;
    let exec = format!("{} %F", exec_arg(exe));
    vec![
        text(
            format!("applications/{id}.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=OSULZER ImageOptimizer\nGenericName={label}\n\
Comment=Resize, compress, rename and describe images\nExec={exec}\nIcon={id}\nTerminal=false\n\
Categories=Graphics;\nMimeType={mime}\n"
            ),
            false,
        ),
        text(
            format!("kio/servicemenus/{id}.desktop"),
            format!(
                "[Desktop Entry]\nType=Service\nMimeType={mime}\nActions={id}\nX-KDE-Priority=TopLevel\n\n\
[Desktop Action {id}]\nName={label}\nIcon={id}\nExec={exec}\n"
            ),
            // Plasma 6 führt nur ausführbare Dienstmenüs aus.
            true,
        ),
        text(
            format!("nemo/actions/{id}.nemo_action"),
            format!("[Nemo Action]\nName={label}\nIcon-Name={id}\nExec={exec}\nSelection=notnone\nMimetypes={mime}\n"),
            false,
        ),
        // Nautilus zeigt Skripte unter „Skripte“ mit dem Dateinamen und übergibt die Auswahl als Argumente.
        text("nautilus/scripts/ImageOptimizer", format!("#!/bin/sh\nexec {} \"$@\"\n", sh_quote(exe)), true),
        DesktopFile { path: format!("icons/hicolor/256x256/apps/{id}.png").into(), content: ICON_PNG.to_vec(), executable: false },
    ]
}

/// Eingetragener Programmpfad aus der Desktop-Datei des Anwendungsmenüs.
pub fn linux_registered_exe(desktop_entry: &str) -> Option<PathBuf> {
    let value = desktop_entry.lines().find_map(|l| l.strip_prefix("Exec="))?;
    parse_exec(value)
}

/// Schnellaktion für den Finder (Rechtsklick → Schnellaktionen), relativ zu `~/Library/Services`.
/// Sie startet das Programm mit den markierten Bildern als Argumente; „Öffnen mit“ würde die Dateien
/// nur als Apple Event liefern, das die Fensterbibliothek nicht auswertet.
pub fn mac_workflow_files(exe: &Path, label: &str) -> Vec<DesktopFile> {
    let script = format!("{MAC_MARKER}{}\n\"$OSULZER_EXE\" \"$@\" >/dev/null 2>&1 &\n", sh_quote(exe));
    let info = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>NSServices</key>
	<array>
		<dict>
			<key>NSIconName</key>
			<string>NSActionTemplate</string>
			<key>NSMenuItem</key>
			<dict>
				<key>default</key>
				<string>{label}</string>
			</dict>
			<key>NSMessage</key>
			<string>runWorkflowAsService</string>
			<key>NSRequiredContext</key>
			<dict>
				<key>NSApplicationIdentifier</key>
				<string>com.apple.finder</string>
			</dict>
			<key>NSSendFileTypes</key>
			<array>
				<string>public.image</string>
			</array>
		</dict>
	</array>
</dict>
</plist>
"#,
        label = xml_escape(label)
    );
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AMApplicationBuild</key>
	<string>523</string>
	<key>AMApplicationVersion</key>
	<string>2.10</string>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<true/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMParameterProperties</key>
				<dict>
					<key>COMMAND_STRING</key>
					<dict/>
					<key>CheckedForUserDefaultShell</key>
					<dict/>
					<key>inputMethod</key>
					<dict/>
					<key>shell</key>
					<dict/>
					<key>source</key>
					<dict/>
				</dict>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>{script}</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/sh</string>
					<key>source</key>
					<string></string>
				</dict>
				<key>BundleIdentifier</key>
				<string>com.apple.RunShellScript</string>
				<key>CFBundleVersion</key>
				<string>2.0.3</string>
				<key>CanShowSelectedItemsWhenRun</key>
				<false/>
				<key>CanShowWhenRun</key>
				<true/>
				<key>Category</key>
				<array>
					<string>AMCategoryUtilities</string>
				</array>
				<key>Class Name</key>
				<string>RunShellScriptAction</string>
				<key>InputUUID</key>
				<string>6F1C5E0A-1B7C-4C55-9E0B-2D7A8F3E4C01</string>
				<key>Keywords</key>
				<array>
					<string>Shell</string>
					<string>Script</string>
				</array>
				<key>OutputUUID</key>
				<string>6F1C5E0A-1B7C-4C55-9E0B-2D7A8F3E4C02</string>
				<key>UUID</key>
				<string>6F1C5E0A-1B7C-4C55-9E0B-2D7A8F3E4C03</string>
				<key>UnlocalizedApplications</key>
				<array>
					<string>Automator</string>
				</array>
				<key>isViewVisible</key>
				<integer>1</integer>
				<key>location</key>
				<string>309.000000:253.000000</string>
				<key>nibPath</key>
				<string>/System/Library/Automator/Run Shell Script.action/Contents/Resources/Base.lproj/main.nib</string>
			</dict>
			<key>isViewVisible</key>
			<integer>1</integer>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>applicationBundleIDsByPath</key>
		<dict/>
		<key>applicationPaths</key>
		<array/>
		<key>inputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject.image</string>
		<key>outputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>presentationMode</key>
		<integer>15</integer>
		<key>processesInput</key>
		<false/>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject.image</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<false/>
		<key>systemImageName</key>
		<string>NSActionTemplate</string>
		<key>useAutomaticInputType</key>
		<false/>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
"#,
        script = xml_escape(&script)
    );
    vec![
        text(format!("{MAC_WORKFLOW}/Contents/Info.plist"), info, false),
        text(format!("{MAC_WORKFLOW}/Contents/document.wflow"), document, false),
    ]
}

/// Eingetragener Programmpfad aus `document.wflow` der Schnellaktion.
pub fn mac_registered_exe(document: &str) -> Option<PathBuf> {
    let document = xml_unescape(document);
    let start = document.find(MAC_MARKER)? + MAC_MARKER.len();
    sh_unquote(&document[start..])
}

/// `Info.plist` des App-Pakets für diese Version.
pub fn mac_info_plist(version: &str) -> String {
    // Unter Windows checkt Git die Vorlage je nach Einstellung mit CRLF aus.
    MAC_INFO_PLIST.replace("\r\n", "\n").replace("{version}", version)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ODD: &str = "/home/a b/it's \"$HOME\" `x` 100% \\ ä/osulzer-imageoptimizer";

    #[test]
    fn sh_quote_roundtrip() {
        for p in ["/usr/bin/x", "/a b/c", "/it's/x", ODD] {
            let quoted = sh_quote(Path::new(p));
            assert_eq!(sh_unquote(&format!("{quoted}\nrest")), Some(PathBuf::from(p)), "{quoted}");
        }
        assert_eq!(sh_quote(Path::new("/it's")), r"'/it'\''s'");
    }

    #[test]
    fn exec_roundtrip_and_escaping() {
        assert_eq!(exec_arg(Path::new("/opt/x")), r#""/opt/x""#);
        assert_eq!(exec_arg(Path::new("/a$b")), r#""/a\\$b""#);
        assert_eq!(exec_arg(Path::new("/100%")), r#""/100%%""#);
        for p in ["/opt/x", ODD] {
            let value = format!("{} %F", exec_arg(Path::new(p)));
            assert_eq!(parse_exec(&value), Some(PathBuf::from(p)), "{value}");
        }
    }

    #[test]
    fn linux_files_point_to_the_program() {
        let exe = Path::new("/home/u/.local/share/OSULZER/ImageOptimizer/osulzer-imageoptimizer");
        let files = linux_files(exe, "ImageOptimizer");
        let paths: Vec<_> = files.iter().map(|f| f.path.to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(
            paths,
            [
                "applications/osulzer-imageoptimizer.desktop",
                "kio/servicemenus/osulzer-imageoptimizer.desktop",
                "nemo/actions/osulzer-imageoptimizer.nemo_action",
                "nautilus/scripts/ImageOptimizer",
                "icons/hicolor/256x256/apps/osulzer-imageoptimizer.png",
            ]
        );
        let entry = String::from_utf8(files[0].content.clone()).unwrap();
        assert!(entry.contains("MimeType=image/jpeg;image/png;image/webp;image/bmp;image/tiff;\n"));
        assert_eq!(linux_registered_exe(&entry), Some(exe.to_path_buf()));
        let script = String::from_utf8(files[3].content.clone()).unwrap();
        assert!(script.starts_with("#!/bin/sh\nexec '/home/u/"));
        assert!(files[1].executable && files[3].executable && !files[0].executable);
        assert_eq!(&files[4].content[1..4], b"PNG");
    }

    #[test]
    fn mac_workflow_roundtrip() {
        let exe = Path::new(ODD);
        let files = mac_workflow_files(exe, "Bilder <optimieren> & mehr");
        assert_eq!(files.len(), 2);
        let info = String::from_utf8(files[0].content.clone()).unwrap();
        assert!(info.contains("<string>Bilder &lt;optimieren&gt; &amp; mehr</string>"));
        assert!(info.contains("<string>public.image</string>"));
        let document = String::from_utf8(files[1].content.clone()).unwrap();
        assert!(!document.contains("\"$OSULZER_EXE\""), "Skript muss XML-maskiert sein");
        assert_eq!(mac_registered_exe(&document), Some(exe.to_path_buf()));
        assert_eq!(mac_registered_exe("<plist/>"), None);
    }

    #[test]
    fn mac_info_plist_has_version_and_executable() {
        let plist = mac_info_plist("1.2.3");
        assert!(plist.contains("<key>CFBundleShortVersionString</key>\n\t<string>1.2.3</string>"));
        assert!(plist.contains("<string>osulzer-imageoptimizer</string>"));
        assert!(!plist.contains("{version}"));
    }
}
