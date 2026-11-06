//! Tilted reformat planes and turned or flipped views in the viewer, driven
//! offscreen through the keyboard, the pointer and the toolbar. Set
//! `SUMMRISE_SNAPSHOT_DIR` to keep renderings for review.
//!
//! The synthetic CT has a marker of 300 HU at L 40 mm, A 35 mm, S 50 mm: where
//! it shows, and what the pointer reads there, tells whether a view is true.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use egui::accesskit::{Role, Toggled};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, Theme, Vec2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use summrise_core::dicom::{DecodedInstance, DicomError, InstanceHeader, Media, Progress, scan};
use summrise_core::imaging::{DVec3, MprAxis};
use summrise_core::{
    DicomFiles, ImageSource, Layout, SeriesData, SeriesKey, Session, Settings, ViewKind, ViewState, Waker,
};
use summrise_testkit::{CT_MARKER_HU, MARKER_POSITION, PhantomOptions, write_phantom_study};

use super::Viewer;
use crate::app::{Action, Toasts};
use crate::placement::Placement;
use crate::theme;
use crate::viewport::Hover;

const CT: SeriesKey = SeriesKey { study: 0, series: 0 };
const MR: SeriesKey = SeriesKey { study: 0, series: 1 };
const STACK: ViewKind = ViewKind::Stack;
const AXIAL: ViewKind = ViewKind::Mpr(MprAxis::Axial);
const CORONAL: ViewKind = ViewKind::Mpr(MprAxis::Coronal);
const SAGITTAL: ViewKind = ViewKind::Mpr(MprAxis::Sagittal);
/// Field of view of the synthetic CT (mm): 96 × 96 pixels of 2.5 mm.
const FOV_MM: f32 = 240.0;

/// Everything `Viewer::show` needs.
struct App {
    viewer: Viewer,
    settings: Settings,
    waker: Waker,
    actions: Vec<Action>,
    toasts: Toasts,
}

/// The viewer on the synthetic CD with the CT loaded completely.
struct Fixture {
    harness: Harness<'static, App>,
    _dir: tempfile::TempDir,
}

fn snapshot_dir() -> Option<PathBuf> {
    std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from)
}

fn marker() -> DVec3 {
    DVec3::from_array(MARKER_POSITION)
}

fn open(layout: Layout) -> Fixture {
    open_with(Settings { default_layout: layout, ..Settings::default() }, Theme::Dark)
}

/// Reads files only as far as the test lets it.
#[derive(Default)]
struct Gate {
    allowed: AtomicUsize,
    started: AtomicUsize,
}

impl Gate {
    fn allow(&self, n: usize) {
        self.allowed.fetch_add(n, Ordering::SeqCst);
    }

    fn allow_all(&self) {
        self.allowed.store(usize::MAX, Ordering::SeqCst);
    }

    fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }
}

impl ImageSource for Gate {
    fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError> {
        let turn = self.started.fetch_add(1, Ordering::SeqCst);
        while turn >= self.allowed.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        DicomFiles.read(media, header)
    }

    fn decode(&self, header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError> {
        DicomFiles.decode(header, bytes)
    }
}

/// The viewer on the synthetic CD, with the CT loaded completely.
fn open_with(settings: Settings, theme: Theme) -> Fixture {
    let mut fixture = start(settings, theme, Arc::new(DicomFiles));
    fixture.wait_until_loaded();
    fixture
}

/// The viewer on the synthetic CD, reading its files from `source`.
fn start(settings: Settings, theme: Theme, source: Arc<dyn ImageSource>) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = PhantomOptions { ct_size: 96, ct_slices: 48, ..PhantomOptions::small() };
    write_phantom_study(dir.path(), &options).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let session = Session::with_source(index, settings.memory_budget_bytes(), None, source);
    let app = App {
        viewer: Viewer::new(session, &settings),
        settings,
        waker: Arc::new(|| {}),
        actions: Vec::new(),
        toasts: Toasts::default(),
    };
    let mut builder =
        Harness::builder().with_size([1440.0, 900.0]).with_theme(theme).with_context_setup(theme::install);
    if snapshot_dir().is_some() {
        builder = builder.wgpu();
    }
    let harness = builder.build_ui_state(
        |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
        app,
    );
    Fixture { harness, _dir: dir }
}

