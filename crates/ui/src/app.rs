//! Application shell: screens (welcome → scanning → viewer), opening media,
//! drag & drop, global shortcuts, dialogs, notifications and persistence.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Frame, Id, Key, KeyboardShortcut, Margin, Modifiers, RichText, Ui, vec2};
use summrise_core::dicom::{DicomError, StudyIndex};
use summrise_core::{Appearance, Job, MediaWatcher, RecentStudies, Session, Settings, Waker, recent_label};

use crate::theme;
use crate::viewer::Viewer;
use crate::welcome;
use crate::widgets::{card, paint_logo};

const SETTINGS_KEY: &str = "settings";
const RECENT_KEY: &str = "recent";

/// Start-up options from the command line / OS.
#[derive(Debug, Default, Clone)]
pub struct AppOptions {
    /// Folder, DICOMDIR or file to open immediately.
    pub open: Option<PathBuf>,
}

/// User intents raised by screens and handled centrally.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    OpenDialog,
    Open(PathBuf),
    RemoveRecent(PathBuf),
    ClearRecent,
    CloseStudy,
    ShowSettings,
    ShowAbout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Default)]
pub(crate) struct Toasts {
    items: Vec<(Level, String, Instant)>,
}

impl Toasts {
    pub(crate) fn info(&mut self, text: impl Into<String>) {
        self.items.push((Level::Info, text.into(), Instant::now()));
    }
    pub(crate) fn warn(&mut self, text: impl Into<String>) {
        self.items.push((Level::Warn, text.into(), Instant::now()));
    }
    pub(crate) fn error(&mut self, text: impl Into<String>) {
        self.items.push((Level::Error, text.into(), Instant::now()));
    }

    fn show(&mut self, ctx: &egui::Context) {
        const TTL: Duration = Duration::from_secs(7);
        self.items.retain(|(_, _, t)| t.elapsed() < TTL);
        if self.items.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        egui::Area::new(Id::unique("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -36.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let p = theme::palette(ui);
                let mut dismiss = None;
                for (i, (level, text, _)) in self.items.iter().enumerate() {
                    let color = match level {
                        Level::Info => p.accent,
                        Level::Warn => p.warn,
                        Level::Error => p.error,
                    };
                    let r = Frame::new()
                        .fill(p.surface)
                        .stroke(egui::Stroke::new(1.0, color))
                        .corner_radius(8)
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.add(egui::Label::new(RichText::new(text).color(p.text)).wrap());
                        });
                    if r.response.interact(egui::Sense::click()).clicked() {
                        dismiss = Some(i);
                    }
                    ui.add_space(6.0);
                }
                if let Some(i) = dismiss {
                    self.items.remove(i);
                }
            });
    }
}

enum Screen {
    Welcome,
    Scanning { path: PathBuf, job: Job<Result<StudyIndex, DicomError>> },
    Viewer(Box<Viewer>),
}

pub struct SumMRIseApp {
    settings: Settings,
    recent: RecentStudies,
    screen: Screen,
    media: MediaWatcher,
    waker: Waker,
    toasts: Toasts,
    pending: Vec<Action>,
    show_settings: bool,
    show_about: bool,
    applied_appearance: Option<Appearance>,
    title: String,
}

