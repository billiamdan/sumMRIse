//! Several series at once and tile grids in the viewer, driven offscreen.
//! Set `SUMMRISE_SNAPSHOT_DIR` to keep renderings (`compare-*.png`).
//!
//! The data: the comparison studies of the testkit (two MR studies of one
//! patient with slices at known positions), the synthetic CD with a 60-image CT,
//! and the PET study.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, Rect, Theme, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use summrise_core::dicom::{Progress, scan};
use summrise_core::imaging::MprAxis;
use summrise_core::pet::Fusion;
use summrise_core::{
    Grid, Layout, Link, SeriesData, SeriesKey, Session, Settings, Tiles, Tool, ViewKind, ViewState, Waker,
};
use summrise_testkit::comparison::{self as phantom, ComparisonOptions, write_comparison_studies};
use summrise_testkit::pet::{self as pet_phantom, PetStudyOptions, write_pet_study};
use summrise_testkit::{MARKER_POSITION, PhantomOptions, write_phantom_study};

use super::Viewer;
use crate::app::{Action, Toasts};
use crate::compare::{self, Command};
use crate::placement::Placement;
use crate::theme;
use crate::tiles;

const fn earlier(series: usize) -> SeriesKey {
    SeriesKey { study: phantom::EARLIER, series }
}
const fn later(series: usize) -> SeriesKey {
    SeriesKey { study: phantom::LATER, series }
}
const AXIAL: SeriesKey = earlier(phantom::AXIAL_5MM);
const FINE: SeriesKey = earlier(phantom::AXIAL_2_5MM);
const SHORT: SeriesKey = earlier(phantom::AXIAL_SHORT);
const SAGITTAL: SeriesKey = earlier(phantom::SAGITTAL);
const LATER_AXIAL: SeriesKey = later(phantom::LATER_AXIAL);
const LATER_SAGITTAL: SeriesKey = later(phantom::LATER_SAGITTAL);
const CT: SeriesKey = SeriesKey { study: 0, series: 0 };
const STACK: ViewKind = ViewKind::Stack;

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

/// The viewer on what `write` puts in a folder, in a window of `size` points.
fn start(
    write: impl FnOnce(&Path),
    first: Option<SeriesKey>,
    settings: Settings,
    size: [f32; 2],
    theme: Theme,
) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path());
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let mut session = Session::new(index, settings.memory_budget_bytes(), None);
    if let Some(key) = first {
        session.select_series(key);
    }
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
    Fixture { harness, _dir: dir }
}

fn studies(options: ComparisonOptions, first: SeriesKey, size: [f32; 2], theme: Theme) -> Fixture {
    let write = move |dir: &Path| {
        write_comparison_studies(dir, &options).expect("fixture");
    };
    let mut f = start(write, Some(first), Settings::default(), size, theme);
    f.wait_for_series(first);
    f
}

/// The comparison studies with `first` on screen, in the default window.
fn open(first: SeriesKey) -> Fixture {
    studies(ComparisonOptions::default(), first, [1440.0, 900.0], Theme::Dark)
}

/// The two series side by side, the first one active.
fn side_by_side(left: SeriesKey, right: SeriesKey) -> Fixture {
    let mut f = open(left);
    f.command(Command::Grid(Some(Grid::SideBySide)));
    f.assign(1, right);
    f.activate(0);
    f
}

/// The synthetic CD with a CT of 60 images, in a window of `size` points.
fn ct(size: [f32; 2]) -> Fixture {
    let options = PhantomOptions { ct_size: 64, ct_slices: 60, ..PhantomOptions::small() };
    let write = move |dir: &Path| {
        write_phantom_study(dir, &options).expect("fixture");
    };
    let mut f = start(write, Some(CT), Settings::default(), size, Theme::Dark);
    f.wait_for_series(CT);
    assert_eq!(f.data(CT).stack_len(), 60);
    f
}

impl Fixture {
    fn run_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < Duration::from_secs(60), "timed out waiting for {what}");
            self.harness.step();
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    fn wait_for_series(&mut self, key: SeriesKey) {
        self.run_until("the series", |f| f.viewer().session.loaded(key).is_some());
        self.steps(4);
    }

    fn steps(&mut self, n: usize) {
        self.harness.run_steps(n);
    }

    fn viewer(&self) -> &Viewer {
        &self.harness.state().viewer
    }

    fn viewer_mut(&mut self) -> &mut Viewer {
        &mut self.harness.state_mut().viewer
    }

    fn data(&self, key: SeriesKey) -> Arc<SeriesData> {
        self.viewer().session.data(key).expect("the series is open")
    }

    fn view(&self, key: SeriesKey) -> &ViewState {
        self.viewer().views.get(&key).expect("the series has a view")
    }

    /// Height of the image of an axial series on screen, in mm.
    fn z(&self, key: SeriesKey) -> f64 {
        let data = self.data(key);
        data.frames()[self.view(key).slot_index(&data)].plane.expect("plane").position.z
    }

    /// Number of the image on screen, from 1.
    fn image(&self, key: SeriesKey) -> usize {
        self.view(key).slice_index(STACK, &self.data(key)) + 1
    }

    fn command(&mut self, command: Command) {
        self.viewer_mut().run_command(command);
        self.steps(3);
    }

    /// Puts a series in viewport `index` as a click in the browser does, with
    /// that viewport active.
    fn assign(&mut self, index: usize, key: SeriesKey) {
        self.activate(index);
        self.viewer_mut().session.select_series(key);
        self.wait_for_series(key);
    }

    fn activate(&mut self, index: usize) {
        self.viewer_mut().activate_viewport(index);
        self.steps(3);
    }

    fn cell(&self, index: usize) -> Rect {
        self.viewer().cells[index].rect
    }

    fn shows(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }

    fn shows_containing(&self, label: &str) -> bool {
        self.harness.query_all_by_label_contains(label).next().is_some()
    }

    /// The rectangle of what is labelled exactly `label` inside `within`.
    fn rect_of(&self, label: &str, within: Rect) -> Rect {
        self.harness
            .query_all_by_label(label)
            .map(|node| node.rect())
            .find(|rect| within.contains_rect(*rect))
            .unwrap_or_else(|| panic!("nothing labelled {label:?} in {within:?}"))
    }

    fn press(&mut self, key: Key) {
        self.harness.key_press(key);
        self.steps(3);
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
        self.steps(4);
    }

    fn double_click_at(&mut self, pos: Pos2) {
        let button =
            |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        let events = [Event::PointerMoved(pos), button(true), button(false), button(true), button(false)];
        self.harness.input_mut().events.extend(events);
        self.steps(4);
    }

    fn scroll_at(&mut self, pos: Pos2, points: f32) {
        self.harness.hover_at(pos);
        self.steps(1);
        self.harness.event(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, points),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Wheel,
        });
        // Long enough for the smooth scrolling of egui to deliver all of it.
        self.steps(12);
    }

    fn click_label(&mut self, label: &str) {
        self.harness.get_by_label(label).click();
        self.steps(4);
    }

    fn open_compare_menu(&mut self) {
        self.click_label("Compare");
    }

    fn snapshot(&mut self, name: &str) {
        if let Some(dir) = snapshot_dir() {
            // Spinners and tooltips settle first.
            self.steps(2);
            let image = self.harness.render().expect("wgpu render");
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            image.save(dir.join(format!("compare-{name}.png"))).expect("save png");
        }
    }

    /// Where a pixel of the first (or only) picture of viewport `index` is on screen.
    fn on_screen(&self, index: usize, key: SeriesKey, x: f64, y: f64) -> Pos2 {
        let displayed = &self.viewer().renders.get(key, STACK).expect("rendered").displayed;
        Placement::of(self.cell(index), displayed, &self.view(key).transform(STACK)).to_screen(x, y)
    }
}

