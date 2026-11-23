//! Abschnitt „KI“ der Einstellungsseite: Anbieter, Adresse, Modell, API-Schlüssel, Anleitung, Test.

use crate::ai::provider::{Provider, is_local_url};
use crate::ai::secret::load_key_for;
use crate::ai::{AiConfig, list_models, test_connection};
use crate::config::{AiSettings, ProviderSettings};
use crate::i18n::{fmt_decimal, t, tf};
use eframe::egui;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver};

/// Was der Abschnitt nach „Speichern“ meldet.
pub struct AiSaved {
    pub settings: AiSettings,
    /// Schlüssel der Anbieter, deren Feld angezeigt wurde (`None` = löschen).
    pub keys: Vec<(Provider, Option<String>)>,
}

enum Reply {
    Models(Result<Vec<String>, String>),
    Test(Result<String, String>),
}

pub struct AiSettingsDialog {
    ai: AiSettings,
    base_url: String,
    model: String,
    /// Eingegebene Schlüssel je Anbieter (erst beim Anzeigen aus dem Schlüsselbund geladen).
    keys: BTreeMap<Provider, String>,
    models: Vec<String>,
    status: Option<Result<String, String>>,
    busy: bool,
    replies: Option<Receiver<Reply>>,
}

impl AiSettingsDialog {
    pub fn new(settings: &AiSettings) -> Self {
        let mut dialog = AiSettingsDialog {
            ai: settings.clone(),
            base_url: String::new(),
            model: String::new(),
            keys: BTreeMap::new(),
            models: Vec::new(),
            status: None,
            busy: false,
            replies: None,
        };
        dialog.load_fields();
        dialog
    }

    /// Läuft gerade ein Test oder das Laden der Modelle?
    pub fn busy(&self) -> bool {
        self.busy
    }

    fn provider(&self) -> Provider {
        self.ai.provider
    }

    fn load_fields(&mut self) {
        let active = self.ai.active();
        self.base_url = active.base_url;
        self.model = active.model;
        let p = self.provider();
        self.keys.entry(p).or_insert_with(|| load_key_for(p).unwrap_or_default());
        self.models.clear();
        self.status = None;
    }

    fn store_fields(&mut self) {
        self.ai.set_active(ProviderSettings { base_url: self.base_url.trim().to_string(), model: self.model.trim().to_string() });
    }

    fn key(&self) -> &str {
        self.keys.get(&self.provider()).map(String::as_str).unwrap_or_default()
    }

    fn config(&self) -> AiConfig {
        let p = self.provider();
        AiConfig {
            base_url: if p.url_editable() { self.base_url.trim().to_string() } else { p.default_url().to_string() },
            model: self.model.trim().to_string(),
            api_key: Some(self.key().trim().to_string()).filter(|k| !k.is_empty()),
            lang: crate::i18n::lang(),
            protocol: p.protocol(),
        }
    }

    fn spawn(&mut self, ctx: &egui::Context, job: impl FnOnce(AiConfig) -> Reply + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let (cfg, ctx) = (self.config(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(job(cfg));
            ctx.request_repaint();
        });
        self.replies = Some(rx);
        self.busy = true;
        self.status = None;
    }

    fn poll(&mut self) {
        let Some(rx) = &self.replies else { return };
        let Ok(reply) = rx.try_recv() else { return };
        self.busy = false;
        self.replies = None;
        match reply {
            Reply::Models(Ok(models)) => {
                if self.model.trim().is_empty()
                    && let Some(first) = models.first()
                {
                    self.model = first.clone();
                }
                self.status = Some(Ok(tf("ai.models_found", &[("n", &models.len())])));
                self.models = models;
            }
            Reply::Models(Err(e)) | Reply::Test(Err(e)) => self.status = Some(Err(e)),
            Reply::Test(Ok(text)) => self.status = Some(Ok(text)),
        }
    }

    /// Bleiben die Bilder auf dem Rechner bzw. im lokalen Netz?
    fn stays_local(&self) -> bool {
        let p = self.provider();
        p.url_editable() && is_local_url(&self.base_url)
    }