impl Fixture {
    /// Runs UI frames until `done`.
    fn run_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < Duration::from_secs(60), "timed out waiting for {what}");
            self.harness.step();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_until_loaded(&mut self) {
        self.run_until("the CT", |f| f.viewer().session.loaded(CT).is_some());
        self.steps(4);
    }

    fn steps(&mut self, n: usize) {
        self.harness.run_steps(n);
    }

    fn viewer(&self) -> &Viewer {
        &self.harness.state().viewer
    }

    fn series(&self) -> Arc<SeriesData> {
        self.viewer().session.loaded(CT).expect("the CT is loaded")
    }

    fn view(&self) -> &ViewState {
        self.viewer().views.get(&CT).expect("the CT is on screen")
    }

    fn view_mut(&mut self) -> &mut ViewState {
        self.harness.state_mut().viewer.views.get_mut(&CT).expect("the CT is on screen")
    }

    /// Whether something on screen is labelled exactly `label`.
    fn shows(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }

    /// The label of the viewport whose label starts with `name`.
    fn viewport_label(&self, name: &str) -> String {
        self.harness
            .query_all_by_label_contains(name)
            .filter(|node| node.accesskit_node().role() == Role::Image)
            .filter_map(|node| node.accesskit_node().label())
            .find(|label| label.starts_with(name))
            .unwrap_or_else(|| panic!("no viewport named {name:?}"))
    }

    /// Screen rectangle of the viewport whose label starts with `name`.
    fn viewport(&self, name: &str) -> Rect {
        self.harness
            .query_all_by_label_contains(name)
            .find(|node| {
                let node = node.accesskit_node();
                node.role() == Role::Image && node.label().is_some_and(|label| label.starts_with(name))
            })
            .unwrap_or_else(|| panic!("no viewport named {name:?}"))
            .rect()
    }

    fn press(&mut self, key: Key) {
        self.harness.key_press(key);
        self.steps(2);
    }

    fn press_with(&mut self, modifiers: Modifiers, key: Key) {
        self.harness.key_press_modifiers(modifiers, key);
        self.steps(2);
    }

    /// Moves the pointer to `pos` and leaves it there.
    fn hover_at(&mut self, pos: Pos2) {
        self.harness.hover_at(pos);
        self.steps(2);
    }

    fn click_at(&mut self, pos: Pos2) {
        self.harness.hover_at(pos);
        for pressed in [true, false] {
            self.harness.event(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        self.steps(3);
    }

    /// Two clicks at `pos` within one frame.
    fn double_click_at(&mut self, pos: Pos2) {
        let button =
            |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        // Queued events are spread over one frame each, a quarter of a second apart.
        let events = [Event::PointerMoved(pos), button(true), button(false), button(true), button(false)];
        self.harness.input_mut().events.extend(events);
        self.steps(3);
    }

    /// Moves the pointer onto the picture of a view, which makes it the active one.
    fn activate(&mut self, kind: ViewKind, viewport: Rect) {
        self.hover_at(viewport.center() + vec2(-90.0, 70.0));
        assert_eq!(self.viewer().active_view, kind, "the view under the pointer is the active one");
    }

    /// What the pointer reads at `pos`.
    fn read_at(&mut self, pos: Pos2) -> Hover {
        self.hover_at(pos);
        self.viewer().hover.map(|(_, hover)| hover).unwrap_or_else(|| panic!("nothing under the pointer at {pos:?}"))
    }

    /// Edge letters (left, right, top, bottom) of a view, as the viewport draws them.
    fn letters(&self, kind: ViewKind, viewport: Rect) -> [String; 4] {
        let displayed = &self.viewer().renders.get(CT, kind).expect("rendered").displayed;
        let place = Placement::of(viewport, displayed, &self.view().transform(kind));
        let l = place.edge_labels(displayed).expect("the view has geometry");
        [l.left, l.right, l.top, l.bottom]
    }

    /// Where the patient position `p` is drawn in a view.
    fn on_screen(&self, kind: ViewKind, viewport: Rect, p: DVec3) -> Pos2 {
        let displayed = &self.viewer().renders.get(CT, kind).expect("rendered").displayed;
        let (x, y) = displayed.patient_to_pixel(p).expect("the view has geometry");
        Placement::of(viewport, displayed, &self.view().transform(kind)).to_screen(x, y)
    }

    /// Value of the picture at the crosshair of a reformat view.
    fn value_at_crosshair(&self, kind: ViewKind) -> Option<f32> {
        let displayed = &self.viewer().renders.get(CT, kind).expect("rendered").displayed;
        let (x, y) = displayed.patient_to_pixel(self.view().cursor)?;
        displayed.value_at(&self.series(), x.round() as usize, y.round() as usize)
    }

    /// Centres of the two handles of the line of the `plane` plane in the `view` view.
    fn handles(&self, plane: &str, view: &str) -> Vec<Pos2> {
        let label = format!("Tilt handle of the {plane} plane in the {view} view");
        self.harness.query_all_by_label(&label).map(|node| node.rect().center()).collect()
    }

    fn snapshot(&mut self, name: &str) {
        if let Some(dir) = snapshot_dir() {
            let image = self.harness.render().expect("wgpu render");
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            image.save(dir.join(format!("{name}.png"))).expect("save png");
        }
    }
}

/// Index of the acquired image closest to the marker.
fn marker_slice(series: &SeriesData) -> usize {
    let distance = |i: &usize| (series.frames()[*i].plane.expect("plane").position.z - marker().z).abs();
    (0..series.len()).min_by(|a, b| distance(a).total_cmp(&distance(b))).expect("images")
}

fn assert_reads_marker(hover: &Hover, what: &str) {
    assert_eq!(hover.value, Some(CT_MARKER_HU as f32), "{what}: {hover:?}");
    let p = hover.patient.expect("patient position");
    assert!((p.x - marker().x).abs() < 8.0 && (p.y - marker().y).abs() < 8.0, "{what}: pointer at {p:?}");
}

/// Turning and flipping the acquired image: the picture, the edge letters,
/// what the pointer reads, clicking and the badge agree in every orientation.
#[test]
fn turned_and_flipped_views_stay_true() {
    let mut f = open(Layout::Single);
    let series = f.series();
    let slice = marker_slice(&series);
    f.view_mut().set_slice(STACK, &series, slice);
    f.steps(2);
    let label = format!("Image {} / 48", slice + 1);
    let viewport = f.viewport(&label);
    // The square picture (240 mm) is fitted into the viewport and centred.
    let points_per_mm = viewport.width().min(viewport.height()) / FOV_MM;
    let centre = viewport.center();
    let at = |right_mm: f32, down_mm: f32| centre + vec2(right_mm, down_mm) * points_per_mm;

    // Standard: patient left is on the right of the screen, anterior at the top, so
    // the marker (L 40, A 35) is in the upper right quarter.
    assert_eq!(f.letters(STACK, viewport), ["R", "L", "A", "P"]);
    assert!(!f.shows("Mirrored"));
    assert_reads_marker(&f.read_at(at(40.0, -35.0)), "standard");
    f.snapshot("orient-1-standard");

    // One right turn: what was at the top is on the right.
    f.press(Key::CloseBracket);
    assert_eq!(f.view().transform(STACK).orientation.quarter_turns(), 1);
    assert_eq!(f.letters(STACK, viewport), ["P", "A", "R", "L"]);
    assert_reads_marker(&f.read_at(at(35.0, 40.0)), "turned right");
    assert_ne!(f.read_at(at(40.0, -35.0)).value, Some(CT_MARKER_HU as f32), "the marker has moved away");
    assert!(!f.shows("Mirrored"), "a turned picture is not mirrored");
    assert!(f.shows(&label), "same image");
    f.snapshot("orient-2-turned-right");

    // Flipped horizontally as well: left and right of the screen change places.
    f.press(Key::H);
    assert!(f.view().transform(STACK).orientation.is_mirrored());
    assert_eq!(f.letters(STACK, viewport), ["A", "P", "R", "L"]);
    assert_reads_marker(&f.read_at(at(-35.0, 40.0)), "turned right and flipped");
    assert!(f.shows("Mirrored"));
    f.snapshot("orient-3-turned-and-flipped");

    // The badge is not an annotation: it stays when annotations are hidden.
    f.press(Key::O);
    assert!(!f.harness.state().settings.show_overlays);
    assert!(f.shows("Mirrored"));
    f.snapshot("orient-4-mirrored-annotations-hidden");
    f.press(Key::O);

    // A click goes to the anatomy under the pointer: the reformat crosshair moves there.
    f.click_at(at(-35.0, 40.0));
    let cursor = f.view().cursor;
    assert!((cursor.x - marker().x).abs() < 2.5 && (cursor.y - marker().y).abs() < 2.5, "crosshair at {cursor:?}");

    // Zoomed in on the marker, the pointer still reads it; double-click fits the picture
    // again and keeps its orientation.
    let before = f.view().transform(STACK);
    f.view_mut().transform_mut(STACK).zoom_about(3.0, [-35.0 * points_per_mm, 40.0 * points_per_mm]);
    f.steps(2);
    assert_reads_marker(&f.read_at(at(-35.0, 40.0)), "zoomed about the marker");
    f.double_click_at(at(0.0, 0.0));
    assert_eq!(f.view().transform(STACK), before, "fitted, still turned and mirrored");

    // A second flip, top to bottom: together a half turn of the turned picture, not a mirror.
    f.press(Key::V);
    assert!(!f.view().transform(STACK).orientation.is_mirrored());
    assert_eq!(f.view().transform(STACK).orientation.quarter_turns(), 3);
    assert_eq!(f.letters(STACK, viewport), ["A", "P", "L", "R"]);
    assert!(!f.shows("Mirrored"));
    assert_reads_marker(&f.read_at(at(-35.0, -40.0)), "turned left");

    // A flip alone, then the general reset.
    f.press(Key::CloseBracket);
    f.press(Key::V);
    assert_eq!(f.letters(STACK, viewport), ["R", "L", "P", "A"]);
    assert!(f.shows("Mirrored"));
    assert_reads_marker(&f.read_at(at(40.0, 35.0)), "flipped vertically");
    f.snapshot("orient-5-flipped-vertically");
    f.press(Key::O);
    assert!(f.shows("Mirrored"), "the badge stays when annotations are hidden");
    f.press(Key::O);
    f.press(Key::R);
    assert!(f.view().transform(STACK).orientation.is_standard());
    assert_eq!(f.letters(STACK, viewport), ["R", "L", "A", "P"]);
    assert!(!f.shows("Mirrored"));
    assert_reads_marker(&f.read_at(at(40.0, -35.0)), "after reset");
    assert!(f.harness.unnamed_widgets().is_empty(), "{:?}", f.harness.unnamed_widgets());
}

/// The orientation belongs to one view of one series.
#[test]
fn orientation_does_not_carry_over_to_another_series() {
    let mut f = open(Layout::Single);
    f.press(Key::CloseBracket);
    f.press(Key::H);
    assert!(f.shows("Mirrored"));

    // The MR series opens in the standard orientation, without a badge.
    f.harness.state_mut().viewer.session.select_series(MR);
    f.run_until("the MR", |f| f.viewer().session.loaded(MR).is_some() && f.viewer().views.contains_key(&MR));
    f.steps(3);
    let mr = f.viewer().views.get(&MR).expect("MR view");
    assert!(mr.transform(STACK).orientation.is_standard());
    assert!(!f.shows("Mirrored"));
    // Sagittal acquisition: anterior on the left, superior at the top.
    let viewport = f.viewport("Image ");
    let displayed = &f.viewer().renders.get(MR, STACK).expect("rendered").displayed;
    let l = Placement::of(viewport, displayed, &mr.transform(STACK)).edge_labels(displayed).expect("geometry");
    assert_eq!([l.left.as_str(), &l.right, &l.top, &l.bottom], ["A", "P", "S", "I"]);

    // Back on the CT the view is as the user left it, and says so.
    f.harness.state_mut().viewer.session.select_series(CT);
    f.steps(4);
    assert!(f.view().transform(STACK).orientation.is_mirrored());
    assert!(f.shows("Mirrored"));
}

/// Tilting with the keyboard: the two other planes turn, their views say
/// that they are oblique, and reset brings the standard planes back.
#[test]
fn planes_tilt_by_keyboard_and_reset() {
    let mut f = open(Layout::Mpr);
    let series = f.series();
    f.view_mut().set_cursor(marker(), &series);
    f.steps(2);
    let axial_label = f.viewport_label("Axial ");
    let axial = f.viewport("Axial ");
    assert_eq!(f.letters(AXIAL, axial), ["R", "L", "A", "P"]);
    assert_eq!(f.letters(CORONAL, f.viewport("Coronal ")), ["R", "L", "S", "I"]);
    assert_eq!(f.letters(SAGITTAL, f.viewport("Sagittal ")), ["A", "P", "S", "I"]);
    for kind in [AXIAL, CORONAL, SAGITTAL] {
        assert_eq!(f.value_at_crosshair(kind), Some(CT_MARKER_HU as f32), "{kind:?}: the crosshair is on the marker");
    }
    f.snapshot("tilt-1-standard-planes");

    // The pointer makes the axial view the active one; Alt+Left tilts counter-clockwise.
    f.activate(AXIAL, axial);
    for _ in 0..6 {
        f.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowLeft);
    }
    let cursor = f.view().cursor;
    assert!((f.view().plane_tilt(MprAxis::Coronal).expect("tilted").degrees - 30.0).abs() < 1e-9);
    assert_eq!(f.view().plane_tilt(MprAxis::Axial), None, "the view tilted in stays as it is");
    // The axial view keeps its name and letters; the two others say what they are.
    assert_eq!(f.viewport_label("Axial "), axial_label);
    assert_eq!(f.letters(AXIAL, axial), ["R", "L", "A", "P"]);
    let coronal = f.viewport("Oblique 30.0° · near Coronal ");
    let sagittal = f.viewport("Oblique 30.0° · near Sagittal ");
    assert_eq!(f.letters(CORONAL, coronal), ["RP", "LA", "S", "I"]);
    assert_eq!(f.letters(SAGITTAL, sagittal), ["AR", "PL", "S", "I"]);
    // The crosshair has not moved, and every plane still passes through the marker.
    assert_eq!(f.view().cursor, cursor);
    for kind in [AXIAL, CORONAL, SAGITTAL] {
        assert_eq!(f.value_at_crosshair(kind), Some(CT_MARKER_HU as f32), "{kind:?} after tilting");
    }
    // What the pointer reads in a tilted view is the anatomy drawn there.
    let pos = f.on_screen(CORONAL, coronal, marker());
    assert_reads_marker(&f.read_at(pos), "tilted coronal view");
    f.snapshot("tilt-2-oblique-30");

    // Double oblique: 25° clockwise in the (tilted) coronal view, which stays as it is.
    let coronal_before = f.viewport_label("Oblique 30.0° · near Coronal ");
    f.activate(CORONAL, coronal);
    for _ in 0..5 {
        f.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowRight);
    }
    assert_eq!(f.viewport_label("Oblique 30.0° · near Coronal "), coronal_before);
    let tilted_axial = f.viewport("Oblique 25.0° · near Axial ");
    assert_eq!(f.letters(AXIAL, tilted_axial), ["RS", "LI", "A", "P"]);
    assert_eq!(f.letters(SAGITTAL, f.viewport("Oblique 38.3° · near Sagittal ")), ["AR", "PL", "SL", "IR"]);
    for kind in [AXIAL, CORONAL, SAGITTAL] {
        assert_eq!(f.value_at_crosshair(kind), Some(CT_MARKER_HU as f32), "{kind:?} double oblique");
    }
    assert!(f.harness.unnamed_widgets().is_empty(), "{:?}", f.harness.unnamed_widgets());
    f.snapshot("tilt-3-double-oblique");

    // Scrolling the tilted axial view moves the crosshair along its tilted normal.
    f.activate(AXIAL, tilted_axial);
    let (normal, before) = (f.view().plane_basis(MprAxis::Axial).normal, f.view().cursor);
    assert!((normal.z - 25f64.to_radians().cos()).abs() < 1e-9 && normal.x.abs() > 0.2);
    f.press(Key::ArrowDown);
    f.press(Key::ArrowDown);
    let moved = f.view().cursor - before;
    assert!(moved.length() > 4.0 && moved.cross(normal).length() < 1e-9, "moved by {moved:?}");

    // Single steps: three times Alt+Right and once with Shift make 8°.
    f.press_with(Modifiers::ALT, Key::R);
    assert!(!f.view().planes_tilted());
    f.view_mut().set_cursor(marker(), &series);
    f.steps(2);
    let axial_label = f.viewport_label("Axial ");
    let coronal_label = f.viewport_label("Coronal ");
    let cursor = f.view().cursor;
    f.activate(AXIAL, axial);
    for _ in 0..3 {
        f.press_with(Modifiers::ALT, Key::ArrowRight);
    }
    f.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowRight);
    let small = f.viewport("Oblique 8.0° · near Coronal ");
    // A direction gets a second letter only from about 15°.
    assert_eq!(f.letters(CORONAL, small), ["R", "L", "S", "I"]);
    // The tilt keys do not scroll.
    assert_eq!(f.viewport_label("Axial "), axial_label);

    // With the axial view turned right, Alt+R resets the planes only…
    f.press(Key::CloseBracket);
    assert_eq!(f.letters(AXIAL, axial), ["P", "A", "R", "L"]);
    f.press_with(Modifiers::ALT, Key::R);
    assert!(!f.view().planes_tilted());
    assert_eq!(f.viewport_label("Coronal "), coronal_label);
    assert_eq!(f.letters(AXIAL, axial), ["P", "A", "R", "L"]);
    assert_eq!(f.view().cursor, cursor, "resetting the planes keeps the crosshair");
    // …and R resets the orientation too.
    for _ in 0..4 {
        f.press_with(Modifiers::ALT, Key::ArrowLeft);
    }
    assert!(f.view().planes_tilted());
    f.press(Key::R);
    assert!(!f.view().planes_tilted());
    assert_eq!(f.letters(AXIAL, axial), ["R", "L", "A", "P"]);
    assert_eq!(f.viewport_label("Sagittal ").split(' ').next(), Some("Sagittal"));
}

