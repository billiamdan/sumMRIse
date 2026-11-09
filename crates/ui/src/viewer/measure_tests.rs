//! The measuring tools in the viewer, driven offscreen through the keyboard,
//! the pointer and the toolbar, on images of known size. Set
//! `SUMMRISE_SNAPSHOT_DIR` to keep renderings for review (`measure-*.png`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use egui::accesskit::{Role, Toggled};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, Theme, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use summrise_core::dicom::{DecodedInstance, DicomError, InstanceHeader, Media, Progress, scan};
use summrise_core::imaging::{DVec3, MprAxis, SlabMode, Window};
use summrise_core::{
    DicomFiles, DisplayOrientation, Fusion, ImageSource, Layout, MeasureKind, OverlayStatus, SeriesData, SeriesKey,
    Session, Settings, Tool, ViewKind, ViewState, Waker,
};
use summrise_testkit::measure::{self as known, write_measure_study};
use summrise_testkit::{MARKER_POSITION, PhantomOptions, write_phantom_study};

use super::Viewer;
use crate::app::{Action, Toasts};
use crate::measure;
use crate::placement::Placement;
use crate::theme;

const SAGITTAL: ViewKind = ViewKind::Mpr(MprAxis::Sagittal);

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

/// Settings of a user who has seen the notice: most tests are not about it.
fn seen_notice(layout: Layout) -> Settings {
    Settings { default_layout: layout, measure_notice_shown: true, ..Settings::default() }
}

/// What a test opens and how.
struct Open {
    write: fn(&Path),
    series: usize,
    settings: Settings,
    size: [f32; 2],
    theme: Theme,
    source: Arc<dyn ImageSource>,
    wait: bool,
}

impl Open {
    fn new(write: fn(&Path), series: usize, layout: Layout) -> Self {
        Self {
            write,
            series,
            settings: seen_notice(layout),
            size: [1440.0, 900.0],
            theme: Theme::Dark,
            source: Arc::new(DicomFiles),
            wait: true,
        }
    }

    fn start(self) -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        (self.write)(dir.path());
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        let settings = self.settings;
        let key = SeriesKey { study: 0, series: self.series };
        let mut session = Session::with_source(index, settings.memory_budget_bytes(), None, self.source);
        session.select_series(key);
        let app = App {
            viewer: Viewer::new(session, &settings),
            settings,
            waker: Arc::new(|| {}),
            actions: Vec::new(),
            toasts: Toasts::default(),
        };
        let mut builder =
            Harness::builder().with_size(self.size).with_theme(self.theme).with_context_setup(theme::install);
        if snapshot_dir().is_some() {
            builder = builder.wgpu();
        }
        let harness = builder.build_ui_state(
            |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
            app,
        );
        let mut fixture = Fixture { harness, key, _dir: dir };
        if self.wait {
            fixture.run_until("the series", |f| f.viewer().session.loaded(key).is_some());
            fixture.steps(4);
        }
        fixture
    }
}

fn known_study(dir: &Path) {
    write_measure_study(dir).expect("fixture");
}

fn phantom_study(dir: &Path) {
    write_phantom_study(dir, &PhantomOptions { ct_size: 96, ct_slices: 48, ..PhantomOptions::small() })
        .expect("fixture");
}

/// A CT of 2.5 mm pixels with 5 mm between its slices.
fn thick_slices(dir: &Path) {
    write_phantom_study(dir, &PhantomOptions { ct_size: 96, ct_slices: 40, ..PhantomOptions::small() })
        .expect("fixture");
}

/// Series 1 of the known study: 5 images with Pixel Spacing 1.0 \ 0.5.
fn non_square() -> Fixture {
    Open::new(known_study, known::NON_SQUARE, Layout::Single).start()
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
        self.harness.run_steps(n);
    }

    fn viewer(&self) -> &Viewer {
        &self.harness.state().viewer
    }

    fn series(&self) -> Arc<SeriesData> {
        self.viewer().session.data(self.key).expect("the series")
    }

    fn view(&self) -> &ViewState {
        self.viewer().views.get(&self.key).expect("the series is on screen")
    }

    fn view_mut(&mut self) -> &mut ViewState {
        let key = self.key;
        self.harness.state_mut().viewer.views.get_mut(&key).expect("the series is on screen")
    }

    fn tool(&self) -> Tool {
        self.viewer().tool
    }

    /// Whether something on screen is labelled exactly `label`.
    fn shows(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }

    /// Screen rectangle of the view whose label starts with `name`.
    fn viewport(&self, name: &str) -> Rect {
        self.harness
            .query_all_by_label_contains(name)
            .find(|node| {
                let node = node.accesskit_node();
                node.role() == Role::Image && node.label().is_some_and(|label| label.starts_with(name))
            })
            .unwrap_or_else(|| panic!("no view named {name:?}"))
            .rect()
    }

    /// The mapping between the screen and the pixels of the view `kind`.
    fn placement(&self, kind: ViewKind, viewport: Rect) -> Placement {
        let rendered = self.viewer().renders.get(self.key, kind).expect("rendered");
        Placement::of(viewport, &rendered.displayed, &self.view().transform(kind))
    }

    /// The screen position of pixel (`x`, `y`) of the acquired view.
    fn at(&self, x: f64, y: f64) -> Pos2 {
        self.placement(ViewKind::Stack, self.viewport("Image ")).to_screen(x, y)
    }

    fn press(&mut self, key: Key) {
        self.harness.key_press(key);
        self.steps(2);
    }

    /// Presses the primary button at `from` and moves to `to`, and leaves it held.
    fn hold_and_move(&mut self, from: Pos2, to: Pos2) {
        self.harness.hover_at(from);
        self.steps(2);
        self.harness.drag_at(from);
        self.steps(1);
        self.harness.hover_at(from + (to - from) * 0.5);
        self.steps(1);
        self.harness.hover_at(to);
        self.steps(2);
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.hold_and_move(from, to);
        self.harness.drop_at(to);
        self.steps(3);
    }

    fn click(&mut self, pos: Pos2) {
        self.harness.hover_at(pos);
        self.steps(1);
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

    /// The tooltip text of the label `label`, as assistive technology gets it.
    fn details(&self, label: &str) -> String {
        self.harness.get_by_label(label).accesskit_node().description().unwrap_or_default()
    }

    fn save(&mut self, name: &str) {
        if let Some(dir) = snapshot_dir() {
            let image = self.harness.render().expect("wgpu render");
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            image.save(dir.join(format!("measure-{name}.png"))).expect("save png");
        }
    }
}

