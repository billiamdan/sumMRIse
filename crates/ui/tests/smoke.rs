//! Offscreen UI smoke test: opens the synthetic CD in the real app, drives
//! layouts and the inspector, and renders every screen through wgpu.
//! Set `SUMMRISE_SNAPSHOT_DIR` to keep the rendered PNGs for review.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use summrise_testkit::{PhantomOptions, write_phantom_study};
use summrise_ui::{AppOptions, SumMRIseApp};

fn settle(harness: &mut Harness<'_, SumMRIseApp>, duration: Duration) {
    let start = Instant::now();
    while start.elapsed() < duration {
        harness.step();
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn first<'a>(harness: &'a Harness<'_, SumMRIseApp>, label: &'a str) -> egui_kittest::Node<'a> {
    harness.query_all_by_label(label).next().unwrap_or_else(|| panic!("no node labelled {label:?}"))
}

fn save(harness: &mut Harness<'_, SumMRIseApp>, name: &str) {
    let image = harness.render().expect("wgpu render");
    assert!(image.width() > 0 && image.height() > 0);
    if let Some(dir) = std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from) {
        std::fs::create_dir_all(&dir).expect("snapshot dir");
        image.save(dir.join(format!("{name}.png"))).expect("save png");
    }
}

#[test]
fn opens_study_and_renders_all_views() {
    let dir = tempfile::tempdir().expect("tempdir");
    let opts = PhantomOptions { ct_size: 128, ct_slices: 60, mr_size: 96, ..PhantomOptions::default() };
    write_phantom_study(dir.path(), &opts).expect("fixture");

    // Welcome screen.
    let mut welcome = Harness::builder()
        .with_size([1280.0, 800.0])
        .wgpu()
        .build_eframe(|cc| SumMRIseApp::new(cc, AppOptions::default()));
    settle(&mut welcome, Duration::from_millis(300));
    welcome.get_by_label_contains("Open folder or disc");
    save(&mut welcome, "01-welcome");

    // Viewer with the CT selected by default.
    let path = dir.path().to_path_buf();
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .wgpu()
        .build_eframe(move |cc| SumMRIseApp::new(cc, AppOptions { open: Some(path), ..AppOptions::default() }));
    settle(&mut harness, Duration::from_secs(3));
    assert!(harness.query_all_by_label("CT CHEST 2.5mm (synthetic)").count() >= 1);
    assert!(harness.query_all_by_label("T2 TSE SAG (synthetic)").count() >= 1);
    save(&mut harness, "02-viewer-ct");

    harness.get_by_label("MPR").click();
    settle(&mut harness, Duration::from_millis(800));
    save(&mut harness, "03-mpr");

    harness.get_by_label("2 × 2").click();
    settle(&mut harness, Duration::from_millis(800));
    save(&mut harness, "04-quad");

    harness.get_by_label("All attributes").click();
    settle(&mut harness, Duration::from_millis(600));
    save(&mut harness, "05-attributes");

    first(&harness, "T2 TSE SAG (synthetic)").click();
    settle(&mut harness, Duration::from_secs(2));
    harness.get_by_label("Overview").click();
    settle(&mut harness, Duration::from_millis(600));
    save(&mut harness, "06-mr-sagittal");

    first(&harness, "Dose report (synthetic)").click();
    settle(&mut harness, Duration::from_secs(1));
    save(&mut harness, "07-secondary-capture");
}