/// Angle on screen (degrees, clockwise from screen-right, modulo 180) of the
/// line through two handle centres.
fn line_angle(handles: &[Pos2]) -> f32 {
    let d = handles[0] - handles[1];
    d.y.atan2(d.x).to_degrees().rem_euclid(180.0)
}

/// Whether `point` lies on the line through two handle centres, between them.
fn between(handles: &[Pos2], point: Pos2) -> bool {
    let (a, b) = (handles[0], handles[1]);
    let along = (b - a).normalized();
    let offset = point - a;
    let distance = (offset.x * along.y - offset.y * along.x).abs();
    let position = offset.dot(along);
    distance < 0.75 && position > 0.0 && position < (b - a).length()
}

/// Distance of `point` from the (endless) line through two handle centres.
fn distance_from_line(handles: &[Pos2], point: Pos2) -> f32 {
    let along = (handles[1] - handles[0]).normalized();
    let offset = point - handles[0];
    (offset.x * along.y - offset.y * along.x).abs()
}

/// Dragging a handle: the line follows the pointer, the two other planes
/// turn, and the view dragged in stays as it is.
#[test]
fn dragging_a_handle_tilts_the_planes() {
    let mut f = open(Layout::Mpr);
    let series = f.series();
    f.view_mut().set_cursor(marker(), &series);
    f.steps(2);
    let axial = f.viewport("Axial ");
    let crosshair = f.on_screen(AXIAL, axial, marker());

    // Each line has a handle on either side of the crosshair: the coronal line is
    // horizontal, the sagittal one vertical.
    let coronal = f.handles("Coronal", "Axial");
    let sagittal = f.handles("Sagittal", "Axial");
    assert_eq!((coronal.len(), sagittal.len()), (2, 2));
    assert!((coronal[0].y - crosshair.y).abs() < 0.5 && (coronal[1].y - crosshair.y).abs() < 0.5);
    assert!((sagittal[0].x - crosshair.x).abs() < 0.5 && (sagittal[1].x - crosshair.x).abs() < 0.5);
    for handle in coronal.iter().chain(&sagittal) {
        assert!(axial.contains(*handle));
    }
    assert!(between(&coronal, crosshair) && between(&sagittal, crosshair), "the lines run through the crosshair");

    // Drag the right-hand handle of the coronal line 30° clockwise around the crosshair.
    let start = if coronal[0].x > crosshair.x { coronal[0] } else { coronal[1] };
    let radius = (start - crosshair).length();
    let at = |degrees: f32| crosshair + radius * Vec2::angled(degrees.to_radians());
    f.harness.hover_at(start);
    f.steps(2);
    f.harness.drag_at(start);
    f.steps(1);
    for step in 1..=30 {
        f.harness.hover_at(at(step as f32));
        f.steps(1);
    }
    f.steps(1);
    // Held: the line is under the pointer, the other line perpendicular to it.
    let coronal = f.handles("Coronal", "Axial");
    let sagittal = f.handles("Sagittal", "Axial");
    let pointer = at(30.0);
    assert!(distance_from_line(&coronal, pointer) < 1.5, "line through {coronal:?}, pointer at {pointer:?}");
    assert!(between(&coronal, crosshair) && between(&sagittal, crosshair));
    assert!((line_angle(&coronal) - 30.0).abs() < 0.5, "{}", line_angle(&coronal));
    assert!((line_angle(&sagittal) - 120.0).abs() < 0.5, "{}", line_angle(&sagittal));
    f.snapshot("tilt-4-dragging-a-handle");
    f.harness.drop_at(at(30.0));
    f.steps(3);

    // The two other planes are turned by 30°; the axial view is unchanged.
    let tilt = f.view().plane_tilt(MprAxis::Coronal).expect("tilted");
    assert!((tilt.degrees - 30.0).abs() < 0.5, "{}", tilt.degrees);
    assert_eq!(f.view().plane_tilt(MprAxis::Axial), None);
    assert_eq!(f.view().cursor, {
        let mut state = ViewState::new(&series);
        state.set_cursor(marker(), &series);
        state.cursor
    });
    assert_eq!(f.letters(AXIAL, axial), ["R", "L", "A", "P"]);
    assert_eq!(f.viewer().active_view, AXIAL, "working with a handle makes its view the active one");
    // In the two other views the lines are still horizontal and vertical.
    let lines = f.handles("Axial", "Coronal");
    assert!((line_angle(&lines) - 0.0).abs() < 0.5 || (line_angle(&lines) - 180.0).abs() < 0.5, "{lines:?}");
    let lines = f.handles("Coronal", "Sagittal");
    assert!((line_angle(&lines) - 90.0).abs() < 0.5, "{lines:?}");
    // Clockwise in the axial view: screen-right of the coronal view points left-posterior.
    let coronal_view = f.viewport("Oblique ");
    assert!(f.viewport_label("Oblique ").contains("· near Coronal "));
    assert_eq!(f.letters(CORONAL, coronal_view), ["RA", "LP", "S", "I"]);

    // A click on a handle is not a click on the picture: the crosshair stays.
    let cursor = f.view().cursor;
    let handle = f.handles("Sagittal", "Axial")[0];
    f.click_at(handle);
    assert_eq!(f.view().cursor, cursor);
    assert!((f.view().plane_tilt(MprAxis::Coronal).expect("tilted").degrees - tilt.degrees).abs() < 1e-9);
    // A click beside it moves the crosshair to the anatomy under the pointer.
    let target = marker() + DVec3::new(-30.0, 20.0, 0.0);
    f.click_at(f.on_screen(AXIAL, axial, target));
    let cursor = f.view().cursor;
    assert!((cursor - target).length() < 2.5, "crosshair at {cursor:?}, clicked {target:?}");
    // The lines run through the new crosshair, at the same angle.
    let coronal = f.handles("Coronal", "Axial");
    assert!(between(&coronal, f.on_screen(AXIAL, axial, cursor)));
    assert!((line_angle(&coronal) - 30.0).abs() < 0.5);

    // Assistive technology turns the planes by one degree per step.
    let before = f.view().plane_tilt(MprAxis::Coronal).expect("tilted").degrees;
    let label = "Tilt handle of the Coronal plane in the Axial view";
    f.harness.query_all_by_label(label).next().expect("handle").increment_accesskit();
    f.steps(2);
    let after = f.view().plane_tilt(MprAxis::Coronal).expect("tilted").degrees;
    assert!((after - before - 1.0).abs() < 1e-6, "{before} -> {after}");
    assert!(f.harness.unnamed_widgets().is_empty(), "{:?}", f.harness.unnamed_widgets());

    // Handles belong to the annotations: hidden with them, back with them.
    f.press(Key::O);
    assert!(f.handles("Coronal", "Axial").is_empty());
    assert!(f.viewport_label("Oblique ").starts_with("Oblique"), "the title still says that the view is oblique");
    f.snapshot("tilt-5-oblique-annotations-hidden");
    f.press(Key::O);
    assert_eq!(f.handles("Coronal", "Axial").len(), 2);
}

