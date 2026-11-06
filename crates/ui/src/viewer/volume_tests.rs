//! The 3D layout in the viewer, driven offscreen through the pointer, the
//! keyboard and the toolbar. Set `SUMMRISE_SNAPSHOT_DIR` to keep renderings
//! (`3d-*.png`) for review.
//!
//! The synthetic CT has a marker of 300 HU at L 40 mm, A 35 mm, S 50 mm, a
//! spine-like cylinder of 700 HU and a contrast-filled vessel of 300 HU.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, Pos2, Rect, Theme, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use summrise_core::dicom::{DecodedInstance, DicomError, InstanceHeader, Media, Progress, scan};
use summrise_core::imaging::render3d::{Look, Quality, StandardView};
use summrise_core::imaging::{DQuat, DVec3};
use summrise_core::{DicomFiles, ImageSource, Layout, SeriesKey, Session, Settings, ViewKind, ViewState, Waker};
use summrise_testkit::{MARKER_POSITION, PhantomOptions, write_phantom_study};

use super::Viewer;
use crate::app::{Action, Toasts};
use crate::theme;

const CT: SeriesKey = SeriesKey { study: 0, series: 0 };
const MR: SeriesKey = SeriesKey { study: 0, series: 1 };
const DOSE_REPORT: SeriesKey = SeriesKey { study: 0, series: 3 };
const CAPTION: &str = "Illustration: colours and surfaces are computed";

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
    _dir: tempfile::TempDir,
}

fn snapshot_dir() -> Option<PathBuf> {
    std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from)
}

fn marker() -> DVec3 {
    DVec3::from_array(MARKER_POSITION)
}

/// Reads files only as far as the test lets it.
#[derive(Default)]
struct Gate {
    allowed: AtomicUsize,
    started: AtomicUsize,
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

/// The viewer on the synthetic CD with the 3D layout remembered, the CT loaded completely.
fn open(theme: Theme) -> Fixture {
    let mut fixture =
        start(Settings { default_layout: Layout::Volume, ..Settings::default() }, theme, Arc::new(DicomFiles));
    fixture.run_until("the CT", |f| f.viewer().session.loaded(CT).is_some());
    fixture.wait_for_full_picture();
    fixture
}

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
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn steps(&mut self, n: usize) {
        self.harness.run_steps(n);
    }

    fn viewer(&self) -> &Viewer {
        &self.harness.state().viewer
    }

    fn view(&self, key: SeriesKey) -> &ViewState {
        self.viewer().views.get(&key).expect("the series is on screen")
    }

    fn view_mut(&mut self, key: SeriesKey) -> &mut ViewState {
        self.harness.state_mut().viewer.views.get_mut(&key).expect("the series is on screen")
    }

    /// The camera and quality of the 3D picture on screen.
    fn shown(&self) -> (summrise_core::imaging::Camera, Quality, Look) {
        let shown = self.viewer().volume_view.shown().expect("a 3D picture is shown");
        (shown.request.camera, shown.quality, shown.request.style.look)
    }

    /// Runs frames until the full picture of the current state is on screen.
    fn wait_for_full_picture(&mut self) {
        self.run_until("the full 3D picture", |f| {
            let key = f.viewer().session.active_series.expect("a series is selected");
            f.viewer().volume_view.shown().is_some_and(|shown| {
                shown.quality == Quality::Full
                    && f.viewer().views.get(&key).is_some_and(|view| {
                        view.camera_orientation() == shown.request.camera.orientation
                            && view.look == shown.request.style.look
                    })
            })
        });
        self.steps(2);
    }

    /// Edge letters (left, right, top, bottom) of the 3D picture on screen.
    fn letters(&self) -> [String; 4] {
        let l = self.shown().0.edge_labels();
        [l.left, l.right, l.top, l.bottom]
    }

    /// Whether something on screen is labelled exactly `label`.
    fn shows(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }

    fn is_enabled(&self, button: &str) -> bool {
        !self.harness.get_by_label(button).accesskit_node().is_disabled()
    }

