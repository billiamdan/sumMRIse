//! The documents of a disc in the viewer, driven offscreen: the list in the
//! browser, the document view for each kind of document, links to images, privacy
//! mode, what cannot be shown, and the keyboard. The data is the synthetic CD
//! with reports of the testkit. Set `SUMMRISE_SNAPSHOT_DIR` to keep renderings.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::{Key, Modifiers, Theme};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use summrise_core::dicom::{Progress, scan};
use summrise_core::workspace::Grid;
use summrise_core::{Layout, SeriesKey, Session, Settings, ViewKind};
use summrise_testkit::archive::zip_folder;
use summrise_testkit::reports::{self, deeply_nested_report, write_reports_cd, write_structured_report};
use summrise_testkit::{PhantomOptions, write_phantom_study};

use super::Viewer;
use crate::app::{Action, Toasts};
use crate::compare::Command;
use crate::theme;

const CT: SeriesKey = SeriesKey { study: 0, series: 0 };
const MR: SeriesKey = SeriesKey { study: 0, series: 1 };
const MR_SERIES: &str = "T2 TSE SAG (synthetic)";

struct App {
    viewer: Viewer,
    settings: Settings,
    waker: summrise_core::Waker,
    actions: Vec<Action>,
    toasts: Toasts,
}

struct Fixture {
    harness: Harness<'static, App>,
    dir: tempfile::TempDir,
}

fn snapshot_dir() -> Option<PathBuf> {
    std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from)
}

/// The viewer on what is at `open` below `dir`.
fn start(dir: tempfile::TempDir, open: PathBuf, settings: Settings, size: [f32; 2], theme: Theme) -> Fixture {
    let index = scan(&open, &Progress::new()).expect("scan");
    let session = Session::new(index, settings.memory_budget_bytes(), None);
    let app = App {
        viewer: Viewer::new(session, &settings),
        settings,
        waker: Arc::new(|| {}),
        actions: Vec::new(),
        toasts: Toasts::default(),
    };
    let mut builder = Harness::builder().with_size(size).with_theme(theme).with_context_setup(theme::install);
    if snapshot_dir().is_some() {
        builder = builder.wgpu();
    }
    let harness = builder.build_ui_state(
        |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
        app,
    );
    let mut fixture = Fixture { harness, dir };
    fixture.harness.state_mut().viewer.documents.reveal = record_reveal;
    fixture.harness.step();
    fixture
}

/// The viewer on the synthetic CD with reports, with the CT on screen.
fn cd(settings: Settings) -> Fixture {
    cd_sized(settings, [1280.0, 800.0], Theme::Dark)
}

fn cd_sized(settings: Settings, size: [f32; 2], theme: Theme) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions::small()).expect("fixture");
    let open = dir.path().to_path_buf();
    let mut f = start(dir, open, settings, size, theme);
    f.run_until("the CT", |f| f.viewer().session.data(CT).is_some_and(|series| series.is_viewable()));
    f.harness.step();
    f
}

/// Which files "Show in folder" asked for, in any test of this module.
static REVEALED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn record_reveal(path: &Path) -> std::io::Result<()> {
    REVEALED.lock().expect("list").push(path.to_path_buf());
    Ok(())
}