/// In a mirrored view the handle still follows the pointer, and the tilt
/// keys still turn the lines the way the arrow points.
#[test]
fn tilting_in_a_mirrored_and_turned_view() {
    let mut f = open(Layout::Mpr);
    let series = f.series();
    f.view_mut().set_cursor(marker(), &series);
    f.steps(2);
    let axial = f.viewport("Axial ");
    f.activate(AXIAL, axial);
    f.press(Key::H);
    f.press(Key::CloseBracket);
    assert!(f.view().transform(AXIAL).orientation.is_mirrored());
    assert!(f.shows("Mirrored"));
    assert_eq!(f.letters(AXIAL, axial), ["P", "A", "L", "R"]);
    // The lines are drawn through the same mapping as the picture: the coronal line,
    // left-right in the patient, runs top to bottom now.
    let coronal = f.handles("Coronal", "Axial");
    assert!((line_angle(&coronal) - 90.0).abs() < 0.5, "{coronal:?}");
    let crosshair = f.on_screen(AXIAL, axial, marker());
    assert!(between(&coronal, crosshair));
    // The crosshair is on the marker, which the pointer reads there.
    assert_reads_marker(&f.read_at(crosshair + vec2(3.0, 3.0)), "mirrored axial reformat");

    // Alt+Right turns the lines clockwise on screen: 90° -> 100°.
    f.activate(AXIAL, axial);
    for _ in 0..2 {
        f.press_with(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowRight);
    }
    let coronal = f.handles("Coronal", "Axial");
    assert!((line_angle(&coronal) - 100.0).abs() < 0.5, "{}", line_angle(&coronal));
    assert!((f.view().plane_tilt(MprAxis::Coronal).expect("tilted").degrees - 10.0).abs() < 1e-9);

    // Dragging a handle 20° counter-clockwise takes the line along: 100° -> 80°.
    let start = coronal[0];
    let radius = (start - crosshair).length();
    let from = (start - crosshair).angle().to_degrees();
    let at = |degrees: f32| crosshair + radius * Vec2::angled((from - degrees).to_radians());
    f.harness.hover_at(start);
    f.steps(2);
    f.harness.drag_at(start);
    f.steps(1);
    for step in 1..=20 {
        f.harness.hover_at(at(step as f32));
        f.steps(1);
    }
    f.harness.drop_at(at(20.0));
    f.steps(3);
    f.hover_at(axial.center() + vec2(-90.0, 70.0));
    let coronal = f.handles("Coronal", "Axial");
    assert!((line_angle(&coronal) - 80.0).abs() < 0.5, "{}", line_angle(&coronal));
    assert!((f.view().plane_tilt(MprAxis::Coronal).expect("tilted").degrees - 10.0).abs() < 0.5);
    for kind in [AXIAL, CORONAL, SAGITTAL] {
        assert_eq!(f.value_at_crosshair(kind), Some(CT_MARKER_HU as f32), "{kind:?}");
    }
    f.snapshot("tilt-6-mirrored-view-tilted");
}