impl SumMRIseApp {
    pub fn new(cc: &eframe::CreationContext<'_>, options: AppOptions) -> Self {
        theme::install(&cc.egui_ctx);
        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, SETTINGS_KEY)).unwrap_or_default();
        let recent: RecentStudies = cc.storage.and_then(|s| eframe::get_value(s, RECENT_KEY)).unwrap_or_default();
        let ctx = cc.egui_ctx.clone();
        let waker: Waker = Arc::new(move || ctx.request_repaint());
        let mut pending = Vec::new();
        if let Some(path) = options.open {
            pending.push(Action::Open(path));
        }
        Self {
            media: MediaWatcher::spawn(Some(waker.clone())),
            settings,
            recent,
            screen: Screen::Welcome,
            waker,
            toasts: Toasts::default(),
            pending,
            show_settings: false,
            show_about: false,
            applied_appearance: None,
            title: String::new(),
        }
    }

    fn apply_appearance(&mut self, ctx: &egui::Context) {
        if self.applied_appearance == Some(self.settings.appearance) {
            return;
        }
        ctx.set_theme(match self.settings.appearance {
            Appearance::System => egui::ThemePreference::System,
            Appearance::Dark => egui::ThemePreference::Dark,
            Appearance::Light => egui::ThemePreference::Light,
        });
        self.applied_appearance = Some(self.settings.appearance);
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::OpenDialog => {
                if let Some(path) =
                    rfd::FileDialog::new().set_title("Open a folder or disc with DICOM images").pick_folder()
                {
                    self.start_scan(path);
                }
            }
            Action::Open(path) => self.start_scan(path),
            Action::RemoveRecent(path) => self.recent.remove(&path),
            Action::ClearRecent => self.recent.clear(),
            Action::CloseStudy => self.screen = Screen::Welcome,
            Action::ShowSettings => self.show_settings = true,
            Action::ShowAbout => self.show_about = true,
        }
    }

    fn start_scan(&mut self, path: PathBuf) {
        if let Screen::Scanning { job, .. } = &self.screen {
            job.cancel();
        }
        log::info!("opening {}", path.display());
        let p = path.clone();
        let job =
            Job::spawn("scan", Some(self.waker.clone()), move |progress| summrise_core::dicom::scan(&p, progress));
        self.screen = Screen::Scanning { path, job };
    }

    fn poll_scan(&mut self) {
        let Screen::Scanning { path, job } = &mut self.screen else {
            return;
        };
        let Some(result) = job.poll() else { return };
        let path = path.clone();
        match result {
            Ok(index) => {
                if !index.warnings.is_empty() {
                    log::info!("index warnings: {:?}", index.warnings);
                }
                if self.settings.remember_recent {
                    self.recent.add(&path, recent_label(&index));
                }
                let studies = index.studies.len();
                let session = Session::new(index, self.settings.memory_budget_bytes(), Some(self.waker.clone()));
                if studies > 1 {
                    self.toasts.info(format!("{studies} studies found — switch between them in the browser."));
                }
                self.screen = Screen::Viewer(Box::new(Viewer::new(session, &self.settings)));
            }
            Err(DicomError::Cancelled) => self.screen = Screen::Welcome,
            Err(e) => {
                self.toasts.error(e.to_string());
                self.screen = Screen::Welcome;
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if let Some(path) = dropped.into_iter().next() {
            self.pending.push(Action::Open(path));
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let open = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
        let settings = KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma);
        let inspector = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
        let browser = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
        if ctx.input_mut(|i| i.consume_shortcut(&open)) {
            self.pending.push(Action::OpenDialog);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&settings)) {
            self.show_settings = true;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&inspector)) {
            self.settings.show_inspector = !self.settings.show_inspector;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&browser)) {
            self.settings.show_browser = !self.settings.show_browser;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F1)) {
            self.show_about = true;
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match &self.screen {
            Screen::Viewer(v) => format!("SumMRIse — {}", v.title()),
            _ => "SumMRIse".to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn scanning_ui(ui: &mut Ui, path: &std::path::Path, job: &Job<Result<StudyIndex, DicomError>>) -> bool {
        let p = theme::palette(ui);
        let mut cancel = false;
        ui.ctx().request_repaint_after(Duration::from_millis(80));
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.28);
            let (logo, _) = ui.allocate_exact_size(vec2(56.0, 56.0), egui::Sense::hover());
            paint_logo(ui.painter(), logo);
            ui.add_space(12.0);
            card(ui, |ui| {
                ui.set_width(420.0);
                let snap = job.progress();
                ui.label(RichText::new(if snap.stage.is_empty() { "Preparing…" } else { &snap.stage }).strong());
                ui.add(
                    egui::Label::new(RichText::new(path.display().to_string()).small().color(p.text_weak)).truncate(),
                );
                ui.add_space(6.0);
                match snap.fraction() {
                    Some(f) => ui.add(egui::ProgressBar::new(f).text(format!("{} / {}", snap.done, snap.total))),
                    None => {
                        ui.add(egui::ProgressBar::new(0.0).animate(true).text(format!("{} files found", snap.done)))
                    }
                };
                ui.add_space(6.0);
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        cancel
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let s = &mut self.settings;
                egui::Grid::new("settings-grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                    ui.label("Appearance");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut s.appearance, Appearance::System, "System");
                        ui.selectable_value(&mut s.appearance, Appearance::Dark, "Dark");
                        ui.selectable_value(&mut s.appearance, Appearance::Light, "Light");
                    });
                    ui.end_row();
                    ui.label("Annotations");
                    ui.checkbox(&mut s.show_overlays, "Show patient/study text, orientation and scale");
                    ui.end_row();
                    ui.label("Privacy");
                    ui.checkbox(&mut s.hide_patient_details, "Hide patient name, ID and birth date");
                    ui.end_row();
                    ui.label("Display");
                    ui.checkbox(&mut s.smooth_interpolation, "Smooth (bilinear) magnification");
                    ui.end_row();
                    ui.label("Recent studies");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut s.remember_recent, "Remember opened folders");
                        ui.label(
                            RichText::new("Stores the folder path, study description and date only.").small().weak(),
                        );
                    });
                    ui.end_row();
                    ui.label("Memory limit");
                    ui.add(egui::Slider::new(&mut s.memory_budget_mb, 512..=32_768).logarithmic(true).suffix(" MB"));
                    ui.end_row();
                });
                if !self.settings.remember_recent {
                    self.recent.clear();
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("About SumMRIse").clicked() {
                        self.show_about = true;
                    }
                    if ui.button("Restore defaults").clicked() {
                        self.settings = Settings::default();
                    }
                });
            });
        self.show_settings = open;
        if let Screen::Viewer(v) = &mut self.screen {
            v.session.set_budget(self.settings.memory_budget_bytes());
        }
    }

    fn about_modal(&mut self, ctx: &egui::Context) {
        let response = egui::Modal::new(Id::unique("about")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.vertical_centered(|ui| {
                let (logo, _) = ui.allocate_exact_size(vec2(64.0, 64.0), egui::Sense::hover());
                paint_logo(ui.painter(), logo);
                ui.heading("SumMRIse");
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).weak());
            });
            ui.add_space(8.0);
            ui.label(
                "A viewer for MRI and CT studies from patient CDs/DVDs. Reads DICOM media (DICOMDIR), \
                 shows every series with correct orientation, contrast presets, multiplanar \
                 reformatting and full metadata.",
            );
            ui.add_space(6.0);
            ui.label(RichText::new("Not a medical device. Not for diagnostic use.").color(theme::palette(ui).warn));
            ui.add_space(6.0);
            ui.label(
                RichText::new("Built with dicom-rs, egui/eframe and wgpu. Licensed under the MIT licence.")
                    .small()
                    .weak(),
            );
            ui.add_space(8.0);
            ui.vertical_centered(|ui| ui.button("Close").clicked()).inner
        });
        if response.inner || response.should_close() {
            self.show_about = false;
        }
    }

    fn drop_overlay(ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::unique("drop")));
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(170));
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Drop to open",
            egui::FontId::proportional(24.0),
            Color32::WHITE,
        );
    }
}

