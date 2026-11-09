//! The viewport model with real series headers: positions in millimetres from
//! the comparison studies of the testkit.

use std::time::{Duration, Instant};

use summrise_dicom::{Progress, scan};
use summrise_testkit::comparison::{self as phantom, ComparisonOptions, write_comparison_studies};
use summrise_testkit::{PhantomOptions, write_phantom_study};

use super::*;
use crate::view::ViewTransform;

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
const STACK: ViewKind = ViewKind::Stack;

/// The comparison studies open, with the series in `shown` loaded and a view
/// for each; the first one active.
struct Fixture {
    session: Session,
    views: Views,
    workspace: Workspace,
    _dir: tempfile::TempDir,
}

fn open_with(options: &ComparisonOptions, grid: Grid, shown: &[SeriesKey]) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    write_comparison_studies(dir.path(), options).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let mut session = Session::new(index, 1 << 30, None);
    session.select_series(shown[0]);
    let mut workspace = Workspace::default();
    workspace.set_grid(grid, Some(shown[0]));
    for (i, &key) in shown.iter().enumerate().skip(1) {
        workspace.assign(i, key);
    }
    session.set_shown(&workspace.shown_series());
    let start = Instant::now();
    while !shown.iter().all(|&key| session.loaded(key).is_some()) {
        session.poll();
        session.set_shown(&workspace.shown_series());
        assert!(start.elapsed() < Duration::from_secs(30), "timed out loading");
        std::thread::sleep(Duration::from_millis(2));
    }
    let views = shown.iter().map(|&key| (key, ViewState::new(&session.data(key).expect("loaded")))).collect();
    let mut fixture = Fixture { session, views, workspace, _dir: dir };
    fixture.frame();
    fixture
}

fn open(grid: Grid, shown: &[SeriesKey]) -> Fixture {
    open_with(&ComparisonOptions::default(), grid, shown)
}

impl Fixture {
    /// What the viewer does once per frame.
    fn frame(&mut self) -> bool {
        self.workspace.follow(&self.session, &mut self.views)
    }

    fn data(&self, key: SeriesKey) -> std::sync::Arc<SeriesData> {
        self.session.data(key).expect("loaded")
    }

    /// Height of the image of `key` on screen, in mm.
    fn z(&self, key: SeriesKey) -> f64 {
        let data = self.data(key);
        data.frames()[self.views[&key].slot_index(&data)].plane.expect("plane").position.z
    }

    /// Scrolls the series of the active viewport to the image at height `z`, as the user does.
    fn go_to_z(&mut self, key: SeriesKey, z: f64) {
        let data = self.data(key);
        let index = data
            .frames()
            .iter()
            .position(|frame| (frame.plane.expect("plane").position.z - z).abs() < 1e-6)
            .unwrap_or_else(|| panic!("no image at {z} mm"));
        self.views.get_mut(&key).expect("view").set_slice(STACK, &data, index);
    }

    fn scroll(&mut self, key: SeriesKey, delta: i64) {
        let data = self.data(key);
        self.views.get_mut(&key).expect("view").scroll(STACK, &data, delta);
    }

    fn status(&self, index: usize) -> LinkStatus {
        self.workspace.link_status(index, &self.session, &self.views)
    }

    fn is_outside(&self, index: usize) -> bool {
        self.workspace.is_outside_range(index, &self.session, &self.views)
    }

    fn transform(&self, key: SeriesKey) -> ViewTransform {
        self.views[&key].transform(STACK)
    }
}

/// Task 6.2 — the scenario of the specification: a 5 mm series scrolled to
/// z = −40 mm and a linked 2.5 mm series; one step of the first moves the
/// second by two slices.
#[test]
fn a_linked_series_follows_by_position_at_another_slice_spacing() {
    let mut f = open(Grid::SideBySide, &[AXIAL, FINE]);
    assert_eq!(f.workspace.link_between(0, 1, &f.session, &f.views), Some(LinkKind::Position));
    assert_eq!((f.status(0), f.status(1)), (LinkStatus::Linked, LinkStatus::Linked));

    f.go_to_z(AXIAL, -40.0);
    assert!(f.frame(), "the other view moved");
    assert_eq!(f.z(AXIAL), -40.0);
    assert_eq!(f.z(FINE), -40.0);
    let fine_at_40 = f.views[&FINE].slice;

    // Towards the feet by one step of the 5 mm series, wherever that lies in its image numbers.
    let height = |index: usize| f.data(AXIAL).frames()[index].plane.expect("plane").position.z;
    let direction = if height(1) > height(0) { -1 } else { 1 };
    f.scroll(AXIAL, direction);
    assert_eq!(f.z(AXIAL), -45.0);
    assert!(f.frame());
    assert_eq!(f.z(FINE), -45.0);
    assert_eq!(f.views[&FINE].slice.abs_diff(fine_at_40), 2, "two slices of the finer series");
    assert!(!f.frame(), "nothing more moves without a change");

    // The other way round, the 2.5 mm series active: the 5 mm one shows its nearest slice.
    f.workspace.activate(1);
    assert!(!f.frame(), "switching the active viewport moves nothing");
    f.go_to_z(FINE, -42.5);
    f.frame();
    // Equally near to -40 and -45: one of them, in any case within half a gap.
    assert!((f.z(AXIAL) + 42.5).abs() <= 2.5 + 1e-9);
    f.go_to_z(FINE, -30.0);
    f.frame();
    assert_eq!(f.z(AXIAL), -30.0);
}

