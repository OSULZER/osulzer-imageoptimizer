//! Einstellungsseite: Sprache, Kontextmenü, KI-Verbindung, Info.

use super::ai_settings::{AiSaved, AiSettingsDialog};
use crate::config::Config;
use crate::i18n::{Lang, set_lang, t, tf};
use crate::shell::{self, Registration};
use eframe::egui;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Ai,
    Agents,
    About,
}

/// Was die Seite in diesem Bild geändert hat.
#[derive(Default)]
pub struct SettingsOutcome {
    pub close: bool,
    /// Sprache oder andere Werte in `Config` geändert; speichern.
    pub config_changed: bool,
    pub ai_saved: Option<AiSaved>,
    /// „Jetzt prüfen“ wurde gedrückt.
    pub check_update: bool,
    /// „Installieren“ wurde gedrückt.
    pub offer_install: bool,
    /// Das Programm wurde deinstalliert; das Fenster schließt sich.
    pub uninstalled: bool,
}

pub struct SettingsPage {
    tab: Tab,
    ai: AiSettingsDialog,
    shell_error: Option<String>,
}

/// Steht so in jeder Kopie (Lizenz von mozjpeg), daher unübersetzt.
const IJG_NOTICE: &str = "This software is based in part on the work of the Independent JPEG Group.";

impl SettingsPage {
    pub fn new(config: &Config) -> Self {
        SettingsPage { tab: Tab::General, ai: AiSettingsDialog::new(&config.ai), shell_error: None }
    }

    pub fn show(&mut self, ctx: &egui::Context, config: &mut Config, exe: &Path, registration: &mut Registration) -> SettingsOutcome {
        let mut out = SettingsOutcome::default();
        egui::Modal::new(egui::Id::unique("einstellungen")).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.heading(t("settings.title"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::General, t("settings.tab.general"));
                ui.selectable_value(&mut self.tab, Tab::Ai, t("settings.tab.ai"));
                ui.selectable_value(&mut self.tab, Tab::Agents, t("settings.tab.agents"));
                ui.selectable_value(&mut self.tab, Tab::About, t("settings.tab.about"));
            });
            ui.separator();
            ui.add_space(6.0);
            // Gleiche Höhe für alle Reiter, damit das Fenster beim Wechsel nicht springt;
            // passt der Inhalt nicht, lässt er sich blättern. Höhe richtet sich nach dem Fenster.
            let height = (ctx.content_rect().height() - 190.0).clamp(240.0, 500.0);
            egui::ScrollArea::vertical().id_salt("einstellungen-inhalt").max_height(height).auto_shrink([false, false]).show(ui, |ui| {
                match self.tab {
                    Tab::General => self.general(ui, config, exe, registration, &mut out),
                    Tab::Ai => out.ai_saved = self.ai.show_inline(ui),
                    Tab::Agents => agents(ui, exe),
                    Tab::About => about(ui),
                }
            });
            ui.add_space(12.0);
            ui.separator();
            if ui.add_enabled(!self.ai.busy(), egui::Button::new(t("settings.close"))).clicked() {
                out.close = true;
            }
        });
        out
    }

    fn general(&mut self, ui: &mut egui::Ui, config: &mut Config, exe: &Path, registration: &mut Registration, out: &mut SettingsOutcome) {
        egui::Grid::new("allgemein").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
            ui.label(t("settings.ui_lang"));
            let mut ui_lang = config.ui_lang();
            lang_combo(ui, "ui-lang", &mut ui_lang);
            if ui_lang != config.ui_lang() {
                // Die KI-Sprache bleibt, wie sie bisher wirkte.
                config.ai_lang = Some(config.ai_lang());
                config.ui_lang = Some(ui_lang);
                set_lang(ui_lang);
                if *registration == Registration::Current {
                    self.shell_error = shell::register(exe, t("menu.label")).err().map(|e| tf("ui.failed", &[("e", &e)]));
                }
                out.config_changed = true;
            }
            ui.end_row();

            ui.label(t("settings.ai_lang"));
            let mut ai_lang = config.ai_lang();
            lang_combo(ui, "ai-lang", &mut ai_lang);
            if ai_lang != config.ai_lang() {
                config.ai_lang = Some(ai_lang);
                out.config_changed = true;
            }
            ui.end_row();

            ui.label(t("settings.install"));
            if crate::install::is_installed() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tf("settings.installed_in", &[("dir", &crate::install::install_dir().display())]));
                    if ui.button(t("install.uninstall")).clicked() && matches!(crate::install::uninstall_with_question(), Some(Ok(()))) {
                        out.uninstalled = true;
                    }
                });
            } else if ui.button(t("install.install")).clicked() {
                // Beim nächsten Start erneut anbieten bzw. jetzt über den Startdialog.
                config.install_declined = false;
                out.offer_install = true;
                out.config_changed = true;
            }
            ui.end_row();

            ui.label(t("settings.updates"));
            ui.horizontal(|ui| {
                if ui.checkbox(&mut config.update_check, t("settings.update_check")).changed() {
                    out.config_changed = true;
                }
                if ui.button(t("settings.update_now")).clicked() {
                    out.check_update = true;
                }
            });
            ui.end_row();
        });

        ui.add_space(12.0);
        ui.label(egui::RichText::new(t("settings.menu")).strong());
        let mut change = None;
        match registration.clone() {
            Registration::Current => {
                ui.label(tf("settings.menu.current", &[("label", &t("menu.label"))]));
                if ui.button(t("settings.menu.remove")).clicked() {
                    change = Some(false);
                }
            }
            Registration::None => {
                ui.label(t("settings.menu.none"));
                if ui.button(t("settings.menu.add")).clicked() {
                    change = Some(true);
                }
            }
            Registration::Stale(other) => {
                ui.label(tf("settings.menu.stale", &[("path", &other.display())]));
                ui.horizontal(|ui| {
                    if ui.button(t("settings.menu.fix")).clicked() {
                        change = Some(true);
                    }
                    if ui.button(t("settings.menu.remove")).clicked() {
                        change = Some(false);
                    }
                });
            }
        }
        if let Some(register) = change {
            let result = if register { shell::register(exe, t("menu.label")) } else { shell::unregister() };
            self.shell_error = result.err().map(|e| tf("ui.failed", &[("e", &e)]));
            *registration = shell::status(exe);
        }
        if let Some(error) = &self.shell_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
}

