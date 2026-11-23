//! MCP-Server für Agenten (Claude Code, Claude Desktop u. a.) über stdin/stdout.

use crate::cli_run::{TextsEntry, ai_config, describe_file, destination_from, presets_json, settings_from, texts_map};
use crate::config::{Config, config_file, load_from};
use crate::headless::{Request, info, optimize};
use crate::naming::NameRules;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ServiceExt, schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Clone, Default)]
pub struct Server;

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct OptimizeParams {
    /// Absolute paths of the images.
    pub paths: Vec<String>,
    /// Preset: web-large (default, 1920 px JPEG Q80), web-medium (1200 px), thumbnail (600 px) or a saved preset name.
    #[serde(default)]
    pub preset: Option<String>,
    /// Longest edge in pixels (never enlarges).
    #[serde(default)]
    pub max_edge: Option<u32>,
    /// jpg, png, webp or keep.
    #[serde(default)]
    pub format: Option<String>,
    /// Quality 1-100. PNG: 100 is lossless, below that the colors are reduced.
    #[serde(default)]
    pub quality: Option<u8>,
    /// Upper limit of the file size in KB.
    #[serde(default)]
    pub max_kb: Option<u32>,
    /// web (subfolder "web" next to the image, default), suffix (next to it with "-web"), replace (original
    /// goes to the recycle bin) or an absolute folder path.
    #[serde(default)]
    pub target: Option<String>,
    /// Same name for all images, numbered -01, -02, ...
    #[serde(default)]
    pub uniform_name: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    /// Text to find in file names (case-insensitive).
    #[serde(default)]
    pub search: Option<String>,
    /// Replacement for `search` (empty removes it).
    #[serde(default)]
    pub replace_with: Option<String>,
    /// SEO texts per image. file_name becomes the new (web-safe) file name; title, alt and description are
    /// embedded as XMP (and EXIF title for JPEG). WordPress reads the alt text on upload.
    #[serde(default)]
    pub texts: Vec<TextsEntry>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct PathsParams {
    /// Absolute paths of the images.
    pub paths: Vec<String>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct PathParam {
    /// Absolute path of the image.
    pub path: String,
}

fn config() -> Config {
    load_from(&config_file())
}

fn pretty(v: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

#[tool_router(server_handler)]
impl Server {
    #[tool(
        description = "Optimize images for the web: resize, compress, convert, rename and embed SEO texts. \
You can see images yourself: pass alt/title/description/file_name per image in `texts` to embed SEO texts \
(XMP, EXIF title for JPEG) without an extra AI key. Originals are never overwritten unless target is 'replace'; \
existing files get a counter (-1, -2). Returns one report per image (output path, size before/after, error)."
    )]
    fn optimize_images(&self, Parameters(p): Parameters<OptimizeParams>) -> String {
        let config = config();
        let settings = match settings_from(&config, p.preset.as_deref(), p.max_edge, p.format.as_deref(), p.quality, p.max_kb) {
            Ok(s) => s,
            Err(e) => return pretty(&json!({ "error": e })),
        };
        let lang = config.ai_lang();
        let rules = NameRules {
            uniform: p.uniform_name.unwrap_or_default(),
            prefix: p.prefix.unwrap_or_default(),
            suffix: p.suffix.unwrap_or_default(),
            search: p.search.unwrap_or_default(),
            replace: p.replace_with.unwrap_or_default(),
        };
        let destination = destination_from(p.target.as_deref());
        if let crate::headless::Destination::Folder(_) = &destination
            && p.target.as_deref().is_some_and(|t| !std::path::Path::new(t.trim()).is_absolute())
        {
            return pretty(&json!({ "error": crate::i18n::t("headless.error.relative") }));
        }
        let req = Request { files: p.paths.iter().map(PathBuf::from).collect(), settings, destination, rules, texts: texts_map(&p.texts, lang), lang };
        let mut out: Vec<serde_json::Value> = optimize(&req).iter().map(|r| json!(r)).collect();
        // Texte ohne passendes Bild melden, damit der Agent den Pfad korrigieren kann.
        for key in crate::headless::unmatched_texts(&req) {
            out.push(json!({ "texts_path": key, "error": crate::i18n::tf("cli.warning.unmatched_texts", &[("path", &key)]) }));
        }
        pretty(&out)
    }

    #[tool(description = "List the available presets (key, name, longest edge, format, quality, size limit).")]
    fn list_presets(&self) -> String {
        pretty(&presets_json(&config()))
    }

    #[tool(description = "Width, height, file size and format of images, before optimizing them.")]
    fn image_info(&self, Parameters(p): Parameters<PathsParams>) -> String {
        let list: Vec<_> = p
            .paths
            .iter()
            .map(|path| match info(std::path::Path::new(path)) {
                Ok(i) => json!(i),
                Err(e) => json!({ "path": path, "error": e }),
            })
            .collect();
        pretty(&list)
    }

    #[tool(
        description = "Let the AI configured in OSULZER ImageOptimizer suggest file name, title, alt text and \
description for one image. Only works if the user set up an AI provider (and allowed cloud providers) in the \
app. If you can see the image yourself, describe it and pass the texts to optimize_images instead."
    )]
    fn describe_image(&self, Parameters(p): Parameters<PathParam>) -> String {
        let config = config();
        let result = ai_config(&config, None, None).and_then(|cfg| describe_file(&cfg, std::path::Path::new(&p.path)));
        match result {
            Ok(texts) => pretty(&texts),
            Err(e) => pretty(&json!({ "error": e })),
        }
    }
}

/// Läuft, bis der Client die Verbindung schließt.
pub fn serve_stdio() -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let service = Server.serve(rmcp::transport::stdio()).await.map_err(|e| e.to_string())?;
        service.waiting().await.map_err(|e| e.to_string())?;
        Ok(())
    })
}