/// Task 2.1, 2.2, 2.4, 6.3 — two series side by side, chosen by selecting the
/// viewport and then the series, each with its own counter, contrast and
/// letters; the empty viewport says what to do.
#[test]
fn two_series_side_by_side_by_select_then_click() {
    let mut f = open(SAGITTAL);
    assert_eq!(f.viewer().workspace.grid(), None);
    assert!(f.shows("Image 5 / 9"), "the first series alone, as ever");

    // The layout is chosen in the menu: the series on screen stays, the other viewport waits.
    f.open_compare_menu();
    f.snapshot("menu");
    f.click_label("Two series side by side");
    assert_eq!(f.viewer().workspace.grid(), Some(Grid::SideBySide));
    assert!(f.shows("Drop a series here"), "the empty viewport");
    assert_eq!(f.viewer().session.active_series, Some(SAGITTAL));
    assert!(f.shows("T2 TSE SAG (synthetic) · 2025-03-02"));
    f.snapshot("side-by-side-empty");

    // Click the empty viewport: it is active, with nothing to act on.
    let empty = f.cell(1);
    f.click_at(empty.center());
    assert_eq!(f.viewer().workspace.active(), 1);
    assert_eq!(f.viewer().session.active_series, None);
    // The series left stays on screen and keeps loading.
    assert!(f.shows("Image 5 / 9"));

    // Then a series in the browser: it goes there.
    f.click_label(phantom::AXIAL_5MM_NAME);
    f.wait_for_series(AXIAL);
    assert_eq!(f.viewer().workspace.viewports()[1].series, Some(AXIAL));
    assert_eq!(f.viewer().workspace.viewports()[0].series, Some(SAGITTAL));
    assert_eq!(f.viewer().session.active_series, Some(AXIAL));
    assert!(f.shows("T1 AX 5mm (synthetic) · 2025-03-02"));
    assert!(f.shows("Image 13 / 25"), "each with its own counter");
    assert!(f.shows("Image 5 / 9"));
    assert!(!f.shows("Drop a series here"));
    f.snapshot("side-by-side");
    f.harness.state_mut().viewer.views.clear();
}

/// Task 2.2 — dragging from the browser, and what the viewport does while a
/// series is held over it. Also replaces what a viewport shows.
#[test]
fn a_series_dragged_from_the_browser_goes_into_the_viewport_it_is_dropped_on() {
    let mut f = open(AXIAL);
    f.command(Command::Grid(Some(Grid::Four)));
    assert_eq!(f.harness.query_all_by_label("Drop a series here").count(), 3);

    let drag = |f: &mut Fixture, name: &str, target: Pos2| {
        let row = f.harness.query_all_by_label(name).map(|node| node.rect()).find(|rect| rect.left() < 300.0);
        let from = row.expect("the series in the browser").center();
        f.harness.hover_at(from);
        f.steps(1);
        f.harness.drag_at(from);
        f.steps(1);
        // Past the drag threshold, then on to the viewport.
        f.harness.hover_at(from + vec2(30.0, 10.0));
        f.steps(2);
        f.harness.hover_at(target);
        f.steps(2);
    };

    let target = f.cell(3).center();
    drag(&mut f, phantom::AXIAL_2_5MM_NAME, target);
    assert!(egui::DragAndDrop::has_payload_of_type::<SeriesKey>(&f.harness.ctx), "a series is held");
    f.snapshot("drag-over");
    f.harness.drop_at(target);
    f.steps(3);
    f.wait_for_series(FINE);
    assert_eq!(f.viewer().workspace.viewports()[3].series, Some(FINE));
    assert_eq!(f.viewer().workspace.active(), 3, "the viewport dropped on is the active one");
    assert_eq!(f.viewer().session.active_series, Some(FINE));
    assert_eq!(f.harness.query_all_by_label("Drop a series here").count(), 2);

    // Dropped on the first viewport, it replaces what that shows; dragging is not selecting.
    let target = f.cell(0).center();
    drag(&mut f, phantom::SAGITTAL_NAME, target);
    assert_eq!(f.viewer().session.active_series, Some(FINE), "nothing changes before the drop");
    f.harness.drop_at(target);
    f.steps(3);
    f.wait_for_series(SAGITTAL);
    assert_eq!(f.viewer().workspace.viewports()[0].series, Some(SAGITTAL));
    assert_eq!(f.viewer().workspace.shown_series(), [SAGITTAL, FINE]);
    f.steps(2);
    assert!(f.viewer().session.state(AXIAL).is_some(), "it may stay in memory");
}

/// In the layouts of one series nothing is dragged and clicks select, as ever.
#[test]
fn the_layouts_of_one_series_are_as_they_were() {
    let mut f = open(AXIAL);
    f.click_label(phantom::SAGITTAL_NAME);
    f.wait_for_series(SAGITTAL);
    assert_eq!(f.viewer().session.active_series, Some(SAGITTAL));
    assert_eq!(f.viewer().workspace.viewports().len(), 1);
    assert!(f.shows("Image 5 / 9"));
    assert!(!f.shows("T2 TSE SAG (synthetic) · 2025-03-02"), "no line with the date in a single view");
    // N has nowhere to go, K nothing to do, and the layouts still work.
    f.press(Key::N);
    f.press(Key::K);
    assert!(f.shows("Image 5 / 9"));
    f.click_label("MPR");
    assert_eq!(f.viewer().workspace.viewports().len(), 3);
    assert!(f.shows_containing("Axial"));
    assert_eq!(f.viewer().active_view, f.viewer().layout.views()[0]);
    // N steps through the views of the preset.
    f.harness.remove_cursor();
    f.steps(2);
    let before = f.viewer().active_view;
    f.press(Key::N);
    assert_ne!(f.viewer().active_view, before);

    f.click_label("Images");
    assert_eq!(f.viewer().layout, Layout::Single);
    assert_eq!(f.viewer().workspace.viewports().len(), 1);
}