impl Fixture {
    fn run_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < Duration::from_secs(60), "timed out waiting for {what}");
            self.harness.step();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn viewer(&self) -> &Viewer {
        &self.harness.state().viewer
    }

    fn shows(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }

    fn shows_part(&self, label: &str) -> bool {
        self.harness.query_all_by_label_contains(label).next().is_some()
    }

    fn top_of(&self, label: &str) -> f32 {
        self.harness.get_by_label(label).rect().top()
    }

    fn steps(&mut self, n: usize) {
        self.harness.run_steps(n);
    }

    fn press(&mut self, key: Key) {
        self.harness.key_press(key);
        self.steps(3);
    }

    /// Clicks what is labelled `label`, after scrolling it into view, and runs a few frames.
    fn click(&mut self, label: &str) {
        self.harness.get_by_label(label).scroll_to_me();
        self.steps(3);
        self.harness.get_by_label(label).click();
        self.steps(3);
    }

    /// Opens a document from the list and waits until it is read.
    fn open(&mut self, title: &str) {
        self.click(title);
        // The state of the pane, not the absence of "Loading the document…": that label
        // can be missing for a frame while the document is still being read.
        self.run_until("the document", |f| f.viewer().documents.is_read());
        self.steps(2);
    }

    fn page_scale(&self, page: usize) -> Option<f32> {
        self.viewer().documents.picture_scale(page)
    }

    fn snapshot(&mut self, name: &str) {
        if let Some(dir) = snapshot_dir() {
            let image = self.harness.render().expect("wgpu render");
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            image.save(dir.join(format!("{name}.png"))).expect("save png");
        }
    }
}

/// Scenarios "Structured report on the disc" and "Loose PDF next to the images", and viewer files.
#[test]
fn documents_are_listed_in_the_browser_with_type_title_and_date() {
    let mut f = cd(Settings::default());
    assert!(f.shows("DOCUMENTS"));
    assert!(f.shows("MR Report"));
    assert!(f.shows("Report · 2026-01-15"));
    assert!(f.shows("X-Ray Radiation Dose Report") && f.shows("Dose report · 2026-01-15"));
    assert!(f.shows("Radiology report (PDF)") && f.shows("PDF document · 2026-01-15"));
    assert!(f.shows("Other files on the disc"));
    assert!(f.shows("REPORT.PDF") && f.shows("PDF file"));
    assert!(f.shows("report_001.rtf") && f.shows("RTF file"));
    assert!(f.shows("report_002.htm") && f.shows("HTML file"));
    // Viewer software on the disc.
    for hidden in ["manual.pdf", "readme.txt", "viewer.exe", "AUTORUN.INF"] {
        assert!(!f.shows(hidden), "{hidden}");
    }
    // Every row has a name for a screen reader, with what it shows.
    assert!(f.shows("MR Report, Report · 2026-01-15"));
    assert!(f.shows("REPORT.PDF, PDF file"));
    f.snapshot("documents-1-list");
}

/// Scenario "No documents".
#[test]
fn no_documents_no_section() {
    let dir = tempfile::tempdir().expect("tempdir");
    let opts = PhantomOptions { include_clutter: false, ..PhantomOptions::small() };
    write_phantom_study(dir.path(), &opts).expect("fixture");
    let open = dir.path().to_path_buf();
    let f = start(dir, open, Settings::default(), [1280.0, 800.0], Theme::Dark);
    assert!(!f.shows("DOCUMENTS"));
    assert!(!f.shows("Other files on the disc"));
}

/// Scenarios "Findings and impression", "Reading a report" and D7.
#[test]
fn a_report_takes_the_place_of_the_images_and_reads_in_order() {
    let mut f = cd(Settings::default());
    f.open("MR Report");
    assert!(f.viewer().documents.is_open());
    // The title in the list and in the header.
    assert_eq!(f.harness.query_all_by_label("MR Report").count(), 2);
    for shown in ["Findings", "No focal lesion.", "Impression", "Normal study.", "Final — verified", "Back to images"]
    {
        assert!(f.shows(shown), "{shown}");
    }
    let tops: Vec<f32> = ["Findings", "No focal lesion.", "Impression", "Normal study."].map(|t| f.top_of(t)).to_vec();
    assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "in document order: {tops:?}");
    // The images are covered: their annotation is gone and their controls are switched off.
    assert!(!f.shows("Image 13 / 24"));
    assert!(f.harness.get_by_label("MPR").accesskit_node().is_disabled());
    f.snapshot("documents-2-report");
    // Back to the images, where they were.
    f.click("Back to images");
    assert!(!f.viewer().documents.is_open());
    assert!(f.shows("Image 13 / 24"));
    assert!(!f.harness.get_by_label("MPR").accesskit_node().is_disabled());
    assert_eq!(f.viewer().session.active_series, Some(CT));
}

