//! egui user interface for SumMRIse, shared by the macOS, Windows and Linux
//! builds. Platform specifics live in the `summrise` binary (apps/desktop).

mod app;
mod browser;
mod cine;
mod compare;
mod crosshair;
mod display_check;
mod documents;
mod export;
mod fidelity;
mod guide;
mod inspector;
mod language;
mod licences;
mod measure;
mod orient;
mod pet;
mod placement;
mod print;
mod render;
mod stacks;
mod theme;
mod tiles;
mod viewer;
mod viewport;
mod volume_view;
mod welcome;
mod widgets;

pub use app::{AppOptions, SumMRIseApp};
pub use language::system_languages;

/// Application identifier (reverse-DNS), used for the macOS bundle, the
/// Linux desktop entry and the settings directory.
pub const APP_ID: &str = "com.summrise.Viewer";
pub const APP_NAME: &str = "SumMRIse";
