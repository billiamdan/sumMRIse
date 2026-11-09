//! Measurements live as long as the study is open: in the real app, one drawn
//! on an image is there while the study is open and gone when the study has
//! been closed and opened again (task 4.4). The tool stays usable afterwards
//! and the notice does not come back.

use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use egui_material_icons::icons::ICON_CLOSE;
use summrise_testkit::{PhantomOptions, write_phantom_study};
use summrise_ui::{AppOptions, SumMRIseApp};

type App = Harness<'static, SumMRIseApp>;

fn settle(app: &mut App, duration: Duration) {
    let start = Instant::now();
    while start.elapsed() < duration {
        app.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Runs frames until `done`.
fn wait_until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
    let start = Instant::now();
    while !done(app) {
        assert!(start.elapsed() < Duration::from_secs(60), "timed out waiting for {what}");
        app.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    settle(app, Duration::from_millis(200));
}

/// The acquired view of the CT, when the series is loaded: its reformats can be chosen.
fn wait_for_the_loaded_ct(app: &mut App) {
    wait_until(app, "the CT", |app| {
        app.query_all_by_label("MPR").next().is_some_and(|node| !node.accesskit_node().is_disabled())
    });
}

fn view(app: &App) -> Rect {
    app.query_all(by().role(Role::Image))
        .find(|node| node.accesskit_node().label().is_some_and(|label| label.starts_with("Image ")))
        .expect("the acquired view")
        .rect()
}

fn image_shown(app: &App) -> String {
    app.query_all(by().role(Role::Image))
        .filter_map(|node| node.accesskit_node().label())
        .find(|label| label.starts_with("Image "))
        .expect("the acquired view")
}

/// The labels on the picture: those of measurements.
fn measurement_labels(app: &App) -> Vec<String> {
    let view = view(app);
    app.query_all(by().role(Role::Label))
        .filter(|node| view.contains_rect(node.rect()))
        .filter_map(|node| {
            let node = node.accesskit_node();
            node.label().or_else(|| node.value())
        })
        .collect()
}

fn drag(app: &mut App, from: Pos2, to: Pos2) {
    app.hover_at(from);
    settle(app, Duration::from_millis(50));
    app.drag_at(from);
    app.step();
    app.hover_at(to);
    settle(app, Duration::from_millis(50));
    app.drop_at(to);
    settle(app, Duration::from_millis(100));
}

#[test]
fn measurements_are_gone_after_closing_and_opening_the_study_again() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_phantom_study(dir.path(), &PhantomOptions::small()).expect("fixture");
    let path = dir.path().to_path_buf();
    let mut app = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_eframe(move |cc| SumMRIseApp::new(cc, AppOptions { open: Some(path), ..AppOptions::default() }));
    wait_for_the_loaded_ct(&mut app);
    let shown = image_shown(&app);

    // The tool, with the one-time notice.
    app.key_press(Key::D);
    settle(&mut app, Duration::from_millis(100));
    assert!(app.query_all_by_label("Measurements are approximate").next().is_some());
    app.get_by_label("Understood").click();
    settle(&mut app, Duration::from_millis(100));

    // A line across the middle of the picture.
    let centre = view(&app).center();
    drag(&mut app, centre - vec2(60.0, 0.0), centre + vec2(60.0, 0.0));
    let labels = measurement_labels(&app);
    assert_eq!(labels.len(), 1, "{labels:?}");
    assert!(labels[0].ends_with(" mm"), "{labels:?}");

    // Closing the study: the welcome screen, with the study in the recent list.
    app.query_all_by_label(ICON_CLOSE.codepoint).next().expect("the close button").click();
    let recent = "Synthetic phantom study";
    wait_until(&mut app, "the welcome screen", |app| app.query_all_by_label_contains(recent).next().is_some());
    assert!(app.query_all(by().role(Role::Image)).next().is_none(), "no view is left");

    // Opening it again: the same image, without the measurement.
    let row = app.query_all_by_label_contains(recent).next().expect("the recent study").rect().center();
    app.hover_at(row);
    settle(&mut app, Duration::from_millis(50));
    for pressed in [true, false] {
        app.event(Event::PointerButton {
            pos: row,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    wait_for_the_loaded_ct(&mut app);
    assert_eq!(image_shown(&app), shown);
    assert_eq!(measurement_labels(&app), Vec::<String>::new(), "no measurements are present");
    app.remove_cursor();
    settle(&mut app, Duration::from_millis(50));

    // The tool works as before; only that the notice was shown was remembered.
    app.key_press(Key::D);
    settle(&mut app, Duration::from_millis(100));
    assert!(app.query_all_by_label("Measurements are approximate").next().is_none());
    let centre = view(&app).center();
    drag(&mut app, centre - vec2(60.0, 0.0), centre + vec2(60.0, 0.0));
    assert_eq!(measurement_labels(&app).len(), 1);
}