/// Task 2.3 — the toolbar, the keys and the inspector follow the active viewport,
/// and only a deliberate act changes it, not the pointer passing over.
#[test]
fn everything_follows_the_active_viewport() {
    let mut f = side_by_side(SAGITTAL, AXIAL);
    assert_eq!(f.viewer().session.active_series, Some(SAGITTAL));
    // The pointer passing over the other viewport reads its value and activates nothing.
    let right = f.cell(1);
    let point = f.on_screen(1, AXIAL, 32.0, 32.0);
    f.harness.hover_at(point);
    f.steps(2);
    assert_eq!(f.viewer().session.active_series, Some(SAGITTAL));
    let hover = f.viewer().hover.expect("a value under the pointer").1;
    assert_eq!(hover.series, AXIAL, "the read-out is of the series under the pointer");

    // A click makes it active: the highlight, the inspector, the arrow keys.
    f.click_at(right.center());
    assert_eq!(f.viewer().workspace.active(), 1);
    assert_eq!(f.viewer().session.active_series, Some(AXIAL));
    let rows = f.viewer().display_rows(Some(&(AXIAL, f.data(AXIAL))));
    let row = rows.iter().find(|(label, _)| label == "Image").map(|(_, value)| value.as_str());
    assert_eq!(row, Some("13 of 25"));
    let sagittal_before = f.image(SAGITTAL);
    let axial_before = f.image(AXIAL);
    f.press(Key::ArrowDown);
    assert_eq!(f.image(AXIAL), axial_before + 1);
    assert_eq!(f.image(SAGITTAL), sagittal_before, "the other series has its own image");
    // The contrast of the toolbar and I act on it too.
    f.press(Key::I);
    assert!(f.view(AXIAL).invert);
    assert!(!f.view(SAGITTAL).invert, "each with its own contrast");

    // N goes on to the next viewport, and scrolling over one makes it active.
    f.press(Key::N);
    assert_eq!(f.viewer().workspace.active(), 0);
    assert_eq!(f.viewer().session.active_series, Some(SAGITTAL));
    f.scroll_at(right.center(), -18.0);
    assert_eq!(f.viewer().session.active_series, Some(AXIAL), "the viewport scrolled is the active one");
    assert_eq!(f.image(AXIAL), axial_before + 2);
}

/// The inspector and the guide describe the active viewport's image, and
/// playback plays its series, with the linked viewport following by position.
#[test]
fn the_inspector_the_guide_and_playback_act_on_the_active_viewport() {
    let mut f = side_by_side(SAGITTAL, AXIAL);
    let guide_says = |f: &mut Fixture, text: &'static str| {
        f.run_until(text, |f| f.shows_containing(text));
    };
    guide_says(&mut f, "The slices are sagittal");
    f.activate(1);
    guide_says(&mut f, "The slices are axial");
    assert!(!f.shows_containing("The slices are sagittal"));

    // With two axial series, Space plays the active one and the other comes along.
    let mut f = side_by_side(AXIAL, FINE);
    let before = f.image(AXIAL);
    f.harness.key_press(Key::Space);
    f.run_until("a step of playback", |f| f.image(AXIAL) != before);
    f.steps(2);
    assert!(f.viewer().cine.is_playing());
    let (axial, fine) = (f.z(AXIAL), f.z(FINE));
    assert!((axial - fine).abs() <= 2.5, "{axial} mm and {fine} mm");
    // Another viewport active: what played stops, as when another series is shown.
    f.activate(1);
    f.steps(2);
    assert!(!f.viewer().cine.is_playing());
}

/// Task 3.3 at the level of the viewer: the scenario of the specification with
/// keys and the mouse. One scroll step of the first series moves the other by two.
#[test]
fn linked_viewports_scroll_together_by_position() {
    let mut f = side_by_side(AXIAL, FINE);
    assert!(f.shows("Link: linked"));
    let data = f.data(AXIAL);
    f.viewer_mut().views.get_mut(&AXIAL).expect("view").set_slice(STACK, &data, 0);
    f.steps(3);
    // Go to z = -40 mm with the keys.
    let up = f.data(AXIAL).frames()[1].plane.expect("plane").position.z
        > f.data(AXIAL).frames()[0].plane.expect("plane").position.z;
    let key_to_feet = if up { Key::ArrowUp } else { Key::ArrowDown };
    let to_head = if up { Key::ArrowDown } else { Key::ArrowUp };
    for _ in 0..30 {
        if f.z(AXIAL) == -40.0 {
            break;
        }
        f.press(if f.z(AXIAL) > -40.0 { key_to_feet } else { to_head });
    }
    assert_eq!(f.z(AXIAL), -40.0);
    assert_eq!(f.z(FINE), -40.0);
    let fine_slice = f.image(FINE);

    f.press(key_to_feet);
    assert_eq!(f.z(AXIAL), -45.0);
    assert_eq!(f.z(FINE), -45.0);
    assert_eq!(f.image(FINE).abs_diff(fine_slice), 2);
    f.snapshot("linked-axial");

    // A link switched off with its indicator: the other no longer moves.
    let indicator = f.rect_of("Link: linked", f.cell(1));
    f.click_at(indicator.center());
    assert!(f.shows("Link: link off"));
    f.press(key_to_feet);
    assert_eq!(f.z(AXIAL), -50.0);
    assert_eq!(f.z(FINE), -45.0, "left alone");
    // K on the other viewport switches it on again, and it comes to the position.
    f.activate(1);
    f.press(Key::K);
    assert!(f.shows("Link: linked"));
    assert_eq!(f.z(AXIAL), -45.0, "the others come to the active viewport");
    f.press(Key::K);
    assert!(f.shows("Link: link off"));
    assert_eq!(f.viewer().workspace.viewports()[1].link, Link::Off);
}

#[test]
fn outside_the_coverage_of_a_series_it_says_so_with_annotations_on_and_off() {
    let mut f = side_by_side(AXIAL, SHORT);
    let index_at = |f: &Fixture, z: f64| {
        let data = f.data(AXIAL);
        data.frames().iter().position(|frame| frame.plane.expect("plane").position.z == z).expect("image")
    };
    let at_20 = index_at(&f, 20.0);
    let data = f.data(AXIAL);
    f.viewer_mut().views.get_mut(&AXIAL).expect("view").set_slice(STACK, &data, at_20);
    f.steps(4);
    assert_eq!(f.z(SHORT), 20.0);
    assert!(!f.shows("outside range"));

    let at_40 = index_at(&f, 40.0);
    f.viewer_mut().views.get_mut(&AXIAL).expect("view").set_slice(STACK, &data, at_40);
    f.steps(4);
    assert_eq!(f.z(SHORT), 20.0, "on its nearest end slice");
    assert!(f.shows("outside range"));
    f.snapshot("outside-range");
    f.press(Key::O);
    assert!(f.shows("outside range"), "whether annotations are shown or not");
}

/// The scenario of the specification: two studies aligned by hand.
#[test]
fn two_studies_are_linked_by_hand_and_the_indicator_says_manual() {
    let mut f = side_by_side(AXIAL, LATER_AXIAL);
    assert_eq!(f.harness.query_all_by_label("Link: not linked").count(), 2);
    let set_z = |f: &mut Fixture, key: SeriesKey, z: f64| {
        let data = f.data(key);
        let index = data.frames().iter().position(|frame| frame.plane.expect("plane").position.z == z).expect("image");
        f.viewer_mut().views.get_mut(&key).expect("view").set_slice(STACK, &data, index);
        f.steps(3);
    };
    set_z(&mut f, AXIAL, -10.0);
    set_z(&mut f, LATER_AXIAL, -3.0);

    f.open_compare_menu();
    f.snapshot("menu-link");
    f.click_label("Link from here");
    assert_eq!(f.harness.query_all_by_label("Link: manual").count(), 2);
    f.harness.key_press(Key::Escape);
    f.steps(2);
    f.snapshot("manual-link");

    set_z(&mut f, AXIAL, 0.0);
    assert_eq!(f.z(LATER_AXIAL), 7.0, "10 mm in the same direction");
}