/// Scenarios "Numeric item with unit" and "Preliminary report".
#[test]
fn a_preliminary_report_says_so_and_shows_its_measurements() {
    let mut f = cd(Settings::default());
    f.open(reports::MEASUREMENTS_TITLE);
    assert!(f.shows("Preliminary — not verified"));
    assert!(f.shows("Diameter: 12 mm"));
    assert!(f.shows("Finding site: Lung"));
    assert!(f.shows("Date of measurement: 2026-01-15"));
    assert!(f.shows("Person observer: READER EXAMPLE"));
    assert!(f.shows("Measured waveform: no value recorded"));
    f.snapshot("documents-3-preliminary");
}

/// Scenario "Referenced image available".
#[test]
fn a_link_returns_to_the_images_with_the_referenced_image_displayed() {
    let mut f = cd(Settings { default_layout: Layout::Single, ..Settings::default() });
    f.open("MR Report");
    let link = format!("Image in {MR_SERIES}");
    assert!(f.harness.query_all(by().label(link.as_str()).role(Role::Link)).next().is_some());
    f.click(&link);
    assert!(!f.viewer().documents.is_open(), "back to the images");
    assert_eq!(f.viewer().session.active_series, Some(MR));
    f.run_until("the image", |f| f.viewer().documents.is_settled() && f.viewer().views.contains_key(&MR));
    f.steps(2);
    let series = f.viewer().session.data(MR).expect("the series");
    let view = f.viewer().views.get(&MR).expect("its view");
    let slot = view.slot_index(&series);
    let instance = series.frames()[slot].instance_number;
    assert_eq!(instance, Some(reports::REFERENCED_MR_INSTANCE as i32));
    // The label on the image names the same image.
    let label = format!("Image {} / {}", slot + 1, series.len());
    f.run_until("the label", |f| f.shows_part(&label));
    assert_eq!(f.viewer().layout, Layout::Single);
    assert_eq!(f.viewer().active_view, ViewKind::Stack);
    f.snapshot("documents-4-link-followed");
}

/// A link from the reformats goes to the image too: the single view shows it.
#[test]
fn a_link_leaves_the_reformats_for_the_single_view() {
    let mut f = cd(Settings::default());
    f.run_until("the CT to load", |f| f.viewer().session.loaded(CT).is_some());
    f.steps(2);
    f.click("MPR");
    assert_eq!(f.viewer().layout, Layout::Mpr);
    f.open("MR Report");
    let link = format!("Image in {MR_SERIES}");
    f.click(&link);
    assert_eq!(f.viewer().layout, Layout::Single);
    assert_eq!(f.viewer().active_view, ViewKind::Stack);
    assert_eq!(f.harness.state().settings.default_layout, Layout::Mpr, "the saved preference is untouched");
}

/// A link from a grid of several series goes to the single view too, and a
/// document opened from the grid covers all its viewports until then.
#[test]
fn a_link_leaves_the_comparison_grid_for_the_single_view() {
    let mut f = cd(Settings::default());
    f.run_until("the CT to load", |f| f.viewer().session.loaded(CT).is_some());
    f.steps(2);
    f.harness.state_mut().viewer.run_command(Command::Grid(Some(Grid::SideBySide)));
    f.steps(2);
    assert!(f.viewer().workspace.is_comparing());
    f.open("MR Report");
    assert!(f.viewer().workspace.is_comparing(), "the grid waits behind the document");
    let link = format!("Image in {MR_SERIES}");
    f.click(&link);
    assert!(!f.viewer().workspace.is_comparing(), "the image is shown alone");
    assert_eq!(f.viewer().layout, Layout::Single);
    assert_eq!(f.viewer().active_view, ViewKind::Stack);
}

