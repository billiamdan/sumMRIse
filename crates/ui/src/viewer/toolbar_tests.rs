//! The toolbar in narrow windows: from the default window size down to the
//! smallest one the app allows, every control of the toolbar is on screen and
//! none covers another; the controls on the left wrap onto another row
//! instead. Set `SUMMRISE_SNAPSHOT_DIR` to keep renderings (`toolbar-*.png`).
//!
//! The data is the PET study of the testkit with its CT shown: the CT offers
//! the PET menu, so the toolbar is at its fullest.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::{Rect, Theme};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use egui_material_icons::icons::ICON_CLOSE;
use summrise_core::dicom::{Progress, scan};
use summrise_core::imaging::SlabMode;
use summrise_core::{Layout, SeriesKey, Session, Settings, Waker};
use summrise_testkit::PhantomOptions;
use summrise_testkit::pet::{self as phantom, PetStudyOptions, write_pet_study};

use super::Viewer;
use crate::app::{Action, Toasts};
use crate::theme;

const CT: SeriesKey = SeriesKey { study: 0, series: phantom::CT };

/// Everything `Viewer::show` needs.
struct App {
    viewer: Viewer,
    settings: Settings,
    waker: Waker,
    actions: Vec<Action>,
    toasts: Toasts,
}

fn snapshot_dir() -> Option<PathBuf> {
    std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from)
}

/// The viewer on the study in `dir` with its CT loaded, in `layout`, in a
/// window of `size` points.
fn open(dir: &Path, layout: Layout, size: [f32; 2]) -> Harness<'static, App> {
    open_series(dir, CT, layout, size)
}

/// The viewer on the study in `dir` with series `key` loaded, in `layout`,
/// in a window of `size` points.
fn open_series(dir: &Path, key: SeriesKey, layout: Layout, size: [f32; 2]) -> Harness<'static, App> {
    let index = scan(dir, &Progress::new()).expect("scan");
    let settings = Settings { default_layout: layout, ..Settings::default() };
    let mut session = Session::new(index, settings.memory_budget_bytes(), None);
    session.select_series(key);
    let app = App {
        viewer: Viewer::new(session, &settings),
        settings,
        waker: Arc::new(|| {}),
        actions: Vec::new(),
        toasts: Toasts::default(),
    };
    let mut builder = Harness::builder().with_size(size).with_theme(Theme::Dark).with_context_setup(theme::install);
    if snapshot_dir().is_some() {
        builder = builder.wgpu();
    }
    let mut harness = builder.build_ui_state(
        |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
        app,
    );
    let start = Instant::now();
    while harness.state().viewer.session.loaded(key).is_none() {
        assert!(start.elapsed() < Duration::from_secs(60), "timed out loading the series");
        harness.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    harness.run_steps(4);
    harness
}

/// The widgets of the toolbar — everything above the views — with their
/// label (or value) and role.
fn toolbar_widgets(harness: &Harness<'_, App>) -> Vec<(String, Role, Rect)> {
    let views_top = harness
        .query_all(by().role(Role::Image))
        .filter(|node| node.accesskit_node().bounding_box().is_some())
        .map(|node| node.rect().top())
        .fold(f32::INFINITY, f32::min);
    harness
        .query_all(by())
        .filter(|node| node.accesskit_node().bounding_box().is_some())
        .filter_map(|node| {
            let rect = node.rect();
            let ak = node.accesskit_node();
            let interesting = !matches!(ak.role(), Role::Window | Role::GenericContainer | Role::Unknown);
            let label = ak.label().or_else(|| ak.value()).unwrap_or_default();
            (interesting && rect.is_positive() && rect.bottom() <= views_top).then(|| (label, ak.role(), rect))
        })
        .collect()
}

/// Every toolbar widget lies inside the window, none covers another, and the
/// group on the right (with Open) is there.
fn assert_toolbar_fits(harness: &Harness<'_, App>, width: f32, what: &str) {
    let widgets = toolbar_widgets(harness);
    assert!(widgets.len() > 15, "{what}: the toolbar has its controls: {widgets:#?}");
    for (label, role, rect) in &widgets {
        assert!(
            rect.left() >= -0.5 && rect.right() <= width + 0.5,
            "{what}: {label:?} ({role:?}) is not inside the window: {rect:?}"
        );
    }
    for (i, (a_label, a_role, a)) in widgets.iter().enumerate() {
        for (b_label, b_role, b) in &widgets[i + 1..] {
            let common = a.intersect(*b);
            let nested = a.contains_rect(*b) || b.contains_rect(*a);
            assert!(
                !common.is_positive() || common.area() < 1.0 || nested,
                "{what}: {a_label:?} ({a_role:?}, {a:?}) and {b_label:?} ({b_role:?}, {b:?}) overlap"
            );
        }
    }
    for control in [" Open", ICON_CLOSE.codepoint, "Window", "Slab", "MPR"] {
        let found = widgets.iter().any(|(label, ..)| label.contains(control))
            || harness.query_all_by_label_contains(control).next().is_some();
        assert!(found, "{what}: {control:?} is missing");
    }
}

fn save(harness: &mut Harness<'_, App>, name: &str) {
    if let Some(dir) = snapshot_dir() {
        let image = harness.render().expect("wgpu render");
        std::fs::create_dir_all(&dir).expect("snapshot dir");
        image.save(dir.join(format!("{name}.png"))).expect("save png");
    }
}

