//! Crash reports with the real panic hook (OpenSpec change `crash-reports`).
//!
//! The hook is global to the process, so everything runs in this one test, in
//! order: panics whose messages quote the patient's name, ID and birth date,
//! the path of the opened folder and DICOM values; a panic with a literal
//! message; an expected panic on damaged data; and panics on rayon's worker
//! threads, caught and not caught.

use std::path::{Path, PathBuf};

use rayon::prelude::*;
use summrise_core::crash;
use summrise_core::dicom::{Progress, panics, scan};
use summrise_testkit::{PATIENT_ID, PATIENT_NAME, PhantomOptions, write_phantom_study};

/// Runs `work` on a thread with this name and waits for it to end.
fn on_thread(name: &str, work: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().name(name.to_owned()).spawn(work).expect("thread").join();
}

fn reports(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|entries| entries.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    found.retain(|p| p.extension().is_some_and(|e| e == "txt") && !p.ends_with("unseen.txt"));
    found.sort();
    found
}

#[test]
fn crash_reports_hold_no_patient_data() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A folder named after the patient, as a disc copied by a relative might be.
    let disc = dir.path().join(format!("{PATIENT_NAME} {PATIENT_ID}"));
    write_phantom_study(&disc, &PhantomOptions::small()).expect("fixture");
    let index = scan(&disc, &Progress::new()).expect("scan");
    let folder = dir.path().join("Crash reports");
    crash::install(Some(folder.clone()));

    // 1. A fault with a message composed at run time from everything private.
    let study = format!("{:?}", index.studies[0]);
    assert!(study.contains("PHANTOM") && study.contains("SYN-0001"), "the Debug text holds the patient");
    let path = disc.display().to_string();
    on_thread("summrise-scan", move || panic!("cannot read {path}: {study} (born 1970-01-01, 19700101)"));
    let first = crash::take_new_report().expect("a report of this run");
    assert_eq!(reports(&folder), std::slice::from_ref(&first));
    let text = std::fs::read_to_string(&first).expect("report");
    for private in ["PHANTOM", "SYNTHETIC", "SYN-0001", "19700101", "1970-01-01", "Crash reports", "cannot read"] {
        assert!(!text.contains(private), "{private:?} is in the report:\n{text}");
    }
    assert!(!text.contains(&dir.path().display().to_string()), "no path of the opened folder");
    assert!(text.contains("Message:  withheld"), "{text}");
    assert!(text.contains("Thread:   summrise-scan"), "{text}");
    assert!(text.contains("Location: crates/core/tests/crash_report.rs:"), "{text}");
    assert!(text.contains("Backtrace:"), "{text}");
    assert_eq!(crash::take_new_report(), None, "told once");
    assert_eq!(crash::report_of_last_run(&folder), None, "not the crash of a run: the window lives on");

    // 2. A literal message of the program is repeated.
    on_thread("summrise-render3d", || panic!("the volume has no slices"));
    let second = crash::take_new_report().expect("a report");
    let text = std::fs::read_to_string(&second).expect("report");
    assert!(text.contains("Message:  the volume has no slices"), "{text}");

    // 3. A thread the program did not name is not named, nor is one named after the patient.
    on_thread(PATIENT_NAME, || panic!("no name"));
    let text = std::fs::read_to_string(crash::take_new_report().expect("a report")).expect("report");
    assert!(text.contains("Thread:   another thread") && !text.contains("PHANTOM"), "{text}");

    // 4. A panic on damaged data, caught: an error, not a crash.
    let before = reports(&folder).len();
    on_thread("summrise-load", || {
        let result = panics::catch(|| panic!("damaged pixel data of {PATIENT_NAME}"));
        assert!(result.is_err());
    });
    assert_eq!(crash::take_new_report(), None);
    assert_eq!(reports(&folder).len(), before, "no report");

    // 5. rayon: a worker's panic caught by the thread that started the work…
    on_thread("summrise-decode-0", || {
        let result = panics::catch(|| {
            (0..64).into_par_iter().for_each(|i| assert_ne!(i, 40, "value {PATIENT_ID}"));
        });
        assert!(result.is_err());
    });
    assert_eq!(crash::take_new_report(), None, "caught: no report");
    assert_eq!(panics::take_held(), None, "and nothing held");
    assert_eq!(reports(&folder).len(), before);

    // …and one that ends a thread of the program: written when it ends.
    on_thread("summrise-thumbnails", || {
        crash::watch(|| (0..64).into_par_iter().for_each(|i| assert_ne!(i, 40, "value {PATIENT_ID}")));
    });
    let worker = crash::take_new_report().expect("a report when the thread ends");
    let text = std::fs::read_to_string(&worker).expect("report");
    assert!(text.contains("Thread:   a worker thread"), "{text}");
    assert!(text.contains("Message:  withheld") && !text.contains("SYN-0001"), "{text}");

    // 6. A fatal one (on the main thread, or flushed where the window ended) is
    // told about at the next start, once.
    panics::hold("held".to_owned());
    let fatal = crash::flush_held(true).expect("written");
    assert_eq!(crash::take_new_report(), None, "the window is gone: nothing to show in it");
    assert_eq!(crash::fatal_report().as_ref(), Some(&fatal), "for the message after the window closed");
    assert_eq!(crash::report_of_last_run(&folder), Some(fatal));
    assert_eq!(crash::report_of_last_run(&folder), None);
    assert!(reports(&folder).len() <= crash::KEPT);
}
