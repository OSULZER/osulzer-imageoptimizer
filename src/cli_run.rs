//! Ausführung der Unterbefehle der Kommandozeile.

use crate::ai::provider::Provider;
use crate::ai::secret::load_key_for;
use crate::ai::{AiConfig, describe};
use crate::cli::{CliArgs, CliCmd, OptimizeArgs};
use crate::config::{Config, ProviderSettings};
use crate::engine::ai_input_jpeg;
use crate::headless::{Destination, FileReport, Request, info, optimize};
use crate::i18n::{set_lang, t, tf};
use crate::naming::NameRules;
use crate::settings::{Format, Settings, Target, builtin_presets, preset_by_key};
use crate::state::{format_bytes, format_saving};
use crate::texts::ImageTexts;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Exit-Codes: 0 alles gut, 1 mindestens ein Bild fehlgeschlagen, 2 Aufruf- oder Einrichtungsfehler.
pub const OK: u8 = 0;
pub const SOME_FAILED: u8 = 1;
pub const USAGE: u8 = 2;

/// Führt einen Unterbefehl aus. Konfiguration nur für KI-Anbieter, Sprache und eigene Vorgaben.
pub fn run(args: CliArgs, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let config = crate::config::load_from(&crate::config::config_file());
    set_lang(config.ui_lang());
    let result = match args.cmd {
        CliCmd::Optimize(o) => run_optimize(&config, o, out, err),
        CliCmd::Presets { json } => run_presets(&config, json, out),
        CliCmd::Info { files, json } => run_info(&files, json, out, err),
        CliCmd::Describe { file, provider, model, json } => run_describe(&config, &file, provider.as_deref(), model.as_deref(), json, out, err),
        CliCmd::Register => register(true),
        CliCmd::Unregister => register(false),
        CliCmd::Uninstall => Ok(crate::install::uninstall_interactive()),
        CliCmd::Mcp => match crate::mcp::serve_stdio() {
            Ok(()) => Ok(OK),
            Err(e) => Err(e),
        },
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            USAGE
        }
    }
}

fn register(on: bool) -> Result<u8, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let result = if on { crate::shell::register(&exe, t("menu.label")) } else { crate::shell::unregister() };
    result.map(|()| OK).map_err(|e| tf("ui.failed", &[("e", &e)]))
}

/// Einstellungen aus Vorgabe (Schlüssel oder Name einer eigenen Vorgabe) und einzelnen Werten.
pub fn settings_from(config: &Config, preset: Option<&str>, max_edge: Option<u32>, format: Option<&str>, quality: Option<u8>, max_kb: Option<u32>) -> Result<Settings, String> {
    let key = preset.unwrap_or("web-large");
    let mut s = preset_by_key(key)
        .or_else(|| config.user_presets.iter().find(|p| p.name == key).cloned())
        .ok_or_else(|| tf("cli.error.preset", &[("name", &key)]))?
        .settings;
    if let Some(edge) = max_edge {
        s.max_edge = Some(edge);
    }
    if let Some(f) = format {
        s.format = match f.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Format::Jpeg,
            "png" => Format::Png,
            "webp" => Format::Webp,
            "keep" | "original" => Format::Original,
            other => return Err(tf("cli.error.format", &[("name", &other)])),
        };
    }
    if let Some(q) = quality {
        if !(1..=100).contains(&q) {
            return Err(tf("cli.error.quality", &[("q", &q)]));
        }
        s.quality = q;
    }
    if let Some(kb) = max_kb {
        s.max_kb = Some(kb);
    }
    Ok(s)
}

/// `web`, `suffix`, `replace` oder ein Ordner.
pub fn destination_from(target: Option<&str>) -> Destination {
    let target = target.map(str::trim).unwrap_or_default();
    match target.to_ascii_lowercase().as_str() {
        "" | "web" => Destination::Target(Target::WebSubfolder),
        "suffix" => Destination::Target(Target::Suffix),
        "replace" => Destination::Target(Target::Replace),
        _ => Destination::Folder(std::path::absolute(target).unwrap_or_else(|_| PathBuf::from(target))),
    }
}

/// SEO texts for one image.
#[derive(Clone, Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct TextsEntry {
    /// Absolute path of the image.
    pub path: String,
    /// New file name without extension (will be made web-safe).
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Alt text (WordPress reads it from the XMP metadata).
    #[serde(default)]
    pub alt: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

pub fn texts_map(entries: &[TextsEntry], lang: crate::i18n::Lang) -> HashMap<String, ImageTexts> {
    entries
        .iter()
        .map(|e| {
            let texts = ImageTexts {
                file_stem: e.file_name.clone().unwrap_or_default(),
                title: e.title.clone().unwrap_or_default(),
                alt: e.alt.clone().unwrap_or_default(),
                description: e.description.clone().unwrap_or_default(),
            };
            (crate::headless::path_key(Path::new(&e.path)), texts.normalized_in(lang))
        })
        .collect()
}

