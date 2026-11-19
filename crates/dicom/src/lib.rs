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
//! * [`Acquisition`] — how the images of a series were made, kept in the index.
//! * [`Stacks`] — how a series that holds several sets of images at the same
//!   slice positions (time points, echoes, b-values …) is split into stacks.
//! * [`Media`] — where all of the above read from: a folder, or a ZIP archive
//!   or an ISO 9660 disc image that is read in memory and never unpacked.
//! * [`Fidelity`] and [`InstanceExtras`] — what limits or completes the
//!   picture: lossy compression, burned-in text, contrast tables, padding and
//!   scanner graphics.
//! * [`pet`] — standardised uptake values of PET series (SUVbw) and why they
//!   cannot always be computed.
//! * [`documents`] — reports and documents on the disc, and reading them.
//!
//! All functions are blocking and CPU/IO bound; run them off the UI thread.
//! Patient-identifying values are never logged above `debug` level.

pub mod acquisition;
mod attrs;
pub mod decode;
pub mod discovery;
pub mod documents;
mod error;
pub mod fidelity;
pub mod format;
mod graphics;
pub mod index;
mod iso9660;
pub mod media;
pub mod metadata;
mod nesting;
pub mod panics;
pub mod pet;
mod pixels;
mod progress;
pub mod stacks;
mod zip_archive;

pub use acquisition::Acquisition;
pub use decode::{
    DecodedInstance, FrameInfo, FrameRef, LoadedSeries, SeriesImages, SeriesPlan, Thumbnail, decode_instance,
    decode_warning, load_series, plan_series, read_instance, series_stacks, thumbnail,
};
pub use discovery::{Discovery, DiscoverySource, discover};
pub use documents::{DocumentContent, DocumentKind, DocumentRef, read_document};
pub use error::{DicomError, Result};
pub use fidelity::{Fidelity, LossyCompression};
pub use index::{IndexStats, InstanceHeader, Patient, Rescale, Series, Study, StudyIndex, index_files, scan};
pub use media::{ArchiveProblem, MAX_ENTRY_BYTES, Media, MediaKind};
pub use metadata::{InstanceMetadata, SummaryRow, SummarySection, TagNode, read_metadata};
pub use pet::{PetHeader, SUV_UNIT, SuvBasis, SuvRefusal, SuvScale};
pub use pixels::{FrameExtras, InstanceExtras};
pub use progress::{Progress, ProgressSnapshot};
pub use stacks::{FrameValues, StackKind, StackValue, Stacks};
