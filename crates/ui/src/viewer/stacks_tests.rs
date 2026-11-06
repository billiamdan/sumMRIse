//! Series split into stacks and cine playback in the viewer, driven offscreen
//! through the keyboard, the playback bar and the clock of the harness (every
//! frame moves the UI clock on by the harness step, so no test depends on
//! real time; only loading runs on threads). Set `SUMMRISE_SNAPSHOT_DIR` to
//! keep renderings for review.
//!
//! The data is the stacks study of the testkit: a marker sphere at L 40, A 35,
//! S 50 mm whose value tells the sets apart (600, 900 and 1200 in time points
//! 1, 2 and 3 of the dynamic series).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use egui::{Key, Theme};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use summrise_core::dicom::{DecodedInstance, DicomError, InstanceHeader, Media, Progress, scan};
use summrise_core::imaging::{DVec3, Look, MprAxis, Quality, Window};
use summrise_core::{
    DicomFiles, ImageSource, Layout, SeriesData, SeriesKey, Session, Settings, ViewKind, ViewState, Waker,
};
use summrise_testkit::MARKER_POSITION;
use summrise_testkit::stacks::{self as phantoms, write_stacks_study};

use super::Viewer;
use crate::app::{Action, Toasts};
use crate::theme;

const fn key(series: usize) -> SeriesKey {
    SeriesKey { study: 0, series }
}
const DYNAMIC: SeriesKey = key(phantoms::DYNAMIC);
const DUAL_ECHO: SeriesKey = key(phantoms::DUAL_ECHO);
const INCOMPLETE: SeriesKey = key(phantoms::INCOMPLETE);
const AXIAL: ViewKind = ViewKind::Mpr(MprAxis::Axial);

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

/// Holds back the images after the first time point (instance numbers above
/// 12) until it is opened; the other files are read at once.
#[derive(Default)]
struct FirstTimePointOnly {
    open: AtomicBool,
}

impl ImageSource for FirstTimePointOnly {
    fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError> {
        while header.instance_number.is_some_and(|n| n > 12) && !self.open.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        DicomFiles.read(media, header)
    }

    fn decode(&self, header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError> {
        DicomFiles.decode(header, bytes)
    }
}

/// The viewer on the stacks study with series `key` selected, reading its
/// files from `source`; every frame moves the UI clock on by `step` seconds.
fn start(key: SeriesKey, settings: Settings, theme: Theme, source: Arc<dyn ImageSource>, step: f32) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    write_stacks_study(dir.path()).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let mut session = Session::with_source(index, settings.memory_budget_bytes(), None, source);
    session.select_series(key);
    let app = App {
        viewer: Viewer::new(session, &settings),
        settings,
        waker: Arc::new(|| {}),
        actions: Vec::new(),
        toasts: Toasts::default(),
    };
    let mut builder = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_theme(theme)
        .with_step_dt(step)
        .with_context_setup(theme::install);
    if snapshot_dir().is_some() {
        builder = builder.wgpu();
    }
    let harness = builder.build_ui_state(
        |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
        app,
    );
    Fixture { harness, key, _dir: dir }
}

/// The viewer with series `key` loaded completely.
fn open(key: SeriesKey, settings: Settings, theme: Theme, step: f32) -> Fixture {
    let mut fixture = start(key, settings, theme, Arc::new(DicomFiles), step);
    fixture.run_until("the series", |f| f.viewer().session.loaded(f.key).is_some());
    fixture.steps(4);
    fixture
}