/// KI-Verbindung aus der Konfiguration (Anbieter/Modell optional überschrieben).
/// Fehler, wenn nicht eingerichtet oder einem Cloud-Anbieter nicht zugestimmt wurde.
pub fn ai_config(config: &Config, provider: Option<&str>, model: Option<&str>) -> Result<AiConfig, String> {
    let mut ai = config.ai.clone();
    if let Some(id) = provider {
        ai.provider = Provider::ALL.into_iter().find(|p| p.id() == id).ok_or_else(|| tf("cli.error.provider", &[("name", &id)]))?;
    }
    if let Some(m) = model {
        let active = ai.active();
        ai.set_active(ProviderSettings { base_url: active.base_url, model: m.to_string() });
    }
    if !ai.is_configured() {
        return Err(t("cli.error.ai_not_configured").into());
    }
    if ai.needs_consent() {
        return Err(tf("cli.error.consent", &[("provider", &ai.provider.name())]));
    }
    let active = ai.active();
    Ok(AiConfig {
        base_url: active.base_url,
        model: active.model,
        api_key: load_key_for(ai.provider),
        lang: config.ai_lang(),
        protocol: ai.provider.protocol(),
    })
}

/// Lässt ein Bild von der eingerichteten KI beschreiben.
pub fn describe_file(cfg: &AiConfig, file: &Path) -> Result<ImageTexts, String> {
    let jpeg = ai_input_jpeg(file).map_err(|e| e.to_string())?;
    describe(cfg, &jpeg, "").map_err(|e| e.to_string())
}

fn run_optimize(config: &Config, o: OptimizeArgs, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, String> {
    let settings = settings_from(config, o.preset.as_deref(), o.max_edge, o.format.as_deref(), o.quality, o.max_kb)?;
    let lang = config.ai_lang();
    let mut texts = match &o.texts {
        Some(file) => {
            let text = std::fs::read_to_string(file).map_err(|e| tf("cli.error.texts", &[("e", &e)]))?;
            let entries: Vec<TextsEntry> = serde_json::from_str(&text).map_err(|e| tf("cli.error.texts", &[("e", &e)]))?;
            let entries: Vec<TextsEntry> =
                entries.into_iter().map(|mut e| {
                    e.path = std::path::absolute(&e.path).map(|p| p.to_string_lossy().into_owned()).unwrap_or(e.path);
                    e
                }).collect();
            texts_map(&entries, lang)
        }
        None => HashMap::new(),
    };
    if o.describe {
        let cfg = ai_config(config, o.provider.as_deref(), o.model.as_deref())?;
        for file in &o.files {
            if texts.contains_key(&crate::headless::path_key(file)) {
                continue;
            }
            match describe_file(&cfg, file) {
                Ok(t) => {
                    texts.insert(crate::headless::path_key(file), t);
                }
                Err(e) => {
                    let _ = writeln!(err, "{}: {e}", file.display());
                }
            }
        }
    }
    let rules = NameRules {
        uniform: o.name.unwrap_or_default(),
        prefix: o.prefix.unwrap_or_default(),
        suffix: o.suffix.unwrap_or_default(),
        search: o.search.unwrap_or_default(),
        replace: o.replace_with.unwrap_or_default(),
    };
    let req = Request { files: o.files, settings, destination: destination_from(o.target.as_deref()), rules, texts, lang };
    for key in crate::headless::unmatched_texts(&req) {
        let _ = writeln!(err, "{}", tf("cli.warning.unmatched_texts", &[("path", &key)]));
    }
    let reports = optimize(&req);
    print_reports(&reports, o.json, out, err);
    Ok(if reports.iter().any(|r| r.error.is_some()) { SOME_FAILED } else { OK })
}

fn print_reports(reports: &[FileReport], as_json: bool, out: &mut dyn Write, err: &mut dyn Write) {
    if as_json {
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(reports).unwrap_or_default());
        return;
    }
    for r in reports {
        match (&r.output, &r.error) {
            (Some(output), _) => {
                let mut line = format!(
                    "{} -> {}  {}x{}  {} -> {} ({})",
                    r.source.display(),
                    output.display(),
                    r.width,
                    r.height,
                    format_bytes(r.bytes_before),
                    format_bytes(r.bytes_after),
                    format_saving(r.bytes_before, r.bytes_after)
                );
                if let Some(m) = &r.message {
                    line += &format!("  [{m}]");
                }
                let _ = writeln!(out, "{line}");
            }
            (None, Some(e)) => {
                let _ = writeln!(err, "{}: {e}", r.source.display());
            }
            (None, None) => {
                let _ = writeln!(out, "{}: {}", r.source.display(), r.message.clone().unwrap_or_default());
            }
        }
    }
}