/// While a document is shown the commands about images rest: ⌘C belongs to its text.
#[test]
fn saving_and_printing_rest_while_a_document_is_shown() {
    let mut f = cd(Settings::default());
    f.run_until("the CT to load", |f| f.viewer().session.loaded(CT).is_some());
    f.steps(2);
    f.open("MR Report");
    for (modifiers, key) in [(Modifiers::COMMAND, Key::S), (Modifiers::COMMAND, Key::P)] {
        f.harness.key_press_modifiers(modifiers, key);
        f.steps(2);
        assert!(!f.viewer().export.is_open(), "{key:?} opens nothing over a document");
    }
    f.press(Key::Escape);
    f.harness.key_press_modifiers(Modifiers::COMMAND, Key::S);
    f.steps(2);
    assert!(f.viewer().export.is_open(), "back on the images it works");
}

/// Scenario "Referenced image missing".
#[test]
fn a_reference_to_an_image_that_is_missing_is_text_and_no_link() {
    let mut f = cd(Settings::default());
    f.open("MR Report");
    assert!(f.shows("Earlier image"));
    assert!(f.shows("image not on this disc"));
    assert_eq!(f.harness.query_all(by().role(Role::Link)).count(), 1, "only the image that is on the disc");
    f.click("image not on this disc");
    assert!(f.viewer().documents.is_open(), "nothing happens");
    assert_eq!(f.viewer().session.active_series, Some(CT));
}

/// Scenario "CT with two acquisitions" and "SHALL NOT calculate an effective dose".
#[test]
fn a_dose_report_is_a_table_of_recorded_values() {
    let mut f = cd(Settings::default());
    f.open(reports::DOSE_REPORT_TITLE);
    for shown in ["Acquisition", "CTDIvol", "DLP", "Chest", "12.5 mGy", "250 mGy·cm", "Abdomen", "16 mGy", "320 mGy·cm"]
    {
        assert!(f.shows(shown), "{shown}");
    }
    assert!(f.shows("Total DLP 570 mGy·cm (as recorded by the scanner)"));
    assert!(f.shows_part("the scanner recorded"));
    assert!(!f.shows_part("ffective dose"), "no effective dose");
    f.snapshot("documents-5-dose");
    // Everything else the report holds is there on request.
    assert!(!f.shows("CT Dose Length Product Total: 570 mGy·cm"));
    f.click("All values in this report");
    assert!(f.shows("CT Dose Length Product Total: 570 mGy·cm"));
    f.snapshot("documents-5-dose-all-values");
}

/// Scenario "Two-page PDF stored in DICOM".
#[test]
fn a_pdf_is_shown_page_by_page_and_stays_sharp_when_zoomed() {
    let mut f = cd(Settings::default());
    f.open(reports::STORED_PDF_TITLE);
    assert!(f.shows("Page 1 of 2"));
    f.run_until("the first page", |f| f.page_scale(0).is_some());
    assert!(f.harness.query_all_by_label("Page 1").next().is_some(), "the page has a name");
    f.snapshot("documents-6-pdf-page-1");

    // Page Down turns the page, and there is no page after the last.
    f.press(Key::PageDown);
    assert!(f.shows("Page 2 of 2"));
    f.run_until("the second page", |f| f.page_scale(1).is_some());
    f.press(Key::PageDown);
    assert!(f.shows("Page 2 of 2"));
    assert!(f.harness.get_by_label("Next page").accesskit_node().is_disabled());
    f.press(Key::PageUp);
    assert!(f.shows("Page 1 of 2"));

    // 100 % by the buttons, then 200 %: the page is rasterised anew at twice the pixels, and is not scaled up.
    f.run_until("the first page", |f| f.page_scale(0).is_some());
    for _ in 0..12 {
        f.click("Zoom out");
    }
    assert!(f.shows("25 %"));
    for _ in 0..3 {
        f.click("Zoom in");
    }
    assert!(f.shows("100 %"));
    f.run_until("the page at 100 %", |f| f.page_scale(0) == Some(1.0));
    for _ in 0..3 {
        f.click("Zoom in");
    }
    assert!(f.shows("200 %"));
    f.run_until("the page at 200 %", |f| f.page_scale(0) == Some(2.0));
    f.steps(2);
    f.snapshot("documents-6-pdf-200-percent");
    // Wider than the view, and all of it can be reached: its left edge has not gone off the view to the left.
    let page = f.harness.get_by_label("Page 1").rect();
    let view = f.harness.get_by_label("Back to images").rect();
    assert_eq!(page.width(), 1190.0);
    assert!(page.left() >= view.left() - 1.0, "the page begins at {} left of the view at {}", page.left(), view.left());
    f.click("Fit to width");
    f.run_until("the width to fit", |f| f.page_scale(0) != Some(2.0));
    f.press(Key::Escape);
    assert!(!f.viewer().documents.is_open());
}