/// From the default window width down to the smallest window the app allows
/// (960 × 600 points), with reformats and a thick slab (the most controls)
/// and with the 3D view.
#[test]
fn every_toolbar_control_is_on_screen_and_none_overlaps_in_narrow_windows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = PetStudyOptions {
        ct: PhantomOptions { ct_size: 96, ct_slices: 48, ..PhantomOptions::small() },
        ..Default::default()
    };
    write_pet_study(dir.path(), &options).expect("fixture");
    for width in [1440.0, 1280.0, 1024.0, 960.0] {
        let mut harness = open(dir.path(), Layout::Mpr, [width, 800.0]);
        harness.state_mut().viewer.views.get_mut(&CT).expect("the CT has a view").set_slab(Some(SlabMode::Mip), 10.0);
        harness.run_steps(4);
        assert!(harness.query_all_by_label_contains("PET colours and fusion").next().is_some(), "the PET menu");
        assert_toolbar_fits(&harness, width, &format!("reformats with a slab at {width} pt"));
        save(&mut harness, &format!("toolbar-{width}-mpr-slab"));

        let mut harness = open(dir.path(), Layout::Volume, [width, 800.0]);
        assert!(harness.query_all_by_label_contains("3D view").next().is_some());
        let what = format!("3D view at {width} pt");
        let widgets = toolbar_widgets(&harness);
        assert!(widgets.iter().all(|(_, _, rect)| rect.right() <= width + 0.5), "{what}: {widgets:#?}");
        // The status bar: its long hint (the 3D view's is the longest) is shortened
        // rather than running under the number of images on the right.
        assert_no_overlap(&status_bar_widgets(&harness, 800.0), width, &format!("status bar of the {what}"));
        save(&mut harness, &format!("toolbar-{width}-3d"));
    }
}

/// The text of the status bar: everything in the bottom 28 points of a window
/// `height` points high.
fn status_bar_widgets(harness: &Harness<'_, App>, height: f32) -> Vec<(String, Role, Rect)> {
    harness
        .query_all(by())
        .filter(|node| node.accesskit_node().bounding_box().is_some())
        .filter_map(|node| {
            let rect = node.rect();
            let ak = node.accesskit_node();
            let text = matches!(ak.role(), Role::Label | Role::Button | Role::CheckBox);
            let label = ak.label().or_else(|| ak.value()).unwrap_or_default();
            (text && rect.is_positive() && rect.top() >= height - 28.0).then(|| (label, ak.role(), rect))
        })
        .collect()
}

/// Every widget lies inside the window and none covers another.
fn assert_no_overlap(widgets: &[(String, Role, Rect)], width: f32, what: &str) {
    assert!(!widgets.is_empty(), "{what}: nothing found");
    for (i, (a_label, _, a)) in widgets.iter().enumerate() {
        assert!(a.left() >= -0.5 && a.right() <= width + 0.5, "{what}: {a_label:?} is not inside the window: {a:?}");
        for (b_label, _, b) in &widgets[i + 1..] {
            let common = a.intersect(*b);
            // A node inside another (a label's text in the label) is not an overlap.
            let nested = a.contains_rect(*b) || b.contains_rect(*a);
            assert!(
                !common.is_positive() || common.area() < 1.0 || nested,
                "{what}: {a_label:?} ({a:?}) and {b_label:?} ({b:?}) overlap"
            );
        }
    }
}

/// The playback bar of a series with several image sets — the image-set
/// control, play, speed and the two ways of playing — stays inside the width
/// of its view without overlaps from 1440 down to 960 points: it wraps onto
/// another row, each label with its control.
#[test]
fn the_playback_bar_stays_inside_its_view_in_narrow_windows() {
    let dir = tempfile::tempdir().expect("tempdir");
    summrise_testkit::stacks::write_stacks_study(dir.path()).expect("fixture");
    let dynamic = SeriesKey { study: 0, series: summrise_testkit::stacks::DYNAMIC };
    for width in [1440.0, 1280.0, 960.0] {
        let mut harness = open_series(dir.path(), dynamic, Layout::Single, [width, 800.0]);
        harness.run_steps(4);
        let view = harness
            .query_all(by().role(Role::Image))
            .filter(|node| node.accesskit_node().bounding_box().is_some())
            .map(|node| node.rect())
            .find(|rect| rect.width() > 200.0)
            .expect("the view");
        let names = ["Image set", "image set", "Play", "Speed", "Loop", "Back and forth"];
        let bar: Vec<(String, Role, Rect)> = harness
            .query_all(by())
            .filter(|node| node.accesskit_node().bounding_box().is_some())
            .filter_map(|node| {
                let (rect, ak) = (node.rect(), node.accesskit_node());
                let label = ak.label().or_else(|| ak.value()).unwrap_or_default();
                let named = names.iter().any(|name| label.contains(name));
                // Under the view and starting within its width (the inspector has an "Image set" row too).
                let under = rect.top() >= view.bottom() - 0.5 && rect.bottom() <= 800.0;
                let within = rect.left() >= view.left() - 0.5 && rect.left() < view.right();
                (named && under && within).then(|| (label, ak.role(), rect))
            })
            .collect();
        let what = format!("playback bar at {width} pt");
        assert!(bar.len() >= 7, "{what}: {bar:#?}");
        assert_no_overlap(&bar, view.right(), &what);
        save(&mut harness, &format!("playback-bar-{width}"));
    }
}
