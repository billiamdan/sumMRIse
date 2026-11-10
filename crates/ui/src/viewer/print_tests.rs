//! Printing in the viewer, driven offscreen: the print window with a fake file
//! chooser and fake print-system scripts in a temporary folder. There is no
//! printer here, so the job ends in the script. Set `SUMMRISE_SNAPSHOT_DIR` to keep
//! renderings of the window and of composed pages for review.
//!
//! The data is the export study of the testkit (`summrise_testkit::export`).

// The fake `lp` and `lpstat` are shell scripts: the tests that print run where
// there are shell scripts, on macOS and Linux. Windows has no `lp` (saved PDFs
// open in the default application), so the tests of saving and opening, and of
// the pages themselves, run there too, with a batch file as the fake opener.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;
use std::sync::Arc;
#[cfg(unix)]
use std::time::Instant;

use egui::Theme;
use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable, by};
use summrise_core::Layout;
use summrise_core::ViewKind;
use summrise_core::imaging::MprAxis;
use summrise_core::print::{
    IMAGES_PER_PAGE, PRINT_DPI, PageHeader, PageOrientation, PagePicture, PageSetup, Paper, PrintSystem, TextBitmap,
    TextRaster, compose_page,
};
use summrise_testkit::export::{self as fx, LUNG_FOV_MM, LUNG_SIZE, write_export_study};
#[cfg(unix)]
use summrise_testkit::{PATIENT_ID, PATIENT_NAME};

#[cfg(unix)]
use super::export_tests::open_loading;
use super::export_tests::{Fixture, open, open_study, snapshot_dir};
use crate::print::{PageFonts, PageOptions, Plan};
use crate::theme;

/// Fake `lp`, `lpstat` and `open`, which keep what they get in `record/`.
struct FakePrinter {
    dir: tempfile::TempDir,
}

/// The fake application that opens files: it keeps the path it is given.
const OPENER: &str = if cfg!(windows) { "open.cmd" } else { "open" };

/// Lists the printers "Office" (the default) and "Home", and accepts every job.
const ACCEPTS: &str =
    "printf '%s\\n' \"$@\" > {record}/args\ncat > {record}/stdin\necho 'request id is Home-1 (1 file(s))'";
/// Refuses every job, without reading it.
#[cfg(unix)]
const REFUSES: &str = "echo 'lp: The printer or class does not exist.' >&2\nexit 1";

impl FakePrinter {
    #[cfg(unix)]
    fn new(lp: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().display();
        let record = format!("{root}/record");
        std::fs::create_dir(&record).expect("record");
        let script = |name: &str, body: String| {
            let path = dir.path().join(name);
            std::fs::write(&path, format!("#!/bin/sh\n[ \"$1\" = --probe ] && exit 0\n{body}\n")).expect("script");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("permissions");
            // A thread forking at the moment the file was written can keep it busy for a moment.
            let start = Instant::now();
            while Command::new(&path).arg("--probe").status().is_err() {
                assert!(start.elapsed().as_secs() < 5, "the script does not start");
            }
        };
        script("lp", lp.replace("{record}", &record));
        script(
            "lpstat",
            "case \"$1\" in\n-e) printf 'Home\\nOffice\\n' ;;\n-d) echo 'system default destination: Office' ;;\nesac"
                .to_owned(),
        );
        script(OPENER, format!("echo \"$1\" > {record}/opened"));
        Self { dir }
    }

    /// Windows: only the opener, a batch file; there is no `lp` to fake.
    #[cfg(windows)]
    fn new(_lp: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let record = dir.path().join("record");
        std::fs::create_dir(&record).expect("record");
        let opened = record.join("opened");
        let script = format!("@echo off\r\necho %~1>\"{}\"\r\n", opened.display());
        std::fs::write(dir.path().join(OPENER), script).expect("script");
        Self { dir }
    }

    #[cfg(unix)]
    fn system(&self) -> PrintSystem {
        let path = |name: &str| self.dir.path().join(name);
        PrintSystem::at(path("lp"), path("lpstat"), path(OPENER))
    }

    /// No `lp` on this computer, as on Windows; the application for PDFs is there.
    fn system_without_lp(&self) -> PrintSystem {
        let path = |name: &str| self.dir.path().join(name);
        PrintSystem::at(path("no-lp"), path("no-lpstat"), path(OPENER))
    }

    fn record(&self, name: &str) -> PathBuf {
        self.dir.path().join("record").join(name)
    }
}

