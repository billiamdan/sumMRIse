//! Saving, copying and exporting in the viewer, driven offscreen with a fake file
//! chooser. Set `SUMMRISE_SNAPSHOT_DIR` to keep renderings of the windows and the
//! files that come out, for review.
//!
//! The data is the export study of the testkit (`summrise_testkit::export`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::accesskit::{Role, Toggled};
use egui::{Event, Key, Modifiers, OutputCommand, Theme};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use summrise_core::dicom::{DecodedInstance, DicomError, InstanceHeader, Media, Progress, scan};
use summrise_core::imaging::MprAxis;
use summrise_core::{
    DicomFiles, ImageSource, Layout, SeriesData, SeriesKey, Session, Settings, ViewKind, ViewState, Waker,
};
use summrise_testkit::export::{
    self as fx, LUNG_BANDS_HU, LUNG_FOV_MM, LUNG_SIZE, lung_band_column, sixty_hu, write_export_study,
};
use summrise_testkit::{PATIENT_ID, PhantomOptions};

use super::Viewer;
use crate::app::{Action, Toasts};
use crate::export::{Chooser, Selected, Source, annotated};
use crate::theme;

/// Everything `Viewer::show` needs.
pub(super) struct App {
    pub(super) viewer: Viewer,
    pub(super) settings: Settings,
    waker: Waker,
    actions: Vec<Action>,
    pub(super) toasts: Toasts,
}

/// Answers every dialog with a place in its folder, and notes what it was asked.
#[derive(Default)]
pub(super) struct FakeChooser {
    folder: PathBuf,
    /// The user cancels the dialogs.
    pub(super) refuses: Mutex<bool>,
    /// The names suggested to the user.
    pub(super) suggested: Mutex<Vec<String>>,
    /// How many times a folder was asked for.
    pub(super) folders: Mutex<usize>,
}

impl Chooser for FakeChooser {
    fn save_file(&self, _title: &str, _kinds: &str, _extensions: &[&str], suggested: &str) -> Option<PathBuf> {
        self.suggested.lock().expect("lock").push(suggested.to_owned());
        (!*self.refuses.lock().expect("lock")).then(|| self.folder.join(suggested))
    }

    fn folder(&self, _title: &str) -> Option<PathBuf> {
        *self.folders.lock().expect("lock") += 1;
        (!*self.refuses.lock().expect("lock")).then(|| self.folder.clone())
    }
}

pub(super) struct Fixture {
    pub(super) harness: Harness<'static, App>,
    pub(super) key: SeriesKey,
    pub(super) chooser: Arc<FakeChooser>,
    /// Where the exports go.
    pub(super) out: tempfile::TempDir,
    pub(super) study: tempfile::TempDir,
}

pub(super) fn snapshot_dir() -> Option<PathBuf> {
    std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from)
}

/// The study `write` makes, with its series `series` fully loaded, in `layout`
/// and a window of `size` points.
pub(super) fn open_study(
    write: impl FnOnce(&Path),
    series: usize,
    layout: Layout,
    size: [f32; 2],
    theme: Theme,
) -> Fixture {
    let study = tempfile::tempdir().expect("tempdir");
    write(study.path());
    let index = scan(study.path(), &Progress::new()).expect("scan");
    let key = SeriesKey { study: 0, series };
    let settings = Settings { default_layout: layout, ..Settings::default() };
    let mut session = Session::with_source(index, settings.memory_budget_bytes(), None, Arc::new(DicomFiles));
    session.select_series(key);
    let out = tempfile::tempdir().expect("tempdir");
    let chooser = Arc::new(FakeChooser { folder: out.path().to_path_buf(), ..FakeChooser::default() });
    let mut viewer = Viewer::new(session, &settings);
    viewer.export.chooser = chooser.clone();
    let app = App { viewer, settings, waker: Arc::new(|| {}), actions: Vec::new(), toasts: Toasts::default() };
    let mut builder = Harness::builder().with_size(size).with_theme(theme).with_context_setup(theme::install);
    if snapshot_dir().is_some() {
        builder = builder.wgpu();
    }
    let harness = builder.build_ui_state(
        |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
        app,
    );
    let mut fixture = Fixture { harness, key, chooser, out, study };
    fixture.run_until("the series", |f| {
        f.harness.state().viewer.session.loaded(key).is_some() && f.harness.state().viewer.views.contains_key(&key)
    });
    fixture.harness.run_steps(4);
    fixture
}

/// The export study with `series` loaded.
pub(super) fn open(series: usize) -> Fixture {
    open_study(
        |dir| drop(write_export_study(dir).expect("fixture")),
        series,
        Layout::Single,
        [1280.0, 800.0],
        Theme::Dark,
    )
}