fn toggled(f: &Fixture, label: &str) -> Option<Toggled> {
    f.harness.get_by_label(label).accesskit_node().toggled()
}

/// Accessible selection (spec): the key shown in the tooltip chooses the tool,
/// and the toolbar button shows it as selected; the tool stays until another
/// is chosen.
#[test]
fn keys_choose_the_tools_and_the_toolbar_button_shows_the_chosen_one() {
    let mut f = non_square();
    assert_eq!(f.tool(), Tool::WindowLevel);
    assert_eq!(toggled(&f, "Measure"), Some(Toggled::False));
    for (key, kind, label) in [
        (Key::D, MeasureKind::Length, "Measure: Length"),
        (Key::Y, MeasureKind::Angle, "Measure: Angle"),
        (Key::E, MeasureKind::Ellipse, "Measure: Ellipse"),
        (Key::Q, MeasureKind::Rectangle, "Measure: Rectangle"),
    ] {
        f.press(key);
        assert_eq!(f.tool(), Tool::Measure(kind));
        assert_eq!(toggled(&f, label), Some(Toggled::True), "{label}");
        assert!(!f.shows("Measure"), "the button shows the chosen tool");
    }
    // Another tool ends it.
    f.harness.get_by_label_contains("Pan").click();
    f.steps(3);
    assert_eq!(f.tool(), Tool::Pan);
    assert_eq!(toggled(&f, "Measure"), Some(Toggled::False));
}

#[test]
fn the_toolbar_menu_offers_the_tools_and_their_keys_in_tooltips() {
    let mut f = non_square();
    f.harness.get_by_label("Measure").click();
    f.steps(3);
    f.save("menu");
    for (label, hint) in [
        ("Length", "Length: drag from one point to the other (D)"),
        ("Angle", "Angle: click three points, the corner second (Y)"),
        ("Ellipse", "Ellipse: drag across a region for its area and values (E)"),
        ("Rectangle", "Rectangle: drag across a region for its area and values (Q)"),
    ] {
        f.harness.get_by_label(label).hover();
        f.steps(8);
        assert!(f.shows(hint), "the tooltip of {label}");
    }
    assert!(f.harness.get_by_label("Delete the selected one").accesskit_node().is_disabled());
    assert!(f.harness.get_by_label("Delete all in this series").accesskit_node().is_disabled());
    f.harness.get_by_label("Ellipse").click();
    f.steps(3);
    assert_eq!(f.tool(), Tool::Measure(MeasureKind::Ellipse));
    assert_eq!(toggled(&f, "Measure: Ellipse"), Some(Toggled::True));
}

/// Non-square pixels (spec), drawn by the pointer: 100 columns read 50.0 mm,
/// 100 rows 100.0 mm; the right angle between them 90.0°.
#[test]
fn a_length_and_an_angle_on_non_square_pixels() {
    let mut f = non_square();
    f.press(Key::D);
    let (origin, across, down) = (f.at(20.0, 40.0), f.at(120.0, 40.0), f.at(20.0, 140.0));
    f.drag(origin, across);
    assert!(f.shows("50.0 mm"));
    f.drag(origin + vec2(4.0, 12.0), down + vec2(4.0, 12.0));
    assert!(f.shows("100.0 mm"));
    assert_eq!(f.view().measurements.len(), 2);

    f.press(Key::Y);
    // Three clicks, on an image without other measurements.
    f.press(Key::ArrowDown);
    let (a, v, b) = (f.at(120.0, 40.0), f.at(20.0, 40.0), f.at(20.0, 140.0));
    f.click(a);
    f.harness.hover_at(v);
    f.steps(2);
    assert_eq!(f.view().measurements.len(), 2, "not finished");
    f.click(v);
    assert!(!f.shows("90.0°"));
    // While the pointer moves to the third point the angle is read.
    f.harness.hover_at(f.at(120.0, 140.0));
    f.steps(2);
    assert!(f.shows("63.4°"), "not the 45° it looks like in pixels");
    f.click(b);
    assert!(f.shows("90.0°"));
    assert_eq!(f.view().measurements.len(), 3);
    f.save("angle-non-square");
}

/// Area in physical units (spec), and statistics, drawn by the pointer.
#[test]
fn a_rectangle_and_an_ellipse_read_area_and_values() {
    let mut f = Open::new(known_study, known::SQUARE, Layout::Single).start();
    f.press(Key::Q);
    // 40 × 20 pixels of 0.5 mm. Values `10 × column + row`, columns 10–50, rows 10–30.
    let (a, b) = (f.at(10.0, 10.0), f.at(50.0, 30.0));
    f.drag(a, b);
    let label = "Area 2.0 cm², Mean 320 HU · SD 118.5 HU, Min 110 HU · Max 530 HU";
    assert!(f.shows(label), "{:?}", labels(&f));
    let details = f.details(label);
    assert!(details.contains("Values of 861 pixels"), "{details}");
    assert!(details.contains("Pixel Spacing") && details.contains("not for diagnosis"), "{details}");
    // The values do not depend on the contrast window or on inverting the picture.
    f.view_mut().window = Window::new(300.0, 80.0);
    f.view_mut().invert = true;
    f.steps(3);
    assert!(f.shows(label), "{:?}", labels(&f));
    f.press(Key::E);
    // Further down the image; a circle of 10 pixels (5 mm) radius.
    let (a, b) = (f.at(30.0, 40.0), f.at(50.0, 60.0));
    f.drag(a, b);
    assert!(
        labels(&f).iter().any(|label| label.starts_with("Area 78.5 mm², Mean 450 ")),
        "π × 5 mm × 5 mm: {:?}",
        labels(&f)
    );
    f.save("regions");
}