/// Text through a font of solid blocks, remembering all of it.
#[derive(Default)]
struct Recording {
    written: Vec<String>,
}

impl TextRaster for Recording {
    fn line(&mut self, text: &str, size: f64) -> TextBitmap {
        self.written.push(text.to_owned());
        let (width, height) = ((text.chars().count() as f64 * size * 0.5).ceil() as usize, size.ceil() as usize);
        TextBitmap { width, height, coverage: vec![255; width * height] }
    }
}

impl Fixture {
    /// The print window open on this fake print system, with its printers known.
    #[cfg(unix)]
    fn open_print_window(&mut self, printer: &FakePrinter) {
        self.harness.state_mut().viewer.export.print_system = printer.system();
        self.keys(Modifiers::COMMAND, Key::P);
        assert!(self.window_open(), "the print window is open");
        self.run_until("the printers", |f| {
            f.harness.query_all(by().label("Printer").role(Role::ComboBox)).next().is_some()
        });
    }

    #[cfg(unix)]
    fn click_button(&mut self, label: &str) {
        self.harness.query_all(by().label(label).role(Role::Button)).next().expect(label).click();
        self.harness.run_steps(3);
    }

    #[cfg(unix)]
    fn choose(&mut self, label: &str, entry: &str) {
        self.harness.query_all(by().label(label).role(Role::ComboBox)).next().expect(label).click();
        self.harness.run_steps(3);
        self.harness.query_all_by_label(entry).last().expect(entry).click();
        self.harness.run_steps(3);
    }

    fn wait_for_preview(&mut self) {
        self.run_until("the preview", |f| f.shows("Preview of the first page"));
        self.harness.run_steps(2);
    }

    /// The options of a job on the series on screen.
    fn plan(&self, options: PageOptions, dpi: u32) -> Plan {
        Plan::new(&Arc::new(self.selected()), options, dpi, true, &self.harness.ctx)
    }

    /// Keeps a composed page for review.
    fn keep_page(&self, plan: &Plan, page: usize, name: &str) {
        if let Some(dir) = snapshot_dir() {
            let image = plan.page(page, &mut PageFonts::new(egui::FontDefinitions::default())).image;
            let bytes = summrise_core::export::encode(&image, summrise_core::export::ImageFormat::Png).expect("png");
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            std::fs::write(dir.join(format!("{name}.png")), bytes).expect("write");
        }
    }
}

fn options(whole_series: bool, per_page: usize) -> PageOptions {
    PageOptions {
        whole_series,
        paper: Paper::A4,
        orientation: PageOrientation::Portrait,
        per_page,
        include_details: false,
    }
}

/// How many pages a PDF says it has.
#[cfg(unix)]
fn pdf_pages(pdf: &[u8]) -> usize {
    let plain = String::from_utf8_lossy(pdf);
    plain.split("/Count ").nth(1).and_then(|rest| rest.split(' ').next()).and_then(|n| n.parse().ok()).expect("/Count")
}

/// Spec "A series on sheets": 60 images with 9 per page: 7 pages, the preview
/// says "Page 1 of 7", the first page holds images 1 to 9, the last 55 to 60.
#[cfg(unix)]
#[test]
fn a_series_on_sheets_has_seven_pages() {
    let mut f = open(fx::SIXTY);
    let printer = FakePrinter::new(ACCEPTS);
    f.open_print_window(&printer);
    assert!(f.shows("Print"));
    assert!(f.shows("Page 1 of 1"), "the image on screen by default");
    f.save_window("print-window-image");

    f.click("Whole series (60 images)");
    f.wait_for_preview();
    assert!(f.shows("Page 1 of 7"));
    assert!(f.shows("Preview of the first page"));
    f.save_window("print-window-sheets");
    f.choose("Images per page", "12");
    f.wait_for_preview();
    assert!(f.shows("Page 1 of 5"));
    f.choose("Images per page", "9");
    f.wait_for_preview();

    let plan = f.plan(options(true, 9), PRINT_DPI / 4);
    assert_eq!(plan.pages(), 7);
    let numbers = |page: usize| {
        let mut fonts = Recording::default();
        let composed = plan.page(page, &mut fonts);
        let found: Vec<String> = fonts.written.into_iter().filter(|text| text.starts_with("Image ")).collect();
        (composed, found)
    };
    let (first, written) = numbers(0);
    assert_eq!(first.cells.len(), 9);
    let wanted: Vec<String> = (1..=9).map(|n| format!("Image {n} / 60")).collect();
    assert_eq!(written, wanted);
    let (last, written) = numbers(6);
    assert_eq!(last.cells.len(), 6);
    let wanted: Vec<String> = (55..=60).map(|n| format!("Image {n} / 60")).collect();
    assert_eq!(written, wanted);
    f.keep_page(&f.plan(options(true, 9), PRINT_DPI), 0, "print-page-sheet-9");
    f.keep_page(&f.plan(options(true, 2), PRINT_DPI), 0, "print-page-sheet-2");
}

