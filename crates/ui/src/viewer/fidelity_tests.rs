//! Unusual but standard images in the viewer (display-fidelity), driven
//! offscreen: palette colour, contrast tables, scanner graphics, the lossy
//! mark next to the "Mirrored" badge, the burned-in text notice of privacy
//! mode and the read-out over padding. Set `SUMMRISE_SNAPSHOT_DIR` to keep
//! renderings for review.
//!
//! The data is the fidelity study of the testkit (`summrise_testkit::fidelity`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{ColorImage, Key, Theme};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use summrise_core::dicom::{Progress, scan};
use summrise_core::imaging::graphics::GRAPHICS_COLOR;
use summrise_core::{DicomFiles, SeriesData, SeriesKey, Session, Settings, ViewKind, ViewState, Waker};
use summrise_testkit::fidelity::{self as fx, write_fidelity_study};

use super::Viewer;
use crate::app::{Action, Toasts};
use crate::placement::Placement;
use crate::render::{Displayed, render_frame};
use crate::theme;

/// Everything `Viewer::show` needs.
struct App {
    viewer: Viewer,
    settings: Settings,
    waker: Waker,
    actions: Vec<Action>,
    toasts: Toasts,
}

struct Fixture {
    harness: Harness<'static, App>,
    key: SeriesKey,
    _dir: tempfile::TempDir,
}

fn snapshot_dir() -> Option<PathBuf> {
    std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from)
}

/// The viewer on the fidelity study with series `series` loaded completely,
/// at the default window size of 1280 × 800 points.
fn open(series: usize, settings: Settings, theme: Theme) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    write_fidelity_study(dir.path()).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let key = SeriesKey { study: 0, series };
    let mut session = Session::with_source(index, settings.memory_budget_bytes(), None, Arc::new(DicomFiles));
    session.select_series(key);
    let app = App {
        viewer: Viewer::new(session, &settings),
        settings,
        waker: Arc::new(|| {}),
        actions: Vec::new(),
        toasts: Toasts::default(),
    };
    let mut builder =
        Harness::builder().with_size([1280.0, 800.0]).with_theme(theme).with_context_setup(theme::install);
    if snapshot_dir().is_some() {
        builder = builder.wgpu();
    }
    let harness = builder.build_ui_state(
        |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
        app,
    );
    let mut fixture = Fixture { harness, key, _dir: dir };
    let start = Instant::now();
    while fixture.viewer().session.loaded(key).is_none() || !fixture.viewer().views.contains_key(&key) {
        assert!(start.elapsed() < Duration::from_secs(60), "timed out loading series {series}");
        fixture.harness.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    fixture.steps(4);
    fixture
}

/// The RGBA of pixel (`x`, `y`) of a rendered picture.
fn pixel(image: &ColorImage, x: usize, y: usize) -> [u8; 4] {
    image.pixels[y * image.size[0] + x].to_array()
}

impl Fixture {
    fn steps(&mut self, n: usize) {
        for _ in 0..n {
            self.harness.step();
        }
    }

    fn viewer(&self) -> &Viewer {
        &self.harness.state().viewer
    }

    fn series(&self) -> Arc<SeriesData> {
        self.viewer().session.data(self.key).expect("the series is selected")
    }

    fn view(&self) -> &ViewState {
        self.viewer().views.get(&self.key).expect("the series is on screen")
    }

    /// The acquired picture as the render path draws it, and its geometry.
    fn rendered(&self) -> (ColorImage, Displayed) {
        let (image, displayed) = render_frame(&self.series(), self.view(), None).expect("an image");
        (image.expect("decoded"), displayed)
    }

    fn shows(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }

    fn shows_containing(&self, text: &str) -> bool {
        self.harness.query_all_by_label_contains(text).next().is_some()
    }

    fn press(&mut self, key: Key) {
        self.harness.key_press(key);
        self.steps(2);
    }

    /// Where the node labelled `label` inside the viewport `viewport` is (the
    /// inspector may show the same text as a row).
    fn rect_in(&self, viewport: &str, label: &str) -> Option<egui::Rect> {
        let viewport = self.harness.get_by_label(viewport).rect();
        self.harness.query_all_by_label(label).map(|node| node.rect()).find(|r| viewport.contains_rect(*r))
    }

