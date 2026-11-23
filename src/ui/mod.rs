//! Fenster des Programms.

pub mod ai_settings;
pub mod ai_worker;
pub mod preview;
pub mod settings_page;
pub mod theme;
pub mod worker;

use crate::config::{self, Config};
use crate::engine::{EngineError, Probe, probe, thumbnail};
use crate::job::{ConflictPolicy, Session};
use crate::settings::{Format, Settings, Target};
use crate::shell::{self, Registration};
use crate::state::{Batch, Status, format_bytes, format_saving};
use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use crate::i18n::{t, tf};
use settings_page::SettingsPage;
use ai_worker::{AiEvent, AiHandle, start_ai_run};
use crate::state::AiStatus;
use crate::texts::{MAX_ALT, MAX_DESCRIPTION, MAX_TITLE};
use preview::PreviewWorker;
use worker::{RunEvent, RunHandle, RunItem, start_run};

/// Fertig berechnete Vorschau des ausgewählten Bildes.
struct PreviewView {
    id: u64,
    settings: Settings,
    result: Result<PreviewTextures, String>,
}

struct PreviewTextures {
    before: egui::TextureHandle,
    after: egui::TextureHandle,
    /// Pixelgröße der Vorschaubilder.
    size: egui::Vec2,
    width: u32,
    height: u32,
    bytes: u64,
    quality_used: Option<u8>,
    message: Option<String>,
}

const TITLE: &str = "OSULZER ImageOptimizer";

/// Öffnet das Fenster. `initial` sind die Dateien dieses Aufrufs, über `incoming` kommen die
/// Dateien später gestarteter Instanzen.
pub fn run_app(initial: Vec<PathBuf>, incoming: Receiver<Vec<PathBuf>>, notes: Vec<String>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([820.0, 480.0])
            .with_drag_and_drop(true)
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            let mut app = App::new(cc.egui_ctx.clone(), initial, incoming);
            if !notes.is_empty() {
                app.notice = Some(notes.join(" · "));
            }
            Ok(Box::new(app))
        }),
    )
}

fn window_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../../assets/icon.png")).map(|i| i.into_rgba8());
    match img {
        Ok(img) => egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() },
        Err(_) => egui::IconData::default(),
    }
}

/// Kantenlänge der Listen-Vorschaubilder in Punkten und in Pixeln (doppelt für hochauflösende Bildschirme).
const THUMB_POINTS: f32 = 50.0;
const THUMB_PIXELS: u32 = 112;
/// So viele Vorschaubilder werden gleichzeitig berechnet.
const THUMB_WORKERS: usize = 2;
const ROW_HEIGHT: f32 = 66.0;
/// Zeilenhöhe, wenn unter dem Namen noch Dateiname und Alt-Text der KI stehen.
const ROW_HEIGHT_TEXTS: f32 = 106.0;
/// Zeilenhöhe, wenn nur der neue Dateiname darunter steht.
const ROW_HEIGHT_NAME: f32 = 86.0;

/// Nachrichten aus Hintergrund-Threads an das Fenster.
enum Msg {
    /// Dateien einer später gestarteten Instanz.
    Incoming(Vec<PathBuf>),
    Probed(PathBuf, Result<Probe, EngineError>),
    Thumb(u64, Option<egui::ColorImage>),
    /// Ergebnis einer Update-Prüfung; `manual`, wenn über „Jetzt prüfen“ angestoßen.
    UpdateChecked { result: Result<Option<crate::update::check::Release>, String>, manual: bool },
    /// Neue Exe liegt bereit (oder Fehler beim Herunterladen/Austauschen).
    UpdateInstalled(Result<(), String>),
}

/// Zustand eines gefundenen Updates.
enum UpdateState {
    Available(crate::update::check::Release),
    Installing(crate::update::check::Release),
    Failed(crate::update::check::Release, String),
}

enum Dialog {
    None,
    ConfirmReplace,
    /// Rückfrage, bevor Quellbilder nach dem Speichern in den Papierkorb gehen.
    ConfirmDelete,
    Conflict,
    SavePreset { name: String, error: Option<String> },
    /// Einmalige Zustimmung, bevor Bilder an einen Cloud-Anbieter gehen.
    CloudConsent,
    /// Angebot, das Programm zu installieren.
    Install { error: Option<String> },
}

struct App {
    ctx: egui::Context,
    config: Config,
    config_file: PathBuf,
    config_dirty: bool,
    batch: Batch,
    session: Arc<Mutex<Session>>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    run: Option<RunHandle>,
    run_total: usize,
    run_finished: usize,
    selected: Option<u64>,
    dialog: Dialog,
    exe: PathBuf,
    registration: Registration,
    last_output_dir: Option<PathBuf>,
    preview_worker: PreviewWorker,
    /// Zuletzt angefragte Vorschau (Bild und Werte).
    requested: Option<(u64, Settings)>,
    view: Option<PreviewView>,
    /// Vorschau in 100 % statt eingepasst.
    actual_size: bool,
    /// Lage der Trennlinie im Vergleich, 0 = ganz links.
    split: f32,
    thumbs: HashMap<u64, egui::TextureHandle>,
    thumb_jobs: Sender<(u64, PathBuf)>,
    settings_page: Option<SettingsPage>,
    ai_run: Option<AiHandle>,
    ai_total: usize,
    ai_done: usize,
    /// Hinweis für die KI; wird bewusst nicht gespeichert.
    ai_hint: String,
    /// Dateiname, der gerade eingetippt wird (erst beim Verlassen bereinigt).
    stem_edit: Option<(u64, String)>,
    /// Hinweis in der unteren Leiste, z. B. wenn der Schlüssel nicht gespeichert werden konnte.
    notice: Option<String>,
    /// Leiste „Dateinamen“ aufgeklappt.
    names_open: bool,
    update: Option<UpdateState>,
}

