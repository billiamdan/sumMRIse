//! UI-agnostic application layer for SumMRIse.
//!
//! Owns everything a front-end needs besides drawing: background jobs,
//! the open session (selection, progressive series loading, memory budget,
//! thumbnails), viewing state (contrast, slices, MPR crosshair, zoom/pan),
//! settings and media detection. Keeping this free of UI types lets a
//! different front-end (e.g. a native SwiftUI shell via FFI) reuse it
//! unchanged.

mod contrast;
pub mod documents;
pub mod export;
pub mod guide;
pub mod jobs;
pub mod measure;
pub mod media;
pub mod orientation;
pub mod pet;
pub mod planes;
pub mod print;
pub mod refinement;
pub mod series;
pub mod session;
pub mod settings;
mod stacks;
pub mod view;
pub mod volume3d;
pub mod workspace;

pub use contrast::ContrastChoice;
pub use jobs::{Decoders, DicomFiles, ImageSource, Job, SeriesLoader, Stream, Waker};
pub use measure::{Grip, MeasureKind, Measurement, Measurements, Picture as MeasuredPicture, Values as PictureValues};
pub use media::MediaWatcher;
pub use orientation::DisplayOrientation;
pub use pet::Fusion;
pub use planes::PlaneTilt;
pub use refinement::{PictureRequest, PreviewSize, Progressive, Shown, Step, Ticket, VolumePictures};
pub use series::{ImageSlot, SeriesData};
pub use session::{
    LoadingSeries, OverBudget, OverlayProblem, OverlayStatus, SeriesKey, SeriesState, Session, SessionEvent,
};
pub use settings::{Appearance, RecentEntry, RecentStudies, Settings, recent_label};
pub use view::{Layout, Tool, ViewKind, ViewState, ViewTransform};
pub use workspace::{Gesture, Grid, Link, LinkKind, LinkStatus, NotLinked, Tiles, Viewport, Workspace};

/// Re-exported lower layers for convenience.
pub use summrise_dicom as dicom;
pub use summrise_imaging as imaging;