/// Scenario "PDF that cannot be rendered": says so, keeps running, offers the folder.
#[test]
fn a_damaged_pdf_says_so_and_offers_its_folder() {
    let mut f = cd(Settings::default());
    let path = f.dir.path().join("REPORT.PDF");
    let whole = std::fs::read(&path).expect("read");
    std::fs::write(&path, &whole[..whole.len() / 3]).expect("truncate");
    f.open("REPORT.PDF");
    assert!(f.shows("This document could not be displayed."));
    assert!(f.shows("It is damaged or uses a feature that is not supported."));
    assert!(f.shows("Show in folder"));
    f.snapshot("documents-7-damaged-pdf");
    f.click("Show in folder");
    assert!(REVEALED.lock().expect("list").contains(&path), "the file is shown in its folder");
    assert!(f.viewer().documents.is_open());
    // The application goes on: back to the images, and on to another document.
    f.press(Key::Escape);
    assert!(f.shows("Image 13 / 24"));
    f.open("MR Report");
    assert!(f.shows("Normal study."));
}

/// A document in DICOM has no file of its own to show.
#[test]
fn a_document_in_dicom_offers_no_folder() {
    let mut f = cd(Settings::default());
    f.open(reports::STORED_PDF_TITLE);
    assert!(!f.shows("Show in folder"));
    f.press(Key::Escape);
    f.open("MR Report");
    assert!(!f.shows("Show in folder"));
}

/// The brief: "Show in folder" exists only for a real folder.
#[test]
fn a_file_in_an_archive_offers_no_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let folder = dir.path().join("CD");
    write_reports_cd(&folder, &PhantomOptions::small()).expect("fixture");
    let zip = dir.path().join("study.zip");
    std::fs::write(&zip, zip_folder(&folder, "CD").expect("zip")).expect("write");
    let mut f = start(dir, zip, Settings::default(), [1280.0, 800.0], Theme::Dark);
    f.open("REPORT.PDF");
    f.run_until("the page", |f| f.page_scale(0).is_some());
    assert!(!f.shows("Show in folder"));
    assert!(f.shows("Page 1 of 1"));
}

/// Scenario "HTML report with a script and a remote image".
#[test]
fn html_is_text_without_scripts_or_remote_content() {
    let mut f = cd(Settings::default());
    f.open("report_002.htm");
    assert!(f.shows("Radiology report"));
    assert!(f.shows("The lungs are clear & the heart is normal."));
    assert!(f.shows("See the archive for more."));
    for absent in ["SCRIPT RAN", "example.invalid", "Remote picture", "Hidden title"] {
        assert!(!f.shows_part(absent), "{absent}");
    }
    assert_eq!(f.harness.query_all(by().role(Role::Link)).count(), 0, "links are not followed");
    f.snapshot("documents-8-html");
}

/// Scenario "RTF report in a Windows code page".
#[test]
fn rtf_in_windows_1251_is_shown_in_cyrillic() {
    let mut f = cd(Settings::default());
    f.open("report_001.rtf");
    assert!(f.shows(reports::RTF_CYRILLIC_TEXT));
    f.snapshot("documents-9-rtf-cyrillic");
}