impl App {
    fn new(ctx: egui::Context, initial: Vec<PathBuf>, incoming: Receiver<Vec<PathBuf>>) -> Self {
        let (tx, rx) = mpsc::channel();
        // Reicht die Dateien anderer Instanzen weiter und weckt das Fenster.
        {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                while let Ok(paths) = incoming.recv() {
                    if tx.send(Msg::Incoming(paths)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }
        let config_file = config::config_file();
        let exe = std::env::current_exe().unwrap_or_default();
        let wake = ctx.clone();
        let thumb_jobs = spawn_thumb_workers(tx.clone(), ctx.clone());
        let mut app = App {
            thumbs: HashMap::new(),
            thumb_jobs,
            settings_page: None,
            ai_run: None,
            ai_total: 0,
            ai_done: 0,
            ai_hint: String::new(),
            stem_edit: None,
            notice: None,
            names_open: false,
            update: None,
            preview_worker: PreviewWorker::new(move || wake.request_repaint()),
            requested: None,
            view: None,
            actual_size: false,
            split: 0.5,
            ctx,
            config: config::load_from(&config_file),
            config_file,
            config_dirty: false,
            batch: Batch::default(),
            session: Arc::new(Mutex::new(Session::default())),
            tx,
            rx,
            run: None,
            run_total: 0,
            run_finished: 0,
            selected: None,
            dialog: Dialog::None,
            registration: shell::status(&exe),
            exe,
            last_output_dir: None,
        };
        // Gespeicherte Regeln sollen sichtbar sein, sonst wirken sie unbemerkt auf neue Fotos.
        app.names_open = app.config.name_rules.is_active();
        if crate::install::should_offer(&app.exe, crate::install::is_installed(), app.config.install_declined, &crate::install::install_dir()) {
            app.dialog = Dialog::Install { error: None };
        }
        if app.config.update_check && crate::update::check::due(app.config.last_update_check, crate::update::now()) {
            app.check_for_update(false);
        }
        app.add_paths(initial);
        app
    }

    /// Liest die Kennzahlen der Dateien im Hintergrund, damit das Fenster sofort bedienbar ist.
    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            for path in paths {
                let result = probe(&path);
                if tx.send(Msg::Probed(path, result)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
    }

    fn save_config(&mut self) {
        if self.config_dirty {
            let _ = config::save_to(&self.config_file, &self.config);
            self.config_dirty = false;
        }
    }

    fn defaults_changed(&mut self) {
        self.batch.defaults_changed();
        self.config_dirty = true;
    }

    /// Bei einheitlichem Namen verschieben neue oder entfernte Bilder die Laufnummern (und ab 100 Bildern
    /// die Stellenzahl); fertige Bilder werden dann unter der neuen Nummer neu geschrieben.
    fn numbering_changed(&mut self) {
        if !self.config.name_rules.uniform.trim().is_empty() {
            self.batch.rules_changed();
        }
    }

    fn running(&self) -> bool {
        self.run.is_some()
    }

    fn poll_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Incoming(paths) => {
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.add_paths(paths);
                }
                Msg::Probed(path, result) => {
                    let readable = result.is_ok();
                    if let Some(id) = self.batch.add(path.clone(), result)
                        && readable
                    {
                        let _ = self.thumb_jobs.send((id, path));
                        self.numbering_changed();
                    }
                }
                Msg::UpdateChecked { result, manual } => {
                    self.config.last_update_check = crate::update::now();
                    self.config_dirty = true;
                    match result {
                        Ok(Some(release)) => self.update = Some(UpdateState::Available(release)),
                        Ok(None) if manual => self.notice = Some(t("update.none").to_string()),
                        Err(e) if manual => self.notice = Some(tf("update.check_failed", &[("e", &e)])),
                        // Ohne Anstoß bleibt eine fehlgeschlagene Prüfung still.
                        _ => {}
                    }
                }
                Msg::UpdateInstalled(result) => match (result, self.update.take()) {
                    (Ok(()), _) => {
                        self.save_config();
                        let files: Vec<PathBuf> = self.batch.items.iter().map(|i| i.path.clone()).collect();
                        match crate::update::restart(&self.exe, &files) {
                            Ok(()) => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                            Err(e) => self.notice = Some(tf("update.restart_failed", &[("e", &e)])),
                        }
                    }
                    (Err(e), Some(UpdateState::Installing(r))) => self.update = Some(UpdateState::Failed(r, e)),
                    (Err(e), _) => self.notice = Some(e),
                },
                Msg::Thumb(id, image) => {
                    if let Some(image) = image
                        && self.batch.get(id).is_some()
                    {
                        let texture = self.ctx.load_texture(format!("thumb-{id}"), image, egui::TextureOptions::LINEAR);
                        self.thumbs.insert(id, texture);
                    }
                }
            }
        }
    }

    fn poll_run(&mut self) {
        let Some(run) = &self.run else { return };
        let mut finished = false;
        while let Ok(event) = run.events.try_recv() {
            match event {
                RunEvent::Started(id) => {
                    if let Some(item) = self.batch.get_mut(id) {
                        item.status = Status::Running;
                    }
                }
                RunEvent::Finished(id, result) => {
                    self.run_finished += 1;
                    if let Ok(outcome) = &result
                        && let Some(dir) = outcome.path.as_ref().and_then(|p| p.parent())
                    {
                        self.last_output_dir = Some(dir.to_path_buf());
                    }
                    self.batch.finish(id, result);
                }
                RunEvent::AllDone => finished = true,
            }
        }
        if finished {
            self.run = None;
        }
    }

    /// Klick auf „Optimieren“: stellt bei Bedarf erst eine Rückfrage.
    /// Wird gerade ein Update geladen? Dann startet kein neuer Lauf (das Programm startet gleich neu).
    fn updating(&self) -> bool {
        matches!(self.update, Some(UpdateState::Installing(_)))
    }

    fn request_run(&mut self) {
        if self.updating() {
            return;
        }
        if self.config.target == Target::Replace {
            self.dialog = Dialog::ConfirmReplace;
            return;
        }
        if self.config.delete_sources {
            self.dialog = Dialog::ConfirmDelete;
            return;
        }
        self.run_unless_conflict();
    }

    /// Startet den Lauf oder fragt, wenn fremde Dateien im Weg liegen.
    fn run_unless_conflict(&mut self) {
        let session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        let conflict = self
            .batch
            .planned_outputs(&self.config.current, self.config.target, &self.config.name_rules, self.config.ai_lang())
            .iter()
            .any(|(_, src, planned)| session.has_foreign_conflict(planned, src));
        drop(session);
        if conflict {
            self.dialog = Dialog::Conflict;
        } else {
            self.start(ConflictPolicy::Overwrite);
        }
    }