/// Spec "Printing on macOS or Linux with CUPS": the printers are listed with the
/// default preselected; `lp` gets the PDF on standard input with the printer
/// chosen, the copies and the neutral title; no file is written.
#[cfg(unix)]
#[test]
fn printing_hands_the_pdf_to_lp_on_standard_input() {
    let mut f = open(fx::SIXTY);
    let printer = FakePrinter::new(ACCEPTS);
    f.open_print_window(&printer);
    let printer_shown = f.harness.query_all(by().label("Printer").role(Role::ComboBox)).next().expect("the list");
    assert_eq!(printer_shown.value().as_deref(), Some("Office"), "the default printer is preselected");
    f.choose("Printer", "Home");
    let copies = f.harness.get_by_label("Copies");
    let (target_node, target_tree) = copies.accesskit_node().locate();
    f.harness.event(egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::SetValue,
        target_node,
        target_tree,
        data: Some(egui::accesskit::ActionData::NumericValue(2.0)),
    }));
    f.harness.run_steps(2);
    assert_eq!(f.harness.get_by_label("Copies").accesskit_node().numeric_value(), Some(2.0));
    f.click("Whole series (60 images)");
    f.wait_for_preview();
    f.click_button("Print");
    f.run_until("the job", |f| !f.window_open());

    let pdf = std::fs::read(printer.record("stdin")).expect("what lp read");
    assert!(pdf.starts_with(b"%PDF"), "a PDF on standard input");
    assert_eq!(pdf_pages(&pdf), 7);
    let arguments = std::fs::read_to_string(printer.record("args")).expect("arguments");
    assert_eq!(arguments.lines().collect::<Vec<_>>(), ["-d", "Home", "-n", "2", "-t", "SumMRIse", "-o", "media=A4"]);
    assert!(f.toasts().iter().any(|t| t == "Sent to Home."), "{:?}", f.toasts());
    assert!(f.exported().is_empty(), "the application wrote no file");
    let plain = String::from_utf8_lossy(&pdf);
    for needle in [PATIENT_NAME, PATIENT_ID, "19700101", "1970-01-01", "/Font"] {
        assert!(!plain.contains(needle), "{needle} in the PDF");
    }
}

/// Spec "Printer refuses the job": a message gives its error text, the window
/// stays open, and "Save as PDF…" is still available; nothing is retried.
#[cfg(unix)]
#[test]
fn a_refused_job_is_reported_and_the_window_stays() {
    let mut f = open(fx::LUNG_CT);
    let printer = FakePrinter::new(REFUSES);
    f.open_print_window(&printer);
    f.wait_for_preview();
    f.click_button("Print");
    f.run_until("the answer", |f| f.shows_containing("does not exist"));
    assert!(f.window_open(), "the window stays open");
    assert!(f.shows("Could not print: lp: The printer or class does not exist."));
    assert!(f.is_enabled("Save as PDF…"));
    f.save_window("print-window-refused");
    assert!(f.toasts().is_empty());

    // Saving works from there, with the neutral name.
    f.click("Save as PDF…");
    f.run_until("the file", |f| !f.window_open());
    assert_eq!(*f.chooser.suggested.lock().expect("lock"), ["SumMRIse-CT-S61.pdf"]);
    assert_eq!(f.exported(), ["SumMRIse-CT-S61.pdf"]);
    let pdf = std::fs::read(f.out.path().join("SumMRIse-CT-S61.pdf")).expect("file");
    assert_eq!(pdf_pages(&pdf), 1);
    assert!(f.toasts().iter().any(|t| t.starts_with("Saved")), "{:?}", f.toasts());
}

