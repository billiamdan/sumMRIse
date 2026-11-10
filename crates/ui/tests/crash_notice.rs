//! The notice about a crash that ended the previous run (OpenSpec change
//! `crash-reports`): it says what happened, that the report holds no patient
//! data, and where the report is; Close closes it.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use summrise_ui::{AppOptions, SumMRIseApp};

fn shows(harness: &Harness<'_, SumMRIseApp>, text: &str) -> bool {
    harness.query_all_by_label_contains(text).next().is_some()
}

#[test]
fn a_crash_of_the_last_run_is_told_at_the_next_start() {
    let report = std::path::PathBuf::from("/home/someone/.local/state/summrise/crash-reports/SumMRIse-crash-2026.txt");
    let options = AppOptions { crash_report: Some(report.clone()), ..AppOptions::default() };
    let mut harness =
        Harness::builder().with_size([960.0, 600.0]).build_eframe(move |cc| SumMRIseApp::new(cc, options));
    harness.run_steps(4);
    assert!(shows(&harness, "SumMRIse closed unexpectedly"));
    assert!(shows(&harness, "stopped because of a fault in the program"));
    assert!(shows(&harness, "It contains no patient data"));
    assert!(shows(&harness, &report.display().to_string()), "the path of the report");
    if let Some(dir) = std::env::var_os("SUMMRISE_SNAPSHOT_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("snapshot dir");
        if let Ok(image) = harness.render() {
            image.save(dir.join("crash-notice.png")).expect("save");
        }
    }
    harness.get_by_label("Copy path").click();
    harness.run_steps(2);
    assert!(shows(&harness, "SumMRIse closed unexpectedly"), "copying leaves it open");
    harness.get_by_label("Close").click();
    harness.run_steps(3);
    assert!(!shows(&harness, "SumMRIse closed unexpectedly"), "Close closes it");
}

#[test]
fn without_a_crash_there_is_no_notice() {
    let mut harness =
        Harness::builder().with_size([960.0, 600.0]).build_eframe(|cc| SumMRIseApp::new(cc, AppOptions::default()));
    harness.run_steps(4);
    assert!(!shows(&harness, "closed unexpectedly"));
}