#[test]
fn a_viewport_keeps_following_while_the_series_still_loads() {
    let mut f = open(Grid::SideBySide, &[AXIAL, FINE]);
    // The view exists, the image at the target is not decoded: from the headers all the same.
    let start = f.z(FINE);
    f.go_to_z(AXIAL, 20.0);
    f.frame();
    assert_eq!(f.z(FINE), 20.0);
    assert_ne!(start, 20.0);
    // The geometry is taken from the frames of the plan, which are there before any pixels.
    let planned = SeriesData::new(summrise_dicom::plan_series(f.session.series(FINE).expect("indexed")));
    assert_eq!(planned.loaded(), 0);
    let direction = DVec3::Z;
    let slices = slices_of(&planned, None, direction);
    assert_eq!(slices.len(), 49);
    assert!(slices.iter().all(Option::is_some));
    let nearest = crossref::nearest_slice(&slices, DVec3::new(0.0, 0.0, 20.0)).expect("slices");
    assert_eq!(planned.frames()[nearest.index].plane.expect("plane").position.z, 20.0);
}

/// Positions count through the image set shown: the follower goes to the
/// image at that height of the set it shows, and changing a set moves nothing.
#[test]
fn a_series_split_into_image_sets_follows_within_the_set_it_shows() {
    use summrise_testkit::stacks::{self as stacks, write_stacks_study};
    const DYNAMIC: SeriesKey = SeriesKey { study: 0, series: stacks::DYNAMIC };
    const ECHOES: SeriesKey = SeriesKey { study: 0, series: stacks::DUAL_ECHO };

    let dir = tempfile::tempdir().expect("tempdir");
    write_stacks_study(dir.path()).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let mut session = Session::new(index, 1 << 30, None);
    session.select_series(DYNAMIC);
    let mut workspace = Workspace::default();
    workspace.set_grid(Grid::SideBySide, Some(DYNAMIC));
    workspace.assign(1, ECHOES);
    session.set_shown(&workspace.shown_series());
    let start = Instant::now();
    while session.loaded(DYNAMIC).is_none() || session.loaded(ECHOES).is_none() {
        session.poll();
        assert!(start.elapsed() < Duration::from_secs(30), "timed out loading");
        std::thread::sleep(Duration::from_millis(2));
    }
    let (dynamic, echoes) = (session.data(DYNAMIC).expect("open"), session.data(ECHOES).expect("open"));
    let mut views: Views =
        [(DYNAMIC, ViewState::new(&dynamic)), (ECHOES, ViewState::new(&echoes))].into_iter().collect();
    workspace.follow(&session, &mut views);
    let height =
        |data: &SeriesData, state: &ViewState| data.frames()[state.slot_index(data)].plane.expect("plane").position.z;

    // The second echo on the right; the left goes to another height.
    views.get_mut(&ECHOES).expect("view").set_stack(&echoes, 1);
    views.get_mut(&DYNAMIC).expect("view").set_slice(STACK, &dynamic, 2);
    assert!(workspace.follow(&session, &mut views));
    let (left, right) = (&views[&DYNAMIC], &views[&ECHOES]);
    assert_eq!(height(&echoes, right), height(&dynamic, left));
    assert_eq!(right.stack(), 1, "it stays in its image set");
    assert!(right.slot_index(&echoes) >= echoes.stack_len(), "an image of the second set");

    // Another time point at the same height: nothing moves.
    let slot = right.slot_index(&echoes);
    views.get_mut(&DYNAMIC).expect("view").set_stack(&dynamic, 2);
    assert!(!workspace.follow(&session, &mut views));
    assert_eq!(views[&ECHOES].slot_index(&echoes), slot);
}

