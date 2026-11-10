//! Copies of the synthetic files in another transfer syntax, re-encoded by the
//! encoders of the DICOM library (OpenSpec change `jpeg-ls-support`).
//!
//! The JPEG-LS encoder (CharLS) is only there when the build has it: the
//! `jpeg-ls` feature of `summrise-dicom` turns it on for the whole build, the
//! testkit included. Without it [`transcode_file`] fails with "not supported".

use std::path::{Path, PathBuf};

use dicom_encoding::adapters::EncodeOptions;
use dicom_object::open_file;
use dicom_pixeldata::Transcode;
use dicom_transfer_syntax_registry::{TransferSyntaxIndex, TransferSyntaxRegistry};

use crate::{FixtureError, WrittenSeries, dicom_err};

/// JPEG-LS Lossless Image Compression.
pub const JPEG_LS_LOSSLESS: &str = "1.2.840.10008.1.2.4.80";
/// JPEG-LS Lossy (Near-Lossless) Image Compression.
pub const JPEG_LS_NEAR_LOSSLESS: &str = "1.2.840.10008.1.2.4.81";

/// Writes `source` to `target` in the transfer syntax `uid`. `quality` (0 to
/// 100) sets how much detail a lossy encoder may discard; `None` is lossless
/// where the transfer syntax allows it.
pub fn transcode_file(source: &Path, target: &Path, uid: &str, quality: Option<u8>) -> Result<(), FixtureError> {
    let ts = TransferSyntaxRegistry.get(uid).ok_or_else(|| FixtureError::Dicom(format!("{uid} is not known")))?;
    let mut obj = open_file(source).map_err(dicom_err)?;
    let mut options = EncodeOptions::default();
    options.quality = quality;
    obj.transcode_with_options(ts, options).map_err(dicom_err)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    obj.write_to_file(target).map_err(dicom_err)?;
    Ok(())
}

/// Writes every file of `series` into `folder` in the transfer syntax `uid`,
/// with the same names; returns the new files.
pub fn transcode_series(
    series: &WrittenSeries,
    root: &Path,
    folder: &Path,
    uid: &str,
    quality: Option<u8>,
) -> Result<Vec<PathBuf>, FixtureError> {
    series
        .files
        .iter()
        .map(|file| {
            let relative = file.strip_prefix(root).unwrap_or(file);
            let target = folder.join(relative);
            transcode_file(file, &target, uid, quality).map(|()| target)
        })
        .collect()
}