/// The labels on the pictures: those of the measurements.
fn labels(f: &Fixture) -> Vec<String> {
    let views: Vec<Rect> = f
        .harness
        .query_all(by().role(Role::Image))
        .filter(|node| node.accesskit_node().bounding_box().is_some())
        .map(|node| node.rect())
        .collect();
    f.harness
        .query_all(by().role(Role::Label))
        .filter(|node| views.iter().any(|view| view.contains_rect(node.rect())))
        .filter_map(|node| {
            let node = node.accesskit_node();
            node.label().or_else(|| node.value())
        })
        .filter(|label| label != "Mirrored")
        .collect()
}

/// Zoom independence (spec) and a turned or mirrored view (brief): the same
/// points give the same length, at any zoom, in every display orientation.
#[test]
fn the_same_points_give_the_same_length_whatever_the_zoom_and_the_orientation() {
    let mut f = non_square();
    f.press(Key::D);
    // 40 columns (20 mm) and 30 rows (30 mm), around the middle of the image: 36.1 mm.
    let points = ((80.0, 85.0), (120.0, 115.0));
    let measure = |f: &mut Fixture| {
        let (a, b) = (f.at(points.0.0, points.0.1), f.at(points.1.0, points.1.1));
        f.drag(a, b);
        assert_eq!(f.view().measurements.len(), 1);
        let shown = labels(f);
        f.view_mut().measurements.clear();
        f.steps(2);
        shown
    };
    assert_eq!(measure(&mut f), ["36.1 mm"], "at 100 %");
    f.view_mut().transform_mut(ViewKind::Stack).zoom_about(4.0, [0.0, 0.0]);
    f.steps(2);
    assert_eq!(f.view().transform(ViewKind::Stack).zoom, 4.0);
    assert_eq!(measure(&mut f), ["36.1 mm"], "at 400 %");
    f.view_mut().transform_mut(ViewKind::Stack).pan = [25.0, -40.0];
    f.steps(2);
    assert_eq!(measure(&mut f), ["36.1 mm"], "panned");
    f.view_mut().reset_transforms();
    for orientation in DisplayOrientation::ALL {
        f.view_mut().transform_mut(ViewKind::Stack).orientation = orientation;
        f.steps(2);
        assert_eq!(measure(&mut f), ["36.1 mm"], "{orientation:?}");
    }
    // A measurement stays on its pixels when the view is turned afterwards.
    f.view_mut().reset_transforms();
    f.steps(2);
    let (a, b) = (f.at(points.0.0, points.0.1), f.at(points.1.0, points.1.1));
    f.drag(a, b);
    f.press(Key::CloseBracket);
    assert!(f.shows("36.1 mm"), "the label stays upright and true");
    f.press(Key::H);
    assert!(f.view().transform(ViewKind::Stack).orientation.is_mirrored());
    // Its end is taken hold of where it is shown now.
    let end = f.at(points.1.0, points.1.1);
    let further = f.at(points.1.0 + 40.0, points.1.1);
    f.drag(end, further);
    // 80 columns (40 mm) and 30 rows (30 mm).
    assert!(f.shows("50.0 mm"), "{:?}", labels(&f));
    f.save("mirrored");
}

/// Adjusting an endpoint (spec): the label updates while dragging.
#[test]
fn dragging_a_handle_updates_the_label_while_dragging() {
    let mut f = Open::new(known_study, known::NON_SQUARE, Layout::Single).start();
    f.press(Key::D);
    // 40 columns of 0.5 mm: 20.0 mm.
    let (a, b) = (f.at(60.0, 60.0), f.at(100.0, 60.0));
    f.drag(a, b);
    assert!(f.shows("20.0 mm"));
    // 5 mm further along the line, 8 points beside the handle at the start of the drag.
    let further = f.at(110.0, 60.0);
    f.hold_and_move(b + vec2(5.0, 5.0), further + vec2(5.0, 5.0));
    assert!(f.shows("25.0 mm"), "while dragging: {:?}", labels(&f));
    f.save("dragging-handle");
    assert_eq!(f.view().measurements.len(), 1, "no new measurement");
    f.harness.drop_at(further);
    f.steps(3);
    assert!(f.shows("25.0 mm"));
    assert_eq!(f.view().measurements.len(), 1);
}

#[test]
fn a_measurement_is_selected_moved_and_deleted() {
    let mut f = non_square();
    f.press(Key::D);
    let (a, b) = (f.at(40.0, 40.0), f.at(80.0, 40.0));
    f.drag(a, b);
    let first = f.view().measurements.selected().expect("what was drawn is selected");
    let (c, d) = (f.at(40.0, 100.0), f.at(100.0, 100.0));
    f.drag(c, d);
    assert!(f.shows("20.0 mm") && f.shows("30.0 mm"));

    // A click on the line selects it, a click on nothing lets go.
    f.click(a + (b - a) * 0.5);
    assert_eq!(f.view().measurements.selected(), Some(first));
    f.click(f.at(150.0, 150.0));
    assert_eq!(f.view().measurements.selected(), None);
    f.press(Key::Delete);
    assert_eq!(f.view().measurements.len(), 2, "nothing is selected");

    // A drag on the line moves it as a whole: the length stays.
    let (from, to) = (a + (b - a) * 0.5, a + (b - a) * 0.5 + vec2(30.0, 60.0));
    f.drag(from, to);
    assert_eq!(f.view().measurements.selected(), Some(first));
    assert!(f.shows("20.0 mm"));
    assert_eq!(f.view().measurements.len(), 2);
    // Delete removes the selected one, Backspace the next.
    f.press(Key::Delete);
    assert!(!f.shows("20.0 mm") && f.shows("30.0 mm"));
    let middle = c + (d - c) * 0.5;
    f.click(middle);
    f.press(Key::Backspace);
    assert!(f.view().measurements.is_empty());
    assert!(!f.shows("30.0 mm"));
}