/// Task 3.6 — the scenario of the specification through the mouse: zoom by a
/// factor of 1.5 and a pan of 40 points; the other keeps its difference.
#[test]
fn linked_viewports_follow_zoom_and_pan() {
    let mut f = side_by_side(AXIAL, FINE);
    let center = f.cell(0).center();
    f.viewer_mut().tool = Tool::Pan;
    f.harness.hover_at(center);
    f.steps(1);
    // Zoom of the left viewport by 1.5 at its centre.
    f.harness.event(Event::Zoom(1.5));
    f.steps(3);
    assert!((f.view(AXIAL).transform(STACK).zoom - 1.5).abs() < 1e-3);
    assert!((f.view(FINE).transform(STACK).zoom - 1.5).abs() < 1e-3, "the same factor");

    // 40 points to the right, with the Pan tool.
    let before = f.view(FINE).transform(STACK).pan;
    let (left_before, _) = (f.view(AXIAL).transform(STACK).pan, ());
    f.harness.drag_at(center);
    f.steps(1);
    f.harness.hover_at(center + vec2(40.0, 0.0));
    f.steps(2);
    f.harness.drop_at(center + vec2(40.0, 0.0));
    f.steps(3);
    let left = f.view(AXIAL).transform(STACK).pan;
    let right = f.view(FINE).transform(STACK).pan;
    assert!((left[0] - left_before[0] - 40.0).abs() < 1e-3, "{left:?}");
    assert!((right[0] - before[0] - 40.0).abs() < 1e-3, "{right:?}");
    assert!((right[1] - before[1]).abs() < 1e-3);

    // Double-click fits both.
    f.double_click_at(center);
    assert_eq!(f.view(AXIAL).transform(STACK).zoom, 1.0);
    assert!((f.view(FINE).transform(STACK).zoom - 1.0).abs() < 1e-3);
    assert!(f.view(FINE).transform(STACK).pan[0].abs() < 1e-3);

    // Link off: left alone.
    f.activate(1);
    f.press(Key::K);
    f.activate(0);
    f.harness.hover_at(center);
    f.steps(1);
    f.harness.event(Event::Zoom(1.5));
    f.steps(3);
    assert!((f.view(AXIAL).transform(STACK).zoom - 1.5).abs() < 1e-3);
    assert!((f.view(FINE).transform(STACK).zoom - 1.0).abs() < 1e-3);
}

/// Task 3.5 and 6.2 — the line of the axial slice on the sagittal series is
/// where its height is, in the picture as it is shown, and goes with annotations.
#[test]
fn the_cross_reference_line_lies_at_the_height_of_the_active_slice() {
    let mut f = side_by_side(AXIAL, SAGITTAL);
    let set_z = |f: &mut Fixture, z: f64| {
        let data = f.data(AXIAL);
        let index = data.frames().iter().position(|frame| frame.plane.expect("plane").position.z == z).expect("image");
        f.viewer_mut().views.get_mut(&AXIAL).expect("view").set_slice(STACK, &data, index);
        f.steps(3);
    };
    let line = |f: &Fixture| f.viewer().cross_lines.get(&1).and_then(|lines| lines.first().copied());

    set_z(&mut f, 10.0);
    let [a, b] = line(&f).expect("a line in the other viewport");
    // Row r of the sagittal image lies at z = 63 - 2 r mm: z = 10 mm is row 26.5.
    let at_10 = f.on_screen(1, SAGITTAL, 0.0, 26.5).y;
    assert!((a.y - at_10).abs() < 0.01 && (b.y - at_10).abs() < 0.01, "{a:?} {b:?}, expected at y = {at_10}");
    // Across the whole picture.
    let picture = Placement::of(
        f.cell(1),
        &f.viewer().renders.get(SAGITTAL, STACK).expect("rendered").displayed,
        &f.view(SAGITTAL).transform(STACK),
    )
    .rect;
    assert!((a.x.min(b.x) - picture.left()).abs() < 0.01 && (a.x.max(b.x) - picture.right()).abs() < 0.01);
    assert!(f.viewer().cross_lines.get(&0).is_none_or(Vec::is_empty), "none in the active viewport");
    f.snapshot("cross-reference");

    // Towards the head it moves up.
    set_z(&mut f, 30.0);
    let [higher, _] = line(&f).expect("a line");
    assert!(higher.y < a.y, "{higher:?} above {a:?}");
    let at_30 = f.on_screen(1, SAGITTAL, 0.0, 16.5).y;
    assert!((higher.y - at_30).abs() < 0.01);

    // In the picture as it is shown: mirrored top to bottom, it moves the other way.
    f.viewer_mut().views.get_mut(&SAGITTAL).expect("view").transform_mut(STACK).flip_vertically();
    f.steps(3);
    let [flipped, _] = line(&f).expect("a line");
    let centre = f.cell(1).center().y;
    assert!(higher.y < centre && flipped.y > centre, "{higher:?} and {flipped:?} on either side of {centre}");
    assert!((flipped.y - (2.0 * centre - higher.y)).abs() < 0.01);

    // Hidden with the annotations, back with them.
    f.press(Key::O);
    assert_eq!(line(&f), None);
    f.press(Key::O);
    assert!(line(&f).is_some());

    // The active viewport now the sagittal one: the line of a sagittal image on the axial one is vertical.
    f.activate(1);
    f.steps(2);
    let [c, d] = f.viewer().cross_lines[&0][0];
    assert!((c.x - d.x).abs() < 0.01 && (c.y - d.y).abs() > 10.0, "{c:?} {d:?}");

    // Parallel planes have no line, nor series of another examination.
    let g = side_by_side(AXIAL, FINE);
    assert!(g.viewer().cross_lines.get(&1).is_none_or(Vec::is_empty));
    let h = side_by_side(AXIAL, LATER_SAGITTAL);
    assert!(h.viewer().cross_lines.get(&1).is_none_or(Vec::is_empty));
}

/// Task 2.4 — series and date stay when annotations are hidden and are never
/// run over by the link indicator: at 960 points they are shortened.
#[test]
fn every_viewport_says_which_series_and_which_date_also_in_a_narrow_window() {
    for width in [1440.0, 960.0] {
        let mut f = studies(ComparisonOptions::default(), SAGITTAL, [width, 600.0], Theme::Dark);
        f.command(Command::CompareStudies { earlier: phantom::EARLIER, later: phantom::LATER });
        f.wait_for_series(LATER_SAGITTAL);
        let (first, second) = ("T2 TSE SAG (synthetic) · 2025-03-02", "T2 TSE SAG (synthetic) · 2026-01-15");
        for hidden in [false, true] {
            if hidden {
                f.press(Key::O);
            }
            assert!(f.shows(first) && f.shows(second), "at {width} pt, annotations hidden: {hidden}");
            // The two lines stay in their viewports, clear of the indicators.
            for (index, line) in [(0, first), (1, second)] {
                let cell = f.cell(index);
                let text = f.rect_of(line, cell);
                let link = f.rect_of("Link: not linked", cell);
                assert!(text.right() <= link.left(), "at {width} pt: {text:?} runs into {link:?}");
                assert!(text.left() >= cell.left() && link.right() <= cell.right());
            }
        }
        f.snapshot(&format!("identity-{width}-annotations-off"));
        f.press(Key::O);
        f.snapshot(&format!("compare-studies-{width}"));
    }
}