/// Scenario "Oversized file".
#[test]
fn an_oversized_file_is_listed_as_too_large_and_not_loaded() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions::small()).expect("fixture");
    let file = std::fs::File::create(dir.path().join("HUGE.PDF")).expect("create");
    file.set_len(summrise_core::dicom::documents::MAX_DOCUMENT_BYTES + 1).expect("set length");
    let open = dir.path().to_path_buf();
    let mut f = start(dir, open, Settings::default(), [1280.0, 800.0], Theme::Dark);
    assert!(f.shows("too large to display"));
    f.click("HUGE.PDF");
    assert!(f.shows("This document could not be displayed."));
    assert!(f.shows_part("larger than 50 MB"));
    assert!(f.shows("Show in folder"));
}

/// Scenario "Selecting a report in privacy mode", per document and not remembered.
#[test]
fn privacy_mode_hides_a_document_until_it_is_revealed() {
    let private = Settings { hide_patient_details: true, ..Settings::default() };
    let mut f = cd(private);
    // In the list a title or the name of a file could identify the patient, so only the type shows.
    assert!(!f.shows("MR Report"));
    assert!(!f.shows("REPORT.PDF"));
    assert!(f.shows("PDF file"));
    f.snapshot("documents-10-privacy-list");

    let first_report = "Report, Report · 2026-01-15";
    f.harness.query_all_by_label(first_report).next().expect("a report").click();
    f.steps(3);
    f.run_until("the document", |f| !f.shows("Loading the document…"));
    f.steps(2);
    assert!(f.shows("Hidden in privacy mode — reports contain personal details"));
    assert!(f.shows("Show anyway"));
    for hidden in ["Measurements", "Diameter: 12 mm", "CT Measurements", "Preliminary — not verified"] {
        assert!(!f.shows(hidden), "{hidden}");
    }
    f.snapshot("documents-10-privacy-hidden");
    f.click("Show anyway");
    assert!(f.shows("Diameter: 12 mm"));
    assert!(f.shows("CT Measurements"));
    assert!(f.shows("Preliminary — not verified"));
    assert!(!f.shows("Hidden in privacy mode — reports contain personal details"));

    // Not remembered: closed and opened again, it is hidden again.
    f.press(Key::Escape);
    f.harness.query_all_by_label(first_report).next().expect("a report").click();
    f.steps(3);
    f.run_until("the document", |f| !f.shows("Loading the document…"));
    f.steps(2);
    assert!(f.shows("Hidden in privacy mode — reports contain personal details"));
    assert!(!f.shows("Diameter: 12 mm"));

    // Revealed, then privacy mode off and on again: hidden again.
    f.click("Show anyway");
    assert!(f.shows("Diameter: 12 mm"));
    f.harness.state_mut().settings.hide_patient_details = false;
    f.steps(2);
    assert!(f.shows("Diameter: 12 mm"));
    assert!(f.shows("MR Report"), "privacy mode off: the list has its titles");
    f.harness.state_mut().settings.hide_patient_details = true;
    f.steps(2);
    assert!(!f.shows("Diameter: 12 mm"));
    assert!(f.shows("Show anyway"));
}

/// The brief: the keyboard. Documents take the focus with Tab; Enter opens; Esc goes back.
#[test]
fn documents_can_be_reached_and_left_with_the_keyboard() {
    let mut f = cd(Settings::default());
    let row = "MR Report, Report · 2026-01-15";
    let mut focused = false;
    for _ in 0..80 {
        f.harness.key_press(Key::Tab);
        f.steps(2);
        focused = f.harness.get_by_label(row).accesskit_node().is_focused();
        if focused {
            break;
        }
    }
    assert!(focused, "Tab reaches the row");
    f.snapshot("documents-11-keyboard-focus");
    f.harness.key_press(Key::Enter);
    f.steps(3);
    assert!(f.viewer().documents.is_open(), "Enter opens the document");
    f.run_until("the document", |f| !f.shows("Loading the document…"));
    f.steps(2);
    assert!(f.shows("Normal study."));
    f.press(Key::Escape);
    assert!(!f.viewer().documents.is_open(), "Esc goes back");
}