/// Spec "No print system": the button reads "Save PDF and open…", the window says
/// that printing continues in the application that opens the PDF, and the file
/// goes where the user chooses before it is opened there.
#[test]
fn without_a_print_system_the_pdf_is_saved_and_opened() {
    let mut f = open(fx::SIXTY);
    let printer = FakePrinter::new(ACCEPTS);
    f.harness.state_mut().viewer.export.print_system = printer.system_without_lp();
    f.keys(Modifiers::COMMAND, Key::P);
    assert!(f.shows("Save PDF and open…"));
    assert!(f.harness.query_all(by().label("Print").role(Role::Button)).next().is_none(), "no Print button");
    assert!(f.shows_containing("Printing continues in the application that opens the PDF"));
    assert!(!f.shows("Printer") && !f.shows("Copies"), "nothing about printers");
    f.wait_for_preview();
    f.save_window("print-window-no-system");
    let focused = f.harness.query_all(by()).find(|node| node.accesskit_node().is_focused());
    assert_eq!(focused.and_then(|node| node.accesskit_node().label()).as_deref(), Some("Save PDF and open…"));

    f.click("Save PDF and open…");
    f.run_until("the file", |f| !f.window_open());
    let saved = f.out.path().join("SumMRIse-CT-S63.pdf");
    assert_eq!(f.exported(), ["SumMRIse-CT-S63.pdf"]);
    assert!(std::fs::read(&saved).expect("file").starts_with(b"%PDF"));
    let opened = std::fs::read_to_string(printer.record("opened")).expect("the file was opened");
    assert_eq!(opened.trim(), saved.display().to_string());
    assert!(f.toasts().iter().any(|t| t.contains("Printing continues")), "{:?}", f.toasts());
}

/// Task 4.3, from the window: a saved PDF of a series opens in the system's PDF
/// reader, where there is one, with its first page as composed. Without one the
/// test says so and passes.
#[cfg(unix)]
#[test]
fn a_saved_pdf_opens_in_a_system_reader() {
    let sips = Path::new("/usr/bin/sips");
    if !sips.exists() {
        eprintln!("skipped: no sips on this computer (the core test also tries pdftoppm)");
        return;
    }
    let mut f = open(fx::LUNG_CT);
    let printer = FakePrinter::new(ACCEPTS);
    f.open_print_window(&printer);
    f.click("Whole series (2 images)");
    f.wait_for_preview();
    f.click("Save as PDF…");
    f.run_until("the file", |f| !f.window_open());
    let pdf = f.out.path().join("SumMRIse-CT-S61.pdf");
    let png = f.out.path().join("first-page.png");
    let output =
        Command::new(sips).args(["-s", "format", "png"]).arg(&pdf).arg("--out").arg(&png).output().expect("sips");
    assert!(output.status.success(), "{output:?}");
    let page = image::open(&png).expect("a picture").to_rgb8();
    let ratio = f64::from(page.height()) / f64::from(page.width());
    assert!((ratio - 297.0 / 210.0).abs() < 0.01, "A4: {} × {}", page.width(), page.height());
    let at = |fx: f64, fy: f64| {
        page.get_pixel((fx * f64::from(page.width())) as u32, (fy * f64::from(page.height())) as u32).0
    };
    assert!(at(0.01, 0.01).iter().all(|v| *v > 240), "white at the corner");
    if let Some(dir) = snapshot_dir() {
        std::fs::create_dir_all(&dir).expect("snapshot dir");
        std::fs::copy(&png, dir.join("print-pdf-through-sips.png")).expect("copy");
        std::fs::copy(&pdf, dir.join("print-sample.pdf")).expect("copy");
    }
}

/// Spec "Picture with burned-in text": the warning appears first, with "Print
/// anyway" and "Cancel"; cancelling leaves the print window, with nothing sent.
#[cfg(unix)]
#[test]
fn a_scanned_page_warns_before_it_is_printed() {
    let mut f = open(fx::SCANNED);
    let printer = FakePrinter::new(ACCEPTS);
    f.open_print_window(&printer);
    f.wait_for_preview();
    f.click_button("Print");
    assert!(f.shows("The picture itself may show personal details"));
    assert!(f.shows("Print anyway"));
    f.save_window("print-warning");
    assert!(!printer.record("stdin").exists(), "nothing sent yet");
    // Two "Cancel" on screen: the warning is the one on top.
    let cancel = f.harness.query_all(by().label("Cancel").role(Role::Button)).last().expect("Cancel");
    cancel.click();
    f.harness.run_steps(3);
    assert!(f.shows("Page 1 of 1"), "back in the print window");
    assert!(!f.shows("Print anyway"));
    assert!(!printer.record("stdin").exists());

    f.click_button("Print");
    f.click("Print anyway");
    f.run_until("the job", |f| !f.window_open());
    assert!(printer.record("stdin").exists(), "sent after the answer");
}

