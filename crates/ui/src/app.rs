//! Application shell: screens (welcome → scanning → viewer), opening media,
//! drag & drop, global shortcuts, dialogs, notifications and persistence.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Frame, Id, Key, KeyboardShortcut, Margin, Modifiers, RichText, Ui, vec2};
use summrise_core::dicom::{DicomError, StudyIndex};
use summrise_core::{Appearance, Job, MediaWatcher, RecentStudies, Session, Settings, Waker, recent_label};
use summrise_i18n::{tr, tr_ctx, tr_n};

use crate::crash_notice::{CrashNotice, When};
use crate::display_check::{self, DisplayCheckScreen};
use crate::language;
use crate::licences;
use crate::theme;
use crate::viewer::Viewer;
use crate::welcome;
use crate::widgets::{card, paint_logo};

const SETTINGS_KEY: &str = "settings";
const RECENT_KEY: &str = "recent";

/// Start-up options from the command line / OS.
#[derive(Debug, Default, Clone)]
pub struct AppOptions {
    /// Folder, DICOMDIR, DICOM file, ZIP archive or disc image to open immediately.
    pub open: Option<PathBuf>,
    /// The languages the operating system prefers, most preferred first
    /// ([`crate::system_languages`]): "System language" stands for the first
    /// one the interface has. Empty (the default, and in tests): English.
    pub system_languages: Vec<String>,
    /// The crash report of a fault that ended the previous run, which the user
    /// has not been told about (`summrise_core::crash::report_of_last_run`).
    pub crash_report: Option<PathBuf>,
}

/// User intents raised by screens and handled centrally.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    OpenDialog,
    /// Choose a ZIP archive or a disc image to open.
    OpenArchiveDialog,
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

    /// How many notifications are waiting to be shown.
    #[cfg(test)]
    pub(crate) fn count_for_test(&self) -> usize {
        self.items.len()
    }

    /// What is being said.
    #[cfg(test)]
    pub(crate) fn texts(&self) -> Vec<&str> {
        self.items.iter().map(|(_, text, _)| text.as_str()).collect()
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
    /// The licences of the third-party software, opened from the About dialog.
    show_licences: bool,
    /// The display check, while it is open.
    display_check: Option<DisplayCheckScreen>,
    /// A crash report the user is being told about.
    crash_notice: Option<CrashNotice>,
    applied_appearance: Option<Appearance>,
    title: String,
    /// See [`AppOptions::system_languages`].
    system_languages: Vec<String>,
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
            show_licences: false,
            display_check: None,
            crash_notice: options.crash_report.map(|report| CrashNotice { when: When::LastRun, report }),
            applied_appearance: None,
            title: String::new(),
            system_languages: options.system_languages,
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
                    rfd::FileDialog::new().set_title(tr!("Open a folder or disc with DICOM images")).pick_folder()
                {
                    self.start_scan(path);
                }
            }
            Action::OpenArchiveDialog => {
                // The content decides what a file is; the filter only helps to find it.
                if let Some(path) = rfd::FileDialog::new()
                    .set_title(tr!("Open a ZIP archive or a disc image"))
                    .add_filter(tr!("ZIP archives and disc images"), &["zip", "iso", "ZIP", "ISO"])
                    .pick_file()
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
                    self.toasts.info(tr_n!(
                        "{n} study found — switch between them in the browser.",
                        "{n} studies found — switch between them in the browser.",
                        studies
                    ));
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
        let settings = KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma);
        let inspector = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
        let browser = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
        self.pending.extend(open_shortcut(ctx));
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
                ui.label(RichText::new(if snap.stage.is_empty() { tr!("Preparing…") } else { &snap.stage }).strong());
                ui.add(
                    egui::Label::new(RichText::new(path.display().to_string()).small().color(p.text_weak)).truncate(),
                );
                ui.add_space(6.0);
                match snap.fraction() {
                    Some(f) => ui.add(egui::ProgressBar::new(f).text(format!("{} / {}", snap.done, snap.total))),
                    None => ui.add(egui::ProgressBar::new(0.0).animate(true).text(tr_n!(
                        "{n} file found",
                        "{n} files found",
                        snap.done
                    ))),
                };
                ui.add_space(6.0);
                if ui.button(tr!("Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        cancel
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        let mut check_display = false;
        // The id stays the same in every language.
        egui::Window::new(tr!("Settings"))
            .id(Id::unique("settings window"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let s = &mut self.settings;
                egui::Grid::new("settings-grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                    let label = ui.label(tr!("Language"));
                    language::choice(ui, &mut s.language, &self.system_languages)
                        .labelled_by(label.id)
                        .on_hover_text(tr!("The language of the interface; it changes at once"));
                    ui.end_row();
                    ui.label(tr!("Appearance"));
                    ui.horizontal(|ui| {
                        // Translators: the appearance of the interface: as the system has it, dark or light.
                        ui.selectable_value(&mut s.appearance, Appearance::System, tr_ctx!("appearance", "System"));
                        ui.selectable_value(&mut s.appearance, Appearance::Dark, tr_ctx!("appearance", "Dark"));
                        ui.selectable_value(&mut s.appearance, Appearance::Light, tr_ctx!("appearance", "Light"));
                    });
                    ui.end_row();
                    ui.label(tr!("Annotations"));
                    ui.checkbox(&mut s.show_overlays, tr!("Show patient/study text, orientation and scale"));
                    ui.end_row();
                    ui.label(tr!("Privacy"));
                    ui.checkbox(&mut s.hide_patient_details, tr!("Hide patient name, ID and birth date"));
                    ui.end_row();
                    // Translators: how the images are drawn on the screen.
                    ui.label(tr!("Display"));
                    ui.checkbox(&mut s.smooth_interpolation, tr!("Smooth (bilinear) magnification"));
                    ui.end_row();
                    ui.label(summrise_i18n::tr!("Explanations"));
                    ui.checkbox(
                        &mut s.show_explanations,
                        summrise_i18n::tr!("Show explanations of series, values and the viewing position"),
                    )
                    .on_hover_text(summrise_i18n::tr!(
                        "Shows what each series is, what the values in the image information mean and where the image on screen lies in the body."
                    ));
                    ui.end_row();
                    ui.label(tr!("Recent studies"));
                    ui.vertical(|ui| {
                        ui.checkbox(&mut s.remember_recent, summrise_i18n::tr!("Remember opened folders and archives"));
                        ui.label(
                            RichText::new(summrise_i18n::tr!(
                                "Stores the path of the folder or archive, the study description and date only."
                            ))
                            .small()
                            .weak(),
                        );
                    });
                    ui.end_row();
                    let memory = ui.label(tr!("Memory limit"));
                    // The slider and its number field are both named by the row label.
                    ui.add(egui::Slider::new(&mut s.memory_budget_mb, 512..=32_768).logarithmic(true).suffix(" MB"))
                        .labelled_by(memory.id);
                    ui.end_row();
                });
                if !self.settings.remember_recent {
                    self.recent.clear();
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr!("About SumMRIse")).clicked() {
                        self.show_about = true;
                    }
                    if ui.button(tr!("Restore defaults")).clicked() {
                        self.settings = Settings::default();
                    }
                    check_display = display_check::open_button(ui).clicked();
                });
            });
        if check_display {
            open = false;
            self.display_check = Some(DisplayCheckScreen::default());
        }
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
                ui.label(RichText::new(tr!("Version {version}", version = env!("CARGO_PKG_VERSION"))).weak());
            });
            ui.add_space(8.0);
            ui.label(tr!("A viewer for MRI and CT studies from patient CDs/DVDs. Reads DICOM media (DICOMDIR), \
                 shows every series with correct orientation, contrast presets, multiplanar \
                 reformatting and full metadata."));
            ui.add_space(6.0);
            // Translators: keep this statement unambiguous and at least as strong as the English.
            let disclaimer = tr!("Not a medical device. Not for diagnostic use.");
            ui.label(RichText::new(disclaimer).color(theme::palette(ui).warn));
            ui.add_space(6.0);
            ui.label(
                RichText::new(tr!(
                    "Licensed under the MIT licence. Built with dicom-rs, egui/eframe, wgpu and other software \
                     by others, under their own licences."
                ))
                .small()
                .weak(),
            );
            ui.add_space(8.0);
            let (check, licences) = ui
                .vertical_centered(|ui| {
                    let check = display_check::open_button(ui).clicked();
                    (check, ui.button(tr!("Third-party licences…")).clicked())
                })
                .inner;
            ui.add_space(4.0);
            (ui.vertical_centered(|ui| ui.button(tr!("Close")).clicked()).inner, check, licences)
        });
        let (close, check, licences) = response.inner;
        if check {
            self.display_check = Some(DisplayCheckScreen::default());
        }
        // In place of the About dialog: one modal at a time.
        self.show_licences |= licences;
        if close || check || licences || response.should_close() {
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
            tr!("Drop to open"),
            egui::FontId::proportional(24.0),
            Color32::WHITE,
        );
    }
}

