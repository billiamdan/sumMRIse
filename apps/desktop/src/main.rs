//! SumMRIse desktop entry point (macOS, Windows, Linux).
//!
//! Usage: `summrise [FOLDER | DICOMDIR | FILE | ARCHIVE.zip | IMAGE.iso]`

// No console window for release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

use eframe::egui;
use summrise_ui::{APP_ID, APP_NAME, AppOptions, SumMRIseApp, system_languages};

const ICON_PNG: &[u8] = include_bytes!("../../../packaging/icons/icon-256.png");

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or("warn,summrise=info,summrise_core=info,summrise_dicom=info,summrise_ui=info"),
    )
    .init();
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
    let options = AppOptions { open, system_languages: system_languages() };
    eframe::run_native(APP_NAME, native, Box::new(move |cc| Ok(Box::new(SumMRIseApp::new(cc, options)))))
        .map_err(|e| anyhow::anyhow!("failed to start the user interface: {e}"))
}