impl Fixture {
    pub(super) fn run_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        loop {
            self.harness.step();
            if done(self) {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(60), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub(super) fn viewer(&self) -> &Viewer {
        &self.harness.state().viewer
    }

    pub(super) fn series(&self) -> Arc<SeriesData> {
        self.viewer().session.data(self.key).expect("the series is selected")
    }

    pub(super) fn view(&self) -> &ViewState {
        self.viewer().views.get(&self.key).expect("the series is on screen")
    }

    pub(super) fn view_mut(&mut self) -> &mut ViewState {
        let key = self.key;
        self.harness.state_mut().viewer.views.get_mut(&key).expect("the series is on screen")
    }

    pub(super) fn shows(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }

    pub(super) fn shows_containing(&self, text: &str) -> bool {
        self.harness.query_all_by_label_contains(text).next().is_some()
    }

    pub(super) fn click(&mut self, label: &str) {
        self.harness.get_by_label(label).click();
        self.harness.run_steps(3);
    }

    pub(super) fn keys(&mut self, modifiers: Modifiers, key: Key) {
        self.harness.key_press_modifiers(modifiers, key);
        self.harness.run_steps(3);
    }

    pub(super) fn is_enabled(&self, label: &str) -> bool {
        !self.harness.get_by_label(label).accesskit_node().is_disabled()
    }

    pub(super) fn is_ticked(&self, label: &str) -> bool {
        self.harness.get_by_label(label).accesskit_node().toggled() == Some(Toggled::True)
    }

    pub(super) fn toasts(&self) -> Vec<String> {
        self.harness.state().toasts.texts().into_iter().map(str::to_owned).collect()
    }

    /// Whether any window of the export is open.
    pub(super) fn window_open(&self) -> bool {
        self.viewer().export.is_open()
    }

    /// The files in the folder of the exports, sorted.
    pub(super) fn exported(&self) -> Vec<String> {
        listing(self.out.path())
    }

    pub(super) fn selected(&self) -> Selected {
        let viewer = self.viewer();
        Source::new(&viewer.session, &viewer.views, viewer.active_view, &viewer.volume_view, viewer.shown_tiles())
            .selected()
            .expect("something on screen")
    }

    pub(super) fn save_window(&mut self, name: &str) {
        if let Some(dir) = snapshot_dir() {
            let image = self.harness.render().expect("wgpu render");
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            image.save(dir.join(format!("{name}.png"))).expect("save png");
        }
    }

    /// Saves one exported file for review.
    pub(super) fn keep_file(&self, name: &str, as_name: &str) {
        if let Some(dir) = snapshot_dir() {
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            std::fs::copy(self.out.path().join(name), dir.join(as_name)).expect("copy");
        }
    }

    /// Everything below "Save…" in the window "Save image", with the options as they are.
    fn save_with_defaults(&mut self) -> String {
        self.keys(Modifiers::COMMAND, Key::S);
        assert!(self.shows("Save image"), "the window is open");
        self.click("Save…");
        assert!(!self.window_open(), "the window closes");
        let names = self.exported();
        names.last().expect("a file was written").clone()
    }
}

pub(super) fn listing(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .expect("folder")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// All files below `root`, with their sizes.
fn tree(root: &Path) -> Vec<(PathBuf, u64)> {
    let mut files = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).expect("folder") {
            let entry = entry.expect("entry");
            if entry.file_type().expect("type").is_dir() {
                folders.push(entry.path());
            } else {
                files.push((entry.path(), entry.metadata().expect("metadata").len()));
            }
        }
    }
    files.sort();
    files
}

fn decode(folder: &Path, name: &str) -> image::DynamicImage {
    image::open(folder.join(name)).expect("an image file")
}

/// Spec "Image-only export of a CT slice": 512 × 512, 8-bit greyscale, −600 HU
/// mid-grey (127 or 128), −1350 HU and below black, +150 HU and above white.
#[test]
fn an_image_only_png_has_the_lung_window_grey_values() {
    let mut f = open(fx::LUNG_CT);
    assert_eq!((f.view().window.width, f.view().window.center), (1500.0, -600.0), "the Lung window");
    let name = f.save_with_defaults();
    assert_eq!(name, "SumMRIse-CT-S61-002.png", "the neutral name of the image on screen, its number 2 of 2");
    assert_eq!(*f.chooser.suggested.lock().expect("lock"), std::slice::from_ref(&name));

    let image = decode(f.out.path(), &name);
    assert_eq!(image.color(), image::ColorType::L8, "8-bit greyscale");
    assert_eq!((image.width(), image.height()), (LUNG_SIZE, LUNG_SIZE));
    let grey = image.to_luma8();
    let at = |band: usize| grey.get_pixel(lung_band_column(band) as u32, 300).0[0];
    assert_eq!(LUNG_BANDS_HU, [-1500.0, -1350.0, -600.0, 150.0, 900.0]);
    assert_eq!(at(0), 0, "below the window");
    assert_eq!(at(1), 0, "-1350 HU");
    assert!((127..=128).contains(&at(2)), "-600 HU gives {}", at(2));
    assert_eq!(at(3), 255, "+150 HU");
    assert_eq!(at(4), 255, "above the window");
}

/// Design D1: the resolution does not depend on the window or the zoom; the
/// picture follows the contrast and inversion on screen.
#[test]
fn image_only_ignores_zoom_and_pan_and_follows_the_contrast() {
    let mut f = open(fx::LUNG_CT);
    let plain = f.selected().image_only().expect("a picture").picture;
    let zoomed = {
        let t = f.view_mut().transform_mut(ViewKind::Stack);
        t.zoom_about(3.0, [40.0, -30.0]);
        f.harness.run_steps(2);
        f.selected().image_only().expect("a picture").picture
    };
    assert_eq!(plain, zoomed, "zoom and pan are not in the picture");

    f.view_mut().invert = true;
    f.harness.run_steps(2);
    let inverted = f.selected().image_only().expect("a picture").picture;
    assert_eq!(inverted.pixel(lung_band_column(1), 10), [255; 3], "-1350 HU, inverted");
    assert_eq!(inverted.pixel(lung_band_column(3), 10), [0; 3], "+150 HU, inverted");
}

/// Spec "Non-square pixels": 256 columns × 128 rows at 1.0 \ 0.5 is saved as
/// 256 × 256, with the disc round in the file.
#[test]
fn a_non_square_image_is_saved_with_correct_proportions() {
    let mut f = open(fx::NON_SQUARE);
    let name = f.save_with_defaults();
    let image = decode(f.out.path(), &name).to_luma8();
    assert_eq!((image.width(), image.height()), (256, 256));
    // 50 mm at 0.5 mm per pixel: a diameter of 200 pixels both ways.
    let lit = |pixels: Vec<u8>| pixels.iter().filter(|v| **v > 127).count() as i64;
    let across = lit((0..256).map(|x| image.get_pixel(x, 128).0[0]).collect());
    let down = lit((0..256).map(|y| image.get_pixel(128, y).0[0]).collect());
    assert!((across - 200).abs() <= 2 && (down - 200).abs() <= 2, "{across} × {down} pixels");
}

/// Design D1: the file follows the turn and flip of its view: the patient's
/// right stays on the side the letters of that view say. In the lung image the
/// bands run from black on the patient's right to white on the left.
#[test]
fn an_image_only_file_is_turned_and_mirrored_like_its_view() {
    let mut f = open(fx::LUNG_CT);
    let picture = |f: &Fixture| f.selected().image_only().expect("a picture");
    let standard = picture(&f);
    assert_eq!(standard.letters.as_ref().map(|l| l.left.clone()).as_deref(), Some("R"));
    assert_eq!(standard.picture.pixel(10, 10), [0; 3]);
    assert_eq!(standard.picture.pixel(500, 10), [255; 3]);

    // Mirrored: the white band and the letter L are on the left now.
    f.view_mut().transform_mut(ViewKind::Stack).flip_horizontally();
    let mirrored = picture(&f);
    assert_eq!(mirrored.letters.as_ref().map(|l| l.left.clone()).as_deref(), Some("L"));
    assert_eq!(mirrored.picture.pixel(10, 10), [255; 3]);
    assert_eq!(mirrored.picture.pixel(500, 10), [0; 3]);
    // The window says so, as the brief asks.
    f.harness.run_steps(2);
    f.keys(Modifiers::COMMAND, Key::S);
    assert!(f.shows_containing("mirrored"), "the export window says that the view is mirrored");
    f.save_window("export-save-mirrored");
    f.click("Cancel");

    // A quarter turn to the right as well: letters and picture agree.
    f.view_mut().transform_mut(ViewKind::Stack).flip_horizontally();
    f.view_mut().transform_mut(ViewKind::Stack).turn_right();
    let turned = picture(&f);
    let letters = turned.letters.expect("letters");
    assert_eq!(letters.top, "R");
    assert_eq!(turned.picture.pixel(10, 10), [0; 3], "the patient's right at the top");
    assert_eq!(turned.picture.pixel(10, 500), [255; 3], "the patient's left at the bottom");
}

/// Spec "Export with annotations" and "Default export with annotations": as
/// displayed while zoomed to 200 %, with orientation letters, scale bar, window
/// values, the study and series texts and the caption; no patient details.
#[test]
fn with_annotations_is_the_view_as_displayed_without_patient_details() {
    let mut f = open(fx::LUNG_CT);
    f.view_mut().transform_mut(ViewKind::Stack).zoom_about(2.0, [0.0, 0.0]);
    f.harness.run_steps(2);
    let selected = f.selected();
    let ctx = f.harness.ctx.clone();
    let settings = f.harness.state().settings.clone();
    let size = egui::vec2(640.0, 480.0);
    let (picture, texts) = annotated::pass(&ctx, &selected, &settings, false, size).expect("a picture");

    // Twice the points, and a band for the caption.
    assert_eq!((picture.width, picture.height), (1280, 960 + 48));
    let joined = texts.join("\n");
    for expected in
        ["R", "L", "A", "P", "W 1500  L -600 HU", "Zoom 200%", "Synthetic phantom study", "S61: LUNG BANDS (synthetic)"]
    {
        assert!(texts.iter().any(|text| text == expected), "{expected:?} in {joined}");
    }
    assert!(texts.iter().any(|text| text == "Image 2 / 2"), "{joined}");
    assert!(texts.iter().any(|text| text == "Not for diagnostic use"), "{joined}");
    // A scale bar with its label: 20, 50 or 100 mm.
    assert!(texts.iter().any(|text| text.ends_with(" mm") && !text.contains('.')), "{joined}");
    // No patient.
    for hidden in ["PHANTOM", "SYNTHETIC PHANTOM", PATIENT_ID, "1970", "Born"] {
        assert!(!joined.contains(hidden), "{hidden:?} in {joined}");
    }

    // As displayed: fitted to 480 points and zoomed to twice that, the image is
    // 960 points wide, centred on the view: the bands are where they are on screen.
    let band_width = 960.0 / LUNG_BANDS_HU.len() as f32;
    let centre = 320.0 * 2.0;
    let mid = (centre, 240.0 * 2.0);
    let pixel = |x: f32| picture.pixel(x as usize, mid.1 as usize);
    assert!((125..=130).contains(&pixel(mid.0)[0]), "-600 HU in the middle: {:?}", pixel(mid.0));
    // The band of +150 HU starts half a band plus... to the right of the centre.
    assert_eq!(pixel(centre + (band_width * 0.5 + 8.0) * 2.0), [255; 3]);
    assert_eq!(pixel(centre - (band_width * 0.5 + 8.0) * 2.0), [0; 3]);

    // The edge letter R is drawn in its colour at the left edge.
    let letter = (0..120).flat_map(|y| (0..60).map(move |x| (x, y + 440))).any(|(x, y)| {
        let [r, g, b] = picture.pixel(x, y);
        r > 200 && g > 150 && b < 150
    });
    assert!(letter, "the orientation letter is in the file");

    // The caption lies in the band under the view.
    let band_pixels = picture.rgb[960 * 1280 * 3..].iter().filter(|v| **v > 128).count();
    assert!(band_pixels > 500, "{band_pixels} bright values in the band");

    if let Some(dir) = snapshot_dir() {
        std::fs::create_dir_all(&dir).expect("snapshot dir");
        let bytes = summrise_core::export::encode(&picture, summrise_core::export::ImageFormat::Png).expect("png");
        std::fs::write(dir.join("export-annotated-zoomed.png"), bytes).expect("write");
    }
}

/// Spec "Details switched on once": the first file has the patient lines and
/// the option is unticked again.
#[test]
fn patient_details_are_included_once_when_asked_for() {
    let mut f = open(fx::LUNG_CT);
    let selected = f.selected();
    let ctx = f.harness.ctx.clone();
    let settings = f.harness.state().settings.clone();
    let size = egui::vec2(640.0, 480.0);
    let (_, texts) = annotated::pass(&ctx, &selected, &settings, true, size).expect("a picture");
    let joined = texts.join("\n");
    for expected in ["SYNTHETIC PHANTOM", "ID SYN-0001", "Born 1970-01-01"] {
        assert!(joined.contains(expected), "{expected:?} in {joined}");
    }

    // In the window: off, and only for pictures with text.
    f.keys(Modifiers::COMMAND, Key::S);
    let label = "Include patient details";
    assert!(!f.is_ticked(label) && !f.is_enabled(label), "image only: nothing to include");
    f.click("With annotations");
    assert!(f.is_enabled(label) && !f.is_ticked(label), "off by default");
    f.save_window("export-save-window");
    f.click(label);
    assert!(f.is_ticked(label));
    f.click("Save…");
    let with_details = f.exported().pop().expect("a file");
    let details_size = std::fs::metadata(f.out.path().join(&with_details)).expect("file").len();
    f.keep_file(&with_details, "export-annotated-with-details.png");

    // Opened again: unticked. Saved by default: another picture without the lines.
    f.keys(Modifiers::COMMAND, Key::S);
    assert!(!f.is_ticked(label), "the choice is not remembered");
    f.click("With annotations");
    assert!(!f.is_ticked(label), "the choice is not remembered");
    f.click("Save…");
    let without = std::fs::metadata(f.out.path().join(&with_details)).expect("file").len();
    assert_ne!(details_size, without, "the file was written again, without the lines");
}

/// Hard rule: privacy mode hides patient details everywhere; the export cannot
/// be told otherwise while it is on.
#[test]
fn privacy_mode_keeps_the_details_out_of_exports() {
    let mut f = open(fx::LUNG_CT);
    f.harness.state_mut().settings.hide_patient_details = true;
    f.harness.run_steps(2);
    f.keys(Modifiers::COMMAND, Key::S);
    f.click("With annotations");
    assert!(!f.is_enabled("Include patient details"));
    f.click("Cancel");
}

/// Task 3.2: an export writes exactly one file in the chosen location and
/// nothing elsewhere: not beside the study, not in the temporary folder.
#[test]
fn an_export_writes_exactly_one_file_in_the_chosen_place() {
    let mut f = open(fx::LUNG_CT);
    let study_before = tree(f.study.path());
    let temp = std::env::temp_dir();
    let ours_in_temp = |temp: &Path| {
        listing(temp).into_iter().filter(|name| name.starts_with("SumMRIse") || name.ends_with(".png")).count()
    };
    let temp_before = ours_in_temp(&temp);

    let name = f.save_with_defaults();
    assert_eq!(f.exported(), std::slice::from_ref(&name));
    assert_eq!(tree(f.study.path()), study_before, "nothing beside the study");
    assert_eq!(ours_in_temp(&temp), temp_before, "nothing in the temporary folder");

    // Metadata: the file holds the patient's data nowhere (task 5.2).
    let bytes = std::fs::read(f.out.path().join(&name)).expect("file");
    for needle in ["PHANTOM", "SYN-0001", "19700101", "1970-01-01", "Klinikum"] {
        assert!(!bytes.windows(needle.len()).any(|w| w == needle.as_bytes()), "{needle} in the file");
    }
    assert!(!name.contains("PHANTOM") && !name.contains("SYN-0001") && !name.contains("1970"));
    assert_eq!(f.toasts(), [format!("Saved {name}")]);
}

/// Cancelling the save dialog writes nothing.
#[test]
fn cancelling_the_file_dialog_writes_nothing() {
    let mut f = open(fx::LUNG_CT);
    *f.chooser.refuses.lock().expect("lock") = true;
    f.keys(Modifiers::COMMAND, Key::S);
    f.click("Save…");
    assert!(f.exported().is_empty());
    assert!(f.toasts().is_empty());
    assert!(!f.window_open());
}

/// JPEG is on offer; the suggestion and the file follow.
#[test]
fn an_image_can_be_saved_as_jpeg() {
    let mut f = open(fx::LUNG_CT);
    f.keys(Modifiers::COMMAND, Key::S);
    f.harness.query_all(by().label("Format").role(Role::ComboBox)).next().expect("the list").click();
    f.harness.run_steps(3);
    f.click("JPEG");
    f.click("Save…");
    assert_eq!(f.exported(), ["SumMRIse-CT-S61-002.jpg"]);
    let image = decode(f.out.path(), "SumMRIse-CT-S61-002.jpg");
    assert_eq!((image.width(), image.height(), image.color()), (LUNG_SIZE, LUNG_SIZE, image::ColorType::L8));
    let at = |band: usize| i32::from(image.to_luma8().get_pixel(lung_band_column(band) as u32, 200).0[0]);
    assert!(at(1) < 6 && at(3) > 250 && (at(2) - 127).abs() < 6, "{} {} {}", at(1), at(2), at(3));
}

/// Spec "Scanned report page": a warning with "Export anyway" and "Cancel"
/// before anything leaves.
#[test]
fn a_scanned_page_warns_before_it_is_exported() {
    let mut f = open(fx::SCANNED);
    f.keys(Modifiers::COMMAND, Key::S);
    f.click("Save…");
    assert!(f.shows("The picture itself may show personal details"));
    assert!(f.exported().is_empty(), "nothing yet");
    f.save_window("export-warning");
    f.click("Cancel");
    assert!(f.exported().is_empty() && !f.window_open(), "cancelled");
    assert!(f.chooser.suggested.lock().expect("lock").is_empty(), "no file dialog either");

    f.keys(Modifiers::COMMAND, Key::S);
    f.click("Save…");
    f.click("Export anyway");
    assert_eq!(f.exported(), ["SumMRIse-OT-S64-001.png"]);
    assert!(!f.window_open());
}

/// The warning goes first for ultrasound with Burned In Annotation too, for the
/// whole series, and for the clipboard; nothing is copied before the answer.
#[test]
fn burned_in_text_warns_before_a_series_export_and_a_copy() {
    let write = |dir: &Path| drop(summrise_testkit::fidelity::write_fidelity_study(dir).expect("fixture"));
    let mut f = open_study(write, summrise_testkit::fidelity::BURNED_IN, Layout::Single, [1280.0, 800.0], Theme::Dark);
    f.keys(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    f.click("Choose folder…");
    assert!(f.shows("The picture itself may show personal details"));
    assert_eq!(*f.chooser.folders.lock().expect("lock"), 0, "no folder asked for yet");
    f.click("Cancel");

    f.harness.event(Event::Copy);
    f.harness.run_steps(2);
    assert!(f.shows("The picture itself may show personal details"));
    f.harness.get_by_label("Copy anyway").click();
    f.harness.step();
    f.harness.step();
    assert!(!f.window_open());
    assert!(f.toasts().iter().any(|t| t.contains("copied")), "{:?}", f.toasts());
}

/// A CT that is not burned in goes without any warning.
#[test]
fn an_ordinary_ct_needs_no_warning() {
    let mut f = open(fx::LUNG_CT);
    f.keys(Modifiers::COMMAND, Key::S);
    f.click("Save…");
    assert!(!f.shows("The picture itself may show personal details"));
    assert_eq!(f.exported().len(), 1);
}

fn copied_image(f: &Fixture) -> Option<egui::ColorImage> {
    f.harness.output().platform_output.commands.iter().find_map(|command| match command {
        OutputCommand::CopyImage(image) => Some(image.clone()),
        _ => None,
    })
}

/// Spec "Pasting into a message": the image-only picture goes to the clipboard.
#[test]
fn the_copy_shortcut_places_the_image_only_picture_on_the_clipboard() {
    let mut f = open(fx::NON_SQUARE);
    f.harness.event(Event::Copy);
    f.harness.step();
    let image = copied_image(&f).expect("an image on the clipboard");
    // Square pixels, without any text; the same as a file would hold.
    assert_eq!(image.size, [256, 256]);
    let plain = f.selected().image_only().expect("a picture").picture;
    assert_eq!(image.pixels[128 * 256 + 128].to_array()[0], plain.pixel(128, 128)[0]);
    assert!(f.toasts().iter().any(|t| t.contains("copied")), "{:?}", f.toasts());
    assert!(!f.window_open(), "no window for a plain copy");
}

/// Design D5: copying text from the inspector keeps working: with a label's text
/// selected, or a text field in use, the copy command is theirs.
#[test]
fn copying_text_stays_copying_text() {
    let write = |dir: &Path| drop(write_export_study(dir).expect("fixture"));
    // So tall that the rows of the inspector are on screen.
    let mut f = open_study(write, fx::LUNG_CT, Layout::Single, [1280.0, 1500.0], Theme::Dark);
    // The search field of the inspector in use.
    f.click("All attributes");
    f.harness.query_all(by().role(Role::TextInput)).next().expect("the search field").click();
    f.harness.run_steps(3);
    f.harness.event(Event::Copy);
    f.harness.step();
    assert!(copied_image(&f).is_none(), "the field has the focus");
    f.click("Overview");

    // Dragging across a label selects its text.
    f.run_until("the inspector", |f| f.shows_containing("W 1500 / L -600"));
    let rect = f.harness.get_by_label("W 1500 / L -600 HU (Lung)").rect();
    let (from, to) = (rect.left_center() + egui::vec2(1.0, 0.0), rect.right_center() - egui::vec2(1.0, 0.0));
    f.harness.hover_at(from);
    f.harness.step();
    f.harness.drag_at(from);
    f.harness.step();
    f.harness.hover_at(to);
    f.harness.step();
    f.harness.drop_at(to);
    f.harness.run_steps(2);
    let selection = f.harness.ctx.plugin::<egui::text_selection::LabelSelectionState>().lock().has_selection();
    assert!(selection, "the drag selected the text at {rect:?}");
    f.harness.event(Event::Copy);
    f.harness.step();
    assert!(copied_image(&f).is_none(), "the selected text is copied, not the image");
    let copied_text = f
        .harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|command| matches!(command, OutputCommand::CopyText(text) if text.contains("1500")));
    assert!(copied_text, "{:?}", f.harness.output().platform_output.commands);

    // Nothing selected any more: the image again.
    f.harness.key_press(Key::Escape);
    f.harness.hover_at(egui::pos2(640.0, 700.0));
    f.harness.drag_at(egui::pos2(640.0, 700.0));
    f.harness.step();
    f.harness.drop_at(egui::pos2(640.0, 700.0));
    f.harness.run_steps(3);
    f.harness.event(Event::Copy);
    f.harness.step();
    assert!(copied_image(&f).is_some(), "an image once no text is selected");
}

/// The keys of the viewer rest while a window is open.
#[test]
fn the_viewer_keys_rest_while_a_window_is_open() {
    let mut f = open(fx::SIXTY);
    let before = f.view().slice;
    f.keys(Modifiers::COMMAND, Key::S);
    f.harness.key_press(Key::ArrowDown);
    f.harness.run_steps(2);
    assert_eq!(f.view().slice, before, "the images do not change behind the window");
    f.harness.key_press(Key::Escape);
    f.harness.run_steps(3);
    assert!(!f.window_open(), "Esc closes the window");
    f.harness.key_press(Key::ArrowDown);
    f.harness.run_steps(2);
    assert_eq!(f.view().slice, before + 1, "and the keys work again");
}

/// The keyboard path: ⌘S, Enter on the button in focus. The warning starts on
/// "Cancel", so that Enter alone never sends a picture out.
#[test]
fn the_windows_work_from_the_keyboard() {
    let mut f = open(fx::LUNG_CT);
    f.keys(Modifiers::COMMAND, Key::S);
    f.harness.key_press(Key::Enter);
    f.harness.run_steps(3);
    assert_eq!(f.exported().len(), 1, "saved with Enter");
    assert!(!f.window_open());

    let mut f = open(fx::SCANNED);
    f.keys(Modifiers::COMMAND, Key::S);
    f.harness.key_press(Key::Enter);
    f.harness.run_steps(3);
    assert!(f.shows("The picture itself may show personal details"));
    f.harness.key_press(Key::Enter);
    f.harness.run_steps(3);
    assert!(f.exported().is_empty() && !f.window_open(), "Enter chose Cancel");
}

/// The toolbar: one icon menu with its four entries and their shortcuts.
#[test]
fn the_toolbar_menu_offers_the_four_commands() {
    let mut f = open(fx::SIXTY);
    f.click("Save, copy or print");
    for entry in ["Save image…", "Copy image", "Export series…", "Print…"] {
        assert!(f.shows(entry) && f.is_enabled(entry), "{entry}");
    }
    f.save_window("export-menu");
    f.click("Export series…");
    assert!(f.shows("Export series"));
    f.click("Cancel");

    f.click("Save, copy or print");
    f.click("Save image…");
    assert!(f.shows("Save image"));
    f.click("Cancel");

    f.click("Save, copy or print");
    f.click("Copy image");
    assert!(f.toasts().iter().any(|t| t.contains("copied")), "{:?}", f.toasts());

    f.click("Save, copy or print");
    f.click("Print…");
    assert!(f.shows_containing("Whole series"), "the print window");
    f.click("Cancel");
}

/// Task 5.3 and spec "Sixty-slice series": 60 files numbered 001 to 060 in
/// display order, in the current contrast, with the progress in between.
#[test]
fn a_sixty_image_series_is_exported_in_display_order() {
    let mut f = open(fx::SIXTY);
    assert!(f.series().is_complete());
    f.keys(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    assert!(f.shows("Export series"));
    assert!(f.shows_containing("all 60 images"));
    f.save_window("export-series-window");
    f.click("Choose folder…");
    f.run_until("the export", |f| !f.window_open());
    let names = f.exported();
    assert_eq!(names.len(), 60);
    assert_eq!(names[0], "SumMRIse-CT-S63-001.png");
    assert_eq!(names[59], "SumMRIse-CT-S63-060.png");
    assert!(f.toasts().iter().any(|t| t.starts_with("60 images exported")), "{:?}", f.toasts());

    // Each file holds the image at that place of the series, through the window on screen.
    let series = f.series();
    let window = f.view().window;
    for (i, name) in names.iter().enumerate() {
        let instance = series.frames()[i].instance_number.expect("an instance number");
        let value = sixty_hu(instance as u32);
        let grey = decode(f.out.path(), name).to_luma8();
        assert_eq!((grey.width(), grey.height()), (32, 32));
        let expected = ((value - window.center + 0.5) / (window.width - 1.0) + 0.5) * 255.0;
        let found = f64::from(grey.get_pixel(5, 5).0[0]);
        assert!((found - expected).abs() <= 1.0, "{name}: {found}, expected about {expected}");
    }
}

/// A series of several image sets: every image in display order, and the view
/// on screen stays where it is.
#[test]
fn a_series_with_several_image_sets_is_exported_whole() {
    let write = |dir: &Path| drop(summrise_testkit::stacks::write_stacks_study(dir).expect("fixture"));
    let mut f = open_study(write, summrise_testkit::stacks::DYNAMIC, Layout::Single, [1280.0, 800.0], Theme::Dark);
    let total = f.series().len();
    assert!(total > f.series().stack_len(), "several image sets");
    f.keys(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    f.click("Choose folder…");
    f.run_until("the export", |f| !f.window_open());
    assert_eq!(f.exported().len(), total);
    assert_eq!(f.view().stack(), 0, "the view on screen did not move");
}

/// Spec: "SHALL NOT overwrite existing files without confirmation".
#[test]
fn existing_files_are_only_replaced_after_asking() {
    let mut f = open(fx::SIXTY);
    let mine = f.out.path().join("SumMRIse-CT-S63-007.png");
    std::fs::write(&mine, b"my own file").expect("write");

    f.keys(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    f.click("Choose folder…");
    assert!(f.shows("Replace existing files?"));
    assert!(f.shows_containing("1 file with the same name"));
    f.save_window("export-overwrite");
    f.click("Cancel");
    assert_eq!(std::fs::read(&mine).expect("read"), b"my own file", "not touched");
    assert_eq!(f.exported().len(), 1, "nothing written");

    f.keys(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    f.click("Choose folder…");
    f.click("Replace");
    f.run_until("the export", |f| !f.window_open());
    assert_eq!(f.exported().len(), 60);
    assert_ne!(std::fs::read(&mine).expect("read"), b"my own file");
}

/// An image source that lets the given number of files through, then waits.
pub(super) struct Gate(pub(super) AtomicUsize);

impl ImageSource for Gate {
    fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError> {
        while self.0.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.0.fetch_sub(1, Ordering::SeqCst);
        DicomFiles.read(media, header)
    }

    fn decode(&self, header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError> {
        DicomFiles.decode(header, bytes)
    }
}

/// The viewer on the series `series` of the export study with `images` images
/// loaded and the rest waiting for the gate.
pub(super) fn open_loading(series: usize, images: usize) -> (Harness<'static, App>, Arc<Gate>, tempfile::TempDir) {
    let study = tempfile::tempdir().expect("tempdir");
    write_export_study(study.path()).expect("fixture");
    let index = scan(study.path(), &Progress::new()).expect("scan");
    let key = SeriesKey { study: 0, series };
    let settings = Settings::default();
    let gate = Arc::new(Gate(AtomicUsize::new(images)));
    let mut session = Session::with_source(index, settings.memory_budget_bytes(), None, gate.clone());
    session.select_series(key);
    let app = App {
        viewer: Viewer::new(session, &settings),
        settings,
        waker: Arc::new(|| {}),
        actions: Vec::new(),
        toasts: Toasts::default(),
    };
    let mut harness = Harness::builder().with_size([1280.0, 800.0]).with_context_setup(theme::install).build_ui_state(
        |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
        app,
    );
    let start = Instant::now();
    loop {
        let viewer = &harness.state().viewer;
        if viewer.session.data(key).is_some_and(|series| series.loaded() >= images) && viewer.views.contains_key(&key) {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "timed out");
        harness.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    harness.run_steps(3);
    (harness, gate, study)
}

/// Exports need a fully loaded series; the menu says so and the key says so.
#[test]
fn a_series_export_needs_the_whole_series() {
    let (mut harness, gate, _study) = open_loading(fx::SIXTY, 3);
    harness.get_by_label("Save, copy or print").click();
    harness.run_steps(3);
    assert!(harness.get_by_label("Export series…").accesskit_node().is_disabled(), "not loaded yet");
    harness.key_press(Key::Escape);
    harness.run_steps(3);
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    harness.run_steps(3);
    assert!(!harness.state().viewer.export.is_open());
    let toasts = harness.state().toasts.texts().join("\n");
    assert!(toasts.contains("all images of the series have loaded"), "{toasts}");
    // The rest arrives without trouble.
    gate.0.store(usize::MAX / 2, Ordering::SeqCst);
}

/// Binding decision 2: image only leaves out scanner graphics, which can hold
/// text; on screen they are drawn.
#[test]
fn image_only_leaves_out_scanner_graphics() {
    use summrise_core::imaging::graphics::GRAPHICS_COLOR;

    let write = |dir: &Path| drop(summrise_testkit::fidelity::write_fidelity_study(dir).expect("fixture"));
    let f = open_study(write, summrise_testkit::fidelity::OVERLAY, Layout::Single, [1280.0, 800.0], Theme::Dark);
    let (screen, _) = crate::render::render_frame(&f.series(), f.view(), None).expect("an image");
    let screen = screen.expect("decoded");
    let colour = egui::Color32::from_rgb(GRAPHICS_COLOR[0], GRAPHICS_COLOR[1], GRAPHICS_COLOR[2]);
    assert!(screen.pixels.contains(&colour), "the scanner graphics are on screen");
    assert!(f.view().show_graphics);

    let picture = f.selected().image_only().expect("a picture").picture;
    let graphics = picture.rgb.as_chunks::<3>().0.iter().filter(|p| p[..] == GRAPHICS_COLOR[..3]).count();
    assert_eq!(graphics, 0, "and not in the file");
    assert!(f.view().show_graphics, "the view keeps them");
}

/// Binding decision 2: the colour map and a PET overlay shown are part of the picture.
#[test]
fn image_only_keeps_the_colour_map_and_the_pet_overlay() {
    use summrise_core::Fusion;
    use summrise_testkit::pet::{self as pet, PetStudyOptions, write_pet_study};

    let write = |dir: &Path| {
        let ct = PhantomOptions { ct_size: 96, ct_slices: 48, ..PhantomOptions::small() };
        write_pet_study(dir, &PetStudyOptions { ct, ..Default::default() }).expect("fixture");
    };
    // The PET itself, in its colour map.
    let f = open_study(write, pet::PET, Layout::Single, [1280.0, 800.0], Theme::Dark);
    let plain = f.selected().image_only().expect("a picture").picture;
    let expected = crate::render::render_frame(&f.series(), f.view(), None).expect("an image").0.expect("decoded");
    assert_eq!(plain.pixel(40, 40), [expected.pixels[40 * expected.size[0] + 40].r(); 3], "grey-based colour map");

    // The CT with the PET over it: more colourful than the CT alone.
    let mut f = open_study(write, pet::CT, Layout::Single, [1280.0, 800.0], Theme::Dark);
    let alone = f.selected().image_only().expect("a picture").picture;
    assert!(alone.is_grey());
    let pet_key = SeriesKey { study: 0, series: pet::PET };
    f.view_mut().set_fusion(Some(Fusion::new(pet_key)));
    f.run_until("the overlay", |f| {
        f.selected().overlay.is_some() && !f.view().fusion().is_none_or(|fusion| fusion.look().is_none())
    });
    // At the plane of the hot sphere.
    let series = f.series();
    let hot_slice = (0..series.stack_len())
        .min_by_key(|i| {
            let z = series.frames()[*i].plane.expect("plane").position.z;
            ((z - summrise_testkit::MARKER_POSITION[2]).abs() * 100.0) as i64
        })
        .expect("a slice");
    f.view_mut().set_slice(ViewKind::Stack, &series, hot_slice);
    f.harness.run_steps(2);
    let fused = f.selected().image_only().expect("a picture").picture;
    assert!(!fused.is_grey(), "the PET is in the picture");
}

/// A reformat is saved as itself, with the plane and slab on screen, neutrally named.
#[test]
fn a_reformat_is_saved_with_its_own_name_and_size() {
    let write = |dir: &Path| drop(write_export_study(dir).expect("fixture"));
    let mut f = open_study(write, fx::SIXTY, Layout::Mpr, [1280.0, 800.0], Theme::Dark);
    f.run_until("the reformats", |f| f.shows_containing("Coronal"));
    {
        let viewer = &mut f.harness.state_mut().viewer;
        viewer.active_view = ViewKind::Mpr(MprAxis::Coronal);
    }
    f.harness.run_steps(2);
    let selected = f.selected();
    assert_eq!(selected.view, ViewKind::Mpr(MprAxis::Coronal));
    let image = selected.image_only().expect("a picture");
    // 120 mm × 147.5 mm of the volume: taller than wide.
    assert!(image.picture.height > image.picture.width, "{} × {}", image.picture.width, image.picture.height);
    assert!(image.mm_per_pixel.is_some());
    let letters = image.letters.expect("letters");
    assert_eq!((letters.left.as_str(), letters.top.as_str()), ("R", "S"));

    f.keys(Modifiers::COMMAND, Key::S);
    f.click("Save…");
    let name = f.exported().pop().expect("a file");
    assert!(name.starts_with("SumMRIse-CT-S63-MPR-"), "{name}");
}

/// Binding decision 2: the 3D view always carries its "Illustration" caption,
/// even as image only, because its picture is computed.
#[test]
fn the_3d_picture_always_carries_its_caption() {
    let write = |dir: &Path| drop(write_export_study(dir).expect("fixture"));
    let mut f = open_study(write, fx::SIXTY, Layout::Volume, [1280.0, 800.0], Theme::Dark);
    f.run_until("the full 3D picture", |f| {
        f.selected().picture_3d.as_ref().is_some_and(crate::volume_view::ShownPicture::is_final)
    });
    let selected = f.selected();
    let shown = selected.image_only().expect("a picture").picture;

    f.keys(Modifiers::COMMAND, Key::S);
    f.save_window("export-save-3d");
    f.click("Save…");
    let name = f.exported().pop().expect("a file");
    assert_eq!(name, "SumMRIse-CT-S63-3D.png");
    f.keep_file(&name, "export-3d-image-only.png");
    let file = decode(f.out.path(), &name).to_rgb8();
    assert_eq!(file.width() as usize, shown.width);
    assert!(file.height() as usize > shown.height, "a band for the caption");
    let band = file.rows().skip(shown.height).flatten().filter(|p| p.0[0] > 128).count();
    assert!(band > 50, "the caption is in the band: {band} bright pixels");

    // With annotations: the caption of the view and that it is not for diagnostic use.
    let ctx = f.harness.ctx.clone();
    let settings = f.harness.state().settings.clone();
    let (_, texts) = annotated::pass(&ctx, &selected, &settings, false, egui::vec2(640.0, 480.0)).expect("a picture");
    let joined = texts.join("\n");
    assert!(joined.contains("Illustration: colours and surfaces are computed"), "{joined}");
    assert!(joined.contains("Not for diagnostic use"), "{joined}");
    assert!(texts.iter().any(|text| text == "R"), "{joined}");
}

/// The field of view of the fixture is what the file names claim to scale from.
#[test]
fn the_scale_of_a_saved_picture_is_known() {
    let f = open(fx::LUNG_CT);
    let image = f.selected().image_only().expect("a picture");
    let mm = image.mm_per_pixel.expect("spacing is known");
    assert!((mm * f64::from(LUNG_SIZE) - LUNG_FOV_MM).abs() < 1e-6);
}

/// In a tile grid a file with annotations shows the grid, as on screen; a file
/// of the image only is the current image, the one in the first tile.
#[test]
fn with_annotations_shows_the_tile_grid_and_image_only_the_current_image() {
    let mut f = open(fx::SIXTY);
    let series = f.series();
    f.view_mut().set_slice(ViewKind::Stack, &series, 30);
    f.harness.run_steps(2);
    for _ in 0..2 {
        f.harness.key_press(egui::Key::T);
        f.harness.run_steps(3);
    }
    let selected = f.selected();
    assert_eq!(selected.tiles, summrise_core::Tiles::ALL[2], "3 × 3");
    let ctx = f.harness.ctx.clone();
    let settings = f.harness.state().settings.clone();
    let (_, texts) = annotated::pass(&ctx, &selected, &settings, false, egui::vec2(900.0, 900.0)).expect("a picture");
    for number in 31..=39 {
        assert!(texts.contains(&number.to_string()), "tile {number} in {texts:?}");
    }
    assert_eq!(selected.state.slot_index(&selected.series), 30, "image only is image 31");
}