    /// The value of the inspector's "Display" row `label`.
    fn row(&self, label: &str) -> Option<String> {
        let active = (self.key, self.series());
        self.viewer().display_rows(Some(&active)).into_iter().find(|(l, _)| l == label).map(|(_, value)| value)
    }

    fn snapshot(&mut self, name: &str) {
        self.harness.check_accessibility();
        if let Some(dir) = snapshot_dir() {
            let image = self.harness.render().expect("wgpu render");
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            image.save(dir.join(format!("{name}.png"))).expect("save png");
        }
    }
}

/// Task 5.1 (palette) — index 3 is drawn as RGB (200, 30, 60); a colour
/// image has no window controls.
#[test]
fn palette_images_are_shown_in_their_colours() {
    let mut f = open(fx::PALETTE_8, Settings::default(), Theme::Dark);
    assert!(f.series().is_color());
    let (image, _) = f.rendered();
    assert_eq!(pixel(&image, 3, 0), [200, 30, 60, 255]);
    assert_eq!(pixel(&image, 0, 3), [200, 30, 60, 255]);
    assert!(!f.shows("W"), "no window width for a colour image");
    f.snapshot("fidelity-1-palette");
}

/// Tasks 5.1 and 5.2 — the image with two contrast tables opens with
/// "Normal": stored 100 is drawn grey 40 while the read-out stays 100; the
/// number keys and A move between tables and windows.
#[test]
fn contrast_tables_are_chosen_like_windows() {
    let mut f = open(fx::VOI_LUT, Settings::default(), Theme::Dark);
    assert_eq!(f.view().lut, Some(0));
    assert_eq!(f.row("Contrast table").as_deref(), Some("Normal"));
    assert_eq!(f.row("Window"), None, "the table replaces the window of the acquired image");
    let (image, displayed) = f.rendered();
    assert_eq!(pixel(&image, 36, 1), [40, 40, 40, 255], "entry 640 of 4095");
    assert_eq!(displayed.value_at(&f.series(), 36, 1), Some(100.0), "the read-out shows the value");
    f.snapshot("fidelity-2-contrast-table");

    // Key 2: the second table, HARDER (entry 2 of 4095 for stored 100: black).
    f.press(Key::Num2);
    assert_eq!(f.view().contrast_name(&f.series()), Some("Harder"));
    assert_eq!(pixel(&f.rendered().0, 36, 1), [0, 0, 0, 255]);
    // A: the automatic window, an ordinary window; key 1 the default table again.
    f.press(Key::A);
    assert_eq!((f.view().lut, f.view().preset.as_deref()), (None, Some("Auto")));
    assert_eq!(f.row("Contrast table"), None);
    assert!(f.row("Window").is_some_and(|w| w.ends_with("(Auto)")));
    f.press(Key::Num1);
    assert_eq!(f.view().lut, Some(0));
    assert!(f.view().is_pristine(&f.series()));
}

/// Tasks 5.1 and 5.3 — the rectangle of the overlay covers image column 20,
/// row 10: drawn in the graphics colour while the value there stays 500; G
/// and the toolbar toggle hide it, R shows it again.
#[test]
fn scanner_graphics_are_drawn_until_the_user_hides_them() {
    let mut f = open(fx::OVERLAY, Settings::default(), Theme::Dark);
    let (image, displayed) = f.rendered();
    assert_eq!(pixel(&image, 20, 10), GRAPHICS_COLOR);
    assert_ne!(pixel(&image, 25, 13), GRAPHICS_COLOR, "inside the outline");
    assert_eq!(displayed.value_at(&f.series(), 20, 10), Some(500.0), "the value under the graphics");
    assert!(f.shows("Scanner graphics"), "the toolbar toggle");
    f.snapshot("fidelity-3-graphics-on");

    f.press(Key::G);
    assert!(!f.view().show_graphics);
    let hidden = f.rendered().0;
    let grey = pixel(&hidden, 20, 10);
    assert!(grey != GRAPHICS_COLOR && grey[0] == grey[1] && grey[1] == grey[2], "the grey image itself: {grey:?}");
    assert_eq!(pixel(&hidden, 25, 13), pixel(&image, 25, 13), "nothing else changed");
    f.snapshot("fidelity-3-graphics-off");
    f.press(Key::R);
    assert!(f.view().show_graphics);
    f.harness.get_by_label("Scanner graphics").click();
    f.steps(2);
    assert!(!f.view().show_graphics, "the toggle hides them too");

    // A series without scanner graphics has no toggle.
    let f = open(fx::LOSSY, Settings::default(), Theme::Dark);
    assert!(!f.shows("Scanner graphics"));
}