    /// Screen rectangle of the 3D viewport.
    fn viewport(&self) -> Rect {
        self.harness
            .query_all_by_label_contains("3D view: ")
            .find(|node| node.accesskit_node().role() == Role::Image)
            .expect("the 3D view")
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

    /// Drags with the primary button from `from` by `by`, in `steps` moves,
    /// running `during` after each move; the button is released at the end.
    fn drag(&mut self, from: Pos2, by: egui::Vec2, steps: usize, mut during: impl FnMut(&mut Self)) {
        self.harness.hover_at(from);
        self.steps(2);
        self.harness.drag_at(from);
        self.steps(1);
        for step in 1..=steps {
            self.harness.hover_at(from + by * (step as f32 / steps as f32));
            self.harness.step();
            during(self);
        }
        self.harness.drop_at(from + by);
        self.steps(2);
    }

    fn snapshot(&mut self, name: &str) {
        if let Some(dir) = snapshot_dir() {
            let image = self.harness.render().expect("wgpu render");
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            image.save(dir.join(format!("{name}.png"))).expect("save png");
        }
    }
}

/// The "3D" layout is offered under the same condition as the reformat
/// layouts, with the same reason while it is not available.
#[test]
fn the_3d_layout_is_offered_when_the_series_forms_a_volume() {
    let gate = Arc::new(Gate::default());
    let mut f = start(Settings::default(), Theme::Dark, gate.clone());
    gate.allowed.store(3, Ordering::SeqCst);
    f.run_until("three images", |f| {
        f.viewer().session.data(CT).is_some_and(|series| series.loaded() == 3)
            && gate.started.load(Ordering::SeqCst) == 4
    });
    f.steps(2);
    assert!(!f.is_enabled("3D") && !f.is_enabled("MPR"));
    f.harness.get_by_label("3D").hover();
    f.harness.run_steps(8);
    assert!(f.shows("Loading 3 / 48"), "the tooltip of the disabled button gives the progress");
    f.snapshot("3d-0-loading-tooltip");

    gate.allowed.store(usize::MAX, Ordering::SeqCst);
    f.run_until("the CT", |f| f.viewer().session.loaded(CT).is_some());
    f.steps(2);
    assert!(f.is_enabled("3D"));
    f.harness.get_by_label("3D").click();
    f.steps(3);
    assert!(f.shows("3D view: Bone"));
    assert_eq!(f.harness.state().settings.default_layout, Layout::Volume, "remembered like the other layouts");

    // Colour images form no volume: the layout is disabled with the reason.
    f.harness.state_mut().viewer.session.select_series(DOSE_REPORT);
    f.run_until("the dose report", |f| f.viewer().session.loaded(DOSE_REPORT).is_some());
    f.steps(3);
    assert!(!f.is_enabled("3D"));
    assert!(!f.shows("3D view: Bone"), "the acquired image instead");
    f.harness.get_by_label("3D").hover();
    f.harness.run_steps(8);
    assert!(f.harness.query_all_by_label_contains("colour images cannot be reformatted").next().is_some());
}

/// The first view is from the front with the head up: the letters, the
/// marker, the guide and the caption say so.
#[test]
fn the_3d_view_opens_from_the_front_and_tells_the_truth_about_it() {
    let mut f = open(Theme::Dark);
    assert!(f.shows("3D view: Bone") && f.shows(CAPTION));
    assert_eq!(f.letters(), ["R", "L", "S", "I"]);
    assert_eq!(f.shown().0.orientation, DQuat::IDENTITY);
    // The guide describes the direction of the camera, with no image marked in the figure.
    assert!(f.shows("3D view"));
    assert!(f.shows("You are looking from the front of the body towards the back."));
    assert!(f.shows("The patient's right side is on the left of the screen."));
    assert!(f.shows("Schematic body outline: the examined region is highlighted."));
    assert!(!f.shows("Coronal image") && !f.shows("Axial image"));
    // The 3D view has its own toolbar control; turning and flipping a picture does not apply.
    assert!(f.shows("Look and standard views") && !f.shows("Rotate and flip"));
    assert!(f.harness.unnamed_widgets().is_empty(), "{:?}", f.harness.unnamed_widgets());
    f.snapshot("3d-1-ct-bone");

    // MIP: the marker on the patient's left and towards the head appears right of the
    // centre and above it, at the place the camera gives for it.
    let ct = f.viewer().session.loaded(CT).expect("CT");
    f.view_mut(CT).set_look(Look::Mip, &ct);
    f.wait_for_full_picture();
    assert!(f.shows("3D view: MIP"));
    let shown = f.viewer().volume_view.shown().expect("picture");
    let (width, height) = (shown.picture.width, shown.picture.height);
    let (x, y) = shown.request.camera.to_pixel(marker(), [width, height]);
    assert!(x > width as f64 / 2.0 + 20.0 && y < height as f64 / 2.0 - 20.0, "marker at ({x}, {y})");
    let pixel = shown.picture.pixel(x.round() as usize, y.round() as usize).expect("inside");
    assert!(pixel[0] >= 250 && pixel[3] == 255, "300 HU in the window W 400 / L 40 is white: {pixel:?}");
    f.snapshot("3d-2-ct-mip");

    for (look, name) in [(Look::Skin, "3d-3-ct-skin"), (Look::VesselsAndBone, "3d-4-ct-vessels-and-bone")] {
        f.view_mut(CT).set_look(look, &ct);
        f.wait_for_full_picture();
        assert!(f.shows(&format!("3D view: {}", look.label())));
        f.snapshot(name);
    }

    // The caption is not an annotation: it stays when annotations are hidden.
    f.press(Key::O);
    assert!(f.shows(CAPTION));
    f.snapshot("3d-5-annotations-hidden");
    f.press(Key::O);
}

/// While the object is dragged every frame shows a preview of that frame's
/// view; when the pointer rests the full picture replaces it. Half a turn
/// about the vertical axis shows the back, with left and right swapped.
#[test]
fn dragging_turns_the_object_with_previews_and_the_full_picture_follows() {
    let mut f = open(Theme::Dark);
    let viewport = f.viewport();
    // 360 points to the right at half a degree per point: half a turn.
    let mut previews = 0;
    f.drag(viewport.center() - vec2(180.0, 0.0), vec2(360.0, 0.0), 24, |f| {
        let (camera, quality, _) = f.shown();
        if camera.orientation != DQuat::IDENTITY {
            assert_eq!(quality, Quality::Preview, "a preview while the object is turned");
            assert_eq!(camera.orientation, f.view(CT).camera_orientation(), "the picture of this frame");
            previews += 1;
        }
    });
    assert!(previews >= 20, "{previews} previews");
    assert_eq!(f.letters(), ["L", "R", "S", "I"], "seen from the back: the patient's left on the left");
    assert!(!f.view(CT).is_pristine(&f.viewer().session.loaded(CT).expect("CT")));
    f.wait_for_full_picture();
    assert_eq!(f.letters(), ["L", "R", "S", "I"]);
    // The guide says so too.
    assert!(f.shows("You are looking from the back of the body towards the front."));
    assert!(f.shows("The patient's left side is on the left of the screen."));
    f.snapshot("3d-6-turned-to-the-back");

    // Shift-drag pans, the right button changes the contrast, neither turns.
    let orientation = f.view(CT).camera_orientation();
    let window = f.view(CT).window;
    f.harness.hover_at(viewport.center());
    f.harness.event(Event::ModifiersChanged(Modifiers::SHIFT));
    f.steps(1);
    f.harness.event(Event::PointerButton {
        pos: viewport.center(),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::SHIFT,
    });
    f.steps(1);
    for step in 1..=10 {
        f.harness.hover_at(viewport.center() + vec2(4.0 * step as f32, 3.0 * step as f32));
        f.harness.step();
    }
    f.harness.event(Event::PointerButton {
        pos: viewport.center() + vec2(40.0, 30.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::SHIFT,
    });
    f.harness.event(Event::ModifiersChanged(Modifiers::NONE));
    f.steps(2);
    let pan = f.view(CT).transform(ViewKind::Volume).pan;
    assert!(pan[0] > 20.0 && pan[1] > 15.0, "panned by {pan:?}");
    assert_eq!(f.view(CT).camera_orientation(), orientation, "panning does not turn");
    assert_eq!(f.view(CT).window, window, "nor change the contrast");
    // Double-click fits the picture again.
    let centre = viewport.center();
    let button = |pressed| Event::PointerButton {
        pos: centre,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    f.harness.input_mut().events.extend([
        Event::PointerMoved(centre),
        button(true),
        button(false),
        button(true),
        button(false),
    ]);
    f.steps(3);
    assert_eq!(f.view(CT).transform(ViewKind::Volume).pan, [0.0, 0.0]);
    assert_eq!(f.view(CT).camera_orientation(), orientation, "fitting keeps the turn");
}

/// The arrow keys turn the object, Shift with an orientation letter shows a
/// standard view, L chooses the next look, R resets; the keys that turn and
/// flip a picture and the slice keys do nothing in the 3D view.
#[test]
fn keys_turn_the_object_show_standard_views_and_reset() {
    let mut f = open(Theme::Dark);
    let slice = f.view(CT).slice;
    // Nine steps of 10° to the right: seen from the patient's right side.
    for _ in 0..9 {
        f.press(Key::ArrowRight);
    }
    assert_eq!(f.letters(), ["P", "A", "S", "I"]);
    assert_eq!(f.view(CT).standard_view(), Some(StandardView::Right));
    // Two steps of 45° with Shift: half a turn in all, seen from the back.
    f.press_with(Modifiers::SHIFT, Key::ArrowRight);
    f.press_with(Modifiers::SHIFT, Key::ArrowRight);
    assert_eq!(f.letters(), ["L", "R", "S", "I"]);
    assert_eq!(f.view(CT).slice, slice, "the arrow keys do not scroll the acquired images");

    for (letter, view, letters) in [
        (Key::A, StandardView::Front, ["R", "L", "S", "I"]),
        (Key::P, StandardView::Back, ["L", "R", "S", "I"]),
        (Key::L, StandardView::Left, ["A", "P", "S", "I"]),
        (Key::R, StandardView::Right, ["P", "A", "S", "I"]),
        (Key::S, StandardView::Top, ["L", "R", "A", "P"]),
        (Key::I, StandardView::Bottom, ["R", "L", "A", "P"]),
    ] {
        f.press_with(Modifiers::SHIFT, letter);
        assert_eq!(f.view(CT).standard_view(), Some(view), "Shift+{letter:?}");
        assert_eq!(f.letters(), letters, "Shift+{letter:?}");
    }
    assert!(!f.view(CT).invert, "Shift+I is a view, not the inversion");
    assert_eq!(f.view(CT).look, Look::Bone, "Shift+L is a view, not the next look");

    // From the front, nine steps up: the near side goes up, seen from below the feet.
    f.press_with(Modifiers::SHIFT, Key::A);
    for _ in 0..9 {
        f.press(Key::ArrowUp);
    }
    assert_eq!(f.letters(), ["R", "L", "A", "P"]);

    // The keys that turn and flip a picture do not apply: the camera covers rotation.
    let orientation = f.view(CT).camera_orientation();
    for key in [Key::CloseBracket, Key::OpenBracket, Key::H, Key::V] {
        f.press(key);
    }
    assert_eq!(f.view(CT).camera_orientation(), orientation);
    assert!(f.view(CT).transform(ViewKind::Volume).orientation.is_standard());
    assert!(!f.shows("Mirrored"));

    // L goes through the looks; R resets look, turn and zoom.
    f.press(Key::L);
    assert_eq!(f.view(CT).look, Look::Skin);
    f.run_until("the skin picture", |f| f.shows("3D view: Skin"));
    f.view_mut(CT).transform_mut(ViewKind::Volume).zoom_about(2.0, [0.0, 0.0]);
    f.press(Key::R);
    assert_eq!((f.view(CT).camera_orientation(), f.view(CT).look), (DQuat::IDENTITY, Look::Bone));
    assert!(f.view(CT).is_pristine(&f.viewer().session.loaded(CT).expect("CT")));
    f.wait_for_full_picture();
    assert_eq!(f.letters(), ["R", "L", "S", "I"]);
    assert!(f.shows("3D view: Bone"));
}

/// Pinching zooms about the pointer, as in the other views.
#[test]
fn zoom_works_as_in_the_other_views() {
    let mut f = open(Theme::Dark);
    let viewport = f.viewport();
    f.harness.hover_at(viewport.center());
    f.steps(1);
    f.harness.event(Event::Zoom(1.6));
    f.steps(3);
    let zoom = f.view(CT).transform(ViewKind::Volume).zoom;
    assert!(zoom > 1.2, "zoomed in to {zoom}");
    // The full picture of the zoomed view: half the field, the same turn.
    f.wait_for_full_picture();
    let (camera, _, _) = f.shown();
    let fitted = f.view(CT).volume_camera(
        &f.viewer().session.loaded(CT).expect("CT").volume().expect("volume").geometry,
        [viewport.width(), viewport.height()],
    );
    assert_eq!(camera, fitted);
    assert!(f.view(CT).slice_count(ViewKind::Volume, &f.viewer().session.loaded(CT).expect("CT")) == 0);
}

/// MR has no Hounsfield units: Surface and MIP are offered, and both follow
/// the contrast window.
#[test]
fn mr_offers_surface_and_mip_which_follow_the_window() {
    let mut f = open(Theme::Dark);
    f.harness.state_mut().viewer.session.select_series(MR);
    f.run_until("the MR", |f| f.viewer().session.loaded(MR).is_some() && f.viewer().views.contains_key(&MR));
    f.wait_for_full_picture();
    assert!(f.shows("3D view: Surface"));
    assert_eq!(f.letters(), ["R", "L", "S", "I"], "the MR opens from the front too");
    f.harness.get_by_label("Look and standard views").click();
    f.steps(3);
    assert!(f.shows("Surface") && f.shows("MIP") && !f.shows("Bone") && !f.shows("Skin"));
    assert!(f.shows("From the front"));
    f.snapshot("3d-7-mr-look-menu");
    f.harness.key_press(Key::Escape);
    f.steps(2);
    f.snapshot("3d-8-mr-surface");

    // The second stored window: the surface follows it.
    let before = f.shown().0;
    f.press(Key::Num2);
    let window = f.view(MR).window;
    assert_eq!((window.center, window.width), (900.0, 1000.0));
    f.wait_for_full_picture();
    let shown = f.viewer().volume_view.shown().expect("picture");
    assert_eq!(shown.request.style.window, window);
    assert_eq!(shown.request.camera, before, "the same view, another window");
    f.snapshot("3d-9-mr-surface-narrow-window");

    f.press(Key::L);
    f.wait_for_full_picture();
    assert!(f.shows("3D view: MIP"));
    f.snapshot("3d-10-mr-mip");
    // The CT keeps its own 3D view.
    f.harness.state_mut().viewer.session.select_series(CT);
    f.steps(3);
    f.wait_for_full_picture();
    assert!(f.shows("3D view: Bone"));
}

/// The toolbar menu of the 3D view, in the dark and the light theme: the
/// looks and the standard views, and it stays open while it is used.
#[test]
fn toolbar_menu_in_both_themes() {
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        let mut f = open(theme);
        f.harness.get_by_label("Look and standard views").click();
        f.steps(3);
        for look in [Look::Bone, Look::Skin, Look::VesselsAndBone, Look::Mip] {
            assert!(f.shows(look.label()), "{look:?}");
        }
        assert!(!f.shows("Surface"), "not for CT");
        for view in StandardView::ALL {
            assert!(f.shows(view.label()), "{view:?}");
        }
        f.harness.get_by_label("From the patient's left").hover();
        f.steps(3);
        assert!(f.harness.unnamed_widgets().is_empty(), "{:?}", f.harness.unnamed_widgets());
        f.snapshot(&format!("3d-11-menu-{name}"));
        f.harness.get_by_label("From the patient's left").click();
        f.steps(3);
        assert_eq!(f.view(CT).standard_view(), Some(StandardView::Left));
        // Still open: the look is chosen in the same menu.
        f.harness.get_by_label("Vessels and bone").click();
        f.steps(3);
        assert_eq!(f.view(CT).look, Look::VesselsAndBone);
        f.harness.key_press(Key::Escape);
        f.steps(2);
        assert!(!f.shows("From the patient's left"), "closed");
        f.wait_for_full_picture();
        assert_eq!(f.letters(), ["A", "P", "S", "I"]);
        assert!(f.shows("3D view: Vessels and bone"));
        f.snapshot(&format!("3d-12-side-view-vessels-{name}"));
    }
}

/// At the default window size (1440 points) the toolbar has room for the
/// "3D" layout button in every layout, and for the control of the 3D view:
/// no two of its widgets overlap. (The slider of a slab projection, shown
/// once a slab mode is chosen, did not fit at this size before the 3D layout
/// either.)
#[test]
fn the_toolbar_fits_at_the_default_window_size_in_every_layout() {
    let mut f = open(Theme::Dark);
    for layout in Layout::ALL {
        f.harness.state_mut().viewer.layout = layout;
        f.steps(3);
        let widgets: Vec<(String, Rect)> = f
            .harness
            .query_all_by_label_contains("")
            .map(|node| (node.accesskit_node().label().unwrap_or_default(), node.rect()))
            .filter(|(_, rect)| rect.max.y < 46.0 && rect.width() < 300.0)
            .collect();
        assert!(widgets.len() >= 15, "{layout:?}: {widgets:?}");
        for (i, (a, ra)) in widgets.iter().enumerate() {
            for (b, rb) in &widgets[i + 1..] {
                let overlap = ra.intersect(*rb);
                assert!(
                    overlap.width() <= 1.0 || overlap.height() <= 1.0,
                    "{layout:?}: {a:?} {ra:?} overlaps {b:?} {rb:?}"
                );
            }
        }
        f.snapshot(&format!("3d-13-toolbar-{layout:?}"));
    }
}