/// Mitgelieferte (mit Schlüssel) und eigene Vorgaben.
pub fn presets_json(config: &Config) -> serde_json::Value {
    let keys = ["web-large", "web-medium", "thumbnail"];
    let builtin = builtin_presets().into_iter().zip(keys).map(|(p, key)| (key.to_string(), p));
    let own = config.user_presets.iter().cloned().map(|p| (p.name.clone(), p));
    let list: Vec<_> = builtin
        .chain(own)
        .map(|(key, p)| {
            json!({
                "key": key,
                "name": p.name,
                "max_edge": p.settings.max_edge,
                "format": format!("{:?}", p.settings.format).to_lowercase(),
                "quality": p.settings.quality,
                "max_kb": p.settings.max_kb,
            })
        })
        .collect();
    json!(list)
}

fn run_presets(config: &Config, as_json: bool, out: &mut dyn Write) -> Result<u8, String> {
    let list = presets_json(config);
    if as_json {
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(&list).unwrap_or_default());
    } else {
        for p in list.as_array().into_iter().flatten() {
            let edge = p["max_edge"].as_u64().map(|e| format!("{e} px")).unwrap_or_else(|| "-".into());
            let _ = writeln!(out, "{:<12} {:<20} {edge:>8}  {}  Q{}", p["key"].as_str().unwrap_or_default(), p["name"].as_str().unwrap_or_default(), p["format"].as_str().unwrap_or_default(), p["quality"]);
        }
    }
    Ok(OK)
}

fn run_info(files: &[PathBuf], as_json: bool, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, String> {
    let mut failed = false;
    let mut list = Vec::new();
    for f in files {
        match info(f) {
            Ok(i) => {
                if !as_json {
                    let _ = writeln!(out, "{}  {}x{}  {}  {}", f.display(), i.width, i.height, format_bytes(i.bytes), i.format);
                }
                list.push(json!(i));
            }
            Err(e) => {
                failed = true;
                let _ = writeln!(err, "{}: {e}", f.display());
                list.push(json!({ "path": f, "error": e }));
            }
        }
    }
    if as_json {
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(&list).unwrap_or_default());
    }
    Ok(if failed { SOME_FAILED } else { OK })
}

fn run_describe(
    config: &Config,
    file: &Path,
    provider: Option<&str>,
    model: Option<&str>,
    as_json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, String> {
    let cfg = ai_config(config, provider, model)?;
    let texts = match describe_file(&cfg, file) {
        Ok(texts) => texts,
        Err(e) => {
            let _ = writeln!(err, "{}: {e}", file.display());
            return Ok(SOME_FAILED);
        }
    };
    if as_json {
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(&texts).unwrap_or_default());
    } else {
        let _ = writeln!(
            out,
            "{}: {}
{}: {}
{}: {}
{}: {}",
            t("ui.file_name"),
            texts.file_stem,
            t("ui.title"),
            texts.title,
            t("ui.alt"),
            texts.alt,
            t("ui.description"),
            texts.description
        );
    }
    Ok(OK)
}

/// Unter Windows an die Konsole des aufrufenden Terminals hängen (die Release-Exe hat keine eigene).
/// Nur, wenn die Ausgabe nicht ohnehin in eine Datei oder Pipe geht.
#[cfg(windows)]
pub fn attach_parent_console() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_UNKNOWN, GetFileType};
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    // SAFETY: reine Abfragen bzw. Anhängen an eine vorhandene Konsole; keine Zeiger werden gelesen.
    unsafe {
        let missing = |which| {
            let handle = GetStdHandle(which);
            handle.is_null() || handle == INVALID_HANDLE_VALUE || GetFileType(handle) == FILE_TYPE_UNKNOWN
        };
        // Auch wenn nur die Fehlerausgabe fehlt (z. B. „… > out.json“), sonst blieben Fehlermeldungen unsichtbar.
        if missing(STD_OUTPUT_HANDLE) || missing(STD_ERROR_HANDLE) {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
pub fn attach_parent_console() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_words_ignore_case() {
        assert_eq!(destination_from(Some("Replace")), Destination::Target(Target::Replace));
        assert_eq!(destination_from(Some("WEB")), Destination::Target(Target::WebSubfolder));
        assert_eq!(destination_from(Some("Suffix")), Destination::Target(Target::Suffix));
    }

    #[test]
    fn quality_outside_range_is_rejected() {
        let c = Config::default();
        assert!(settings_from(&c, None, None, None, Some(0), None).is_err());
        assert!(settings_from(&c, None, None, None, Some(101), None).is_err());
        assert!(settings_from(&c, None, None, None, Some(70), None).is_ok());
    }
}