/// The toolbar menu does what the keys do, and stays open while it is used.
#[test]
fn toolbar_menu_turns_flips_and_resets_the_planes() {
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        let mut f = open_with(Settings { default_layout: Layout::Mpr, ..Settings::default() }, theme);
        let series = f.series();
        f.view_mut().set_cursor(marker(), &series);
        f.steps(2);
        let axial = f.viewport("Axial ");
        // On its way to the toolbar the pointer crosses the coronal view, which
        // becomes the active one: the menu says so and lets the user pick the view.
        let coronal = f.viewport("Coronal ");
        f.activate(CORONAL, coronal);
        f.harness.get_by_label("Rotate and flip").click();
        f.steps(3);
        let selected = |f: &Fixture, view: &str| f.harness.get_by_label(view).accesskit_node().toggled();
        assert_eq!(selected(&f, "Coronal"), Some(Toggled::True));
        f.harness.get_by_label("Axial").click();
        f.steps(3);
        assert_eq!(selected(&f, "Axial"), Some(Toggled::True));
        assert_eq!(selected(&f, "Coronal"), Some(Toggled::False));
        assert_eq!(f.viewer().active_view, AXIAL);
        // Nothing to reset yet.
        assert!(f.harness.get_by_label("Reset planes").accesskit_node().is_disabled());

        f.harness.get_by_label("Rotate right").click();
        f.steps(3);
        assert_eq!(f.letters(AXIAL, axial), ["P", "A", "R", "L"]);
        // The menu is still open for the next step.
        f.harness.get_by_label("Flip vertically").click();
        f.steps(3);
        assert_eq!(f.letters(AXIAL, axial), ["P", "A", "L", "R"]);
        assert!(f.shows("Mirrored"));
        f.harness.get_by_label("Flip horizontally").click();
        f.steps(3);
        assert_eq!(f.letters(AXIAL, axial), ["A", "P", "L", "R"]);
        assert!(!f.shows("Mirrored"), "two flips are a half turn");
        f.harness.get_by_label("Rotate left").click();
        f.steps(3);
        assert_eq!(f.letters(AXIAL, axial), ["L", "R", "P", "A"]);
        assert_eq!(f.view().transform(AXIAL).orientation.quarter_turns(), 2);
        // Only the active view was turned.
        assert!(f.view().transform(CORONAL).orientation.is_standard());

        // Tilted planes can be reset from the menu.
        f.view_mut().tilt_planes(MprAxis::Axial, (-30f64).to_radians());
        f.view_mut().tilt_planes(MprAxis::Coronal, 25f64.to_radians());
        f.steps(3);
        assert!(!f.harness.get_by_label("Reset planes").accesskit_node().is_disabled());
        assert!(f.harness.unnamed_widgets().is_empty(), "{:?}", f.harness.unnamed_widgets());
        f.harness.get_by_label("Flip horizontally").hover();
        f.steps(3);
        f.snapshot(&format!("menu-{name}"));
        f.harness.get_by_label("Reset planes").click();
        f.steps(3);
        assert!(!f.view().planes_tilted());
        assert_eq!(f.view().transform(AXIAL).orientation.quarter_turns(), 2, "the planes only");

        // The pointer over the open menu does not reach the view underneath.
        f.harness.get_by_label("Flip vertically").hover();
        f.steps(2);
        assert_eq!(f.viewer().active_view, AXIAL);
        assert!(f.viewer().hover.is_none(), "nothing is read through the menu");

        // A click elsewhere closes the menu.
        let sagittal = f.viewport("Sagittal ");
        f.click_at(sagittal.center() + vec2(-90.0, 70.0));
        f.steps(2);
        assert!(f.harness.query_all_by_label("Rotate right").next().is_none());
    }
}

