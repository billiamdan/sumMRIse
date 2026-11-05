//! DICOM media import for SumMRIse.
//!
//! * [`scan`] — find files on a CD/DVD/folder (DICOMDIR first, folder scan as
//!   fallback) and index their headers into studies and series.
//! * [`plan_series`] — display order and volume geometry of a series from the
//!   indexed headers, before any pixel data is read.
//! * [`read_instance`] / [`decode_instance`] — pixel data of one file
//!   (compressed transfer syntaxes, rescale to modality units, multi-frame),
//!   so that images can be shown as they arrive.
//! * [`load_series`] — the same for a whole series at once, into frames or a
//!   volume.
//! * [`read_metadata`] — full attribute tree plus a curated summary.
//!
//! All functions are blocking and CPU/IO bound; run them off the UI thread.
//! Patient-identifying values are never logged above `debug` level.

mod attrs;
pub mod decode;
pub mod discovery;
mod error;
pub mod format;
pub mod index;
pub mod metadata;
mod progress;

pub use decode::{
    DecodedInstance, FrameInfo, FrameRef, LoadedSeries, SeriesImages, SeriesPlan, Thumbnail, decode_instance,
    decode_warning, load_series, plan_series, read_instance, thumbnail,
};
pub use discovery::{Discovery, DiscoverySource, discover};
pub use error::{DicomError, Result};
pub use index::{IndexStats, InstanceHeader, Patient, Rescale, Series, Study, StudyIndex, index_files, scan};
pub use metadata::{InstanceMetadata, SummarySection, TagNode, read_metadata};
pub use progress::{Progress, ProgressSnapshot};