    fn start(&mut self, policy: ConflictPolicy) {
        self.save_config();
        // Keine Quelldatei der Liste darf von einem Ergebnis überschrieben werden.
        self.session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .protect_sources(self.batch.items.iter().map(|i| i.path.clone()));
        let stems = self.batch.final_stems(&self.config.name_rules, self.config.ai_lang());
        let items: Vec<RunItem> = self
            .batch
            .runnable()
            .into_iter()
            .filter_map(|id| {
                let item = self.batch.get(id)?;
                Some(RunItem {
                    id,
                    path: item.path.clone(),
                    settings: self.batch.effective(id, &self.config.current).clone(),
                    texts: item.texts.clone(),
                    lang: self.config.ai_lang(),
                    stem: stems.get(&id).cloned().flatten(),
                    delete_source: self.config.delete_sources,
                })
            })
            .collect();
        if items.is_empty() {
            return;
        }
        self.run_total = items.len();
        self.run_finished = 0;
        let ctx = self.ctx.clone();
        self.run = Some(start_run(items, self.config.target, policy, self.session.clone(), move || ctx.request_repaint()));
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let enabled = !self.running();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.label(t("ui.preset"));
                let presets = self.config.all_presets();
                let active = presets.iter().find(|p| p.settings == self.config.current).map(|p| p.name.clone());
                let shown = active.clone().unwrap_or_else(|| t("ui.custom_settings").to_string());
                egui::ComboBox::from_id_salt("vorgabe").selected_text(shown).width(170.0).show_ui(ui, |ui| {
                    for preset in &presets {
                        if ui.selectable_label(active.as_deref() == Some(&preset.name), &preset.name).clicked() {
                            self.config.current = preset.settings.clone();
                            self.config.last_preset = Some(preset.name.clone());
                            self.defaults_changed();
                        }
                    }
                });
                if ui.button(t("ui.save_preset_button")).on_hover_text(t("ui.save_preset_hover")).clicked() {
                    self.dialog = Dialog::SavePreset { name: String::new(), error: None };
                }
                if let Some(name) = active.filter(|n| self.config.can_delete(n))
                    && ui.button(t("ui.delete")).on_hover_text(t("ui.delete_preset_hover")).clicked()
                {
                    self.config.delete_preset(&name);
                    self.config_dirty = true;
                    self.save_config();
                }
                ui.separator();
                ui.label(t("ui.save_to"));
                let before = self.config.target;
                egui::ComboBox::from_id_salt("ziel").selected_text(target_label(before)).width(210.0).show_ui(ui, |ui| {
                    for target in [Target::WebSubfolder, Target::Suffix, Target::Replace] {
                        ui.selectable_value(&mut self.config.target, target, target_label(target));
                    }
                });
                if self.config.target != before {
                    // Anderes Ziel: bereits fertige Bilder müssen neu geschrieben werden.
                    self.batch.target_changed();
                    self.config_dirty = true;
                }
                let active = self.config.name_rules.is_active();
                let label = egui::RichText::new(t("names.button"));
                let label = if active && !self.names_open { label.color(theme::ACCENT).strong() } else { label };
                let button = ui.add(egui::Button::new(label).selected(self.names_open));
                let button = if active { button.on_hover_text(t("names.active_hover")) } else { button };
                if button.clicked() {
                    self.names_open = !self.names_open;
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(enabled && self.ai_run.is_none(), egui::Button::new(t("settings.open"))).clicked() {
                    self.registration = shell::status(&self.exe);
                    self.settings_page = Some(SettingsPage::new(&self.config));
                }
            });
        });
        ui.add_space(4.0);
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal_wrapped(|ui| {
                if settings_controls(ui, "vorgabe", &mut self.config.current, false) {
                    self.defaults_changed();
                }
                ui.separator();
                // „Original ersetzen“ legt die Quellen ohnehin in den Papierkorb.
                let replacing = self.config.target == Target::Replace;
                let mut delete = self.config.delete_sources || replacing;
                let checkbox = ui
                    .add_enabled(!replacing, egui::Checkbox::new(&mut delete, t("ui.delete_sources")))
                    .on_hover_text(t("ui.delete_sources_hover"))
                    .on_disabled_hover_text(t("ui.delete_sources_replace_hover"));
                if checkbox.changed() {
                    self.config.delete_sources = delete;
                    self.config_dirty = true;
                    if delete {
                        // Bereits fertige Bilder laufen erneut, damit auch ihre Quellen verschwinden.
                        self.batch.target_changed();
                    }
                }
            });
            if self.names_open {
                ui.add_space(8.0);
                if name_rules_controls(ui, &mut self.config.name_rules) {
                    self.batch.rules_changed();
                    self.config_dirty = true;
                }
            }
        });
        ui.add_space(8.0);
    }

    /// Fragt GitHub im Hintergrund (nur mit echtem Signaturschlüssel, sonst wäre kein Update gültig).
    fn check_for_update(&mut self, manual: bool) {
        if !crate::update::sign::has_real_key() {
            if manual {
                self.notice = Some(t("update.disabled").to_string());
            }
            return;
        }
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let result = crate::update::check::fetch_latest(crate::update::check::LATEST_URL, env!("CARGO_PKG_VERSION"));
            let _ = tx.send(Msg::UpdateChecked { result, manual });
            ctx.request_repaint();
        });
    }

    fn install_update(&mut self, release: crate::update::check::Release) {
        let (tx, ctx, exe) = (self.tx.clone(), self.ctx.clone(), self.exe.clone());
        let r = release.clone();
        self.update = Some(UpdateState::Installing(release));
        std::thread::spawn(move || {
            let mut new = exe.as_os_str().to_owned();
            new.push(".new");
            let new = PathBuf::from(new);
            let result = crate::update::check::download_verified(&r, crate::update::sign::PUBLIC_KEY, &new)
                .and_then(|()| crate::update::swap::swap(&exe, &new));
            let _ = std::fs::remove_file(&new);
            let _ = tx.send(Msg::UpdateInstalled(result));
            ctx.request_repaint();
        });
    }

    /// Leiste über der unteren Leiste, wenn eine neue Version bereitsteht.
    fn update_bar(&mut self, ui: &mut egui::Ui) {
        let busy = self.running() || self.ai_run.is_some();
        let mut action = None;
        ui.add_space(6.0);
        ui.horizontal(|ui| match &self.update {
            Some(UpdateState::Available(r)) | Some(UpdateState::Failed(r, _)) => {
                let text = tf("update.available", &[("v", &r.tag.trim_start_matches('v'))]);
                ui.label(egui::RichText::new(text).color(theme::ACCENT).strong());
                if !r.notes_url.is_empty() {
                    ui.hyperlink_to(t("update.notes"), &r.notes_url);
                }
                if let Some(UpdateState::Failed(_, e)) = &self.update {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t("update.later")).clicked() {
                        action = Some(false);
                    }
                    let button = ui.add_enabled(!busy, egui::Button::new(t("update.install")));
                    if button.on_disabled_hover_text(t("update.busy")).clicked() {
                        action = Some(true);
                    }
                });
            }
            Some(UpdateState::Installing(_)) => {
                ui.spinner();
                ui.label(t("update.installing"));
            }
            None => {}
        });
        match (action, self.update.take()) {
            (Some(true), Some(UpdateState::Available(r) | UpdateState::Failed(r, _))) => self.install_update(r),
            (Some(false), _) => {}
            (_, state) => self.update = state,
        }
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let totals = self.batch.totals();
            let mut summary = match totals.count {
                1 => tf("ui.summary_one", &[("size", &format_bytes(totals.source_bytes))]),
                n => tf("ui.summary_many", &[("n", &n), ("size", &format_bytes(totals.source_bytes))]),
            };
            if totals.done > 0 {
                summary += &tf(
                    "ui.summary_done",
                    &[
                        ("n", &totals.done),
                        ("result", &format_bytes(totals.result_bytes)),
                        ("source", &format_bytes(totals.done_source_bytes)),
                        ("saving", &format_saving(totals.done_source_bytes, totals.result_bytes)),
                    ],
                );
            }
            if totals.failed > 0 {
                summary += &tf("ui.summary_failed", &[("n", &totals.failed)]);
            }
            let summary_full = summary.clone();
            let notice = self.notice.clone();
            // Knöpfe von rechts; die Summe bekommt den restlichen Platz und wird notfalls gekürzt.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(run) = &self.run {
                    if ui.button(t("ui.cancel")).clicked() {
                        run.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    let fraction = self.run_finished as f32 / self.run_total.max(1) as f32;
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_width(220.0)
                            .text(tf("ui.progress", &[("done", &self.run_finished), ("total", &self.run_total)])),
                    );
                } else {
                    let open = self.batch.runnable().len();
                    let label = if open > 0 { tf("ui.optimize_n", &[("n", &open)]) } else { t("ui.optimize").to_string() };
                    if theme::primary_button(ui, &label, open > 0 && self.ai_run.is_none()).clicked() {
                        self.request_run();
                    }
                    self.ai_controls(ui);
                    if let Some(dir) = self.last_output_dir.clone()
                        && ui.button(t("ui.open_target")).clicked()
                    {
                        shell::open_folder(&dir);
                    }
                    if ui.button(t("ui.add_images")).clicked() {
                        let extensions: Vec<&str> = shell::EXTENSIONS.iter().map(|e| e.trim_start_matches('.')).collect();
                        if let Some(files) = rfd::FileDialog::new().add_filter(t("ui.images"), &extensions).pick_files() {
                            self.add_paths(files);
                        }
                    }
                    if !self.batch.items.is_empty() && ui.button(t("ui.clear_list")).clicked() {
                        if let Some(run) = &self.ai_run {
                            run.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                        self.batch.clear();
                        self.thumbs.clear();
                        self.selected = None;
                        self.requested = None;
                        self.view = None;
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let summary = ui.add(egui::Label::new(&summary).truncate());
                    summary.on_hover_text(&summary_full);
                    if let Some(notice) = &notice {
                        ui.add(egui::Label::new(egui::RichText::new(notice).color(ui.visuals().error_fg_color)).truncate());
                    }
                });
            });
        });
        ui.add_space(6.0);
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        if self.batch.items.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new(tf("ui.empty_hint", &[("label", &t("menu.label"))])).size(16.0).color(theme::TEXT_WEAK));
            });
            return;
        }
        let enabled = !self.running();
        let mut select = None;
        let mut remove = None;
        ui.add_space(6.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        let previews = self.batch.preview_names(&self.config.current, self.config.target, &self.config.name_rules, self.config.ai_lang());
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for item in &self.batch.items {
                let selected = self.selected == Some(item.id);
                let new_name = previews.get(&item.id).cloned();
                let (clicked, removed) = list_row(ui, item, self.thumbs.get(&item.id), selected, enabled, new_name);
                if clicked {
                    select = Some(item.id);
                }
                if removed {
                    remove = Some(item.id);
                }
            }
        });
        if let Some(id) = select {
            self.selected = if self.selected == Some(id) { None } else { Some(id) };
        }
        if let Some(id) = remove {
            if let Some(run) = &self.ai_run {
                run.skip(id);
            }
            self.batch.remove(id);
            self.thumbs.remove(&id);
            self.numbering_changed();
            if self.selected == Some(id) {
                self.selected = None;
            }
        }
    }

    fn ai_configured(&self) -> bool {
        self.config.ai.is_configured()
    }

    /// Knopf „KI-Beschreibung“ und Hinweisfeld (rechts-nach-links, links vom Optimieren-Knopf).
    fn ai_controls(&mut self, ui: &mut egui::Ui) {
        if let Some(run) = &self.ai_run {
            if ui.button(t("ui.ai_cancel")).clicked() {
                run.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            ui.spinner();
            ui.label(tf("ui.ai_progress", &[("done", &self.ai_done), ("total", &self.ai_total)]));
            return;
        }
        let candidates = self.batch.ai_candidates().len();
        let ready = self.ai_configured() && candidates > 0 && !self.running();
        let button = ui.add_enabled(ready, egui::Button::new(t("ui.ai_describe")));
        let button = if self.ai_configured() {
            button.on_hover_text(t("ui.ai_describe_hover"))
        } else {
            button.on_disabled_hover_text(t("ui.ai_not_configured"))
        };
        if button.clicked() {
            if self.config.ai.needs_consent() {
                self.dialog = Dialog::CloudConsent;
            } else {
                self.start_ai();
            }
        }
        if self.ai_configured() {
            ui.add(
                egui::TextEdit::singleline(&mut self.ai_hint)
                    .hint_text(t("ui.ai_hint"))
                    .desired_width(240.0),
            );
        }
    }

    fn start_ai(&mut self) {
        if self.updating() {
            return;
        }
        let items: Vec<(u64, PathBuf)> = self
            .batch
            .ai_candidates()
            .into_iter()
            .filter_map(|id| self.batch.get(id).map(|i| (id, i.path.clone())))
            .collect();
        if items.is_empty() {
            return;
        }
        for (id, _) in &items {
            self.batch.set_ai_status(*id, AiStatus::Waiting);
        }
        let cfg = crate::ai::AiConfig {
            base_url: self.config.ai.active().base_url,
            model: self.config.ai.active().model,
            api_key: crate::ai::secret::load_key_for(self.config.ai.provider),
            lang: self.config.ai_lang(),
            protocol: self.config.ai.provider.protocol(),
        };
        self.ai_total = items.len();
        self.ai_done = 0;
        let ctx = self.ctx.clone();
        self.ai_run = Some(start_ai_run(items, cfg, self.ai_hint.clone(), move || ctx.request_repaint()));
    }

    fn poll_ai(&mut self) {
        let Some(run) = &self.ai_run else { return };
        let mut finished = false;
        while let Ok(event) = run.events.try_recv() {
            match event {
                AiEvent::Started(id) => self.batch.set_ai_status(id, AiStatus::Running),
                AiEvent::Finished(id, result) => {
                    self.ai_done += 1;
                    self.batch.ai_finished(id, result);
                }
                AiEvent::AllDone => finished = true,
            }
        }
        if finished {
            // Abgebrochene, noch wartende Bilder wieder freigeben.
            for item in &mut self.batch.items {
                if matches!(item.ai, AiStatus::Waiting | AiStatus::Running) {
                    item.ai = AiStatus::Idle;
                }
            }
            self.ai_run = None;
        }
    }

    /// Abschnitt „Texte“ in der Seitenleiste.
    fn texts_editor(&mut self, ui: &mut egui::Ui, id: u64) {
        let Some(item) = self.batch.get(id) else { return };
        let mut texts = item.texts.clone().unwrap_or_default();
        let had_texts = item.texts.is_some();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t("ui.texts_heading")).strong());
            if let AiStatus::Failed(e) = &item.ai {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
        let mut changed = false;
        let width = (ui.available_width() - 8.0).max(200.0);
        let label_row = |ui: &mut egui::Ui, label: &str, count: Option<(usize, usize)>| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).color(theme::TEXT_WEAK));
                if let Some((n, max)) = count {
                    ui.label(egui::RichText::new(format!("{n}/{max}")).small().color(theme::TEXT_WEAK));
                }
            });
        };
        // Dateiname: beim Tippen roh, beim Verlassen bereinigt.
        label_row(ui, t("ui.file_name"), None);
        let mut buffer = match &self.stem_edit {
            Some((edit_id, text)) if *edit_id == id => text.clone(),
            _ => texts.file_stem.clone(),
        };
        let response = ui.add(egui::TextEdit::singleline(&mut buffer).hint_text(t("ui.file_name_hint")).desired_width(width));
        if response.changed() || response.has_focus() {
            self.stem_edit = Some((id, buffer.clone()));
        }
        if response.lost_focus() {
            let clean = crate::texts::slugify_in(&buffer, self.config.ai_lang());
            if clean != texts.file_stem {
                texts.file_stem = clean;
                changed = true;
            }
            self.stem_edit = None;
        }
        for (label, value, max, lines) in [
            (t("ui.title"), &mut texts.title, MAX_TITLE, 1),
            (t("ui.alt"), &mut texts.alt, MAX_ALT, 2),
            (t("ui.description"), &mut texts.description, MAX_DESCRIPTION, 3),
        ] {
            ui.add_space(2.0);
            label_row(ui, label, Some((value.chars().count(), max)));
            let edit = egui::TextEdit::multiline(value).desired_rows(lines).desired_width(width);
            if ui.add(edit).changed() {
                let clamped: String = value.chars().take(max).collect();
                *value = clamped;
                changed = true;
            }
        }
        ui.add_space(4.0);
        if changed {
            self.batch.edit_texts(id, Some(texts));
        }
        if had_texts && ui.button(t("ui.discard_texts")).clicked() {
            self.batch.edit_texts(id, None);
            self.stem_edit = None;
        }
    }

    /// Übernimmt eine fertige Vorschau, wenn sie noch zur aktuellen Anfrage passt.
    fn poll_preview(&mut self) {
        let Some((id, settings, result)) = self.preview_worker.poll() else { return };
        if self.requested.as_ref() != Some(&(id, settings.clone())) {
            return;
        }
        let load = |name: &str, img: &image::RgbaImage| {
            let size = [img.width() as usize, img.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            self.ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
        };
        let result = result.map(|p| PreviewTextures {
            before: load("vorher", &p.before),
            after: load("nachher", &p.after),
            size: egui::vec2(p.before.width() as f32, p.before.height() as f32),
            width: p.encoded.width,
            height: p.encoded.height,
            bytes: p.encoded.data.len() as u64,
            quality_used: p.encoded.quality_used,
            message: crate::state::message_for(p.encoded.note, None),
        });
        self.view = Some(PreviewView { id, settings, result });
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected else { return };
        let Some(item) = self.batch.get(id) else { return };
        let path = item.path.clone();
        let source = item.probe.as_ref().ok().map(|p| p.bytes);
        let has_own = item.own.is_some();
        let settings = self.batch.effective(id, &self.config.current).clone();

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading(path.file_name().unwrap_or_default().to_string_lossy());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("×").on_hover_text(t("ui.close_preview")).clicked() {
                    self.selected = None;
                }
            });
        });
        let Some(source) = source else {
            ui.label(t("ui.no_preview"));
            return;
        };

        let wanted = (id, settings.clone());
        if self.requested.as_ref() != Some(&wanted) {
            self.preview_worker.request(id, path, settings.clone());
            self.requested = Some(wanted);
        }
        let current = self.view.as_ref().filter(|v| v.id == id);
        let fresh = current.is_some_and(|v| v.settings == settings);
        let info: Option<Result<String, String>> = current.map(|v| match &v.result {
            Ok(t) => {
                let mut text = format!("{}×{} · {} ({})", t.width, t.height, format_bytes(t.bytes), format_saving(source, t.bytes));
                if let Some(q) = t.quality_used {
                    text += &tf("ui.quality_used", &[("q", &q)]);
                }
                if let Some(m) = &t.message {
                    text += &format!(" · {m}");
                }
                Ok(text)
            }
            Err(e) => Err(e.clone()),
        });

        // Oben der Vergleich mit festem Anteil der Höhe, darunter alle Bedienelemente zum Blättern.
        let image_height = (ui.available_height() * 0.5).max(180.0);
        let image_size = egui::vec2(ui.available_width(), image_height);
        ui.allocate_ui_with_layout(image_size, egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.set_min_size(image_size);
            let Some(Ok(t)) = self.view.as_ref().filter(|v| v.id == id).map(|v| &v.result) else {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
                return;
            };
            let (before, after, size) = (t.before.id(), t.after.id(), t.size);
            if self.actual_size {
                let points = size / ui.ctx().pixels_per_point();
                egui::ScrollArea::both().id_salt("vergleich").max_height(image_height).auto_shrink(false).show(ui, |ui| {
                    let (rect, response) = ui.allocate_exact_size(points, egui::Sense::click_and_drag());
                    compare(ui, rect, &response, before, after, &mut self.split);
                });
            } else {
                let available = image_size;
                let scale = (available.x / size.x).min(available.y / size.y).min(1.0 / ui.ctx().pixels_per_point());
                let shown = size * scale;
                let area = egui::Rect::from_min_size(ui.max_rect().min, available);
                let rect = egui::Rect::from_center_size(area.center(), shown);
                let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                compare(ui, rect, &response, before, after, &mut self.split);
            }
        });

        ui.add_space(6.0);
        egui::ScrollArea::vertical().id_salt("vorschau-bedienung").auto_shrink(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.actual_size, false, t("ui.fit"));
                ui.selectable_value(&mut self.actual_size, true, "100 %");
                ui.separator();
                match &info {
                    Some(Ok(text)) => {
                        ui.label(egui::RichText::new(text).strong());
                    }
                    Some(Err(e)) => {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    None => {}
                }
                if !fresh {
                    ui.spinner();
                }
            });
            ui.add_space(4.0);
            ui.add_enabled_ui(!self.running(), |ui| {
                let mut own = has_own;
                if ui.checkbox(&mut own, t("ui.own_values")).changed() {
                    self.batch.set_own(id, own.then(|| self.config.current.clone()));
                }
                if let Some(mut values) = self.batch.get(id).and_then(|i| i.own.clone()) {
                    let changed = settings_controls(ui, "eigen", &mut values, true);
                    if changed {
                        self.batch.set_own(id, Some(values));
                    }
                }
            });
            ui.add_space(4.0);
            ui.separator();
            ui.add_enabled_ui(!self.running() && self.ai_run.is_none(), |ui| self.texts_editor(ui, id));
            ui.add_space(6.0);
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(page) = &mut self.settings_page {
            let out = page.show(ctx, &mut self.config, &self.exe, &mut self.registration);
            if let Some(saved) = out.ai_saved {
                self.config.ai = saved.settings;
                self.config_dirty = true;
                self.notice = saved
                    .keys
                    .iter()
                    .find_map(|(p, k)| crate::ai::secret::store_key_for(*p, k.as_deref()).err())
                    .map(|e| tf("ui.key_not_saved", &[("e", &e)]));
            }
            if out.config_changed {
                self.config_dirty = true;
            }
            if out.check_update {
                self.check_for_update(true);
            }
            if out.offer_install && (self.running() || self.ai_run.is_some()) {
                self.notice = Some(t("update.busy").to_string());
            } else if out.offer_install {
                self.settings_page = None;
                self.dialog = Dialog::Install { error: None };
                return;
            }
            if out.config_changed || out.close {
                self.save_config();
            }
            if out.close {
                self.settings_page = None;
            }
        }
        let mut close = false;
        let mut start = None;
        let mut consent = false;
        let mut install = None;
        let mut confirmed_delete = false;
        match &mut self.dialog {
            Dialog::None => {}
            Dialog::Install { error } => {
                egui::Modal::new(egui::Id::unique("installieren")).frame(theme::modal_frame(ctx)).show(ctx, |ui| {
                    ui.set_width(480.0);
                    ui.heading(t("install.title"));
                    ui.add_space(10.0);
                    ui.label(t("install.intro"));
                    ui.add_space(4.0);
                    let bullet = |ui: &mut egui::Ui, text: &str| {
                        ui.horizontal_top(|ui| {
                            ui.colored_label(theme::ACCENT, "•");
                            ui.add(egui::Label::new(text).wrap());
                        });
                    };
                    bullet(ui, t("install.item_copy"));
                    ui.indent("installationsordner", |ui| {
                        egui::Frame::new()
                            .fill(ui.visuals().extreme_bg_color)
                            .corner_radius(6)
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                let dir = crate::install::install_dir().display().to_string();
                                ui.add(egui::Label::new(egui::RichText::new(dir).monospace().color(theme::TEXT_WEAK)).wrap());
                            });
                    });
                    bullet(ui, t("install.item_menu"));
                    bullet(ui, t("install.item_start"));
                    bullet(ui, t("install.item_apps"));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(t("install.note")).small().color(theme::TEXT_WEAK));
                    if let Some(e) = error {
                        ui.add_space(4.0);
                        ui.colored_label(ui.visuals().error_fg_color, e.as_str());
                    }
                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        if theme::primary_button(ui, t("install.install"), true).clicked() {
                            install = Some(true);
                        }
                        if ui.button(t("install.not_now")).clicked() {
                            close = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let never = egui::RichText::new(t("install.never")).color(theme::TEXT_WEAK);
                            if ui.add(egui::Button::new(never).frame(false)).clicked() {
                                install = Some(false);
                            }
                        });
                    });
                });
            }
            Dialog::CloudConsent => {
                let ai = &self.config.ai;
                let provider = if ai.provider.url_editable() { ai.active().base_url } else { ai.provider.name().to_string() };
                egui::Modal::new(egui::Id::unique("zustimmung")).frame(theme::modal_frame(ctx)).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.heading(t("consent.title"));
                    ui.label(tf("consent.text", &[("provider", &provider)]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t("consent.allow")).clicked() {
                            consent = true;
                        }
                        if ui.button(t("ui.cancel")).clicked() {
                            close = true;
                        }
                    });
                });
            }
            Dialog::ConfirmReplace => {
                egui::Modal::new(egui::Id::unique("ersetzen")).frame(theme::modal_frame(ctx)).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.heading(t("ui.replace_title"));
                    ui.label(t("ui.replace_text"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t("ui.replace")).clicked() {
                            start = Some(ConflictPolicy::Rename);
                        }
                        if ui.button(t("ui.cancel")).clicked() {
                            close = true;
                        }
                    });
                });
            }
            Dialog::ConfirmDelete => {
                egui::Modal::new(egui::Id::unique("loeschen")).frame(theme::modal_frame(ctx)).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.heading(t("ui.delete_title"));
                    ui.label(t("ui.delete_text"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t("ui.delete_confirm")).clicked() {
                            confirmed_delete = true;
                        }
                        if ui.button(t("ui.cancel")).clicked() {
                            close = true;
                        }
                    });
                });
            }
            Dialog::Conflict => {
                egui::Modal::new(egui::Id::unique("konflikt")).frame(theme::modal_frame(ctx)).show(ctx, |ui| {
                    ui.set_width(440.0);
                    ui.heading(t("ui.conflict_title"));
                    ui.label(t("ui.conflict_text"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t("ui.overwrite")).clicked() {
                            start = Some(ConflictPolicy::Overwrite);
                        }
                        if ui.button(t("ui.rename")).on_hover_text(t("ui.rename_hover")).clicked() {
                            start = Some(ConflictPolicy::Rename);
                        }
                        if ui.button(t("ui.skip")).clicked() {
                            start = Some(ConflictPolicy::Skip);
                        }
                        if ui.button(t("ui.cancel")).clicked() {
                            close = true;
                        }
                    });
                });
            }
            Dialog::SavePreset { name, error } => {
                let mut save = false;
                egui::Modal::new(egui::Id::unique("vorgabe-speichern")).frame(theme::modal_frame(ctx)).show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.heading(t("ui.save_preset_title"));
                    ui.label(t("ui.preset_name"));
                    let field = ui.text_edit_singleline(name);
                    field.request_focus();
                    if let Some(error) = error {
                        ui.colored_label(ui.visuals().error_fg_color, error.as_str());
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t("ui.save")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            save = true;
                        }
                        if ui.button(t("ui.cancel")).clicked() {
                            close = true;
                        }
                    });
                });
                if save {
                    match self.config.save_preset(name, &self.config.current.clone()) {
                        Ok(()) => {
                            self.config.last_preset = Some(name.trim().to_string());
                            self.config_dirty = true;
                            close = true;
                        }
                        Err(text) => *error = Some(text),
                    }
                }
            }
        }
        match install {
            Some(true) => match crate::install::install(&self.exe) {
                Ok(installed) => {
                    self.save_config();
                    let files: Vec<PathBuf> = self.batch.items.iter().map(|i| i.path.clone()).collect();
                    // Aus dem Installationsordner neu starten, damit Kontextmenü und Updates dorthin zeigen.
                    if crate::update::restart(&installed, &files).is_ok() {
                        self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    self.dialog = Dialog::None;
                }
                Err(e) => self.dialog = Dialog::Install { error: Some(tf("ui.failed", &[("e", &e)])) },
            },
            Some(false) => {
                self.config.install_declined = true;
                self.config_dirty = true;
                self.dialog = Dialog::None;
            }
            None => {}
        }
        if consent {
            self.dialog = Dialog::None;
            self.config.ai.give_consent();
            self.config_dirty = true;
            self.save_config();
            self.start_ai();
        }
        if confirmed_delete {
            self.dialog = Dialog::None;
            self.run_unless_conflict();
        } else if let Some(policy) = start {
            self.dialog = Dialog::None;
            self.start(policy);
        } else if close {
            self.dialog = Dialog::None;
            self.save_config();
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_messages();
        self.poll_run();
        self.poll_preview();
        self.poll_ai();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Hineingezogene Dateien.
        let dropped: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        self.add_paths(dropped);

        egui::Panel::top("oben").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("unten").show(ui, |ui| self.bottom_bar(ui));
        if self.update.is_some() {
            egui::Panel::bottom("update").show(ui, |ui| self.update_bar(ui));
        }
        if self.selected.is_some() {
            egui::Panel::right("vorschau")
                .resizable(true)
                .default_size(520.0)
                .min_size(360.0)
                .show(ui, |ui| self.preview_panel(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| self.list(ui));
        self.dialogs(&ui.ctx().clone());
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.save_config();
    }
}

fn format_label(f: Format) -> &'static str {
    match f {
        Format::Original => t("ui.format_original"),
        Format::Jpeg => "JPEG",
        Format::Png => "PNG",
        Format::Webp => "WebP",
    }
}

fn target_label(target: Target) -> &'static str {
    match target {
        Target::WebSubfolder => t("ui.target_web"),
        Target::Suffix => t("ui.target_suffix"),
        Target::Replace => t("ui.target_replace"),
    }
}