/// Task 5.5 — the lossy mark has a row of its own under "Mirrored" and stays
/// when the annotations are hidden.
#[test]
fn the_lossy_mark_and_the_mirrored_badge_never_cover_each_other() {
    for theme in [Theme::Dark, Theme::Light] {
        let mut f = open(fx::LOSSY, Settings::default(), theme);
        let badge = |f: &Fixture, label: &str| f.rect_in("Image 1 / 1", label);
        assert!(badge(&f, "Mirrored").is_none());
        let alone = badge(&f, "Lossy 10:1 (JPEG)").expect("the lossy mark on the image");
        // The inspector states the same fact.
        f.harness.run_steps(20);
        assert!(f.harness.query_all_by_label("Lossy 10:1 (JPEG)").count() >= 2, "badge and inspector row");
        f.press(Key::H);
        let lossy = badge(&f, "Lossy 10:1 (JPEG)").expect("the lossy mark");
        let mirrored = badge(&f, "Mirrored").expect("the mirrored badge");
        assert!(!lossy.intersects(mirrored), "{lossy:?} and {mirrored:?}");
        assert!(lossy.top() >= mirrored.bottom(), "the lossy mark is the row below");
        assert_eq!(lossy, alone, "the mark does not move when the view is mirrored");
        let name = if theme == Theme::Dark { "fidelity-4-lossy-and-mirrored" } else { "fidelity-4-lossy-light" };
        f.snapshot(name);
        f.press(Key::O);
        assert!(!f.harness.state().settings.show_overlays);
        assert!(badge(&f, "Lossy 10:1 (JPEG)").is_some() && badge(&f, "Mirrored").is_some(), "not annotations");
    }
}

/// Task 5.6 — privacy mode on an image with Burned In Annotation YES says
/// that patient details may be part of the picture.
#[test]
fn privacy_mode_says_that_burned_in_text_cannot_be_hidden() {
    let notice = "Patient details may be part of the picture itself and cannot be hidden.";
    let private = Settings { hide_patient_details: true, ..Settings::default() };
    let mut f = open(fx::BURNED_IN, private.clone(), Theme::Dark);
    assert!(f.shows(notice));
    f.snapshot("fidelity-5-privacy-notice");
    f.press(Key::O);
    assert!(f.shows(notice), "also with the annotations hidden");
    f.harness.state_mut().settings.hide_patient_details = false;
    f.steps(2);
    assert!(!f.shows(notice), "privacy mode off: no notice");
    // An image without burned-in text: no notice in privacy mode.
    let f = open(fx::LOSSY, private, Theme::Dark);
    assert!(!f.shows(notice));
}

/// Task 5.4 — over the padded corner of the CT slice the read-out says
/// "Outside the scanned area", over the body it shows HU.
#[test]
fn padding_reads_outside_the_scanned_area() {
    let mut f = open(fx::PADDED_CT, Settings::default(), Theme::Dark);
    let viewport = f.harness.get_by_label("Image 3 / 4").rect();
    let (image, displayed) = f.rendered();
    assert_eq!(pixel(&image, 0, 0), [0, 0, 0, 255], "padding is background");
    let place = Placement::of(viewport, &displayed, &f.view().transform(ViewKind::Stack));
    f.harness.hover_at(place.to_screen(1.0, 1.0));
    f.steps(3);
    assert!(f.shows_containing("Outside the scanned area"));
    assert!(!f.shows_containing("-3024"));
    f.snapshot("fidelity-6-padding");
    f.harness.hover_at(place.to_screen(32.0, 32.0));
    f.steps(3);
    assert!(!f.shows_containing("Outside the scanned area"));
    assert!(f.shows_containing(" HU"), "a value in HU over the body");
    // Inverted, the padding stays black.
    f.press(Key::I);
    assert_eq!(pixel(&f.rendered().0, 0, 0), [0, 0, 0, 255]);
}