impl Fixture {
    /// Runs UI frames until `done` (loading runs on threads).
    fn run_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < Duration::from_secs(60), "timed out waiting for {what}");
            self.harness.step();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

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

    fn view_mut(&mut self) -> &mut ViewState {
        let key = self.key;
        self.harness.state_mut().viewer.views.get_mut(&key).expect("the series is on screen")
    }

    /// Whether something on screen is labelled exactly `label`.
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

    fn click(&mut self, label: &str) {
        self.harness.get_by_label(label).click();
        self.steps(2);
    }

    /// The value of the inspector's "Display" row `label`.
    fn row(&self, label: &str) -> Option<String> {
        let active = (self.key, self.series());
        let rows = self.viewer().display_rows(Some(&active));
        rows.into_iter().find(|(l, _)| l == label).map(|(_, value)| value)
    }

    /// Value of the picture of a reformat view at the crosshair.
    fn value_at_crosshair(&self, kind: ViewKind) -> Option<f32> {
        let displayed = &self.viewer().renders.get(self.key, kind).expect("rendered").displayed;
        let (x, y) = displayed.patient_to_pixel(self.view().cursor)?;
        displayed.value_at(&self.series(), x.round() as usize, y.round() as usize)
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

/// Tasks 4.1, 4.2 and 4.5 — the stack control and the arrow keys change the
/// time point at the same image; scrolling stays in the stack; the
/// annotations, the inspector and the browser name the stacks.
#[test]
fn the_stack_control_and_the_arrow_keys_change_the_time_point() {
    for theme in [Theme::Dark, Theme::Light] {
        let mut f = open(DYNAMIC, Settings::default(), theme, 0.25);
        assert!(f.shows("Image 7 / 12, Time point 1 / 3"), "image 7 of the first time point");
        assert!(f.shows_containing("3 time points × 12 images"), "the browser row says so");
        assert!(f.harness.get_by_label("Previous image set").accesskit_node().is_disabled(), "the first time point");

        // Right: the next time point, at the same image and position.
        f.press(Key::ArrowRight);
        assert!(f.shows("Image 7 / 12, Time point 2 / 3"));
        assert_eq!((f.view().stack(), f.view().slice), (1, 6));
        assert_eq!(f.row("Image").as_deref(), Some("7 of 12"));
        assert_eq!(f.row("Image set").as_deref(), Some("Time point 2 / 3"));
        assert_eq!(f.row("Image sets").as_deref(), Some("3 time points × 12 images"));
        let position = f.series().frames()[f.view().slot_index(&f.series())].plane.expect("plane").position.z;
        assert_eq!(position, 50.0, "S 50 mm");
        f.snapshot(&format!("stacks-1-time-point-2-{}", if theme == Theme::Dark { "dark" } else { "light" }));

        // Up and Down change the image inside the stack, and stop at its end.
        f.press(Key::ArrowDown);
        assert!(f.shows("Image 8 / 12, Time point 2 / 3"));
        f.press(Key::End);
        f.press(Key::ArrowDown);
        assert!(f.shows("Image 12 / 12, Time point 2 / 3"), "not image 1 of time point 3");

        // The buttons of the control, and the ends of the stacks.
        f.click("Next image set");
        assert!(f.shows("Image 12 / 12, Time point 3 / 3"));
        assert!(f.harness.get_by_label("Next image set").accesskit_node().is_disabled());
        f.press(Key::ArrowRight);
        assert_eq!(f.view().stack(), 2, "the last time point");
        f.click("Previous image set");
        f.press(Key::ArrowLeft);
        assert!(f.shows("Image 12 / 12, Time point 1 / 3"));
        assert!(!f.view().is_pristine(&f.series()));
    }
}

/// A series that cannot be split evenly keeps today's list: Right shows the
/// next image and there is no stack control.
#[test]
fn a_series_that_is_not_split_keeps_its_arrow_keys() {
    let mut f = open(INCOMPLETE, Settings::default(), Theme::Dark, 0.25);
    assert!(f.shows("Image 18 / 35"));
    f.press(Key::ArrowRight);
    assert!(f.shows("Image 19 / 35"));
    f.press(Key::ArrowLeft);
    f.press(Key::ArrowLeft);
    assert!(f.shows("Image 17 / 35"));
    assert!(!f.shows("Next image set") && !f.shows("Image set"));
    assert_eq!(f.row("Image set"), None);
    assert!(f.row("Reformatting").is_some_and(|r| r.contains("several images share the same slice position")));
    assert!(f.viewer().session.stacks(INCOMPLETE).is_none(), "the browser does not claim stacks for it");
}

/// Task 4.1 — the reformats show the stack chosen: at the marker (L 40,
/// A 35, S 50 mm) the axial reformat reads 600, then 900, then 1200.
#[test]
fn the_reformats_show_the_time_point_chosen() {
    let settings = Settings { default_layout: Layout::Mpr, ..Settings::default() };
    let mut f = open(DYNAMIC, settings, Theme::Dark, 0.25);
    let series = f.series();
    f.view_mut().set_cursor(DVec3::from_array(MARKER_POSITION), &series);
    f.steps(3);
    assert_eq!(f.value_at_crosshair(AXIAL), Some(600.0));
    f.press(Key::ArrowRight);
    assert_eq!(f.value_at_crosshair(AXIAL), Some(900.0), "the marker of time point 2");
    assert_eq!(f.value_at_crosshair(ViewKind::Mpr(MprAxis::Sagittal)), Some(900.0));
    f.press(Key::ArrowRight);
    assert_eq!(f.value_at_crosshair(AXIAL), Some(1200.0));
    assert_eq!(f.view().cursor, DVec3::from_array(MARKER_POSITION), "the crosshair stays");
    assert!(f.shows_containing("Time point 3 / 3"));
    f.snapshot("stacks-2-mpr-time-point-3");
}

/// The 3D view shows the volume of the time point chosen: the marker's
/// value (600, 900, 1200) shows as ever brighter maximum intensity.
#[test]
fn the_3d_view_shows_the_time_point_chosen() {
    let settings = Settings { default_layout: Layout::Volume, ..Settings::default() };
    let mut f = open(DYNAMIC, settings, Theme::Dark, 0.25);
    assert_eq!(f.viewer().layout, Layout::Volume);
    {
        let view = f.view_mut();
        view.look = Look::Mip;
        // 300 … 1500: the three marker values stay apart on the grey scale.
        view.window = Window::new(900.0, 1200.0);
        view.preset = None;
    }
    let mut brightest = Vec::new();
    for time_point in 1..=3 {
        if time_point > 1 {
            // In the 3D view the arrow keys turn the object; the playback bar changes the stack.
            let series = f.series();
            f.view_mut().step_stack(&series, 1);
        }
        f.steps(2);
        assert!(f.shows_containing(&format!("Time point {time_point} / 3")));
        f.run_until("the full 3D picture", |f| {
            f.viewer()
                .volume_view
                .shown()
                .is_some_and(|shown| shown.quality == Quality::Full && shown.request.style.look == Look::Mip)
        });
        let shown = f.viewer().volume_view.shown().expect("a picture");
        brightest.push(shown.picture.rgba.as_chunks::<4>().0.iter().map(|pixel| pixel[0]).max().expect("pixels"));
    }
    assert!(brightest[0] < brightest[1] && brightest[1] < brightest[2], "brightest grey per time point: {brightest:?}");
}

/// Task 3.3 in the viewer — each echo shows its own window; time points share one.
#[test]
fn each_echo_shows_its_own_window() {
    let mut f = open(DUAL_ECHO, Settings::default(), Theme::Dark, 0.25);
    assert!(f.shows("Image 7 / 12, Echo 1 / 2 · TE 15 ms"));
    assert_eq!(f.row("Window").as_deref(), Some("W 4000 / L 2000 (Window 1)"));
    f.press(Key::ArrowRight);
    assert!(f.shows("Image 7 / 12, Echo 2 / 2 · TE 90 ms"));
    assert_eq!(f.row("Window").as_deref(), Some("W 700 / L 350 (Window 1)"));
    f.view_mut().window = Window::new(200.0, 500.0);
    f.view_mut().preset = None;
    f.press(Key::ArrowLeft);
    assert_eq!(f.row("Window").as_deref(), Some("W 4000 / L 2000 (Window 1)"));
    f.press(Key::ArrowRight);
    assert_eq!(f.row("Window").as_deref(), Some("W 500 / L 200 (custom)"));
    f.snapshot("stacks-3-echo-2");
    // R resets every echo to its own window and stays on echo 2.
    f.press(Key::R);
    assert_eq!(f.row("Window").as_deref(), Some("W 700 / L 350 (Window 1)"));
    assert_eq!(f.view().stack(), 1);
}

/// Task 4.4 — Space plays the time points at the image shown, and pauses.
#[test]
fn space_plays_the_time_points_and_pauses() {
    // 50 ms per frame: at the default 10 images per second, a step every second frame.
    let mut f = open(DYNAMIC, Settings::default(), Theme::Dark, 0.05);
    assert!(f.shows("Play") && !f.shows("Pause"));
    f.press(Key::Space);
    assert!(f.viewer().cine.is_playing());
    assert!(f.shows("Pause"), "the button says what it does now");
    let mut seen = vec![f.view().stack()];
    for _ in 0..12 {
        f.steps(1);
        if seen.last() != Some(&f.view().stack()) {
            seen.push(f.view().stack());
        }
    }
    assert!(seen.len() >= 4, "the time points change: {seen:?}");
    assert!(seen.windows(2).all(|w| w[1] == (w[0] + 1) % 3), "in a loop: {seen:?}");
    assert_eq!(f.view().slice, 6, "at the same image");
    f.snapshot("cine-1-playing");

    f.press(Key::Space);
    assert!(!f.viewer().cine.is_playing());
    let paused = f.view().stack();
    f.steps(10);
    assert_eq!(f.view().stack(), paused, "paused");
    assert!(f.shows("Play"));
    f.snapshot("cine-2-paused");
    // The button does the same as Space.
    f.click("Play");
    assert!(f.viewer().cine.is_playing());
    f.click("Pause");
    assert!(!f.viewer().cine.is_playing());
}

/// Task 4.4 — playback waits at an image that is still loading; the loader
/// reads it next, and playback goes on once it is decoded.
#[test]
fn playback_waits_for_an_image_still_loading() {
    let gate = Arc::new(FirstTimePointOnly::default());
    let mut f = start(DYNAMIC, Settings::default(), Theme::Dark, gate.clone(), 0.05);
    f.run_until("image 7 of time point 1", |f| f.viewer().views.contains_key(&f.key) && f.series().frame(6).is_some());
    f.press(Key::Space);
    f.steps(10);
    assert_eq!(f.view().stack(), 0, "image 7 of time point 2 is not loaded yet");
    assert!(f.viewer().cine.is_waiting());
    assert!(f.shows("Waiting for images to load"));
    f.snapshot("cine-3-waiting");
    // The bar fits beside the browser and the inspector in a smaller window too.
    f.harness.set_size(egui::vec2(1280.0, 800.0));
    f.steps(3);
    f.snapshot("cine-4-waiting-1280");

    gate.open.store(true, Ordering::SeqCst);
    f.run_until("the next time point", |f| f.view().stack() == 1);
    assert!(f.series().frame(18).is_some());
    assert!(!f.viewer().cine.is_waiting());
    assert_eq!(f.view().slice, 6);
}

/// Task 4.4 — playback stops when another series is selected.
#[test]
fn playback_stops_when_another_series_is_selected() {
    let mut f = open(DYNAMIC, Settings::default(), Theme::Dark, 0.05);
    f.press(Key::Space);
    assert!(f.viewer().cine.is_playing());
    f.harness.query_all_by_label("CINE 1 SLICE (synthetic)").next().expect("browser row").click();
    f.steps(3);
    assert_eq!(f.viewer().session.active_series, Some(key(phantoms::CINE)));
    assert!(!f.viewer().cine.is_playing());
}