/// The dialog a keyboard shortcut asks for: the folder dialog with Command+O
/// (Ctrl+O), the dialog for archives and disc images with Shift added.
fn open_shortcut(ctx: &egui::Context) -> Option<Action> {
    let folder = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
    let archive = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::O);
    // The longer shortcut first: with Shift held, the shorter one matches as well.
    if ctx.input_mut(|i| i.consume_shortcut(&archive)) {
        Some(Action::OpenArchiveDialog)
    } else if ctx.input_mut(|i| i.consume_shortcut(&folder)) {
        Some(Action::OpenDialog)
    } else {
        None
    }
}

impl eframe::App for SumMRIseApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        language::apply(&ctx, &self.settings, &self.system_languages);
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
        if self.show_licences {
            self.show_licences = licences::show(&ctx);
        }
        if let Some(check) = &mut self.display_check
            && !check.show(&ctx)
        {
            self.display_check = None;
        }
        // A fault that stopped part of the program while the window lives on.
        if self.crash_notice.is_none()
            && let Some(report) = summrise_core::crash::take_new_report()
        {
            self.crash_notice = Some(CrashNotice { when: When::ThisRun, report });
        }
        if let Some(notice) = &self.crash_notice
            && !notice.show(&ctx)
        {
            self.crash_notice = None;
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

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;

    use super::*;

    /// The shortcut for archives is the one for folders with Shift: each asks
    /// for its own dialog, and never for both.
    #[test]
    fn open_shortcuts_ask_for_one_dialog_each() {
        let mut harness =
            Harness::new_ui_state(|ui, asked: &mut Vec<Action>| asked.extend(open_shortcut(ui.ctx())), Vec::new());
        let mut asked_after = |modifiers: Modifiers| {
            harness.state_mut().clear();
            harness.key_press_modifiers(modifiers, Key::O);
            harness.step();
            harness.state().clone()
        };
        assert_eq!(asked_after(Modifiers::COMMAND | Modifiers::SHIFT), [Action::OpenArchiveDialog]);
        assert_eq!(asked_after(Modifiers::COMMAND), [Action::OpenDialog]);
        assert_eq!(asked_after(Modifiers::NONE), []);
        assert_eq!(asked_after(Modifiers::SHIFT), []);
    }
}