/// Keys that move through the images do not move them behind a document.
#[test]
fn the_images_behind_a_document_stay_where_they_are() {
    let mut f = cd(Settings::default());
    f.run_until("the view", |f| f.viewer().views.contains_key(&CT));
    let before = f.viewer().views[&CT].slice;
    f.open("MR Report");
    for key in [Key::PageDown, Key::PageDown, Key::ArrowDown, Key::End, Key::R, Key::I] {
        f.press(key);
    }
    let view = &f.viewer().views[&CT];
    assert_eq!(view.slice, before);
    assert!(!view.invert);
    // A series in the browser goes back to the images, even the one on screen.
    f.harness.query_all_by_label("CT CHEST 2.5mm (synthetic)").next().expect("in the browser").click();
    f.steps(3);
    assert!(!f.viewer().documents.is_open());
    assert_eq!(f.viewer().session.active_series, Some(CT));
}

/// The brief: malicious media. A report nested beyond what is followed says what is missing.
#[test]
fn a_report_nested_too_deeply_says_what_is_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() })
        .expect("fixture");
    std::fs::write(dir.path().join("DEEP"), deeply_nested_report(100, "Deep down").expect("bytes")).expect("write");
    let open = dir.path().to_path_buf();
    let mut f = start(dir, open, Settings::default(), [1280.0, 800.0], Theme::Dark);
    f.open("Deep report");
    // Waited for, not looked for once: on a slow runner the report can arrive a few frames
    // after "Loading the document…" has gone.
    let notice = "Part of this report is not shown: it is nested too deeply or is too long.";
    let start = Instant::now();
    while !f.shows(notice) && start.elapsed() < Duration::from_secs(60) {
        f.harness.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    let pane: Vec<String> = f
        .harness
        .query_all(by())
        .filter_map(|node| node.accesskit_node().label())
        .skip_while(|label| label != "Back to images")
        .take(20)
        .collect();
    assert!(f.shows(notice), "the document pane shows: {pane:?}");
    assert!(!f.shows("Deep down"));
}

/// A disc with a report and no images opens on the report: there is nothing else to look at.
#[test]
fn a_disc_without_images_opens_on_its_document() {
    let dir = tempfile::tempdir().expect("tempdir");
    let basic_text_sr = "1.2.840.10008.5.1.4.1.1.88.11";
    let spec = reports::ReportSpec::new(
        basic_text_sr,
        reports::MR_REPORT_TITLE,
        vec![reports::ReportNode::text("Note", "Text of a lonely report.")],
    );
    write_structured_report(&dir.path().join("SR1"), &spec).expect("report");
    let open = dir.path().to_path_buf();
    let mut f = start(dir, open, Settings::default(), [1280.0, 800.0], Theme::Dark);
    f.run_until("the document", |f| f.shows_part("Text of a lonely report."));
    assert!(f.viewer().session.active_series.is_none());
    // Back to the images: there are none, and the viewer says so.
    f.press(Key::Escape);
    assert!(f.shows("This study has no images on this disc."));
    // It does not jump back into the report by itself.
    f.steps(4);
    assert!(!f.viewer().documents.is_open());
    assert!(f.shows("MR Report"));
}

