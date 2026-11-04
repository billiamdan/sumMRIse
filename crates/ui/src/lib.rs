//! egui user interface for SumMRIse, shared by the macOS, Windows and Linux
//! builds. Platform specifics live in the `summrise` binary (apps/desktop).

mod app;
mod browser;
mod inspector;
mod render;
mod theme;
mod viewer;
mod viewport;
mod welcome;
mod widgets;

pub use app::{AppOptions, SumMRIseApp};

/// Application identifier (reverse-DNS), used for the macOS bundle, the
/// Linux desktop entry and the settings directory.
pub const APP_ID: &str = "com.summrise.Viewer";
pub const APP_NAME: &str = "SumMRIse";
