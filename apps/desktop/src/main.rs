//! SumMRIse desktop entry point (macOS, Windows, Linux).
//!
//! Usage: `summrise [FOLDER | DICOMDIR | FILE | ARCHIVE.zip | IMAGE.iso]`, or
//! `summrise --version` (prints the version and exits; packaging checks use it).
//!
//! `SUMMRISE_TEST_CRASH=window` (or `background`) makes the program fail on
//! purpose as the window opens (or in a background thread), to check the crash
//! report and the message about it on a real system (`field-validation`).

// No console window for release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;

use eframe::egui;
use summrise_core::crash;
use summrise_ui::{APP_ID, APP_NAME, AppOptions, SumMRIseApp, system_languages};

const ICON_PNG: &[u8] = include_bytes!("../../../packaging/icons/icon-256.png");

fn main() -> anyhow::Result<()> {
    if std::env::args_os().nth(1).is_some_and(|a| a == "--version") {
        #[allow(clippy::print_stdout)]
        {
            println!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
        }
        return Ok(());
    }
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or("warn,summrise=info,summrise_core=info,summrise_dicom=info,summrise_ui=info"),
    )
    .init();
    // Crash reports hold no patient data (summrise_core::crash).
    let crash_folder = crash::default_folder();
    crash::install(crash_folder.clone());
    log::info!("{APP_NAME} {} starting", env!("CARGO_PKG_VERSION"));

    // Finder may pass a `-psn_…` process serial number on old macOS versions.
    let open =
        std::env::args_os().skip(1).map(PathBuf::from).find(|p| !p.to_string_lossy().starts_with("-psn") && p.exists());

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_app_id(APP_ID)
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([960.0, 600.0])
        .with_drag_and_drop(true);
    match eframe::icon_data::from_png_bytes(ICON_PNG) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(e) => log::warn!("window icon unavailable: {e}"),
    }

    let native = eframe::NativeOptions { viewport, persist_window: true, centered: true, ..Default::default() };
    let crash_report = crash_folder.as_deref().and_then(crash::report_of_last_run);
    let options = AppOptions { open, system_languages: system_languages(), crash_report };
    let test_crash = std::env::var("SUMMRISE_TEST_CRASH").unwrap_or_default();
    let create = move |cc: &eframe::CreationContext<'_>| -> Result<Box<dyn eframe::App>, _> {
        let app = SumMRIseApp::new(cc, options);
        match test_crash.as_str() {
            "window" => panic!("crash test (SUMMRISE_TEST_CRASH=window)"),
            "background" => {
                let _ = std::thread::Builder::new()
                    .name("summrise-crash-test".to_owned())
                    .spawn(|| panic!("crash test (SUMMRISE_TEST_CRASH=background)"));
            }
            _ => {}
        }
        Ok(Box::new(app))
    };
    // A fault on the thread of the window ends the event loop; it arrives here.
    let run = std::panic::catch_unwind(AssertUnwindSafe(|| eframe::run_native(APP_NAME, native, Box::new(create))));
    match run {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            let (title, text) = summrise_ui::start_failure_message(&e.to_string());
            message_box(&title, &text);
            Err(anyhow::anyhow!("failed to start the user interface: {e}"))
        }
        Err(_) => {
            // A panic that rayon passed on to this thread has its report held.
            let report = crash::flush_held(true).or_else(crash::fatal_report);
            let (title, text) = summrise_ui::fatal_message(report.as_deref());
            // Told now: no notice at the next start. Where no message box could be
            // shown (no zenity on Linux, say), the next start tells.
            if message_box(&title, &text)
                && let Some(folder) = &crash_folder
            {
                crash::mark_told(folder);
            }
            std::process::exit(101)
        }
    }
}

/// Shows a message box of the system; whether the user saw and closed it.
fn message_box(title: &str, text: &str) -> bool {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(title)
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show()
        == rfd::MessageDialogResult::Ok
}
