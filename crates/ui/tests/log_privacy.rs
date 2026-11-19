//! What the log says at its default level (info and above) when a study is
//! opened: no patient attributes, and no path of what was opened either,
//! because a folder or an archive can be named after the patient. Paths are
//! logged at debug level only (privacy specification, "No identifiers in logs").
//!
//! The logger is global to the process: everything runs in this one test.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use log::{Level, LevelFilter, Log, Metadata, Record};
use summrise_core::dicom::{Progress, scan};
use summrise_testkit::{PATIENT_ID, PATIENT_NAME, PhantomOptions, write_phantom_study};
use summrise_ui::{AppOptions, SumMRIseApp};

/// Keeps every record.
struct Capture(Mutex<Vec<(Level, String)>>);

impl Log for Capture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        // The application's own lines, as its default filter keeps them.
        if record.target().starts_with("summrise") {
            self.0.lock().expect("records").push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

#[test]
fn the_default_log_names_neither_the_patient_nor_what_was_opened() {
    log::set_logger(&CAPTURE).expect("the only logger of this process");
    log::set_max_level(LevelFilter::Debug);

    let dir = tempfile::tempdir().expect("tempdir");
    // A folder named after the patient, as a copy of a disc often is.
    let folder = dir.path().join(format!("{PATIENT_NAME} {PATIENT_ID}"));
    write_phantom_study(&folder, &PhantomOptions::small()).expect("fixture");

    // The real application opens it.
    let open = Some(folder.clone());
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .build_eframe(move |cc| SumMRIseApp::new(cc, AppOptions { open, ..AppOptions::default() }));
    let start = Instant::now();
    while harness.query_all_by_label_contains("CT CHEST").next().is_none() {
        assert!(start.elapsed() < Duration::from_secs(60), "the study did not open");
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    harness.run_steps(20);

    // A DICOMDIR that cannot be read, and one whose files are gone: both are warned about.
    let broken = dir.path().join(format!("{PATIENT_NAME} broken"));
    write_phantom_study(&broken, &PhantomOptions::small()).expect("fixture");
    std::fs::write(broken.join("DICOMDIR"), b"not a DICOMDIR at all").expect("write");
    scan(&broken, &Progress::new()).expect("the folder is scanned instead");
    let gone = dir.path().join(format!("{PATIENT_ID} gone"));
    write_phantom_study(&gone, &PhantomOptions::small()).expect("fixture");
    let images = gone.join("DICOM");
    assert!(images.is_dir(), "the images of the synthetic CD are in DICOM/");
    std::fs::rename(&images, gone.join("MOVED")).expect("rename");
    let _ = scan(&gone, &Progress::new());

    let records = CAPTURE.0.lock().expect("records");
    let default_level: Vec<&String> =
        records.iter().filter(|(level, _)| *level <= Level::Info).map(|(_, text)| text).collect();
    assert!(default_level.iter().any(|text| text.contains("opening")), "{default_level:?}");
    assert!(default_level.iter().any(|text| text.contains("DICOMDIR")), "the warnings are there: {default_level:?}");
    let root = dir.path().display().to_string();
    for text in &default_level {
        for private in ["PHANTOM", "SYNTHETIC", "SYN-0001", root.as_str()] {
            assert!(!text.contains(private), "{private:?} in the log at its default level: {text}");
        }
    }
    // For those who look for a fault with RUST_LOG=debug, the path is still there.
    let debug: Vec<&String> = records.iter().filter(|(level, _)| *level == Level::Debug).map(|(_, t)| t).collect();
    assert!(debug.iter().any(|text| text.contains(&folder.display().to_string())), "the path at debug level");
}