/// The shortened line keeps the date, and fits.
#[test]
fn a_shortened_identity_line_keeps_the_date() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui.painter();
        let long = "A VERY LONG SERIES DESCRIPTION INDEED (synthetic)";
        let whole = compare::identity_line(painter, long, "2025-03-02", 1000.0);
        assert_eq!(whole, format!("{long} · 2025-03-02"));
        let short = compare::identity_line(painter, long, "2025-03-02", 140.0);
        assert!(short.ends_with("… · 2025-03-02"), "{short}");
        assert!(short.starts_with('A'));
        assert!(crate::widgets::text_width(painter, std::slice::from_ref(&short), 11.5) <= 140.5, "{short}");
    });
    output.textures_delta.clear();
}

/// The scenario of the specification: the follow-up on the same disc.
#[test]
fn compare_studies_opens_the_matching_series_with_the_earlier_study_on_the_left() {
    let mut f = open(SAGITTAL);
    f.open_compare_menu();
    assert!(f.shows("Compare studies"), "two studies of one patient");
    f.click_label("Compare studies");
    f.wait_for_series(LATER_SAGITTAL);
    let viewports = f.viewer().workspace.viewports();
    assert_eq!(viewports[0].series, Some(SAGITTAL));
    assert_eq!(viewports[1].series, Some(LATER_SAGITTAL));
    assert_eq!(f.viewer().workspace.grid(), Some(Grid::SideBySide));
    assert_eq!(f.viewer().workspace.active(), 0);
    assert!(f.shows("T2 TSE SAG (synthetic) · 2025-03-02") && f.shows("T2 TSE SAG (synthetic) · 2026-01-15"));
    let left = f.rect_of("T2 TSE SAG (synthetic) · 2025-03-02", f.cell(0));
    let right = f.rect_of("T2 TSE SAG (synthetic) · 2026-01-15", f.cell(1));
    assert!(left.left() < right.left(), "the earlier study on the left");

    // The two examinations share no scanner positions: nothing follows until the user links them.
    assert_eq!(f.harness.query_all_by_label("Link: not linked").count(), 2);
    let before = f.image(LATER_SAGITTAL);
    f.press(Key::ArrowDown);
    assert_eq!(f.image(LATER_SAGITTAL), before);

    // Replacing a side: a click in the browser puts another series of the study there.
    f.activate(1);
    f.click_label(phantom::AXIAL_5MM_NAME);
    f.wait_for_series(LATER_AXIAL);
    assert_eq!(f.viewer().workspace.viewports()[1].series, Some(LATER_AXIAL));
}

/// The study picker lists the other study without replacing what is shown.
#[test]
fn the_study_picker_only_lists_another_study_while_comparing() {
    let mut f = side_by_side(AXIAL, FINE);
    let shown = f.viewer().workspace.shown_series();
    f.viewer_mut().session.active_study = phantom::LATER;
    f.steps(3);
    assert_eq!(f.viewer().workspace.shown_series(), shown);
    assert!(f.shows(phantom::SAGITTAL_NAME));
    assert_eq!(f.viewer().session.active_series, Some(AXIAL));
    // A series clicked now goes into the active viewport.
    f.click_label(phantom::SAGITTAL_NAME);
    f.wait_for_series(LATER_SAGITTAL);
    assert_eq!(f.viewer().workspace.viewports()[0].series, Some(LATER_SAGITTAL));
    assert_eq!(f.viewer().session.active_study, phantom::LATER);
}

/// The scenario of the specification: no series of the same modality and plane.
#[test]
fn without_matching_series_each_study_opens_with_its_default_series() {
    let options = ComparisonOptions { matching: false, ..ComparisonOptions::default() };
    let mut f = studies(options, SAGITTAL, [1440.0, 900.0], Theme::Dark);
    f.command(Command::CompareStudies { earlier: phantom::EARLIER, later: phantom::LATER });
    let later_default = later(0);
    f.wait_for_series(later_default);
    let viewports = f.viewer().workspace.viewports();
    assert_eq!(viewports[0].series, f.viewer().session.default_series(phantom::EARLIER));
    assert_eq!(viewports[1].series, Some(later_default));
    assert!(f.shows("CT COR (synthetic) · 2026-01-15"));
}

/// Offered only with two studies of the same patient.
#[test]
fn compare_studies_is_not_offered_for_one_study_or_for_two_patients() {
    let mut f = ct([1440.0, 900.0]);
    f.open_compare_menu();
    assert!(f.shows("Two series side by side"), "the menu is open");
    assert!(!f.shows_containing("Compare studies"));
    // The studies of another patient do not count: from a study of its own, the other two are offered.
    let options = ComparisonOptions { other_patient: true, ..ComparisonOptions::default() };
    let other = SeriesKey { study: phantom::OTHER_PATIENT, series: 0 };
    let mut f = studies(options, other, [1440.0, 900.0], Theme::Dark);
    f.open_compare_menu();
    assert!(f.shows("Compare studies"));
    let session = &f.viewer().session;
    assert_eq!(summrise_core::workspace::comparable_studies(session.studies()), [(phantom::EARLIER, phantom::LATER)]);
}

/// Binding decision 5 — a PET over a CT stays as it is in a layout of one
/// series; in a multi-series layout it is that of the active viewport's series,
/// and a viewport beside it says it has none now instead of showing an old one.
#[test]
fn a_pet_overlay_is_drawn_in_the_active_viewport_only_and_the_others_say_so() {
    let write = |dir: &Path| {
        write_pet_study(dir, &PetStudyOptions::default()).expect("fixture");
    };
    let ct = SeriesKey { study: 0, series: pet_phantom::CT };
    let pet = SeriesKey { study: 0, series: pet_phantom::PET };
    let mut f = start(write, Some(ct), Settings::default(), [1440.0, 900.0], Theme::Dark);
    f.wait_for_series(ct);
    f.viewer_mut().views.get_mut(&ct).expect("view").set_fusion(Some(Fusion::new(pet)));
    f.run_until("the overlay", |f| {
        matches!(f.viewer().session.overlay_status(), Some(summrise_core::OverlayStatus::Ready(_)))
    });
    f.steps(3);
    assert_eq!(f.viewer().session.overlay(), Some(pet));

    // Side by side with the PET series itself: the CT is active and keeps its overlay.
    f.command(Command::Grid(Some(Grid::SideBySide)));
    f.assign(1, pet);
    f.activate(0);
    f.steps(2);
    assert_eq!(f.viewer().session.overlay(), Some(pet));
    assert!(!f.shows("PET overlay only in the active viewport"));
    assert!(f.viewer().renders.get(ct, STACK).expect("rendered").draws_overlay());
    assert!(!f.viewer().renders.get(pet, STACK).expect("rendered").draws_overlay());
    f.snapshot("pet-active");

    // The PET viewport active: the session keeps no overlay, and the CT says it has none now.
    f.activate(1);
    f.steps(2);
    assert_eq!(f.viewer().session.overlay(), None);
    assert!(f.shows("PET overlay only in the active viewport"), "not a stale overlay without a word");
    assert!(!f.viewer().renders.get(ct, STACK).expect("rendered").draws_overlay(), "the picture without it");
    assert!(f.view(ct).fusion().is_some(), "the choice stays with the series");
    assert!(f.viewer().session.loaded(ct).is_some(), "both stay loaded");
    f.snapshot("pet-elsewhere");

    // Back: it is drawn again.
    f.activate(0);
    f.run_until("the overlay again", |f| {
        matches!(f.viewer().session.overlay_status(), Some(summrise_core::OverlayStatus::Ready(_)))
    });
    f.steps(2);
    assert!(!f.shows("PET overlay only in the active viewport"));
    assert_eq!(f.viewer().session.overlay(), Some(pet));
    assert!(f.viewer().renders.get(ct, STACK).expect("rendered").draws_overlay());

    // And in the layouts of one series as ever.
    f.click_label("Images");
    assert_eq!(f.viewer().session.active_series, Some(ct));
    assert_eq!(f.viewer().session.overlay(), Some(pet));
    assert_eq!(f.viewer().workspace.viewports().len(), 1);
    f.steps(2);
    assert!(f.viewer().renders.get(ct, STACK).expect("rendered").draws_overlay());
}