/// Spec "Printed pages are private by default": no patient details on default
/// pages, and they are there when asked for, once; never in the title or names.
#[cfg(unix)]
#[test]
fn pages_are_private_by_default() {
    let mut f = open(fx::SIXTY);
    let printer = FakePrinter::new(ACCEPTS);

    // The text on default pages, with the synthetic patient's name, ID and birth date.
    let mut default = options(true, 9);
    let mut fonts = Recording::default();
    f.plan(default, PRINT_DPI / 4).page(0, &mut fonts);
    let text = fonts.written.join("\n");
    for needle in ["PHANTOM", PATIENT_ID, "1970", "Born"] {
        assert!(!text.contains(needle), "{needle} in {text}");
    }
    assert!(text.contains("Not for diagnostic use"));
    assert!(text.contains("Synthetic phantom study · 2026-01-15"), "the study and its date: {text}");
    assert!(text.contains("S63: SIXTY SLICES (synthetic)"), "the series: {text}");
    assert!(text.contains("Page 1 of 7"), "{text}");
    assert!(text.contains("W 2000  L 0 HU"), "window values: {text}");

    default.include_details = true;
    let mut fonts = Recording::default();
    f.plan(default, PRINT_DPI / 4).page(0, &mut fonts);
    let text = fonts.written.join("\n");
    for needle in ["SYNTHETIC PHANTOM", "ID SYN-0001", "Born 1970-01-01"] {
        assert!(text.contains(needle), "{needle} in {text}");
    }

    // In the window: unticked, ticked for one job, unticked again when it opens again.
    f.open_print_window(&printer);
    let label = "Include patient details";
    assert!(f.is_enabled(label) && !f.is_ticked(label), "off by default");
    f.click(label);
    assert!(f.is_ticked(label));
    f.wait_for_preview();
    f.click_button("Print");
    f.run_until("the job", |f| !f.window_open());
    let first = std::fs::read(printer.record("stdin")).expect("the first job");

    f.open_print_window(&printer);
    assert!(!f.is_ticked(label), "not remembered");
    f.wait_for_preview();
    f.click_button("Print");
    f.run_until("the job", |f| !f.window_open());
    let second = std::fs::read(printer.record("stdin")).expect("the second job");
    assert_ne!(first, second, "the first job's pages have lines that the second's have not");

    // Privacy mode hides patient details everywhere.
    f.harness.state_mut().settings.hide_patient_details = true;
    f.harness.run_steps(2);
    f.open_print_window(&printer);
    assert!(!f.is_enabled(label) && !f.is_ticked(label));
    f.click("Cancel");
    assert!(!f.window_open());
}

/// Spec "Non-square pixels": the picture on the page is square.
#[test]
fn a_non_square_image_prints_with_correct_proportions() {
    let f = open(fx::NON_SQUARE);
    let plan = f.plan(options(false, 1), PRINT_DPI / 2);
    let page = plan.page(0, &mut Recording::default());
    let shown = page.cells[0].picture;
    assert!(shown.width.abs_diff(shown.height) <= 1, "{shown:?}");
    // The disc is round on the page: 100 of 128 mm in both directions.
    let lit = |pixels: Vec<[u8; 3]>| pixels.iter().filter(|p| p[0] > 127).count() as f64;
    let across = lit((0..shown.width).map(|x| page.image.pixel(shown.x + x, shown.y + shown.height / 2)).collect());
    let down = lit((0..shown.height).map(|y| page.image.pixel(shown.x + shown.width / 2, shown.y + y)).collect());
    let expected = shown.width as f64 * 100.0 / 128.0;
    assert!((across - expected).abs() <= 3.0 && (down - expected).abs() <= 3.0, "{across} × {down}, {expected}");
    f.keep_page(&f.plan(options(false, 1), PRINT_DPI), 0, "print-page-non-square");
}

