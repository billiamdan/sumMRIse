//! DICOM media import for SumMRIse.
//!
//! * [`scan`] — find files on a CD/DVD/folder (DICOMDIR first, folder scan as
//!   fallback) and index their headers into studies and series.
//! * [`load_series`] — decode pixel data (compressed transfer syntaxes,
//!   rescale to modality units, multi-frame) into frames or a volume.
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

pub use decode::{FrameInfo, FrameRef, LoadedSeries, SeriesImages, Thumbnail, load_series, thumbnail};
pub use discovery::{Discovery, DiscoverySource, discover};
pub use error::{DicomError, Result};
pub use index::{IndexStats, InstanceHeader, Patient, Series, Study, StudyIndex, index_files, scan};
pub use metadata::{InstanceMetadata, SummarySection, TagNode, read_metadata};
pub use progress::{Progress, ProgressSnapshot};