/// Task 6.4 — both stay on screen and the notice names the setting.
#[test]
fn over_the_memory_limit_both_stay_and_it_is_said() {
    let mut f = side_by_side(AXIAL, FINE);
    assert!(!f.shows_containing("Over the memory limit"));
    f.viewer_mut().session.set_budget(1 << 16);
    f.steps(3);
    assert!(f.shows_containing("Over the memory limit"), "in the status bar");
    assert!(f.viewer().session.loaded(AXIAL).is_some() && f.viewer().session.loaded(FINE).is_some());
    assert!(f.shows("Image 13 / 25") && f.shows("Image 25 / 49"), "both remain displayed");
    let over = f.viewer().session.over_budget().expect("over the limit");
    let notice = super::workspace::memory_notice(over);
    assert_eq!(
        notice,
        "The series on screen need about 0.6 MB together, more than the memory limit of 0.1 MB. They stay loaded; the limit can be raised in Settings."
    );
    assert_eq!(f.harness.state().toasts.count_for_test(), 1, "and once as a notification");
    f.snapshot("memory-notice");
}

// ---------------------------------------------------------------------------
// Tile grids

/// The numbers on the tiles on screen, top left first: what is labelled
/// "Image n / 60" inside `cell`, by position.
fn tiles_on_screen(f: &Fixture, cell: Rect) -> Vec<usize> {
    let mut tiles: Vec<(i32, i32, usize)> = f
        .harness
        .query_all(by().role(Role::Image))
        .filter_map(|node| {
            let label = node.accesskit_node().label()?;
            let number = label.strip_prefix("Image ")?.split(" / ").next()?.parse().ok()?;
            let rect = node.rect();
            cell.contains_rect(rect).then_some((rect.top() as i32, rect.left() as i32, number))
        })
        .collect();
    tiles.sort_unstable();
    tiles.into_iter().map(|(_, _, number)| number).collect()
}

/// Task 5.1 to 5.4 — the scenario of the specification with its numbers: T,
/// one step, Page Down, the end of the series and a tile opened.
#[test]
fn nine_tiles_of_a_ct_series_with_their_numbers() {
    let mut f = ct([1440.0, 900.0]);
    let cell = f.viewer().central;
    assert_eq!(f.image(CT), 31, "the series opens in the middle");
    assert_eq!(f.viewer().cells[0].tiles, Tiles::ONE);

    f.press(Key::T);
    assert_eq!(f.viewer().cells[0].tiles, Tiles::ALL[1]);
    f.press(Key::T);
    assert_eq!(f.viewer().cells[0].tiles, Tiles::ALL[2]);
    let cell_rect = f.cell(0);
    assert_eq!(cell_rect, cell);
    assert_eq!(tiles_on_screen(&f, cell), (31..=39).collect::<Vec<_>>());
    f.snapshot("tiles-1440");

    // One step: the grid moves by one image.
    f.press(Key::ArrowDown);
    assert_eq!(tiles_on_screen(&f, cell), (32..=40).collect::<Vec<_>>());
    // The same with the wheel.
    f.scroll_at(cell.center(), -18.0);
    assert_eq!(f.image(CT), 33);
    f.scroll_at(cell.center(), 18.0);
    assert_eq!(tiles_on_screen(&f, cell), (32..=40).collect::<Vec<_>>());
    // Page Down from there: a whole grid.
    f.press(Key::PageDown);
    assert_eq!(tiles_on_screen(&f, cell), (41..=49).collect::<Vec<_>>());
    f.press(Key::PageUp);
    assert_eq!(tiles_on_screen(&f, cell), (32..=40).collect::<Vec<_>>());

    // The end of the series: the rest stays empty, and it goes no further than image 60.
    let data = f.data(CT);
    f.viewer_mut().views.get_mut(&CT).expect("view").set_slice(STACK, &data, 57);
    f.steps(3);
    assert_eq!(tiles_on_screen(&f, cell), [58, 59, 60]);
    f.snapshot("tiles-end-of-series");
    for _ in 0..5 {
        f.press(Key::ArrowDown);
    }
    assert_eq!(tiles_on_screen(&f, cell), [60]);
    f.press(Key::PageDown);
    assert_eq!(f.image(CT), 60);

    // Opening one tile: one image again, that one.
    let data = f.data(CT);
    f.viewer_mut().views.get_mut(&CT).expect("view").set_slice(STACK, &data, 30);
    f.steps(3);
    assert_eq!(tiles_on_screen(&f, cell), (31..=39).collect::<Vec<_>>());
    let tile_of_35 = f.rect_of("Image 35 / 60", cell);
    f.double_click_at(tile_of_35.center());
    assert_eq!(f.viewer().workspace.viewports()[0].tiles, Tiles::ONE);
    assert_eq!(f.viewer().cells[0].tiles, Tiles::ONE);
    assert_eq!(f.image(CT), 35);
    assert!(f.shows("Image 35 / 60"));
    // T goes round: to the largest grid and back to one image.
    for _ in 0..4 {
        f.press(Key::T);
    }
    assert_eq!(f.viewer().cells[0].tiles, Tiles::ALL[4]);
    assert_eq!(f.viewer().renders.entries_for_test(), 25, "a picture per tile, no more");
    f.snapshot("tiles-5-by-5");
    f.press(Key::T);
    assert_eq!(f.viewer().cells[0].tiles, Tiles::ONE);
    f.steps(2);
    assert_eq!(f.viewer().renders.entries_for_test(), 1, "the pictures of the tiles are gone");
    assert_eq!(f.image(CT), 35, "the current image stayed");
}

/// A single click in a tile keeps the grid where it is: there is no selected tile.
#[test]
fn a_click_in_a_tile_moves_nothing() {
    let mut f = ct([1440.0, 900.0]);
    f.press(Key::T);
    f.press(Key::T);
    let cell = f.cell(0);
    let tile = f.rect_of("Image 35 / 60", cell);
    f.click_at(tile.center());
    assert_eq!(f.image(CT), 31);
    assert_eq!(tiles_on_screen(&f, cell), (31..=39).collect::<Vec<_>>());
    // Also not in the first one.
    let tile = f.rect_of("Image 31 / 60", cell);
    f.click_at(tile.center());
    assert_eq!(f.image(CT), 31);
}