/// Spec "The scale bar on paper", from the data: with 4 images per page, a 50 mm
/// bar that keeps its proportion to the picture.
#[test]
fn the_scale_bar_of_a_printed_slice_is_true() {
    let f = open(fx::LUNG_CT);
    let mut fonts = Recording::default();
    let page = f.plan(options(true, 4), PRINT_DPI).page(0, &mut fonts);
    let layout = &page.cells[0];
    let bar = layout.scale_bar.expect("a scale bar");
    assert_eq!(bar.millimetres, 50);
    assert!(fonts.written.iter().any(|t| t == "50 mm"), "{:?}", fonts.written);
    let measured = (bar.columns.1 - bar.columns.0 + 1) as f64;
    let expected = layout.picture.width as f64 * 50.0 / LUNG_FOV_MM;
    assert!((measured - expected).abs() <= 1.0, "{measured}, {expected}");
    assert_eq!(f.selected().series.frames()[0].width, LUNG_SIZE);
    f.keep_page(&f.plan(options(true, 4), PRINT_DPI), 0, "print-page-lung-4");
}

/// Design D11: letters follow the view; a mirrored view prints mirrored and says so.
#[cfg(unix)]
#[test]
fn a_mirrored_view_prints_mirrored_and_says_so() {
    let mut f = open(fx::LUNG_CT);
    let printer = FakePrinter::new(ACCEPTS);
    let mut fonts = Recording::default();
    let plain = f.plan(options(false, 1), PRINT_DPI / 4).page(0, &mut fonts);
    assert!(fonts.written.iter().all(|t| !t.contains("Mirrored")));
    let shown = plain.cells[0].picture;
    assert_eq!(plain.image.pixel(shown.x + 3, shown.y + 3), [0; 3], "-1500 HU on the patient's right, at the left");

    f.view_mut().transform_mut(ViewKind::Stack).flip_horizontally();
    f.harness.run_steps(2);
    let mut fonts = Recording::default();
    let mirrored = f.plan(options(false, 1), PRINT_DPI / 4).page(0, &mut fonts);
    assert!(fonts.written.iter().any(|t| t.contains("Mirrored")), "{:?}", fonts.written);
    let shown = mirrored.cells[0].picture;
    assert_eq!(mirrored.image.pixel(shown.x + 3, shown.y + 3), [255; 3], "the white band at the left now");
    // The letters: L on the left of the picture now, in the order the composer draws them.
    let at = fonts.written.iter().position(|t| t == "L").expect("L");
    assert_eq!(fonts.written[at + 1], "R", "{:?}", fonts.written);
    f.open_print_window(&printer);
    assert!(f.shows_containing("mirrored"));
    f.click("Cancel");
}

/// Design D11: a reformat prints like an acquired image, marked as a reformat;
/// the 3D view prints with its "Illustration" caption.
#[test]
fn a_reformat_and_the_3d_view_print_as_what_they_are() {
    let write = |dir: &Path| drop(write_export_study(dir).expect("fixture"));
    let mut f = open_study(write, fx::SIXTY, Layout::Mpr, [1280.0, 800.0], Theme::Dark);
    f.run_until("the reformats", |f| f.shows_containing("Coronal"));
    f.harness.state_mut().viewer.active_view = ViewKind::Mpr(MprAxis::Coronal);
    f.harness.run_steps(2);
    let mut fonts = Recording::default();
    let page = f.plan(options(false, 1), PRINT_DPI / 4).page(0, &mut fonts);
    let text = fonts.written.join("\n");
    assert!(text.contains("Reformat: Coronal"), "{text}");
    assert!(text.contains("Coronal ") && text.contains("W 2000  L 0 HU"), "{text}");
    assert!(page.cells[0].scale_bar.is_some(), "a reformat has a known scale");
    assert!(page.cells[0].picture.height > page.cells[0].picture.width, "taller than wide");

    let mut f = open_study(write, fx::SIXTY, Layout::Volume, [1280.0, 800.0], Theme::Dark);
    f.run_until("the full 3D picture", |f| {
        f.selected().picture_3d.as_ref().is_some_and(crate::volume_view::ShownPicture::is_final)
    });
    let mut fonts = Recording::default();
    let page = f.plan(options(false, 1), PRINT_DPI / 4).page(0, &mut fonts);
    let text = fonts.written.join("\n");
    assert!(text.contains("Illustration: colours and surfaces are computed"), "{text}");
    assert!(page.cells[0].scale_bar.is_none(), "the 3D view has no scale bar");
    f.keep_page(&f.plan(options(false, 1), PRINT_DPI), 0, "print-page-3d");
}