fn lang_combo(ui: &mut egui::Ui, id: &str, value: &mut Lang) {
    egui::ComboBox::from_id_salt(id).selected_text(value.native_name()).width(160.0).show_ui(ui, |ui| {
        for l in Lang::ALL {
            ui.selectable_value(value, l, l.native_name());
        }
    });
}

/// Befehl für Claude Code.
pub fn claude_code_command(exe: &Path) -> String {
    format!("claude mcp add osulzer-imageoptimizer -- \"{}\" mcp", exe.display())
}

/// Eintrag für die Konfiguration von Claude Desktop.
pub fn claude_desktop_json(exe: &Path) -> String {
    let value = serde_json::json!({
        "mcpServers": { "osulzer-imageoptimizer": { "command": exe.to_string_lossy(), "args": ["mcp"] } }
    });
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

fn agents(ui: &mut egui::Ui, exe: &Path) {
    ui.label(t("agents.intro"));
    ui.add_space(10.0);
    let block = |ui: &mut egui::Ui, title: &str, hint: &str, text: String| {
        ui.label(egui::RichText::new(title).strong());
        ui.label(egui::RichText::new(hint).color(super::theme::TEXT_WEAK));
        ui.horizontal(|ui| {
            let mut shown = text.clone();
            ui.add(egui::TextEdit::multiline(&mut shown).code_editor().desired_rows(text.lines().count().max(1)).desired_width(470.0).interactive(false));
            if ui.button(t("agents.copy")).clicked() {
                ui.ctx().copy_text(text);
            }
        });
        ui.add_space(10.0);
    };
    block(ui, t("agents.claude_code"), t("agents.claude_code_hint"), claude_code_command(exe));
    block(ui, t("agents.claude_desktop"), t("agents.claude_desktop_hint"), claude_desktop_json(exe));
    ui.label(egui::RichText::new(tf("agents.cli_hint", &[("exe", &exe.display())])).color(super::theme::TEXT_WEAK));
}

fn about(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(t("app.title")).strong().size(16.0));
    ui.label(tf("settings.about.version", &[("v", &env!("CARGO_PKG_VERSION"))]));
    ui.add_space(6.0);
    ui.label(t("settings.about.text"));
    ui.add_space(6.0);
    ui.label(t("settings.about.license"));
    ui.hyperlink_to(t("settings.about.third_party"), "https://github.com/OSULZER/osulzer-imageoptimizer/releases/latest");
    ui.hyperlink_to("github.com/OSULZER/osulzer-imageoptimizer", "https://github.com/OSULZER/osulzer-imageoptimizer");
    ui.add_space(6.0);
    ui.label(egui::RichText::new(IJG_NOTICE).color(super::theme::TEXT_WEAK));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_quote_the_exe_path() {
        let exe = Path::new(r"C:\Users\Max Muster\Programs\osulzer-imageoptimizer.exe");
        assert_eq!(
            claude_code_command(exe),
            r#"claude mcp add osulzer-imageoptimizer -- "C:\Users\Max Muster\Programs\osulzer-imageoptimizer.exe" mcp"#
        );
        let v: serde_json::Value = serde_json::from_str(&claude_desktop_json(exe)).unwrap();
        assert_eq!(v["mcpServers"]["osulzer-imageoptimizer"]["command"], exe.to_string_lossy().as_ref());
        assert_eq!(v["mcpServers"]["osulzer-imageoptimizer"]["args"][0], "mcp");
    }
}