/// The scenario of the specification: what is under the pointer is of the
/// image of that tile. The marker sphere, 300 HU, tells images apart.
#[test]
fn the_value_under_the_pointer_is_of_the_image_of_that_tile() {
    let mut f = ct([1440.0, 900.0]);
    f.press(Key::T);
    f.press(Key::T);
    let cell = f.cell(0);
    let data = f.data(CT);
    // The image through the centre of the marker, in a tile that is not the first.
    let marker = MARKER_POSITION;
    let through = (0..60)
        .min_by(|&a, &b| {
            let distance = |i: usize| (data.frames()[i].plane.expect("plane").position.z - marker[2]).abs();
            distance(a).total_cmp(&distance(b))
        })
        .expect("images");
    let first = through.saturating_sub(4);
    f.viewer_mut().views.get_mut(&CT).expect("view").set_slice(STACK, &data, first);
    f.steps(3);
    let number = through + 1;
    let tile = f.rect_of(&format!("Image {number} / 60"), cell);
    let plane = data.frames()[through].plane.expect("plane");
    let spacing = plane.col_spacing;
    let (x, y) = ((marker[0] - plane.position.x) / spacing, (marker[1] - plane.position.y) / spacing);

    let displayed = &f.viewer().renders.get_tile(CT, through).expect("rendered").displayed;
    let place = Placement::of(tile, displayed, &f.view(CT).transform(STACK));
    let on_marker = place.to_screen(x, y);
    f.harness.hover_at(on_marker);
    f.steps(2);
    let hover = f.viewer().hover.expect("under the pointer").1;
    assert_eq!(hover.image, Some(number));
    assert_eq!(hover.series, CT);
    assert_eq!(hover.pixel, (x.round() as usize, y.round() as usize));
    assert_eq!(hover.value, Some(summrise_testkit::CT_MARKER_HU as f32));
    let patient = hover.patient.expect("position");
    assert!((patient.z - plane.position.z).abs() < 1e-6, "the position of that image");
    assert!((patient.x - marker[0]).abs() < spacing && (patient.y - marker[1]).abs() < spacing);
    assert!(
        f.shows_containing(&format!("Image {number}   ·   x {} y {}", hover.pixel.0, hover.pixel.1)),
        "in the status bar"
    );
    assert!(f.shows_containing("300 HU"), "with the value");

    // The same place in the first tile, an image without the marker at that spot: another value.
    let first_tile = f.rect_of(&format!("Image {} / 60", first + 1), cell);
    let displayed = &f.viewer().renders.get(CT, STACK).expect("rendered").displayed;
    let place = Placement::of(first_tile, displayed, &f.view(CT).transform(STACK));
    f.harness.hover_at(place.to_screen(x, y));
    f.steps(2);
    let hover = f.viewer().hover.expect("under the pointer").1;
    assert_eq!(hover.image, Some(first + 1));
    assert_ne!(hover.value, Some(summrise_testkit::CT_MARKER_HU as f32));

    // All tiles share contrast and zoom: a zoom in a tile applies to every one.
    f.harness.hover_at(tile.center());
    f.steps(1);
    f.harness.event(Event::Zoom(2.0));
    f.steps(3);
    assert!((f.view(CT).transform(STACK).zoom - 2.0).abs() < 1e-3);
    // Its centre stays where it is: the zoom is about the point in that tile.
    assert!(f.view(CT).transform(STACK).pan[0].abs() < 0.01);
    // Cleared for the next image: the pictures follow the window.
    f.press(Key::Num2);
    f.steps(2);
    assert_eq!(f.view(CT).preset.as_deref(), Some("Lung"));
}

/// Reformats and the 3D view offer no tiles; T leaves them alone; a grid too
/// small is not shown, and comes back when there is room.
#[test]
fn reformats_have_no_tiles_and_grids_shrink_with_the_viewport() {
    let mut f = ct([1440.0, 900.0]);
    f.click_label("MPR");
    assert!(matches!(f.viewer().active_view, ViewKind::Mpr(_)));
    f.harness.remove_cursor();
    f.steps(2);
    f.press(Key::T);
    assert!(f.viewer().workspace.viewports().iter().all(|viewport| viewport.tiles == Tiles::ONE));
    assert!(f.viewer().cells.iter().all(|cell| cell.tiles == Tiles::ONE));
    f.open_compare_menu();
    let disabled = |f: &Fixture, name: &str| f.harness.get_by_label(name).accesskit_node().is_disabled();
    assert!(disabled(&f, "2 × 2 images"), "no tiles of a reformat");
    assert!(disabled(&f, "5 × 5 images"));
    assert!(!disabled(&f, "1 × 1 images"));
    f.harness.key_press(Key::Escape);
    f.steps(2);

    // The acquired viewport of the 2 × 2 layout can show them.
    f.click_label("2 × 2");
    f.viewer_mut().active_view = STACK;
    f.steps(2);
    f.harness.remove_cursor();
    f.steps(2);
    f.viewer_mut().active_view = STACK;
    f.press(Key::T);
    assert_eq!(f.viewer().workspace.viewports()[0].view, STACK);
    assert_eq!(f.viewer().workspace.viewports()[0].tiles, Tiles::ALL[1]);
    assert_eq!(f.viewer().cells[0].tiles, Tiles::ALL[1]);
    assert_eq!(f.viewer().cells[1].tiles, Tiles::ONE);
    f.snapshot("tiles-in-quad");
    // Reformats elsewhere stay as they are.
    assert_eq!(f.viewer().workspace.viewports()[1].view, ViewKind::Mpr(MprAxis::Axial));
}

/// Task 5.3 and 5.4 — at the smallest window, with both panels, the menu offers
/// what fits and T goes no further; the choice is kept and applies again with room.
#[test]
fn only_grids_with_tiles_of_at_least_96_points_are_offered_in_a_small_window() {
    let mut f = ct([960.0, 600.0]);
    let cell = f.cell(0);
    let size = tiles::area(cell, false).size();
    let expected: Vec<Tiles> = Tiles::ALL.into_iter().filter(|tiles| tiles.fits([size.x, size.y])).collect();
    assert!(expected.len() >= 2 && expected.len() < 5, "{size:?}: {expected:?}");

    f.open_compare_menu();
    for tiles in Tiles::ALL {
        let name = format!("{0} images", tiles.label());
        assert_eq!(
            f.harness.get_by_label(&name).accesskit_node().is_disabled(),
            !expected.contains(&tiles),
            "{name} in a viewport of {size:?}"
        );
    }
    f.harness.key_press(Key::Escape);
    f.steps(2);

    // T goes round the grids that fit, and no further.
    let mut seen = vec![Tiles::ONE];
    for _ in 0..expected.len() - 1 {
        f.press(Key::T);
        seen.push(f.viewer().cells[0].tiles);
    }
    assert_eq!(seen, expected);
    f.snapshot("tiles-960");
    let tile_side = crate::tiles::rects(tiles::area(cell, false), *expected.last().expect("a grid"))[0].size();
    assert!(tile_side.x >= 96.0 && tile_side.y >= 96.0, "{tile_side:?}");
    f.press(Key::T);
    assert_eq!(f.viewer().cells[0].tiles, Tiles::ONE, "and back to one image");

    // A grid chosen in a larger viewport falls back to the largest that fits.
    let viewport = Rect::from_min_size(Pos2::ZERO, vec2(320.0, 600.0));
    let data = f.data(CT);
    assert_eq!(tiles::shown(Tiles::ALL[4], viewport, true, STACK, &data), Tiles::ALL[2]);
    assert_eq!(tiles::shown(Tiles::ALL[4], viewport, true, ViewKind::Mpr(MprAxis::Axial), &data), Tiles::ONE);
}