/// Task 4.7: the job runs in the background, with progress and cancel, and a
/// cancelled job hands nothing over.
#[cfg(unix)]
#[test]
fn a_print_job_can_be_cancelled() {
    let mut f = open(fx::SIXTY);
    let printer = FakePrinter::new(ACCEPTS);
    f.open_print_window(&printer);
    f.click("Whole series (60 images)");
    f.choose("Images per page", "1");
    f.wait_for_preview();
    f.click_button("Print");
    assert!(f.window_open());
    f.run_until("the progress", |f| f.shows_containing("Composing pages"));
    f.save_window("print-window-progress");
    f.click("Cancel");
    f.run_until("the job to stop", |f| f.is_enabled("Save as PDF…"));
    assert!(f.window_open(), "the window stays, ready for another try");
    assert!(!printer.record("stdin").exists(), "nothing was handed over");
    assert!(!f.is_enabled("Whole series (60 images)") || f.shows("Page 1 of 60"));
}

/// The window works at the smallest window size, from the keyboard: everything
/// on screen, Tab through it, Enter on the button in focus, Esc to close.
#[cfg(unix)]
#[test]
fn the_window_fits_the_smallest_window_and_works_from_the_keyboard() {
    let write = |dir: &Path| drop(write_export_study(dir).expect("fixture"));
    for theme in [Theme::Dark, Theme::Light] {
        let mut f = open_study(write, fx::SIXTY, Layout::Single, [960.0, 600.0], theme);
        let printer = FakePrinter::new(ACCEPTS);
        f.open_print_window(&printer);
        f.click("Whole series (60 images)");
        f.wait_for_preview();
        f.save_window(&format!("print-window-960-{theme:?}"));
        for label in
            ["Print", "Save as PDF…", "Cancel", "Include patient details", "Copies", "Paper", "Images per page"]
        {
            let node = f.harness.query_all_by_label(label).next().unwrap_or_else(|| panic!("{label}"));
            let rect = node.rect();
            assert!(
                rect.left() >= 0.0 && rect.right() <= 960.0 && rect.top() >= 0.0 && rect.bottom() <= 600.0,
                "{label} at {rect:?}"
            );
        }
        let focused = |f: &Fixture| {
            let node = f.harness.query_all(by()).find(|node| node.accesskit_node().is_focused());
            node.and_then(|node| node.accesskit_node().label())
        };
        // A press of Tab moves through the window and stays in it.
        for _ in 0..24 {
            f.harness.key_press(Key::Tab);
            f.harness.run_steps(2);
            assert!(focused(&f).is_some(), "the focus is somewhere");
        }
        f.harness.key_press(Key::Escape);
        f.harness.run_steps(3);
        assert!(!f.window_open(), "Esc closes it");
    }

    // ⌘P and Enter print, from the button in focus.
    let mut f = open(fx::LUNG_CT);
    let printer = FakePrinter::new(ACCEPTS);
    f.open_print_window(&printer);
    f.harness.run_steps(2);
    f.harness.key_press(Key::Enter);
    f.run_until("the job", |f| !f.window_open());
    assert!(printer.record("stdin").exists(), "printed with Enter");
}

/// Spec "Series still loading": only the current image can be printed, and the
/// window says that the whole series can be printed when it has loaded.
#[cfg(unix)]
#[test]
fn while_a_series_loads_only_the_current_image_can_be_printed() {
    let (mut harness, gate, _study) = open_loading(fx::SIXTY, 3);
    let printer = FakePrinter::new(ACCEPTS);
    harness.state_mut().viewer.export.print_system = printer.system();
    // The image on screen is the one the loader started with: decoded by now.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::P);
    harness.run_steps(3);
    assert!(harness.state().viewer.export.is_open());
    let series = harness.query_all_by_label_contains("Whole series").next().expect("the option");
    assert!(series.accesskit_node().is_disabled(), "not until it has loaded");
    assert!(harness.query_all_by_label("The whole series can be printed when it has loaded.").next().is_some());
    assert!(harness.query_all_by_label("Page 1 of 1").next().is_some());
    gate.0.store(usize::MAX / 2, std::sync::atomic::Ordering::SeqCst);
}