#[test]
fn the_menu_deletes_the_selected_one_and_all_of_them() {
    let mut f = non_square();
    f.press(Key::D);
    let (a, b) = (f.at(40.0, 40.0), f.at(80.0, 40.0));
    f.drag(a, b);
    let (c, d) = (f.at(40.0, 100.0), f.at(100.0, 100.0));
    f.drag(c, d);
    // One more on the next image.
    f.press(Key::ArrowDown);
    f.drag(a, b);
    assert_eq!(f.view().measurements.len(), 3);
    f.harness.get_by_label("Measure: Length").click();
    f.steps(3);
    f.harness.get_by_label("Delete the selected one").click();
    f.steps(3);
    assert_eq!(f.view().measurements.len(), 2);
    f.harness.get_by_label("Measure: Length").click();
    f.steps(3);
    f.harness.get_by_label("Delete all in this series").click();
    f.steps(3);
    assert!(f.view().measurements.is_empty(), "those on other images too");
}

#[test]
fn esc_cancels_what_is_being_drawn() {
    let mut f = non_square();
    f.press(Key::D);
    let (a, b) = (f.at(40.0, 40.0), f.at(80.0, 40.0));
    f.hold_and_move(a, b);
    assert!(f.shows("20.0 mm"), "drawn while dragging");
    f.press(Key::Escape);
    assert!(!f.shows("20.0 mm"));
    f.harness.drop_at(b);
    f.steps(3);
    assert!(f.view().measurements.is_empty(), "nothing is left after letting go");

    f.press(Key::Y);
    f.click(a);
    f.click(b);
    assert!(f.view().measurements.is_busy());
    f.press(Key::Escape);
    assert!(!f.view().measurements.is_busy());
    assert_eq!(f.tool(), Tool::Measure(MeasureKind::Angle), "the tool stays chosen");

    // Another tool ends what is being drawn too.
    f.click(a);
    assert!(f.view().measurements.is_busy());
    f.harness.get_by_label_contains("Zoom").click();
    f.steps(3);
    assert!(!f.view().measurements.is_busy());
}