    /// Zeigt die Felder. `Some`, sobald „Speichern“ gedrückt wurde.
    pub fn show_inline(&mut self, ui: &mut egui::Ui) -> Option<AiSaved> {
        self.poll();
        let ctx = ui.ctx().clone();

        // Anbieter
        ui.horizontal(|ui| {
            ui.label(t("ai.provider"));
            let mut chosen = self.provider();
            egui::ComboBox::from_id_salt("ki-anbieter").selected_text(chosen.name()).width(240.0).height(400.0).show_ui(ui, |ui| {
                for p in Provider::ALL {
                    ui.selectable_value(&mut chosen, p, p.name());
                }
            });
            if chosen != self.provider() && !self.busy {
                self.store_fields();
                self.ai.provider = chosen;
                self.load_fields();
            }
        });
        let p = self.provider();
        let privacy = if self.stays_local() {
            egui::RichText::new(t("ai.privacy.local")).color(super::theme::GOOD)
        } else if p == Provider::Custom && self.base_url.trim().is_empty() {
            egui::RichText::new(t("ai.privacy.custom")).color(super::theme::TEXT_WEAK)
        } else {
            let name = if p.url_editable() { self.base_url.trim().to_string() } else { p.name().to_string() };
            egui::RichText::new(tf("ai.privacy.cloud", &[("provider", &name)])).color(super::theme::ACCENT)
        };
        ui.label(privacy);
        ui.add_space(8.0);

        egui::Grid::new("ki-felder").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
            ui.label(t("ai.address"));
            if p.url_editable() {
                ui.add(egui::TextEdit::singleline(&mut self.base_url).hint_text("http://localhost:1234/v1").desired_width(380.0));
            } else {
                ui.label(egui::RichText::new(p.default_url()).color(super::theme::TEXT_WEAK));
            }
            ui.end_row();

            ui.label(t("ai.model"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.model).hint_text(if p == Provider::Anthropic { "claude-haiku-5-5" } else { t("ai.model_hint") }).desired_width(260.0));
                if !self.models.is_empty() {
                    egui::ComboBox::from_id_salt("ki-modelle").selected_text(t("ai.choose")).width(100.0).show_ui(ui, |ui| {
                        for m in self.models.clone() {
                            ui.selectable_value(&mut self.model, m.clone(), m);
                        }
                    });
                }
            });
            ui.end_row();

            ui.label(t("ai.key"));
            if p.needs_key() || p == Provider::Custom {
                let hint = if p.needs_key() { t("ai.key_required") } else { t("ai.optional") };
                let key = self.keys.entry(p).or_default();
                ui.add(egui::TextEdit::singleline(key).password(true).hint_text(hint).desired_width(380.0));
            } else {
                ui.label(egui::RichText::new(t("ai.key_not_needed")).color(super::theme::TEXT_WEAK));
            }
            ui.end_row();
        });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let url_ok = !self.config().base_url.is_empty();
            let key_ok = !p.needs_key() || !self.key().trim().is_empty();
            let ready = url_ok && key_ok && !self.busy;
            if ui.add_enabled(ready, egui::Button::new(t("ai.load_models"))).clicked() {
                self.spawn(&ctx, |cfg| Reply::Models(list_models(&cfg).map_err(|e| e.to_string())));
            }
            let can_test = ready && !self.model.trim().is_empty();
            if ui.add_enabled(can_test, egui::Button::new(t("ai.test"))).clicked() {
                self.spawn(&ctx, |cfg| {
                    Reply::Test(
                        test_connection(&cfg)
                            .map(|d| tf("ai.test_ok", &[("s", &fmt_decimal(d.as_secs_f64(), 1))]))
                            .map_err(|e| e.to_string()),
                    )
                });
            }
            if self.busy {
                ui.spinner();
            }
        });
        match &self.status {
            Some(Ok(text)) => {
                ui.colored_label(super::theme::GOOD, text);
            }
            Some(Err(text)) => {
                ui.colored_label(ui.visuals().error_fg_color, text);
            }
            None => {}
        }

        ui.add_space(10.0);
        guide(ui, p);

        ui.add_space(10.0);
        let mut result = None;
        if ui.button(t("ai.save")).clicked() {
            self.store_fields();
            let keys = self
                .keys
                .iter()
                .filter(|(p, _)| p.needs_key() || **p == Provider::Custom)
                .map(|(p, k)| (*p, Some(k.trim().to_string()).filter(|k| !k.is_empty())))
                .collect();
            self.status = Some(Ok(t("ai.saved").to_string()));
            result = Some(AiSaved { settings: self.ai.clone(), keys });
        }
        result
    }
}

/// Kasten mit der Anleitung zum Anbieter.
fn guide(ui: &mut egui::Ui, p: Provider) {
    egui::Frame::group(ui.style()).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(t("ai.guide_title")).strong());
        ui.add_space(4.0);
        ui.label(t(p.guide_key()));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if let Some(url) = p.key_url() {
                let label = if p.is_local() { t("ai.link_download") } else { t("ai.link_key") };
                ui.hyperlink_to(label, url);
            }
            if let Some(url) = p.pricing_url() {
                ui.hyperlink_to(t("ai.link_pricing"), url);
            }
            if let Some(cost) = p.cost_tier() {
                ui.label(egui::RichText::new(tf("ai.cost", &[("tier", &t(cost))])).color(super::theme::TEXT_WEAK));
            }
        });
    });
}
