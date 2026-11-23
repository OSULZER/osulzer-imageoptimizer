//! Auswertung der Aufrufargumente: Dateien für das Fenster oder Unterbefehle der Kommandozeile.

use clap::{Args, Parser, Subcommand};
use std::ffi::OsString;
use std::path::PathBuf;

// Wird einmal pro Start erzeugt; die Größe spielt keine Rolle.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Kontextmenü-Eintrag setzen und beenden.
    Register,
    /// Kontextmenü-Eintrag entfernen und beenden.
    Unregister,
    /// Fenster mit diesen Dateien öffnen (auch ohne Dateien).
    Open(Vec<PathBuf>),
    /// Unterbefehl der Kommandozeile.
    Cli(CliArgs),
}

/// Optimize images for the web from the command line (OSULZER ImageOptimizer).
/// Without a command, the window opens with the given files.
#[derive(Debug, PartialEq, Eq, Parser)]
#[command(name = "osulzer-imageoptimizer", version)]
pub struct CliArgs {
    #[command(subcommand)]
    pub cmd: CliCmd,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq, Eq, Subcommand)]
pub enum CliCmd {
    /// Resize, compress, rename and tag images. Originals are never overwritten unless --target replace.
    Optimize(OptimizeArgs),
    /// List the built-in and saved presets.
    Presets {
        #[arg(long)]
        json: bool,
    },
    /// Show dimensions, size and format of images.
    Info {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Let the configured AI suggest file name, title, alt text and description for one image.
    Describe {
        file: PathBuf,
        /// Override the configured provider (openai, anthropic, gemini, openrouter, mistral, ollama, lmstudio, custom).
        #[arg(long)]
        provider: Option<String>,
        /// Override the configured model.
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Add the entry to the Explorer context menu.
    Register,
    /// Remove the entry from the Explorer context menu.
    Unregister,
    /// Run as MCP server on stdin/stdout (for Claude Code, Claude Desktop and other agents).
    Mcp,
    /// Uninstall (called from Windows Apps); asks whether to keep the settings.
    Uninstall,
}

#[derive(Debug, PartialEq, Eq, Default, Args)]
pub struct OptimizeArgs {
    /// Image files.
    #[arg(required = true)]
    pub files: Vec<PathBuf>,
    /// web-large (default), web-medium, thumbnail or the name of a saved preset.
    #[arg(long)]
    pub preset: Option<String>,
    /// Longest edge in pixels (never enlarges).
    #[arg(long)]
    pub max_edge: Option<u32>,
    /// jpg, png, webp or keep.
    #[arg(long)]
    pub format: Option<String>,
    /// Quality 1–100. PNG: 100 is lossless, below that the colors are reduced.
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub quality: Option<u8>,
    /// Upper limit of the file size in KB.
    #[arg(long)]
    pub max_kb: Option<u32>,
    /// web (subfolder, default), suffix (next to the original with -web), replace (original to the
    /// recycle bin) or a folder path.
    #[arg(long)]
    pub target: Option<String>,
    /// Text placed before every file name.
    #[arg(long)]
    pub prefix: Option<String>,
    /// Text placed after every file name.
    #[arg(long)]
    pub suffix: Option<String>,
    /// Text to find in file names (case-insensitive).
    #[arg(long)]
    pub search: Option<String>,
    /// Replacement for --search (empty removes it).
    #[arg(long)]
    pub replace_with: Option<String>,
    /// Same name for all files, numbered -01, -02, …
    #[arg(long)]
    pub name: Option<String>,
    /// Let the configured AI describe each image (title, alt text, description, file name).
    #[arg(long)]
    pub describe: bool,
    /// JSON file with texts per image: [{"path": …, "file_name": …, "title": …, "alt": …, "description": …}].
    #[arg(long)]
    pub texts: Option<PathBuf>,
    /// Override the configured AI provider for --describe.
    #[arg(long)]
    pub provider: Option<String>,
    /// Override the configured AI model for --describe.
    #[arg(long)]
    pub model: Option<String>,
    /// Print a JSON array instead of text.
    #[arg(long)]
    pub json: bool,
}

/// Erste Argumente, die in den Kommandozeilen-Modus schalten.
pub const CLI_COMMANDS: [&str; 12] =
    ["optimize", "presets", "info", "describe", "register", "unregister", "mcp", "uninstall", "--help", "-h", "--version", "-V"];

fn absolute(p: PathBuf) -> PathBuf {
    std::path::absolute(&p).unwrap_or(p)
}

/// Passt `name` auf das Muster mit `*` und `?` (Groß-/Kleinschreibung egal)?
fn wildcard_match(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.to_lowercase().chars().collect(), name.to_lowercase().chars().collect());
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Absolute Pfade; Muster wie `fotos\*.jpg` werden aufgelöst (cmd und PowerShell tun das nicht).
fn expand(files: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for f in files.into_iter().map(absolute) {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !name.contains(['*', '?']) {
            out.push(f);
            continue;
        }
        let dir = f.parent().map(PathBuf::from).unwrap_or_default();
        let mut hits: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.file_name().is_some_and(|n| wildcard_match(&name, &n.to_string_lossy())))
            .collect();
        hits.sort();
        // Kein Treffer: Muster stehen lassen, damit die Meldung zeigt, was nicht gefunden wurde.
        if hits.is_empty() { out.push(f) } else { out.extend(hits) }
    }
    out
}

/// `args` ohne den Programmnamen. Relative Pfade werden absolut gemacht, damit eine bereits
/// laufende Instanz mit anderem Arbeitsordner sie findet.
pub fn parse_args(args: Vec<OsString>) -> Result<Command, clap::Error> {
    if args.len() == 1 {
        match args[0].to_str() {
            Some("--register") => return Ok(Command::Register),
            Some("--unregister") => return Ok(Command::Unregister),
            _ => {}
        }
    }
    let first = args.first().and_then(|a| a.to_str());
    if first.is_some_and(|f| CLI_COMMANDS.contains(&f)) {
        let mut parsed = CliArgs::try_parse_from(std::iter::once(OsString::from("osulzer-imageoptimizer")).chain(args))?;
        match &mut parsed.cmd {
            CliCmd::Optimize(o) => {
                o.files = expand(std::mem::take(&mut o.files));
                o.texts = o.texts.take().map(absolute);
            }
            CliCmd::Info { files, .. } => *files = expand(std::mem::take(files)),
            CliCmd::Describe { file, .. } => *file = absolute(std::mem::take(file)),
            _ => {}
        }
        return Ok(Command::Cli(parsed));
    }
    Ok(Command::Open(args.into_iter().map(PathBuf::from).map(absolute).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn register_works_both_ways() {
        assert_eq!(parse_args(os(&["--register"])).unwrap(), Command::Register);
        assert_eq!(parse_args(os(&["--unregister"])).unwrap(), Command::Unregister);
        assert_eq!(parse_args(os(&["register"])).unwrap(), Command::Cli(CliArgs { cmd: CliCmd::Register }));
    }

    #[test]
    fn no_arguments_opens_an_empty_window() {
        assert_eq!(parse_args(vec![]).unwrap(), Command::Open(vec![]));
    }

    #[test]
    fn everything_else_is_a_file_path() {
        let got = parse_args(os(&[r"C:\Fotos\Müller & Söhne\bild 1.jpg", r"\\nas\freigabe\ä.png"])).unwrap();
        assert_eq!(
            got,
            Command::Open(vec![PathBuf::from(r"C:\Fotos\Müller & Söhne\bild 1.jpg"), PathBuf::from(r"\\nas\freigabe\ä.png")])
        );
    }

    #[test]
    fn relative_paths_become_absolute() {
        let Ok(Command::Open(paths)) = parse_args(os(&["bild.jpg"])) else { panic!() };
        assert_eq!(paths, vec![std::env::current_dir().unwrap().join("bild.jpg")]);
    }

    #[test]
    fn a_file_named_like_a_flag_next_to_other_files_is_a_file() {
        let Ok(Command::Open(paths)) = parse_args(os(&[r"C:\a.jpg", "--register"])) else { panic!() };
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn explorer_paths_open_the_window_even_if_named_like_a_command() {
        let Ok(Command::Open(paths)) = parse_args(os(&[r"C:\x\optimize", r"C:\x\mcp"])) else { panic!() };
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn subcommand_is_recognized() {
        let Ok(Command::Cli(CliArgs { cmd: CliCmd::Optimize(o) })) =
            parse_args(os(&["optimize", "a.jpg", "--name", "Kunde", "--quality", "70", "--json"]))
        else {
            panic!()
        };
        assert_eq!(o.files, vec![std::env::current_dir().unwrap().join("a.jpg")]);
        assert_eq!((o.name.as_deref(), o.quality, o.json), (Some("Kunde"), Some(70), true));
        assert_eq!(parse_args(os(&["mcp"])).unwrap(), Command::Cli(CliArgs { cmd: CliCmd::Mcp }));
    }

    #[test]
    fn wildcards_are_expanded() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.jpg", "b.JPG", "c.png"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let pattern = dir.path().join("*.jpg");
        let Ok(Command::Cli(CliArgs { cmd: CliCmd::Optimize(o) })) = parse_args(vec![OsString::from("optimize"), pattern.into_os_string()]) else {
            panic!()
        };
        let mut names: Vec<_> = o.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["a.jpg", "b.JPG"]);
    }

    #[test]
    fn unknown_option_is_an_error() {
        assert!(parse_args(os(&["optimize", "--bogus", "a.jpg"])).is_err());
        assert!(parse_args(os(&["optimize", "--quality", "500", "a.jpg"])).is_err());
    }
}