/// Right-drag keeps adjusting the contrast, Shift-drag keeps panning; a
/// double click does not fit the picture while clicks place points.
#[test]
fn other_gestures_stay_with_the_viewport() {
    let mut f = non_square();
    f.press(Key::D);
    let (a, b) = (f.at(40.0, 40.0), f.at(80.0, 80.0));
    let window = f.view().window;
    f.harness.hover_at(a);
    f.steps(2);
    let button = |pressed| Event::PointerButton {
        pos: a,
        button: PointerButton::Secondary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    f.harness.event(button(true));
    f.steps(1);
    f.harness.hover_at(b);
    f.steps(2);
    f.harness.event(button(false));
    f.steps(3);
    assert_ne!(f.view().window, window, "contrast");
    assert!(f.view().measurements.is_empty());
    assert!(!f.view().measurements.is_busy());
    assert_eq!(f.tool(), Tool::Measure(MeasureKind::Length), "the tool stays chosen");

    f.view_mut().transform_mut(ViewKind::Stack).zoom_about(2.0, [0.0, 0.0]);
    f.steps(2);
    f.press(Key::Y);
    let pos = f.viewport("Image ").center();
    // Two clicks in a row at one place, as a double click.
    let press =
        |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let events = [Event::PointerMoved(pos), press(true), press(false), press(true), press(false)];
    f.harness.input_mut().events.extend(events);
    f.steps(3);
    assert_eq!(f.view().transform(ViewKind::Stack).zoom, 2.0, "the view is not fitted");
    assert!(f.view().measurements.is_busy(), "the first point is placed");
}

/// Scrolling away and back (spec), with the wheel's own path: the keys.
#[test]
fn a_measurement_shows_on_its_image_only() {
    let mut f = non_square();
    f.press(Key::D);
    let (a, b) = (f.at(40.0, 40.0), f.at(80.0, 40.0));
    f.drag(a, b);
    assert!(f.shows("Image 3 / 5") && f.shows("20.0 mm"));
    f.press(Key::ArrowDown);
    assert!(f.shows("Image 4 / 5") && !f.shows("20.0 mm"));
    f.press(Key::ArrowUp);
    assert!(f.shows("Image 3 / 5") && f.shows("20.0 mm"));
    // Hiding the annotations does not hide what was measured.
    f.press(Key::O);
    assert!(!f.harness.state().settings.show_overlays && f.shows("20.0 mm"));
}

/// Playback stops when a measurement is drawn, so the picture stays put under it.
#[test]
fn drawing_stops_playback() {
    let mut f = non_square();
    f.press(Key::Space);
    assert!(f.viewer().cine.is_playing());
    f.press(Key::D);
    assert!(f.viewer().cine.is_playing(), "choosing a tool does not");
    let (a, b) = (f.at(40.0, 40.0), f.at(80.0, 40.0));
    f.hold_and_move(a, b);
    assert!(!f.viewer().cine.is_playing());
    let image = f.view().slice;
    f.harness.drop_at(b);
    f.steps(3);
    assert_eq!(f.view().slice, image);
    assert_eq!(f.view().measurements.len(), 1);
    assert!(f.shows("20.0 mm"));
}

/// An image set is a set of other images: the measurement stays on its image.
#[test]
fn a_measurement_on_an_image_set_stays_on_that_image() {
    fn stacks(dir: &Path) {
        summrise_testkit::stacks::write_stacks_study(dir).expect("fixture");
    }
    let mut f = Open::new(stacks, summrise_testkit::stacks::DYNAMIC, Layout::Single).start();
    f.press(Key::D);
    let (a, b) = (f.at(10.0, 10.0), f.at(14.0, 10.0));
    f.drag(a, b);
    assert!(f.shows("20.0 mm"), "4 pixels of 5 mm: {:?}", labels(&f));
    let slot = f.view().slot_index(&f.series());
    f.press(Key::ArrowRight);
    assert_ne!(f.view().slot_index(&f.series()), slot);
    assert!(!f.shows("20.0 mm"));
    f.press(Key::ArrowLeft);
    assert!(f.shows("20.0 mm"));
}

/// Reformat of thick slices (spec), and how the measurement belongs to its plane.
#[test]
fn a_length_on_a_reformat_of_thick_slices() {
    let mut f = Open::new(thick_slices, 0, Layout::Mpr).start();
    // Nothing of the reformat's lines gets in the way while measuring.
    let handle = "Tilt handle of the Coronal plane in the Sagittal view";
    assert!(f.shows(handle));
    f.press(Key::D);
    assert!(!f.shows(handle), "the tilt handles are out of the way");
    let view = f.viewport("Sagittal ");
    let series = f.series();
    f.view_mut().set_slice(SAGITTAL, &series, 45);
    f.steps(2);
    let x = f.view().cursor.x;
    let at = |f: &Fixture, y: f64, z: f64| {
        let rendered = f.viewer().renders.get(f.key, SAGITTAL).expect("rendered");
        let (px, py) = rendered.displayed.patient_to_pixel(DVec3::new(x, y, z)).expect("a reformat has positions");
        f.placement(SAGITTAL, view).to_screen(px, py)
    };
    // 40 mm along the head–foot direction.
    let (top, bottom) = (at(&f, 0.0, 40.0), at(&f, 0.0, 0.0));
    assert!((top.x - bottom.x).abs() < 0.01 && top.y < bottom.y, "head at the top");
    f.drag(top, bottom);
    assert!(f.shows("≈ 40.0 mm"), "{:?}", labels(&f));
    let details = f.details("≈ 40.0 mm");
    assert!(details.contains("5 mm apart"), "{details}");
    // The tooltip itself.
    let label = f.harness.get_by_label("≈ 40.0 mm").rect().center();
    f.harness.hover_at(label);
    f.steps(8);
    assert!(f.harness.query_all_by_label_contains("5 mm apart").next().is_some(), "the tooltip");
    f.save("reformat");

    // Moving away along the normal hides it, returning shows it again.
    f.press_on_view(view, Key::ArrowDown);
    assert!(!f.shows("≈ 40.0 mm"));
    f.press_on_view(view, Key::ArrowUp);
    assert!(f.shows("≈ 40.0 mm"));
    // So does tilting.
    f.view_mut().tilt_planes(MprAxis::Axial, 5f64.to_radians());
    f.steps(3);
    assert!(!f.shows("≈ 40.0 mm"));
    f.view_mut().tilt_planes(MprAxis::Axial, (-5f64).to_radians());
    f.steps(3);
    assert!(f.shows("≈ 40.0 mm"));
    // A click while measuring does not move the crosshair.
    let cursor = f.view().cursor;
    f.click(at(&f, 30.0, -30.0));
    assert_eq!(f.view().cursor, cursor);
    assert_eq!(f.viewer().active_view, SAGITTAL);
}

impl Fixture {
    fn press_on_view(&mut self, view: Rect, key: Key) {
        self.harness.hover_at(view.center() + vec2(-90.0, 70.0));
        self.steps(2);
        self.press(key);
    }
}

#[test]
fn statistics_of_a_slab_carry_its_name() {
    let mut f = Open::new(phantom_study, 0, Layout::Mpr).start();
    f.view_mut().set_slab(Some(SlabMode::Mip), 10.0);
    f.steps(3);
    f.press(Key::Q);
    let view = f.viewport("Axial ");
    let place = f.placement(ViewKind::Mpr(MprAxis::Axial), view);
    // A corner of the picture: air.
    let (a, b) = (place.to_screen(2.0, 2.0), place.to_screen(8.0, 8.0));
    f.drag(a, b);
    let label = labels(&f).into_iter().find(|l| l.starts_with("≈ Area")).expect("the label of the region");
    assert!(label.contains(", MIP values, ≈ Mean -1000 HU · SD 0 HU, ≈ Min -1000 HU"), "{label}");
    f.save("slab");
}

/// On a CT with a PET drawn over it, statistics are of the CT, in its unit.
#[test]
fn statistics_under_a_pet_overlay_are_of_the_series_shown() {
    fn pet(dir: &Path) {
        let ct = PhantomOptions { ct_size: 60, ct_slices: 50, ..PhantomOptions::small() };
        summrise_testkit::pet::write_pet_study(
            dir,
            &summrise_testkit::pet::PetStudyOptions { ct, ..Default::default() },
        )
        .expect("fixture");
    }
    let mut f = Open::new(pet, summrise_testkit::pet::CT, Layout::Single).start();
    let overlay = SeriesKey { study: 0, series: summrise_testkit::pet::PET };
    f.view_mut().set_fusion(Some(Fusion::new(overlay)));
    f.run_until("the PET overlay", |f| matches!(f.viewer().session.overlay_status(), Some(OverlayStatus::Ready(_))));
    f.steps(4);
    let series = f.series();
    // The slice through the hot sphere, whose CT value is 300 HU.
    let slice = (0..series.len())
        .min_by_key(|&i| {
            ((series.frames()[i].plane.expect("plane").position.z - MARKER_POSITION[2]).abs() * 100.0) as i64
        })
        .expect("images");
    let shown = f.view().slice;
    f.view_mut().scroll(ViewKind::Stack, &series, slice as i64 - shown as i64);
    f.steps(3);
    let centre = {
        let rendered = f.viewer().renders.get(f.key, ViewKind::Stack).expect("rendered");
        rendered.displayed.patient_to_pixel(DVec3::from_array(MARKER_POSITION)).expect("position")
    };
    f.press(Key::Q);
    let (a, b) = (f.at(centre.0 - 2.0, centre.1 - 2.0), f.at(centre.0 + 2.0, centre.1 + 2.0));
    f.drag(a, b);
    let label = labels(&f).into_iter().find(|l| l.starts_with("Area")).expect("the label of the region");
    assert!(label.contains("Mean 300 HU · SD 0 HU"), "{label}");
    assert!(!label.contains("SUV"), "{label}");
    f.save("under-pet");
}

/// Screenshot without spacing and a colour image (spec).
#[test]
fn a_colour_screenshot_without_spacing() {
    let mut f = Open::new(phantom_study, 3, Layout::Single).start();
    f.press(Key::D);
    let (a, b) = (f.at(10.0, 20.0), f.at(133.0, 20.0));
    f.drag(a, b);
    assert!(f.shows("123 px (uncalibrated)"), "{:?}", labels(&f));
    assert!(f.details("123 px (uncalibrated)").contains("no pixel spacing"));
    f.press(Key::Q);
    let (a, b) = (f.at(20.0, 40.0), f.at(60.0, 60.0));
    f.drag(a, b);
    assert!(f.shows("Area 800 px² (uncalibrated), Statistics not available for colour images"), "{:?}", labels(&f));
    f.save("colour");
}

/// Projection X-ray (spec), and its scale bar (brief).
#[test]
fn a_radiograph_is_calibrated_at_the_detector() {
    let mut f = Open::new(known_study, known::PROJECTION, Layout::Single).start();
    f.press(Key::D);
    let (a, b) = (f.at(10.0, 10.0), f.at(90.0, 10.0));
    f.drag(a, b);
    let label = "20.0 mm, at detector — true size may be smaller";
    assert!(f.shows(label), "{:?}", labels(&f));
    assert!(f.details(label).contains("Imager Pixel Spacing"));
    f.save("radiograph");

    let series = f.series();
    let meta = f.viewer().session.series(f.key).expect("the series").clone();
    let rendered = f.viewer().renders.get(f.key, ViewKind::Stack).expect("rendered");
    assert_eq!(measure::scale_label(50.0, &rendered.displayed, &series, &meta), "50 mm at detector");
}

#[test]
fn the_scale_bar_of_other_images_reads_as_before() {
    let f = non_square();
    let series = f.series();
    let meta = f.viewer().session.series(f.key).expect("the series").clone();
    let rendered = f.viewer().renders.get(f.key, ViewKind::Stack).expect("rendered");
    assert_eq!(measure::scale_label(50.0, &rendered.displayed, &series, &meta), "50 mm");
}

/// First use (spec): the notice comes once, and only that it was shown is
/// remembered. While a tool is chosen, the status bar says it too.
#[test]
fn the_notice_is_shown_once_and_the_status_bar_says_it_while_measuring() {
    let notice = "Measurements are approximate";
    let in_status_bar = "Approximate — not for diagnosis";
    let mut open = Open::new(known_study, known::NON_SQUARE, Layout::Single);
    open.settings = Settings::default();
    let mut f = open.start();
    assert!(!f.shows(notice) && !f.shows(in_status_bar));
    assert!(!f.harness.state().settings.measure_notice_shown);

    f.press(Key::D);
    assert!(f.shows(notice) && f.shows("They are not for diagnosis."));
    assert!(f.shows(in_status_bar));
    assert!(f.harness.state().settings.measure_notice_shown);
    f.save("notice");
    f.harness.get_by_label("Understood").click();
    f.steps(3);
    assert!(!f.shows(notice));
    assert_eq!(f.tool(), Tool::Measure(MeasureKind::Length), "the tool stays chosen");
    // A drag now measures: nothing covers the picture.
    let (a, b) = (f.at(40.0, 40.0), f.at(80.0, 40.0));
    f.drag(a, b);
    assert!(f.shows("20.0 mm"));
    assert!(f.shows(in_status_bar), "also while the pointer is on the picture");

    // Not again, with this or another tool.
    f.press(Key::E);
    assert!(!f.shows(notice));
    f.harness.get_by_label_contains("Contrast").click();
    f.steps(3);
    assert!(!f.shows(in_status_bar));
    f.press(Key::D);
    assert!(!f.shows(notice) && f.shows(in_status_bar));
    // The settings hold the fact and nothing of what was measured.
    let stored = format!("{:?}", f.harness.state().settings);
    assert!(stored.contains("measure_notice_shown: true") && !stored.contains("mm"), "{stored}");

    // Next time, with these settings: no notice.
    let mut open = Open::new(known_study, known::NON_SQUARE, Layout::Single);
    open.settings = Settings { measure_notice_shown: true, ..Settings::default() };
    let mut f = open.start();
    f.press(Key::Y);
    assert!(!f.shows(notice) && f.shows(in_status_bar));
}

/// The 3D view has no measurements: the tools are not offered.
#[test]
fn the_3d_view_does_not_offer_the_tools() {
    let mut f = Open::new(phantom_study, 0, Layout::Volume).start();
    assert!(f.harness.query_all_by_label_contains("Measure").next().is_none());
    f.press(Key::D);
    assert_eq!(f.tool(), Tool::WindowLevel);
    // A tool chosen before stays for the images, and the status bar does not speak of it here.
    f.harness.get_by_label("MPR").click();
    f.steps(3);
    f.harness.hover_at(f.viewport("Axial ").center());
    f.steps(2);
    f.press(Key::D);
    assert_eq!(f.tool(), Tool::Measure(MeasureKind::Length));
    assert!(f.shows("Approximate — not for diagnosis"));
    f.harness.get_by_label("3D").click();
    f.steps(3);
    assert!(f.harness.query_all_by_label_contains("Measure").next().is_none());
    assert!(!f.shows("Approximate — not for diagnosis"));
}

/// Reads files only as far as the test lets it.
#[derive(Default)]
struct Gate {
    allowed: AtomicUsize,
    started: AtomicUsize,
}

impl Gate {
    fn allow_all(&self) {
        self.allowed.store(usize::MAX, Ordering::SeqCst);
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

/// Nothing is drawn on a placeholder; a measurement on an image that is gone
/// says so, and reads again when the image is there.
#[test]
fn an_image_that_is_not_there_is_not_measured() {
    let gate = Arc::new(Gate::default());
    let mut open = Open::new(known_study, known::NON_SQUARE, Layout::Single);
    open.source = gate.clone();
    open.wait = false;
    let mut f = open.start();
    gate.allowed.store(1, Ordering::SeqCst);
    let series = f.viewer().session.data(f.key);
    assert!(series.is_some(), "the series is planned");
    f.run_until("the first image", |f| f.viewer().session.data(f.key).is_some_and(|s| s.is_viewable()));
    f.steps(3);
    f.press(Key::Q);
    f.press(Key::ArrowDown);
    assert!(f.harness.query_all_by_label_contains("Loading image 4 / 5").next().is_some());
    let rendered = f.viewport("Loading image 4 / 5");
    let place = f.placement(ViewKind::Stack, rendered);
    let (a, b) = (place.to_screen(10.0, 10.0), place.to_screen(50.0, 30.0));
    f.drag(a, b);
    assert!(f.view().measurements.is_empty(), "there is nothing to see to measure");
    assert!(!f.view().measurements.is_busy());
    gate.allow_all();
    f.run_until("the series", |f| f.viewer().session.loaded(f.key).is_some());
    f.steps(3);
    f.drag(a, b);
    assert_eq!(f.view().measurements.len(), 1);
    // 40 columns of 0.5 mm and 20 rows of 1.0 mm.
    assert!(labels(&f).iter().any(|label| label.starts_with("Area 4.0 cm², Mean 100 ")), "{:?}", labels(&f));
}

/// Task 4.3: every tool drawn, and everything that can be operated has a name.
#[test]
fn every_control_is_named_while_measuring() {
    let mut f = non_square();
    for (key, points) in [
        (Key::D, [(20.0, 20.0), (60.0, 20.0)]),
        (Key::E, [(20.0, 60.0), (60.0, 90.0)]),
        (Key::Q, [(100.0, 60.0), (140.0, 90.0)]),
    ] {
        f.press(key);
        let (a, b) = (f.at(points[0].0, points[0].1), f.at(points[1].0, points[1].1));
        f.drag(a, b);
    }
    f.press(Key::Y);
    for (x, y) in [(120.0, 130.0), (80.0, 130.0), (80.0, 170.0)] {
        let at = f.at(x, y);
        f.click(at);
    }
    assert_eq!(f.view().measurements.len(), 4);
    f.save("every-tool");
    f.harness.get_by_label("Measure: Angle").click();
    f.steps(3);
    let unnamed: Vec<String> = f
        .harness
        .query_all(by())
        .filter(|node| {
            let node = node.accesskit_node();
            let operable =
                matches!(node.role(), Role::Button | Role::CheckBox | Role::ComboBox | Role::Slider | Role::SpinButton);
            let named = node.label().is_some_and(|l| !l.trim().is_empty())
                || node.value().is_some_and(|v| !v.trim().is_empty());
            operable && !named
        })
        .map(|node| format!("{:?} at {:?}", node.accesskit_node().role(), node.rect()))
        .collect();
    assert!(unnamed.is_empty(), "without a name: {unnamed:?}");
    // The measurements are known to assistive technology.
    for label in ["20.0 mm", "90.0°"] {
        assert_eq!(f.harness.get_by_label(label).accesskit_node().role(), Role::Label);
    }
}

/// Inside a region: a click selects it, a drag inside a selected one moves it,
/// and a drag inside one that is not selected draws.
#[test]
fn inside_a_region_selected_or_not() {
    let mut f = non_square();
    f.press(Key::Q);
    // A hundred columns and rows around the middle of the image.
    let (a, b) = (f.at(50.0, 50.0), f.at(150.0, 150.0));
    f.drag(a, b);
    assert_eq!(f.view().measurements.len(), 1);
    let area = "Area 50.0 cm²";
    assert!(labels(&f).iter().any(|l| l.starts_with(area)), "{:?}", labels(&f));
    let region = f.view().measurements.selected();
    assert!(region.is_some(), "what was drawn is selected");
    // Selected: a drag inside moves it.
    let (from, to) = (f.at(100.0, 100.0), f.at(110.0, 120.0));
    f.drag(from, to);
    assert_eq!(f.view().measurements.len(), 1, "moved, not drawn");
    assert!(labels(&f).iter().any(|l| l.starts_with(area)));
    // Not selected: with the length tool a line can be drawn inside it.
    f.click(f.at(10.0, 10.0));
    assert_eq!(f.view().measurements.selected(), None);
    f.press(Key::D);
    let (from, to) = (f.at(90.0, 100.0), f.at(130.0, 100.0));
    f.drag(from, to);
    assert_eq!(f.view().measurements.len(), 2);
    assert!(f.shows("20.0 mm"));
    // A click inside selects it again.
    let inside = f.at(70.0, 90.0);
    f.click(inside);
    assert_eq!(f.view().measurements.selected(), region);
}

/// With every toolbar item and a long tool name the toolbar still fits, down
/// to the smallest window.
#[test]
fn the_toolbar_keeps_fitting_with_a_tool_chosen() {
    for width in [1440.0, 1280.0, 1024.0, 960.0] {
        let mut open = Open::new(phantom_study, 0, Layout::Mpr);
        open.size = [width, 800.0];
        let mut f = open.start();
        f.press(Key::Q);
        assert!(f.shows("Measure: Rectangle"));
        let views_top = f
            .harness
            .query_all(by().role(Role::Image))
            .filter(|node| node.accesskit_node().bounding_box().is_some())
            .map(|node| node.rect().top())
            .fold(f32::INFINITY, f32::min);
        let widgets: Vec<(String, Rect)> = f
            .harness
            .query_all(by())
            .filter(|node| node.accesskit_node().bounding_box().is_some())
            .filter_map(|node| {
                let (rect, ak) = (node.rect(), node.accesskit_node());
                let interesting = !matches!(ak.role(), Role::Window | Role::GenericContainer | Role::Unknown);
                let label = ak.label().or_else(|| ak.value()).unwrap_or_default();
                (interesting && rect.is_positive() && rect.bottom() <= views_top).then_some((label, rect))
            })
            .collect();
        assert!(widgets.len() > 15, "{widgets:?}");
        for (i, (a_label, a)) in widgets.iter().enumerate() {
            assert!(a.left() >= -0.5 && a.right() <= width + 0.5, "{a_label:?} is outside at {width}: {a:?}");
            for (b_label, b) in &widgets[i + 1..] {
                let common = a.intersect(*b);
                let nested = a.contains_rect(*b) || b.contains_rect(*a);
                assert!(!common.is_positive() || common.area() < 1.0 || nested, "{a_label:?} and {b_label:?} overlap");
            }
        }
    }
}

/// Renderings for review: every tool on the CT in both themes, at 1440 and at
/// 960 points, in a mirrored view too.
#[test]
fn renderings_of_the_tools_in_both_themes() {
    if snapshot_dir().is_none() {
        return;
    }
    for (name, size, look) in [("dark-1440", [1440.0, 900.0], Theme::Dark), ("light-960", [960.0, 600.0], Theme::Light)]
    {
        let mut open = Open::new(phantom_study, 0, Layout::Single);
        (open.size, open.theme) = (size, look);
        let mut f = open.start();
        f.press(Key::D);
        let (a, b) = (f.at(28.0, 48.0), f.at(62.0, 48.0));
        f.drag(a, b);
        f.press(Key::E);
        let (a, b) = (f.at(38.0, 16.0), f.at(58.0, 28.0));
        f.drag(a, b);
        f.press(Key::Q);
        let (a, b) = (f.at(4.0, 4.0), f.at(18.0, 14.0));
        f.drag(a, b);
        f.press(Key::Y);
        for (x, y) in [(20.0, 76.0), (40.0, 80.0), (30.0, 62.0)] {
            let at = f.at(x, y);
            f.click(at);
        }
        f.harness.remove_cursor();
        f.steps(2);
        f.save(&format!("ct-{name}"));
        f.press(Key::CloseBracket);
        f.press(Key::H);
        f.steps(2);
        f.save(&format!("ct-turned-mirrored-{name}"));
    }
}

/// In a tile grid a measurement belongs to the image of the tile it was drawn
/// on, not to the first tile: it moves with that image around the grid, leaves
/// with it, and reads the same on the image alone.
#[test]
fn a_measurement_on_a_tile_belongs_to_the_image_of_that_tile() {
    let mut f = Open::new(phantom_study, 0, Layout::Single).start();
    let series = f.series();
    f.view_mut().set_slice(ViewKind::Stack, &series, 30);
    f.press(Key::T);
    f.press(Key::T);
    f.press(Key::D);

    // The tile of image 33, the third of images 31 to 39.
    let tile = f.viewport("Image 33 /");
    f.drag(tile.center() - vec2(30.0, 0.0), tile.center() + vec2(30.0, 0.0));
    assert_eq!(f.view().measurements.len(), 1, "one measurement, in one tile");
    let shown = labels(&f);
    assert_eq!(shown.len(), 1, "drawn on one tile only: {shown:?}");
    let reading = shown[0].clone();
    assert!(reading.ends_with(" mm"), "{reading}");
    let on_tile = |f: &Fixture, tile: Rect| {
        let label = f.harness.query_all(by().role(Role::Label)).find(|node| {
            let node = node.accesskit_node();
            node.label().or_else(|| node.value()).is_some_and(|text| text == reading)
        });
        label.is_some_and(|label| tile.contains_rect(label.rect()))
    };
    assert!(on_tile(&f, tile), "on the tile of image 33");
    f.save("tile");

    // One step: image 33 is now in the second tile, and the measurement with it.
    f.press(Key::ArrowDown);
    let moved = f.viewport("Image 33 /");
    assert_ne!(moved, tile);
    assert!(on_tile(&f, moved), "it moves with its image");

    // A whole grid further image 33 is no longer shown, and neither is the measurement.
    f.press(Key::PageDown);
    assert!(labels(&f).is_empty(), "{:?}", labels(&f));
    assert_eq!(f.view().measurements.len(), 1, "it is kept");

    // On the image alone (4 × 4, 5 × 5, then one image) it reads the same.
    f.view_mut().set_slice(ViewKind::Stack, &series, 32);
    for _ in 0..3 {
        f.press(Key::T);
    }
    assert_eq!(labels(&f), std::slice::from_ref(&reading));
    f.press(Key::ArrowDown);
    assert!(labels(&f).is_empty(), "image 34 has none");
}

/// A file with annotations holds the measurements of the view, drawn by the
/// same code as on screen.
#[test]
fn a_measurement_is_in_a_file_saved_with_annotations() {
    let mut f = Open::new(phantom_study, 0, Layout::Single).start();
    f.press(Key::D);
    let view = f.viewport("Image ");
    f.drag(view.center() - vec2(60.0, 0.0), view.center() + vec2(60.0, 0.0));
    let reading = labels(&f).into_iter().next().expect("a label");
    let viewer = f.viewer();
    let source = crate::export::Source::new(
        &viewer.session,
        &viewer.views,
        viewer.active_view,
        &viewer.volume_view,
        viewer.shown_tiles(),
    );
    let selected = source.selected().expect("a picture");
    let ctx = f.harness.ctx.clone();
    let settings = f.harness.state().settings.clone();
    let (_, texts) =
        crate::export::annotated::pass(&ctx, &selected, &settings, false, vec2(640.0, 480.0)).expect("a picture");
    assert!(texts.contains(&reading), "{reading:?} in {texts:?}");
}