/// Felder der Leiste „Dateinamen“. Gibt zurück, ob sich etwas geändert hat.
fn name_rules_controls(ui: &mut egui::Ui, rules: &mut crate::naming::NameRules) -> bool {
    let before = rules.clone();
    egui::Frame::new()
        .fill(theme::SURFACE)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t("names.title")).strong().size(15.0));
                ui.label(egui::RichText::new(t("names.subtitle")).color(theme::TEXT_WEAK));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let any = *rules != crate::naming::NameRules::default();
                    if ui.add_enabled(any, egui::Button::new(t("names.reset"))).clicked() {
                        *rules = crate::naming::NameRules::default();
                    }
                });
            });
            ui.add_space(10.0);
            let field = |ui: &mut egui::Ui, value: &mut String, hint: &str| {
                // Feste Größe: im Grid würde die Spalte das Feld sonst auf ihre bisherige Breite stauchen.
                ui.add_sized([230.0, ui.spacing().interact_size.y], egui::TextEdit::singleline(value).hint_text(hint));
            };
            let label = |ui: &mut egui::Ui, text: &str| {
                ui.label(egui::RichText::new(text).color(theme::TEXT_WEAK));
            };
            egui::Grid::new("namensregeln").num_columns(4).spacing([14.0, 10.0]).show(ui, |ui| {
                label(ui, t("names.uniform"));
                field(ui, &mut rules.uniform, t("names.uniform_example"));
                ui.label(egui::RichText::new(t("names.uniform_hint")).color(theme::TEXT_WEAK).small());
                ui.label("");
                ui.end_row();

                label(ui, t("names.prefix"));
                field(ui, &mut rules.prefix, t("names.prefix_example"));
                label(ui, t("names.suffix"));
                field(ui, &mut rules.suffix, t("names.suffix_example"));
                ui.end_row();

                label(ui, t("names.search"));
                field(ui, &mut rules.search, "IMG_");
                label(ui, t("names.replace"));
                field(ui, &mut rules.replace, t("names.replace_example"));
                ui.end_row();
            });
        });
    *rules != before
}