impl eframe::App for SumMRIseApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_appearance(&ctx);
        self.handle_dropped_files(&ctx);
        self.handle_shortcuts(&ctx);
        for action in std::mem::take(&mut self.pending) {
            self.handle_action(action);
        }
        self.poll_scan();

        let mut actions = Vec::new();
        let panel_fill = theme::palette(ui).bg;
        match &mut self.screen {
            Screen::Welcome => {
                let media = self.media.media().to_vec();
                egui::CentralPanel::default_margins()
                    .frame(Frame::new().fill(panel_fill).inner_margin(Margin::same(24)))
                    .show(ui, |ui| welcome::show(ui, &self.recent, &media, &mut actions));
            }
            Screen::Scanning { path, job } => {
                let cancel = egui::CentralPanel::default_margins()
                    .frame(Frame::new().fill(panel_fill))
                    .show(ui, |ui| Self::scanning_ui(ui, path, job))
                    .inner;
                if cancel {
                    job.cancel();
                }
            }
            Screen::Viewer(viewer) => viewer.show(ui, &mut self.settings, &self.waker, &mut actions, &mut self.toasts),
        }
        self.pending.extend(actions);
        if !self.pending.is_empty() {
            ctx.request_repaint();
        }
        if self.show_settings {
            self.settings_window(&ctx);
        }
        if self.show_about {
            self.about_modal(&ctx);
        }
        self.toasts.show(&ctx);
        Self::drop_overlay(&ctx);
        self.update_title(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
        eframe::set_value(storage, RECENT_KEY, &self.recent);
    }

    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(30)
    }
}