/// In the acquired-images layout there are no planes to reset.
#[test]
fn menu_in_the_single_layout_offers_turn_and_flip_only() {
    let mut f = open(Layout::Single);
    f.harness.get_by_label("Rotate and flip").click();
    f.steps(3);
    // One view: nothing to choose from.
    assert!(f.harness.query_all_by_label("Acquired").next().is_none());
    assert!(f.harness.get_by_label("Reset planes").accesskit_node().is_disabled());
    f.harness.get_by_label("Flip horizontally").click();
    f.steps(3);
    assert!(f.view().transform(STACK).orientation.is_mirrored());
    assert!(f.shows("Mirrored"));
    f.snapshot("menu-single-layout-mirrored");
    // Tilt keys do nothing to the planes without a reformat view.
    f.press_with(Modifiers::ALT, Key::ArrowRight);
    assert!(!f.view().planes_tilted());
}

/// Turning or flipping the acquired image while the series loads is the
/// user's doing: a remembered reformat layout does not take over that view
/// when loading completes.
#[test]
fn a_view_turned_while_loading_is_not_taken_over_by_the_remembered_layout() {
    let gate = Arc::new(Gate::default());
    let settings = Settings { default_layout: Layout::Mpr, ..Settings::default() };
    let mut f = start(settings, Theme::Dark, gate.clone());
    gate.allow(3);
    f.run_until("three images", |f| {
        f.viewer().session.data(CT).is_some_and(|series| series.loaded() == 3) && gate.started() == 4
    });
    f.steps(2);
    assert!(f.shows("Image 25 / 48, Loading 3 / 48"), "acquired images while loading");

    f.press(Key::H);
    assert!(f.view().transform(STACK).orientation.is_mirrored());
    assert!(!f.view().is_pristine(&f.viewer().session.data(CT).expect("loading")));
    gate.allow_all();
    f.wait_until_loaded();

    // Nothing changes under the user's hands; the reformats are on offer, and the
    // saved preference is untouched.
    assert_eq!(f.viewer().layout, Layout::Single);
    assert!(f.shows("Image 25 / 48") && f.shows("Mirrored"));
    let reformat_on_screen = f.harness.query_all_by_label_contains("Axial ").any(|node| {
        let node = node.accesskit_node();
        node.role() == Role::Image && node.label().is_some_and(|label| label.starts_with("Axial "))
    });
    assert!(!reformat_on_screen, "no reformat view took over");
    assert_eq!(f.harness.state().settings.default_layout, Layout::Mpr);
}