/// Bedienelemente für Längste Kante, Format, Qualität und Max. Dateigröße.
/// `compact` ordnet sie für die schmale Vorschauspalte in Zeilen an.
/// Gibt zurück, ob sich etwas geändert hat.
fn settings_controls(ui: &mut egui::Ui, id: &str, s: &mut Settings, compact: bool) -> bool {
    let before = s.clone();
    if compact {
        ui.horizontal(|ui| {
            edge_control(ui, s);
            ui.add_space(12.0);
            format_control(ui, id, s);
        });
        ui.horizontal(|ui| quality_control(ui, s));
        ui.horizontal(|ui| size_control(ui, s));
    } else {
        edge_control(ui, s);
        ui.separator();
        format_control(ui, id, s);
        ui.separator();
        quality_control(ui, s);
        size_control(ui, s);
    }
    *s != before
}

fn edge_control(ui: &mut egui::Ui, s: &mut Settings) {
    let mut limit_edge = s.max_edge.is_some();
    ui.checkbox(&mut limit_edge, t("ui.max_edge")).on_hover_text(t("ui.max_edge_hover"));
    match (limit_edge, &mut s.max_edge) {
        (true, Some(edge)) => {
            ui.add(egui::DragValue::new(edge).range(16..=20000).suffix(" px"));
        }
        (true, None) => s.max_edge = Some(1920),
        (false, _) => s.max_edge = None,
    }
}