/// A study of which the disc holds only a report: the picker gets to it, and away from it.
#[test]
fn a_study_without_images_opens_on_its_document_and_closes_with_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() })
        .expect("fixture");
    let spec = reports::ReportSpec {
        study_uid: "2.25.42".to_owned(),
        // Its own SOP Instance UID: not a duplicate of the first report of the CD.
        series: 191,
        ..reports::ReportSpec::new(
            "1.2.840.10008.5.1.4.1.1.88.11",
            "Report of another study",
            vec![reports::ReportNode::text("Note", "Text of the other study.")],
        )
    };
    write_structured_report(&dir.path().join("OTHER"), &spec).expect("report");
    let open = dir.path().to_path_buf();
    let mut f = start(dir, open, Settings::default(), [1280.0, 800.0], Theme::Dark);
    let lonely = f.viewer().session.studies().iter().position(|s| s.series.is_empty()).expect("the other study");
    f.harness.state_mut().viewer.session.select_study(lonely);
    f.run_until("its report", |f| f.shows_part("Text of the other study."));
    assert!(f.viewer().session.active_series.is_none());
    // Back to the study with images: the report of the other study goes with its study.
    let images = 1 - lonely;
    f.harness.state_mut().viewer.session.select_study(images);
    f.steps(3);
    assert!(!f.viewer().documents.is_open());
    assert_eq!(f.viewer().session.active_series.map(|key| key.study), Some(images));
}

/// The brief: report text can be selected and copied.
#[test]
fn report_text_can_be_selected_and_copied() {
    let mut f = cd(Settings::default());
    f.open("MR Report");
    let rect = f.harness.get_by_label("No focal lesion.").rect();
    f.harness.drag_at(rect.left_center() + egui::vec2(1.0, 0.0));
    f.harness.step();
    f.harness.drop_at(rect.right_center() - egui::vec2(1.0, 0.0));
    f.harness.step();
    f.harness.event(egui::Event::Copy);
    f.harness.step();
    let copied = f.harness.output().platform_output.commands.iter().find_map(|command| match command {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    });
    assert_eq!(copied.as_deref(), Some("No focal lesion."));
}

/// Layout at the smallest window and in the light theme: nothing overlaps, and the
/// text wraps within the window. The pictures are for review.
#[test]
fn the_document_view_fits_the_smallest_window_in_both_themes() {
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        let mut f = cd_sized(Settings::default(), [960.0, 600.0], theme);
        f.open(reports::MEASUREMENTS_TITLE);
        let status = f.harness.get_by_label("Preliminary — not verified").rect();
        assert!(status.right() <= 960.0, "{name}: {status:?}");
        f.snapshot(&format!("documents-12-960-report-{name}"));
        f.press(Key::Escape);
        f.open(reports::STORED_PDF_TITLE);
        f.run_until("the page", |f| f.page_scale(0).is_some());
        for label in ["Next page", "Zoom in", "Fit to width"] {
            let rect = f.harness.get_by_label(label).rect();
            assert!(rect.right() <= 960.0, "{name}: {label} at {rect:?}");
        }
        f.snapshot(&format!("documents-12-960-pdf-{name}"));
        f.press(Key::Escape);
        f.open(reports::DOSE_REPORT_TITLE);
        f.snapshot(&format!("documents-12-960-dose-{name}"));
        // And wide.
        let mut f = cd_sized(Settings::default(), [1440.0, 900.0], theme);
        f.open(reports::MR_REPORT_TITLE);
        f.snapshot(&format!("documents-12-1440-report-{name}"));
        f.press(Key::Escape);
        f.open(reports::STORED_PDF_TITLE);
        f.run_until("the page", |f| f.page_scale(0).is_some());
        f.steps(2);
        f.snapshot(&format!("documents-12-1440-pdf-{name}"));
    }
}

#[test]
fn every_widget_of_the_document_view_has_a_name() {
    let mut f = cd(Settings::default());
    assert_eq!(f.harness.unnamed_widgets(), [] as [String; 0]);
    for document in [reports::MR_REPORT_TITLE, reports::DOSE_REPORT_TITLE, reports::STORED_PDF_TITLE, "report_001.rtf"]
    {
        f.open(document);
        f.steps(4);
        assert_eq!(f.harness.unnamed_widgets(), [] as [String; 0], "{document}");
        f.press(Key::Escape);
    }
}