/// A series assigned later, its first image not decoded yet, opens at the
/// position of the viewport beside it.
#[test]
fn a_series_put_beside_another_opens_at_its_position() {
    let mut f = open(Grid::SideBySide, &[AXIAL]);
    f.go_to_z(AXIAL, -40.0);
    f.frame();

    f.workspace.assign(1, FINE);
    f.session.set_shown(&f.workspace.shown_series());
    // Its view does not exist yet: it waits and moves nothing.
    assert!(!f.frame());
    assert_eq!(f.z(AXIAL), -40.0);
    let start = Instant::now();
    while f.session.data(FINE).is_none_or(|data| !data.is_viewable()) {
        f.session.poll();
        assert!(start.elapsed() < Duration::from_secs(30), "timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
    f.views.insert(FINE, ViewState::new(&f.data(FINE)));
    assert!(f.frame());
    assert_eq!(f.z(FINE), -40.0);
    assert_eq!(f.z(AXIAL), -40.0, "the other viewport stays where it is");
}

#[test]
fn beyond_the_coverage_of_a_series_it_stays_on_its_end_slice_and_says_so() {
    // The short series covers -20 to +20 mm.
    let mut f = open(Grid::SideBySide, &[AXIAL, SHORT]);
    f.go_to_z(AXIAL, 20.0);
    f.frame();
    assert_eq!(f.z(SHORT), 20.0);
    assert!(!f.is_outside(1));
    f.go_to_z(AXIAL, 25.0);
    f.frame();
    assert_eq!(f.z(SHORT), 20.0, "on its nearest end slice");
    assert!(f.is_outside(1));
    assert!(!f.is_outside(0), "the active viewport is never outside");
    f.go_to_z(AXIAL, 60.0);
    f.frame();
    assert_eq!(f.z(SHORT), 20.0);
    assert!(f.is_outside(1));
    f.go_to_z(AXIAL, -60.0);
    f.frame();
    assert_eq!(f.z(SHORT), -20.0, "the other end");
    assert!(f.is_outside(1));
    f.go_to_z(AXIAL, 0.0);
    f.frame();
    assert_eq!(f.z(SHORT), 0.0);
    assert!(!f.is_outside(1));
}

#[test]
fn a_viewport_with_its_link_off_is_left_alone() {
    let mut f = open(Grid::SideBySide, &[AXIAL, FINE]);
    f.go_to_z(AXIAL, 0.0);
    f.frame();
    assert_eq!(f.z(FINE), 0.0);

    f.workspace.toggle_link(1);
    assert_eq!(f.status(1), LinkStatus::Off);
    assert_eq!(f.status(0), LinkStatus::NotLinked(NotLinked::OtherLinkOff), "and the other one says why it is alone");
    assert_eq!(f.workspace.link_between(0, 1, &f.session, &f.views), None);
    f.go_to_z(AXIAL, 25.0);
    assert!(!f.frame());
    assert_eq!(f.z(FINE), 0.0, "no longer moved");
    assert!(!f.is_outside(1));

    // And it no longer drives the other one.
    f.workspace.activate(1);
    f.go_to_z(FINE, 10.0);
    assert!(!f.frame());
    assert_eq!(f.z(AXIAL), 25.0);

    // Switched on again: as the active viewport, the others come to it.
    f.workspace.toggle_link(1);
    assert_eq!(f.status(1), LinkStatus::Linked);
    assert!(f.frame());
    assert_eq!(f.z(AXIAL), 10.0);
    // As another viewport: it goes to the active one.
    f.workspace.activate(0);
    f.workspace.toggle_link(1);
    f.go_to_z(AXIAL, -5.0);
    f.frame();
    assert_eq!(f.z(FINE), 10.0);
    f.workspace.toggle_link(1);
    f.frame();
    assert_eq!(f.z(FINE), -5.0);
}

/// Different slice directions and different Frames of Reference are not
/// linked by position, and the indicator can say why.
#[test]
fn series_are_linked_by_position_only_when_they_can_be() {
    let mut f = open(Grid::SideBySide, &[AXIAL, SAGITTAL]);
    assert_eq!(f.workspace.link_between(0, 1, &f.session, &f.views), None);
    assert_eq!(f.status(1), LinkStatus::NotLinked(NotLinked::OtherDirection));
    let x = f.views[&SAGITTAL].slice;
    f.go_to_z(AXIAL, 40.0);
    assert!(!f.frame());
    assert_eq!(f.views[&SAGITTAL].slice, x);
    assert!(!f.is_outside(1));

    let f2 = open(Grid::SideBySide, &[AXIAL, LATER_AXIAL]);
    assert_eq!(f2.status(1), LinkStatus::NotLinked(NotLinked::NoCommonFrame));
    assert_eq!(f2.status(0), LinkStatus::NotLinked(NotLinked::NoCommonFrame));

    let mut alone = open(Grid::SideBySide, &[AXIAL]);
    assert_eq!(alone.status(0), LinkStatus::NotLinked(NotLinked::Alone));
    assert!(!alone.frame());
}

/// The scenario of the specification: two studies aligned by hand.
#[test]
fn link_from_here_keeps_the_offset_between_two_studies() {
    let mut f = open(Grid::SideBySide, &[AXIAL, LATER_AXIAL]);
    assert!(f.workspace.can_link_from_here(&f.session, &f.views));
    // The user finds the same vertebra in both: -10 mm here, 7 mm higher in the later study's coordinates.
    f.go_to_z(AXIAL, -10.0);
    f.go_to_z(LATER_AXIAL, -3.0);
    f.frame();
    assert_eq!(f.z(LATER_AXIAL), -3.0, "not linked yet");
    f.workspace.link_from_here(&f.session, &f.views);
    assert_eq!((f.status(0), f.status(1)), (LinkStatus::Manual, LinkStatus::Manual));
    assert_eq!(f.workspace.link_between(0, 1, &f.session, &f.views), Some(LinkKind::Manual));
    assert!(!f.frame(), "linking moves nothing");

    // 10 mm towards the head.
    f.go_to_z(AXIAL, 0.0);
    assert!(f.frame());
    assert_eq!(f.z(LATER_AXIAL), 7.0);
    // Back and on towards the feet: the same offset, also when the distances are not whole steps.
    f.go_to_z(AXIAL, -20.0);
    f.frame();
    assert_eq!(f.z(LATER_AXIAL), -13.0);
    // From the other viewport it works the other way round.
    f.workspace.activate(1);
    assert!(!f.frame());
    f.go_to_z(LATER_AXIAL, 2.0);
    assert!(f.frame());
    assert_eq!(f.z(AXIAL), -5.0);
    f.go_to_z(LATER_AXIAL, 67.0);
    f.frame();
    assert_eq!(f.z(AXIAL), 60.0, "the last slice of both");
    assert!(!f.is_outside(0));
    // Unlinked, aligned anew and linked again: now the other one ends first.
    f.workspace.automatic_links();
    f.go_to_z(LATER_AXIAL, 2.0);
    f.go_to_z(AXIAL, 5.0);
    f.frame();
    assert_eq!((f.z(LATER_AXIAL), f.z(AXIAL)), (2.0, 5.0));
    f.workspace.link_from_here(&f.session, &f.views);
    f.frame();
    f.go_to_z(LATER_AXIAL, 62.0);
    f.frame();
    assert_eq!(f.z(AXIAL), 60.0, "its end slice, 5 mm short of where it would be");
    assert!(f.is_outside(0));

    // Link off, and back to automatic: nothing follows any more between the two studies.
    f.workspace.automatic_links();
    assert!(!f.workspace.has_manual_links());
    assert_eq!(f.status(1), LinkStatus::NotLinked(NotLinked::NoCommonFrame));
}

#[test]
fn a_manual_link_counts_in_the_same_direction_of_the_patient_at_another_spacing() {
    let mut f = open(Grid::SideBySide, &[FINE, LATER_AXIAL]);
    f.go_to_z(FINE, 0.0);
    f.go_to_z(LATER_AXIAL, 7.0);
    f.workspace.link_from_here(&f.session, &f.views);
    f.frame();
    // 10 mm: the later series, every 5 mm, moves to the slice nearest to 10 mm further.
    f.go_to_z(FINE, 10.0);
    f.frame();
    assert_eq!(f.z(LATER_AXIAL), 17.0);
    // 2.5 mm: the nearest slice of a series every 5 mm is half a gap away either way.
    f.go_to_z(FINE, 12.5);
    f.frame();
    assert!((f.z(LATER_AXIAL) - 19.5).abs() <= 2.5 + 1e-9, "{}", f.z(LATER_AXIAL));
    // The offset does not creep: back at 10 mm, at the slice of then.
    f.go_to_z(FINE, 10.0);
    f.frame();
    assert_eq!(f.z(LATER_AXIAL), 17.0);
}

#[test]
fn link_from_here_needs_a_similar_slice_direction_and_respects_a_link_switched_off() {
    let f = open(Grid::SideBySide, &[AXIAL, SAGITTAL]);
    assert!(!f.workspace.can_link_from_here(&f.session, &f.views));

    let mut f = open(Grid::Four, &[AXIAL, LATER_AXIAL, FINE, LATER_SAGITTAL]);
    f.workspace.toggle_link(2);
    f.workspace.link_from_here(&f.session, &f.views);
    assert_eq!(f.status(0), LinkStatus::Manual);
    assert_eq!(f.status(1), LinkStatus::Manual);
    assert_eq!(f.status(2), LinkStatus::Off, "switched off by the user: it stays so");
    assert!(!matches!(f.status(3), LinkStatus::Manual), "the sagittal one has another direction");
    // A new series in a manual viewport starts afresh.
    f.workspace.assign(1, LATER_SAGITTAL);
    assert!(!matches!(f.status(1), LinkStatus::Manual));
}

/// The scenario of the specification: zoom by 1.5 and pan by 40 points, and
/// each keeps its own difference.
#[test]
fn linked_viewports_follow_zoom_and_pan_by_the_same_factor_and_distance() {
    let mut f = open(Grid::SideBySide, &[AXIAL, FINE]);
    // The right one already differs.
    *f.views.get_mut(&FINE).expect("view").transform_mut(STACK) =
        ViewTransform { zoom: 2.0, pan: [10.0, -6.0], ..ViewTransform::default() };

    let zoom = Gesture::Zoom { factor: 1.5, anchor: [0.0, 0.0] };
    f.views.get_mut(&AXIAL).expect("view").transform_mut(STACK).zoom_about(1.5, [0.0, 0.0]);
    f.workspace.follow_gesture(0, zoom, &f.session, &mut f.views);
    assert_eq!(f.transform(AXIAL).zoom, 1.5, "not applied twice");
    assert_eq!(f.transform(FINE).zoom, 3.0);
    assert_eq!(f.transform(FINE).pan, [15.0, -9.0], "the same point stays at the centre of the viewport");

    let pan = Gesture::Pan { delta: [40.0, 0.0] };
    f.workspace.follow_gesture(0, pan, &f.session, &mut f.views);
    assert_eq!(f.transform(FINE).pan, [55.0, -9.0], "40 points to the right");
    assert_eq!(f.transform(AXIAL).pan, [0.0, 0.0], "the viewport the user moved has its own part already");

    // A linked zoom stops at the limits of the view.
    f.workspace.follow_gesture(0, Gesture::Zoom { factor: 100.0, anchor: [0.0, 0.0] }, &f.session, &mut f.views);
    assert_eq!(f.transform(FINE).zoom, ViewTransform::MAX_ZOOM);
}

#[test]
fn a_viewport_with_its_link_off_keeps_its_zoom_and_pan_and_turn_and_contrast_are_not_linked() {
    let mut f = open(Grid::SideBySide, &[AXIAL, FINE]);
    let window = f.views[&FINE].window;
    // Turned and with a contrast of its own on the left: neither reaches the right.
    {
        let left = f.views.get_mut(&AXIAL).expect("view");
        left.transform_mut(STACK).turn_right();
        left.window = summrise_imaging::Window::new(100.0, 200.0);
    }
    f.workspace.follow_gesture(0, Gesture::Zoom { factor: 1.5, anchor: [0.0, 0.0] }, &f.session, &mut f.views);
    assert_eq!(f.transform(FINE).orientation, ViewTransform::default().orientation);
    assert_eq!(f.views[&FINE].window, window);
    assert_eq!(f.transform(FINE).zoom, 1.5);

    f.workspace.toggle_link(1);
    let before = f.transform(FINE);
    f.workspace.follow_gesture(0, Gesture::Zoom { factor: 1.5, anchor: [0.0, 0.0] }, &f.session, &mut f.views);
    f.workspace.follow_gesture(0, Gesture::Pan { delta: [40.0, 0.0] }, &f.session, &mut f.views);
    assert_eq!(f.transform(FINE), before);
}

/// Series of two studies or in other slice directions follow no zoom either.
#[test]
fn only_linked_series_follow_a_zoom() {
    let mut f = open(Grid::SideBySide, &[AXIAL, LATER_AXIAL]);
    f.workspace.follow_gesture(0, Gesture::Zoom { factor: 2.0, anchor: [0.0, 0.0] }, &f.session, &mut f.views);
    assert_eq!(f.transform(LATER_AXIAL).zoom, 1.0);
    f.workspace.link_from_here(&f.session, &f.views);
    f.workspace.follow_gesture(0, Gesture::Zoom { factor: 2.0, anchor: [0.0, 0.0] }, &f.session, &mut f.views);
    assert_eq!(f.transform(LATER_AXIAL).zoom, 2.0, "a manual link counts");
}

/// A series shown twice has one view state: one image, one zoom, moved once.
#[test]
fn a_series_shown_twice_shares_its_view_and_moves_once() {
    let mut f = open(Grid::Four, &[AXIAL, FINE]);
    f.workspace.assign(2, AXIAL);
    f.frame();
    f.go_to_z(AXIAL, 10.0);
    f.frame();
    assert_eq!(f.z(FINE), 10.0);
    f.workspace.assign(3, FINE);
    f.frame();
    let shown = f.workspace.shown_series();
    assert_eq!(shown, [AXIAL, FINE], "each series once");
    f.workspace.follow_gesture(0, Gesture::Zoom { factor: 2.0, anchor: [0.0, 0.0] }, &f.session, &mut f.views);
    assert_eq!(f.transform(FINE).zoom, 2.0, "not four times");
    assert_eq!(f.transform(AXIAL).zoom, 1.0, "the source's own zoom is the viewport's to change");
}

/// Task 6.2 — the line of the axial slice at z = 12 mm on the sagittal series.
#[test]
fn the_cross_reference_of_an_axial_slice_lies_at_its_row_of_the_sagittal_image() {
    let mut f = open(Grid::SideBySide, &[AXIAL, SAGITTAL]);
    // The series has slices every 5 mm: go to +10 mm, then look at the plane and the row it gives.
    f.go_to_z(AXIAL, 10.0);
    f.frame();
    let cutting = f.workspace.cross_reference(1, &f.session, &f.views).expect("same Frame of Reference");
    assert_eq!(cutting.position.z, 10.0);
    assert_eq!(f.workspace.cross_reference(0, &f.session, &f.views), None, "not on its own viewport");

    let data = f.data(SAGITTAL);
    let image = data.frames()[f.views[&SAGITTAL].slot_index(&data)].plane.expect("plane");
    let [a, b] = crossref::intersection_segment(&cutting, &image).expect("a line");
    // Row r of the sagittal image is at z = 63 - 2 r mm.
    assert!((a.1 - 26.5).abs() < 1e-9 && (b.1 - 26.5).abs() < 1e-9, "{a:?} {b:?}");

    // It follows scrolling: towards the head it moves up.
    f.go_to_z(AXIAL, 30.0);
    f.frame();
    let cutting = f.workspace.cross_reference(1, &f.session, &f.views).expect("line");
    let [a, _] = crossref::intersection_segment(&cutting, &image).expect("a line");
    assert!((a.1 - 16.5).abs() < 1e-9, "{a:?}");

    // At z = 12 mm exactly, the scenario of the specification (with a plane at that height).
    let mut at_12 = cutting;
    at_12.position.z = 12.0;
    let [a, _] = crossref::intersection_segment(&at_12, &image).expect("a line");
    assert!((a.1 - 25.5).abs() < 1e-9, "{a:?}");

    // Parallel: both axial. No line, although the plane is offered.
    let g = open(Grid::SideBySide, &[AXIAL, FINE]);
    let cutting = g.workspace.cross_reference(1, &g.session, &g.views).expect("same Frame of Reference");
    let data = g.data(FINE);
    let image = data.frames()[g.views[&FINE].slot_index(&data)].plane.expect("plane");
    assert_eq!(crossref::intersection_segment(&cutting, &image), None);

    // Another Frame of Reference: no line.
    let h = open(Grid::SideBySide, &[AXIAL, LATER_SAGITTAL]);
    assert_eq!(h.workspace.cross_reference(1, &h.session, &h.views), None);
}

/// Task 1.1 — Images, MPR, 2 × 2 and 3D are presets of one series.
#[test]
fn the_presets_fill_the_grid_with_the_one_series() {
    let mut workspace = Workspace::default();
    assert!(!workspace.is_comparing());
    let key = Some(AXIAL);
    workspace.show_preset(Layout::Single, key, STACK);
    assert_eq!(workspace.viewports().len(), 1);
    assert_eq!((workspace.viewports()[0].series, workspace.viewports()[0].view), (key, STACK));

    for layout in [Layout::Mpr, Layout::Quad, Layout::Volume] {
        let views = layout.views();
        let last = *views.last().expect("a view");
        workspace.show_preset(layout, key, last);
        let shown: Vec<ViewKind> = workspace.viewports().iter().map(|viewport| viewport.view).collect();
        assert_eq!(shown, views);
        assert!(workspace.viewports().iter().all(|viewport| viewport.series == key));
        assert_eq!(workspace.active(), views.len() - 1);
    }

    // A tile grid stays with the acquired images: to the 2 × 2 layout and to another series.
    workspace.show_preset(Layout::Single, key, STACK);
    workspace.set_tiles(0, Tiles::ALL[2]);
    workspace.show_preset(Layout::Single, Some(FINE), STACK);
    assert_eq!(workspace.viewports()[0].tiles, Tiles::ALL[2]);
    workspace.show_preset(Layout::Quad, Some(FINE), STACK);
    assert_eq!(workspace.viewports()[0].tiles, Tiles::ALL[2]);
    assert_eq!(workspace.viewports()[1].tiles, Tiles::ONE);
    // Reformats and the 3D view have none.
    workspace.set_tiles(1, Tiles::ALL[2]);
    assert_eq!(workspace.viewports()[1].tiles, Tiles::ONE);
    workspace.show_preset(Layout::Volume, Some(FINE), ViewKind::Volume);
    assert!(workspace.viewports().iter().all(|viewport| viewport.tiles == Tiles::ONE));
    // A preset's series is never pinned by a grid that is gone, and asks for no following.
    let index = Workspace::default();
    assert_eq!(index.link_status(5, &empty_session().0, &Views::new()), LinkStatus::Off);
}

fn empty_session() -> (Session, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    write_phantom_study(dir.path(), &PhantomOptions::small()).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    (Session::new(index, 1 << 30, None), dir)
}

#[test]
fn grids_keep_what_they_can() {
    let mut workspace = Workspace::default();
    workspace.show_preset(Layout::Single, Some(AXIAL), STACK);
    workspace.set_tiles(0, Tiles::ALL[1]);

    workspace.set_grid(Grid::SideBySide, Some(AXIAL));
    assert!(workspace.is_comparing());
    assert_eq!(workspace.viewports().len(), 2);
    assert_eq!(workspace.viewports()[0].series, Some(AXIAL));
    assert_eq!(workspace.viewports()[0].tiles, Tiles::ALL[1], "the choice comes along");
    assert_eq!(workspace.viewports()[1].series, None);
    assert_eq!(workspace.active(), 0);
    // A preset does not touch a grid that is on screen.
    workspace.show_preset(Layout::Mpr, Some(FINE), STACK);
    assert_eq!(workspace.viewports().len(), 2);

    workspace.assign(1, FINE);
    workspace.set_grid(Grid::Four, None);
    assert_eq!(workspace.viewports().len(), 4);
    assert_eq!(workspace.shown_series(), [AXIAL, FINE]);
    workspace.activate(3);
    assert_eq!(workspace.next_viewport(), 0);
    assert_eq!(workspace.activate(3), None);
    workspace.adopt(Some(SHORT));
    assert_eq!(workspace.viewports()[3].series, Some(SHORT), "the active viewport takes the series selected");
    assert_eq!(workspace.shown_series(), [AXIAL, FINE, SHORT]);

    workspace.set_grid(Grid::Stacked, None);
    assert_eq!(workspace.viewports().len(), 2);
    assert_eq!(workspace.active(), 0, "the active viewport is gone: the first takes over");
    assert_eq!(workspace.shown_series(), [AXIAL, FINE]);
    workspace.leave_grid();
    assert!(!workspace.is_comparing());
    assert_eq!(workspace.viewports()[0].tiles, Tiles::ALL[1]);
}

/// Task 5.1 — the images of a grid, within 60 images.
#[test]
fn a_grid_shows_the_following_images_from_the_current_one() {
    let shown = |tiles: Tiles, image: usize| -> Vec<Option<usize>> {
        // Numbers from 1, as on screen.
        tiles.images(image - 1, 60).map(|tile| tile.map(|index| index + 1)).collect()
    };
    let nine = Tiles::ALL[2];
    assert_eq!(nine.side(), 3);
    assert_eq!(nine.count(), 9);
    assert_eq!(nine.label(), "3 × 3");
    assert_eq!(shown(nine, 31), (31..=39).map(Some).collect::<Vec<_>>(), "the first tile is the current image");
    assert_eq!(shown(nine, 32), (32..=40).map(Some).collect::<Vec<_>>());
    assert_eq!(shown(nine, 41), (41..=49).map(Some).collect::<Vec<_>>());
    assert_eq!(shown(nine, 58), [Some(58), Some(59), Some(60), None, None, None, None, None, None]);
    assert_eq!(shown(nine, 60), [Some(60), None, None, None, None, None, None, None, None]);
    assert_eq!(shown(Tiles::ONE, 31), [Some(31)]);
    assert_eq!(Tiles::ALL[4].images(0, 60).count(), 25);
    assert_eq!(Tiles::ALL[4].images(0, 2).flatten().count(), 2);
}

#[test]
fn grids_with_tiles_under_96_points_are_not_offered() {
    let offered = |size: [f32; 2]| -> Vec<usize> {
        Tiles::ALL.into_iter().filter(|tiles| tiles.fits(size)).map(Tiles::side).collect()
    };
    // The scenario of the specification.
    assert_eq!(offered([320.0, 800.0]), [1, 2, 3]);
    assert_eq!(offered([800.0, 320.0]), [1, 2, 3], "lower counts as well");
    assert_eq!(offered([1200.0, 800.0]), [1, 2, 3, 4, 5]);
    assert_eq!(offered([100.0, 100.0]), [1]);
    // Exactly 96 points are enough: two gaps of 2 points and three tiles of 96.
    assert_eq!(offered([292.0, 292.0]), [1, 2, 3]);
    assert_eq!(offered([291.9, 292.0]), [1, 2]);

    let area = [320.0, 800.0];
    assert_eq!(Tiles::ALL[4].largest_fitting(area), Tiles::ALL[2]);
    assert_eq!(Tiles::ALL[1].largest_fitting(area), Tiles::ALL[1]);
    assert_eq!(Tiles::ONE.next_fitting(area), Tiles::ALL[1]);
    assert_eq!(Tiles::ALL[1].next_fitting(area), Tiles::ALL[2]);
    assert_eq!(Tiles::ALL[2].next_fitting(area), Tiles::ONE, "T comes back to one image");
    assert_eq!(Tiles::ONE.next_fitting([100.0, 100.0]), Tiles::ONE);
}

// ---------------------------------------------------------------------------
// Which studies can be compared

#[test]
fn studies_of_one_patient_are_recognised_by_id_or_by_name_and_birth_date() {
    use summrise_dicom::Patient;
    let patient = |id: Option<&str>, name: Option<&str>, birth: Option<&str>| Patient {
        id: id.map(str::to_owned),
        name: name.map(str::to_owned),
        birth_date: birth.map(str::to_owned),
        ..Patient::default()
    };
    let a = patient(Some("1"), Some("A^B"), Some("19700101"));
    assert!(same_patient(&a, &patient(Some("1"), Some("OTHER"), None)), "the ID decides");
    assert!(!same_patient(&a, &patient(Some("2"), Some("A^B"), Some("19700101"))), "another ID: another patient");
    let no_id = patient(None, Some("A^B"), Some("19700101"));
    assert!(same_patient(&a, &no_id), "without an ID: the name and the birth date");
    assert!(same_patient(&no_id, &patient(Some(" "), Some("A^B"), Some("19700101"))), "a blank ID is none");
    assert!(!same_patient(&no_id, &patient(None, Some("A^B"), None)), "both are needed");
    assert!(!same_patient(&no_id, &patient(None, Some("A^C"), Some("19700101"))));
    assert!(!same_patient(&patient(None, None, None), &patient(None, None, None)), "nothing known: not the same");
}

fn indexed(options: &ComparisonOptions) -> (tempfile::TempDir, summrise_dicom::StudyIndex) {
    let dir = tempfile::tempdir().expect("tempdir");
    write_comparison_studies(dir.path(), options).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    (dir, index)
}

/// Task 4.1 — the follow-up on one disc: earlier study first, the sagittal
/// T2 series of both; another patient's study stays out.
#[test]
fn two_studies_of_one_patient_can_be_compared_and_series_are_matched() {
    let (_dir, index) = indexed(&ComparisonOptions { other_patient: true, ..ComparisonOptions::default() });
    let studies = &index.studies;
    assert_eq!(studies.len(), 3);
    assert_eq!(studies[phantom::EARLIER].date.as_deref(), Some(phantom::EARLIER_DATE));
    assert_eq!(studies[phantom::LATER].date.as_deref(), Some(phantom::LATER_DATE));
    assert_ne!(
        studies[phantom::EARLIER].series[0].frame_of_reference,
        studies[phantom::LATER].series[0].frame_of_reference,
        "two examinations do not share a Frame of Reference"
    );
    assert_eq!(
        studies[phantom::EARLIER].series[0].frame_of_reference,
        studies[phantom::EARLIER].series[phantom::SAGITTAL].frame_of_reference,
        "one examination does (task 3.1: it is in the index)"
    );
    assert_eq!(comparable_studies(studies), [(phantom::EARLIER, phantom::LATER)]);

    // Looking at the sagittal one, or nothing in particular with the exact matches tied by images.
    let pair = matching_series(studies, phantom::EARLIER, phantom::LATER, Some(SAGITTAL));
    assert_eq!(pair, Some((SAGITTAL, LATER_SAGITTAL)));
    let pair = matching_series(studies, phantom::EARLIER, phantom::LATER, Some(LATER_AXIAL));
    assert_eq!(pair, Some((AXIAL, LATER_AXIAL)));
    let pair = matching_series(studies, phantom::EARLIER, phantom::LATER, None);
    assert_eq!(pair, Some((AXIAL, LATER_AXIAL)), "25 images each against 9");
}

#[test]
fn without_a_series_of_the_same_modality_and_plane_nothing_matches() {
    let (_dir, index) = indexed(&ComparisonOptions { matching: false, ..ComparisonOptions::default() });
    assert_eq!(index.studies[phantom::LATER].series[0].modality, "CT");
    assert_eq!(comparable_studies(&index.studies), [(phantom::EARLIER, phantom::LATER)]);
    assert_eq!(matching_series(&index.studies, phantom::EARLIER, phantom::LATER, None), None);
}

#[test]
fn studies_of_different_patients_or_a_single_study_offer_nothing() {
    let (_dir, index) = indexed(&ComparisonOptions::default());
    assert_eq!(comparable_studies(&index.studies[..1]), []);
    let mut studies = index.studies.clone();
    studies[1].patient.id = Some("SOMEONE ELSE".to_owned());
    assert_eq!(comparable_studies(&studies), []);
}

/// The earlier study is the one on the left whatever the order of the index,
/// and a study without a date never counts as the earlier one.
#[test]
fn the_earlier_study_comes_first() {
    let (_dir, index) = indexed(&ComparisonOptions::default());
    let mut studies = index.studies.clone();
    studies.swap(0, 1);
    assert_eq!(comparable_studies(&studies), [(1, 0)]);
    studies[1].date = None;
    assert_eq!(comparable_studies(&studies), [(0, 1)]);
}

/// Descriptions decide among series of the same modality and plane: exact
/// first, then the words they share.
#[test]
fn descriptions_are_compared_exactly_and_then_by_their_words() {
    let (_dir, index) = indexed(&ComparisonOptions::default());
    let mut studies = index.studies.clone();
    let describe = |study: &mut summrise_dicom::Study, series: usize, text: &str| {
        study.series[series].description = Some(text.to_owned());
    };
    // Nothing exact: "T2 SAG" shares its words with "t2_sag fs", not with "PD SAG".
    describe(&mut studies[phantom::EARLIER], phantom::AXIAL_5MM, "PD AX");
    describe(&mut studies[phantom::LATER], phantom::LATER_AXIAL, "T1 AX");
    describe(&mut studies[phantom::EARLIER], phantom::SAGITTAL, "T2 SAG");
    describe(&mut studies[phantom::LATER], phantom::LATER_SAGITTAL, "t2_sag fs");
    let pair = matching_series(&studies, phantom::EARLIER, phantom::LATER, None);
    assert_eq!(pair, Some((SAGITTAL, LATER_SAGITTAL)), "two words of three shared beat one of three");
    // Exact (whatever the case) beats any share of words.
    describe(&mut studies[phantom::EARLIER], phantom::AXIAL_5MM, "t1 ax");
    let pair = matching_series(&studies, phantom::EARLIER, phantom::LATER, None);
    assert_eq!(pair, Some((AXIAL, LATER_AXIAL)));
}