fn format_control(ui: &mut egui::Ui, id: &str, s: &mut Settings) {
    ui.label(t("ui.format"));
    egui::ComboBox::from_id_salt((id, "format")).selected_text(format_label(s.format)).width(110.0).show_ui(ui, |ui| {
        for format in [Format::Original, Format::Jpeg, Format::Png, Format::Webp] {
            ui.selectable_value(&mut s.format, format, format_label(format));
        }
    });
}

fn quality_control(ui: &mut egui::Ui, s: &mut Settings) {
    ui.label(t("ui.quality"));
    ui.add(egui::Slider::new(&mut s.quality, 1..=100)).on_hover_text(t("ui.quality_hover"));
}

fn size_control(ui: &mut egui::Ui, s: &mut Settings) {
    let mut limit_size = s.max_kb.is_some();
    ui.checkbox(&mut limit_size, t("ui.max_size")).on_hover_text(t("ui.max_size_hover"));
    match (limit_size, &mut s.max_kb) {
        (true, Some(kb)) => {
            ui.add(egui::DragValue::new(kb).range(10..=50_000).suffix(" KB"));
        }
        (true, None) => s.max_kb = Some(300),
        (false, _) => s.max_kb = None,
    }
}

/// Zeichnet „vorher“ links und „nachher“ rechts der verschiebbaren Trennlinie.
fn compare(
    ui: &egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    before: egui::TextureId,
    after: egui::TextureId,
    split: &mut f32,
) {
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        *split = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    }
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let x = rect.left() + rect.width() * *split;
    ui.painter_at(rect).image(before, rect, uv, egui::Color32::WHITE);
    let right = egui::Rect::from_min_max(egui::pos2(x, rect.top()), rect.max);
    ui.painter_at(right).image(after, rect, uv, egui::Color32::WHITE);

    let painter = ui.painter_at(rect);
    painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(2.0, egui::Color32::WHITE));
    let visible = ui.clip_rect().intersect(rect);
    let label = |pos: egui::Pos2, align: egui::Align2, text: &str| {
        let galley_rect = painter.text(pos, align, text, egui::FontId::proportional(14.0), egui::Color32::WHITE);
        painter.rect_filled(galley_rect.expand(4.0), 3.0, egui::Color32::from_black_alpha(150));
        painter.text(pos, align, text, egui::FontId::proportional(14.0), egui::Color32::WHITE);
    };
    label(egui::pos2(visible.left() + 10.0, visible.top() + 10.0), egui::Align2::LEFT_TOP, t("ui.before"));
    label(egui::pos2(visible.right() - 10.0, visible.top() + 10.0), egui::Align2::RIGHT_TOP, t("ui.after"));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
}