/// Where the list of printers cannot be read, the system's default printer is
/// still tried, and what the print system then says is reported.
#[cfg(unix)]
#[test]
fn without_a_list_of_printers_the_default_printer_is_tried() {
    let mut f = open(fx::LUNG_CT);
    let printer = FakePrinter::new(ACCEPTS);
    let path = |name: &str| printer.dir.path().join(name);
    std::fs::write(path("broken"), "#!/bin/sh\necho 'lpstat: Scheduler is not running.' >&2\nexit 1\n")
        .expect("script");
    std::fs::set_permissions(path("broken"), std::fs::Permissions::from_mode(0o755)).expect("permissions");
    let start = Instant::now();
    while Command::new(path("broken")).status().is_err() {
        assert!(start.elapsed().as_secs() < 5, "the script does not start");
    }
    f.harness.state_mut().viewer.export.print_system = PrintSystem::at(path("lp"), path("broken"), path("open"));
    f.keys(Modifiers::COMMAND, Key::P);
    f.run_until("the list to fail", |f| f.shows("The system's default printer"));
    f.wait_for_preview();
    f.click_button("Print");
    f.run_until("the job", |f| !f.window_open());
    let arguments = std::fs::read_to_string(printer.record("args")).expect("arguments");
    assert_eq!(arguments.lines().collect::<Vec<_>>(), ["-n", "1", "-t", "SumMRIse", "-o", "media=A4"]);
}

/// Every sheet in the window ends in as many pages as it says.
#[test]
fn every_sheet_makes_the_pages_it_says() {
    let f = open(fx::SIXTY);
    for per_page in IMAGES_PER_PAGE {
        let plan = f.plan(options(true, per_page), PRINT_DPI / 8);
        assert_eq!(plan.pages(), 60usize.div_ceil(per_page));
        let last = plan.page(plan.pages() - 1, &mut Recording::default());
        let left = 60 % per_page;
        assert_eq!(last.cells.len(), if left == 0 { per_page } else { left }, "{per_page} per page");
    }
}

/// `PageFonts`: the fonts of the interface, in every script it has, without a
/// window; the same shapes as on screen.
#[test]
fn text_on_pages_comes_from_the_fonts_of_the_interface() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let output = ctx.run_ui(egui::RawInput::default(), |_| {});
    output.drop_without_applying_deltas();
    let definitions = crate::export::font_definitions(&ctx);
    let mut fonts = PageFonts::new(definitions);
    for text in ["Not for diagnostic use", "Nicht für die Diagnostik", "Не для диагностики", "Изображение 12 / 60"]
    {
        let line = fonts.line(text, 40.0);
        assert!(line.width > 200 && line.height >= 40, "{text}: {} × {}", line.width, line.height);
        let inked = line.coverage.iter().filter(|c| **c > 128).count();
        let share = inked as f64 / line.coverage.len() as f64;
        assert!((0.05..0.6).contains(&share), "{text}: ink on {share} of the line");
    }
    // The same width as egui lays out on screen at that size.
    let on_screen = ctx.fonts_mut(|f| {
        f.layout_no_wrap("Изображение".to_owned(), egui::FontId::proportional(40.0), egui::Color32::WHITE)
    });
    let line = fonts.line("Изображение", 40.0);
    assert!((line.width as f32 - on_screen.size().x).abs() <= 3.0, "{} and {}", line.width, on_screen.size().x);
    // Different letters, different shapes: no blanks, no boxes for everything.
    assert_ne!(fonts.line("Я", 40.0).coverage, fonts.line("Ж", 40.0).coverage);
    // Sizes follow.
    assert!(fonts.line("Я", 80.0).height > fonts.line("Я", 40.0).height);

    // A page in Cyrillic, through the same fonts: ink where the header is, and on black in the cell.
    let setup = PageSetup { paper: Paper::A4, orientation: PageOrientation::Portrait, per_page: 4, dpi: PRINT_DPI };
    let header = PageHeader {
        study: "КТ грудной клетки · 2026-01-15".to_owned(),
        series: "Серия 1: аксиальные срезы".to_owned(),
        patient: Vec::new(),
    };
    let picture = PagePicture {
        picture: summrise_core::export::Picture { width: 64, height: 64, rgb: vec![90; 64 * 64 * 3] },
        mm_per_pixel: Some(3.75),
        letters: None,
        number: "Изображение 12 / 60".to_owned(),
        window: Some("W 400  L 40 HU".to_owned()),
        notes: vec!["Отражено".to_owned()],
    };
    let page = compose_page(&setup, &header, 0, 1, &[picture], true, &mut fonts);
    let top = &page.image.rgb[..300 * page.image.width * 3];
    assert!(top.iter().filter(|v| **v < 100).count() > 2000, "the header is printed");
    if let Some(dir) = snapshot_dir() {
        let bytes = summrise_core::export::encode(&page.image, summrise_core::export::ImageFormat::Png).expect("png");
        std::fs::create_dir_all(&dir).expect("snapshot dir");
        std::fs::write(dir.join("print-page-cyrillic.png"), bytes).expect("write");
    }
}