/// Tiles in a multi-series layout: per viewport, with their images, and the
/// cross-reference line drawn on each picture; the active viewport's line
/// follows its first tile.
#[test]
fn tiles_of_one_viewport_beside_another_series_and_their_cross_reference_lines() {
    let mut f = side_by_side(AXIAL, SAGITTAL);
    f.press(Key::T);
    assert_eq!(f.viewer().cells[0].tiles, Tiles::ALL[1]);
    assert_eq!(f.viewer().cells[1].tiles, Tiles::ONE);
    assert_eq!(tiles_on_screen(&f, f.cell(0)).len(), 4);
    let shown = f.image(AXIAL);
    assert!(f.shows(&format!("Image {shown} / 25")));
    assert!(f.shows(&format!("Image {} / 25", shown + 3)));
    f.steps(2);
    assert_eq!(f.viewer().cross_lines[&1].len(), 1, "the line of the current image of the active viewport");
    f.snapshot("tiles-beside-a-series");

    // With the sagittal series active and in tiles: a line on every tile of the axial one.
    f.activate(1);
    f.press(Key::T);
    f.steps(2);
    assert_eq!(f.viewer().cross_lines[&0].len(), 4, "on each of the four pictures");
    f.press(Key::O);
    assert!(f.viewer().cross_lines[&0].is_empty(), "gone with the annotations");
    assert!(f.shows("T1 AX 5mm (synthetic) · 2025-03-02"), "the line with series and date stays");
    f.snapshot("tiles-annotations-off");
    // Privacy mode hides the patient in a grid as well: nothing here but numbers and series.
    f.press(Key::O);
    f.harness.state_mut().settings.hide_patient_details = true;
    f.steps(3);
    f.snapshot("tiles-privacy");
}

/// Task 2.1 in the light theme: the chrome changes, the viewports stay black,
/// and what they say stays legible.
#[test]
fn an_empty_viewport_says_what_to_do_in_both_themes() {
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        let mut f = studies(ComparisonOptions::default(), SAGITTAL, [1440.0, 900.0], theme);
        f.command(Command::Grid(Some(Grid::Four)));
        f.activate(1);
        assert_eq!(f.harness.query_all_by_label("Drop a series here").count(), 3);
        f.snapshot(&format!("empty-viewports-{name}"));
    }
    // Light text on the black viewport, whatever the theme.
    let contrast = |c: egui::Color32| {
        let l = |v: u8| f32::from(v) / 255.0;
        (0.2126 * l(c.r()) + 0.7152 * l(c.g()) + 0.0722 * l(c.b()) + 0.05) / 0.05
    };
    assert!(contrast(theme::OVERLAY_TEXT) > 7.0);
    assert!(contrast(theme::DARK.text_weak) > 4.5);
}

/// The toolbar with the Compare menu on its one new place, and the header of a
/// viewport, leave nothing overlapping in the narrowest window, with four viewports.
#[test]
fn four_viewports_in_the_smallest_window() {
    let mut f = studies(ComparisonOptions::default(), AXIAL, [960.0, 600.0], Theme::Dark);
    f.command(Command::Grid(Some(Grid::Four)));
    f.assign(1, FINE);
    f.assign(2, SAGITTAL);
    f.assign(3, SHORT);
    f.activate(0);
    f.steps(3);
    f.snapshot("four-960");
    // Series and date of every viewport inside it and clear of its indicator.
    for index in 0..4 {
        let cell = f.cell(index);
        let identity = f
            .harness
            .query_all(by().role(Role::Label))
            .map(|node| {
                let ak = node.accesskit_node();
                (ak.label().or_else(|| ak.value()).unwrap_or_default(), node.rect())
            })
            .find(|(label, rect)| label.contains(" · 2025-03-02") && cell.contains_rect(*rect));
        let (label, identity) = identity.unwrap_or_else(|| panic!("no line with the date in viewport {index}"));
        let link = f
            .harness
            .query_all_by_label_contains("Link: ")
            .map(|node| node.rect())
            .find(|rect| cell.contains_rect(*rect))
            .expect("an indicator");
        assert!(identity.right() <= link.left() + 0.5, "{label}: {identity:?} runs into {link:?}");
    }
}

/// While a series loads, a viewport beside it shows its own progress and
/// follows from the headers, with its place and date said.
#[test]
fn a_series_still_loading_shows_where_it_is_and_says_which_it_is() {
    use std::sync::atomic::{AtomicBool, Ordering};

    use summrise_core::dicom::{DecodedInstance, DicomError, InstanceHeader, Media};
    use summrise_core::{DicomFiles, ImageSource};

    /// Holds back the files of the 2.5 mm series until it is opened.
    #[derive(Default)]
    struct Gate {
        open: AtomicBool,
    }
    impl ImageSource for Gate {
        fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError> {
            let held = header.path.parent().is_some_and(|folder| folder.ends_with("ST000001/SE000002"));
            while held && !self.open.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            DicomFiles.read(media, header)
        }

        fn decode(&self, header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError> {
            DicomFiles.decode(header, bytes)
        }
    }

    let dir = tempfile::tempdir().expect("tempdir");
    write_comparison_studies(dir.path(), &ComparisonOptions::default()).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let settings = Settings::default();
    let gate = Arc::new(Gate::default());
    let mut session = Session::with_source(index, settings.memory_budget_bytes(), None, gate.clone());
    session.select_series(AXIAL);
    let app = App {
        viewer: Viewer::new(session, &settings),
        settings,
        waker: Arc::new(|| {}),
        actions: Vec::new(),
        toasts: Toasts::default(),
    };
    let mut builder =
        Harness::builder().with_size([1280.0, 800.0]).with_theme(Theme::Dark).with_context_setup(theme::install);
    if snapshot_dir().is_some() {
        builder = builder.wgpu();
    }
    let harness = builder.build_ui_state(
        |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
        app,
    );
    let mut f = Fixture { harness, _dir: dir };
    f.wait_for_series(AXIAL);

    f.command(Command::Grid(Some(Grid::SideBySide)));
    f.viewer_mut().workspace.assign(1, FINE);
    f.steps(3);
    // Nothing decoded: the other viewport waits with its number, and says which series it is.
    assert!(f.shows_containing("Loading image 25 / 49"));
    assert!(f.shows("T2 AX 2.5mm (synthetic) · 2025-03-02"));
    f.snapshot("still-loading");
    assert_eq!(f.viewer().session.active_series, Some(AXIAL), "and the active one stays active");

    // The user scrolls while it loads; when its first image arrives it opens at that position.
    let data = f.data(AXIAL);
    let index = data.frames().iter().position(|frame| frame.plane.expect("plane").position.z == -40.0).expect("image");
    f.viewer_mut().views.get_mut(&AXIAL).expect("view").set_slice(STACK, &data, index);
    f.steps(3);
    gate.open.store(true, Ordering::SeqCst);
    f.run_until("the first image", |f| f.viewer().views.contains_key(&FINE));
    f.steps(4);
    assert_eq!(f.z(FINE), -40.0);
    f.wait_for_series(FINE);
    assert_eq!(f.z(FINE), -40.0, "and stays there when loading completes");
}