/// Startet die Hintergrund-Threads für die Listen-Vorschaubilder.
fn spawn_thumb_workers(tx: Sender<Msg>, ctx: egui::Context) -> Sender<(u64, PathBuf)> {
    let (jobs, queue) = mpsc::channel::<(u64, PathBuf)>();
    let queue = Arc::new(Mutex::new(queue));
    for _ in 0..THUMB_WORKERS {
        let (queue, tx, ctx) = (queue.clone(), tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            loop {
                let job = queue.lock().unwrap_or_else(|p| p.into_inner()).recv();
                let Ok((id, path)) = job else { return };
                let image = std::panic::catch_unwind(|| thumbnail(&path, THUMB_PIXELS)).ok().and_then(Result::ok).map(|t| {
                    egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw())
                });
                if tx.send(Msg::Thumb(id, image)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }
    jobs
}

/// Zeichnet eine Listenzeile. Die ganze Zeile ist klickbar.
/// Gibt zurück, ob die Zeile angeklickt und ob „entfernen“ gedrückt wurde.
fn list_row(
    ui: &mut egui::Ui,
    item: &crate::state::Item,
    thumb: Option<&egui::TextureHandle>,
    selected: bool,
    enabled: bool,
    new_name: Option<String>,
) -> (bool, bool) {
    let width = ui.available_width();
    let ai_active = !matches!(item.ai, AiStatus::Idle);
    let with_texts = ai_active || item.texts.is_some();
    let tall = with_texts || new_name.is_some();
    let height = if with_texts {
        ROW_HEIGHT_TEXTS
    } else if tall {
        ROW_HEIGHT_NAME
    } else {
        ROW_HEIGHT
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    // Kopfbereich (Vorschaubild, Name, Ergebnis) liegt immer im oberen Teil der Zeile.
    let head_y = rect.top() + ROW_HEIGHT / 2.0;
    let response = response.on_hover_text(item.path.display().to_string());
    let painter = ui.painter_at(rect);

    let fill = if selected {
        theme::ACCENT_SOFT
    } else if response.hovered() {
        theme::ROW_HOVER
    } else {
        theme::SURFACE
    };
    painter.rect_filled(rect, egui::CornerRadius::same(10), fill);
    if selected {
        let bar = egui::Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 10.0), egui::vec2(4.0, rect.height() - 20.0));
        painter.rect_filled(bar, egui::CornerRadius::same(2), theme::ACCENT);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // Vorschaubild
    let thumb_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 14.0, head_y - THUMB_POINTS / 2.0),
        egui::vec2(THUMB_POINTS, THUMB_POINTS),
    );
    match thumb {
        Some(texture) => {
            egui::Image::new(texture).corner_radius(6).paint_at(ui, thumb_rect);
        }
        None => {
            painter.rect_filled(thumb_rect, egui::CornerRadius::same(6), egui::Color32::from_rgb(0x2c, 0x2c, 0x34));
        }
    }

    // Name und Angaben zur Quelle
    let text_left = thumb_rect.right() + 14.0;
    let remove_width = 44.0;
    let result_width = (width * 0.36).clamp(200.0, 360.0);
    let name_right = rect.right() - remove_width - result_width - 12.0;
    let name_clip = egui::Rect::from_x_y_ranges(text_left..=name_right, rect.y_range());
    let name_painter = ui.painter_at(name_clip);
    let name = item.path.file_name().unwrap_or_default().to_string_lossy();
    name_painter.text(
        egui::pos2(text_left, head_y - 10.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(15.0),
        theme::TEXT,
    );
    let (detail, detail_color) = match &item.probe {
        Ok(p) => (format!("{}×{} · {}", p.width, p.height, format_bytes(p.bytes)), theme::TEXT_WEAK),
        Err(_) => (t("ui.unreadable").to_string(), ui.visuals().error_fg_color),
    };
    let detail_rect = name_painter.text(
        egui::pos2(text_left, head_y + 11.0),
        egui::Align2::LEFT_CENTER,
        detail,
        egui::FontId::proportional(12.5),
        detail_color,
    );
    if item.own.is_some() {
        name_painter.text(
            egui::pos2(detail_rect.right() + 10.0, head_y + 11.0),
            egui::Align2::LEFT_CENTER,
            t("ui.own_values_tag"),
            egui::FontId::proportional(12.5),
            theme::ACCENT,
        );
    }

    // Ergebnis
    let result_left = rect.right() - remove_width - result_width;
    let result_clip = egui::Rect::from_x_y_ranges(result_left..=rect.right() - remove_width, rect.y_range());
    let result_painter = ui.painter_at(result_clip);
    let line = |dy: f32, text: String, size: f32, color: egui::Color32| {
        result_painter.text(
            egui::pos2(result_left, head_y + dy),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(size),
            color,
        );
    };
    let error = ui.visuals().error_fg_color;
    match (&item.probe, &item.status) {
        (Err(e), _) | (_, Status::Failed(e)) => line(0.0, e.clone(), 13.0, error),
        (Ok(_), Status::Pending) => line(0.0, t("ui.ready").into(), 13.0, theme::TEXT_WEAK),
        (Ok(_), Status::Running) => {
            let spinner = egui::Rect::from_center_size(egui::pos2(result_left + 9.0, head_y), egui::vec2(18.0, 18.0));
            ui.put(spinner, egui::Spinner::new().size(16.0));
            result_painter.text(
                egui::pos2(result_left + 26.0, head_y),
                egui::Align2::LEFT_CENTER,
                t("ui.working"),
                egui::FontId::proportional(13.0),
                theme::TEXT_WEAK,
            );
        }
        (Ok(p), Status::Done(o)) => {
            if o.path.is_some() {
                line(-10.0, format!("{}×{} · {}", o.width, o.height, format_bytes(o.bytes)), 14.5, theme::TEXT);
                let mut second = format_saving(p.bytes, o.bytes);
                if let Some(m) = &o.message {
                    second += &format!(" · {m}");
                }
                line(11.0, second, 12.5, theme::GOOD);
            } else {
                line(0.0, o.message.clone().unwrap_or_default(), 13.0, theme::TEXT_WEAK);
            }
        }
    }

    // Texte der KI bzw. ihr Zustand
    if tall {
        let line_left = text_left;
        let line_clip = egui::Rect::from_x_y_ranges(line_left..=rect.right() - remove_width, rect.y_range());
        let p = ui.painter_at(line_clip);
        let y1 = head_y + 36.0;
        let y2 = head_y + 56.0;
        match &item.ai {
            AiStatus::Waiting => {
                p.text(egui::pos2(line_left, y1), egui::Align2::LEFT_CENTER, t("ui.ai_waiting"), egui::FontId::proportional(13.0), theme::TEXT_WEAK);
            }
            AiStatus::Running => {
                let spinner = egui::Rect::from_center_size(egui::pos2(line_left + 8.0, y1), egui::vec2(16.0, 16.0));
                ui.put(spinner, egui::Spinner::new().size(14.0));
                p.text(egui::pos2(line_left + 24.0, y1), egui::Align2::LEFT_CENTER, t("ui.ai_running"), egui::FontId::proportional(13.0), theme::TEXT_WEAK);
            }
            AiStatus::Failed(e) => {
                p.text(egui::pos2(line_left, y1), egui::Align2::LEFT_CENTER, e, egui::FontId::proportional(13.0), ui.visuals().error_fg_color);
            }
            AiStatus::Idle => {}
        }
        if !matches!(item.ai, AiStatus::Waiting | AiStatus::Running | AiStatus::Failed(_)) {
            if new_name.is_some() || item.texts.is_some() {
                let name = new_name.map(|n| tf("ui.new_name", &[("name", &n)])).unwrap_or_else(|| t("ui.name_kept").to_string());
                p.text(egui::pos2(line_left, y1), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(13.5), theme::ACCENT);
            }
            if let Some(texts) = &item.texts {
                let alt = if texts.alt.is_empty() { t("ui.no_alt").to_string() } else { tf("ui.quoted", &[("text", &texts.alt)]) };
                p.text(egui::pos2(line_left, y2), egui::Align2::LEFT_CENTER, alt, egui::FontId::proportional(12.5), theme::TEXT_WEAK);
            }
        }
    }

    // Entfernen
    let remove_rect = egui::Rect::from_center_size(egui::pos2(rect.right() - remove_width / 2.0, head_y), egui::vec2(30.0, 30.0));
    let remove = ui
        .add_enabled_ui(enabled, |ui| ui.put(remove_rect, egui::Button::new(egui::RichText::new("×").size(18.0)).frame(false)))
        .inner
        .on_hover_text(t("ui.remove"));
    (response.clicked() && !remove.hovered(), remove.clicked())
}
