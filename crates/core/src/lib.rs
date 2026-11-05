//! UI-agnostic application layer for SumMRIse.
//!
//! Owns everything a front-end needs besides drawing: background jobs,
//! the open session (selection, decoded-series cache, thumbnails), viewing
//! state (contrast, slices, MPR crosshair, zoom/pan), settings and media
//! detection. Keeping this free of UI types lets a different front-end
//! (e.g. a native SwiftUI shell via FFI) reuse it unchanged.

pub mod jobs;
pub mod media;
pub mod session;
pub mod settings;
pub mod view;

pub use jobs::{Job, Stream, Waker};
pub use media::MediaWatcher;
pub use session::{SeriesKey, SeriesState, Session, SessionEvent};
pub use settings::{Appearance, RecentEntry, RecentStudies, Settings, recent_label};
pub use view::{Layout, Tool, ViewKind, ViewState, ViewTransform};

/// Re-exported lower layers for convenience.
pub use summrise_dicom as dicom;
pub use summrise_imaging as imaging;
