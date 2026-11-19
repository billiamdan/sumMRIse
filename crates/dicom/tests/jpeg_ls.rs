//! JPEG-LS in builds with the `jpeg-ls` feature, as the release packages are
//! built (OpenSpec change `jpeg-ls-support`): the synthetic series, re-encoded
//! by CharLS, decode to the values of the uncompressed originals — exactly for
//! JPEG-LS Lossless, within the error bound for near-lossless.

#![cfg(feature = "jpeg-ls")]

use std::path::Path;

use summrise_dicom::format::{lossy_family, transfer_syntax_name, transfer_syntax_supported};
use summrise_dicom::{LoadedSeries, Progress, SeriesImages, decode_instance, load_series, read_instance, scan};
use summrise_testkit::transcode::{JPEG_LS_LOSSLESS, JPEG_LS_NEAR_LOSSLESS, transcode_series};
use summrise_testkit::{PhantomOptions, write_phantom_study};

/// Every grey value of a decoded series, image by image.
fn values(series: &LoadedSeries) -> Vec<Vec<f32>> {
    let volume_values = |volume: &summrise_imaging::Volume| -> Vec<Vec<f32>> {
        volume.slices.iter().map(|slice| (0..slice.len()).map(|i| slice.get(i)).collect()).collect()
    };
    match &series.images {
        SeriesImages::Volume(volume) => volume_values(volume),
        SeriesImages::Stacks(volumes) => volumes.iter().flat_map(volume_values).collect(),
        SeriesImages::Frames(frames) => frames
            .iter()
            .map(|frame| match &frame.pixels {
                summrise_imaging::FramePixels::Gray(buffer) => (0..buffer.len()).map(|i| buffer.get(i)).collect(),
                summrise_imaging::FramePixels::Rgb(bytes) => bytes.iter().map(|&b| f32::from(b)).collect(),
            })
            .collect(),
    }
}

/// The series of `modality` in the original study and in a copy re-encoded
/// in the transfer syntax `uid`.
fn original_and_copy(modality: &str, uid: &str, quality: Option<u8>) -> (LoadedSeries, LoadedSeries, u16) {
    let dir = tempfile::tempdir().expect("tempdir");
    let original_dir = dir.path().join("original");
    let study = write_phantom_study(&original_dir, &PhantomOptions::small()).expect("fixture");
    let series = study.series(modality).expect("series");
    let copy_dir = dir.path().join("copy");
    transcode_series(series, &original_dir, &copy_dir, uid, quality).expect("CharLS encodes");

    let load = |root: &Path| {
        let index = scan(root, &Progress::new()).expect("scan");
        let found =
            index.studies.iter().flat_map(|s| &s.series).find(|s| s.uid == series.uid).expect("the series").clone();
        let loaded = load_series(&index.media, &found, &Progress::new()).expect("decoded");
        (index, found, loaded)
    };
    let (_, _, original) = load(&original_dir);
    let (index, copy, decoded) = load(&copy_dir);
    assert!(copy.instances.iter().all(|i| i.transfer_syntax_uid.trim_end_matches('\0') == uid), "re-encoded");
    assert!(copy.unsupported_transfer_syntaxes().is_empty(), "no \"not supported by this build\"");
    // The progressive path of the viewer decodes the same values, file by file.
    let header = &copy.instances[0];
    let one = decode_instance(read_instance(&index.media, header).expect("read")).expect("decode");
    assert_eq!(one.frames.len() as u32, header.frames.max(1));
    (original, decoded, header.bits_stored)
}

/// Spec scenario "JPEG-LS lossless series in a release build": the values equal
/// those of the uncompressed original (signed CT values in Hounsfield units).
#[test]
fn jpeg_ls_lossless_decodes_to_the_values_of_the_original() {
    assert!(transfer_syntax_supported(JPEG_LS_LOSSLESS));
    assert!(transfer_syntax_name(JPEG_LS_LOSSLESS).contains("JPEG-LS"));
    assert_eq!(lossy_family(JPEG_LS_LOSSLESS), None, "lossless: no Lossy mark");
    let (original, decoded, _) = original_and_copy("CT", JPEG_LS_LOSSLESS, None);
    assert_eq!(decoded.units.as_deref(), Some("HU"));
    assert_eq!(values(&decoded), values(&original), "bit-exact");
    assert_eq!(decoded.value_range, original.value_range);
}

/// Near-lossless JPEG-LS: every value within the error bound CharLS was given,
/// and the series counts as lossy.
#[test]
fn jpeg_ls_near_lossless_stays_within_its_error_bound() {
    assert!(transfer_syntax_supported(JPEG_LS_NEAR_LOSSLESS));
    assert_eq!(lossy_family(JPEG_LS_NEAR_LOSSLESS), Some("JPEG-LS"));
    let quality = 90u8;
    let (original, decoded, bits_stored) = original_and_copy("MR", JPEG_LS_NEAR_LOSSLESS, Some(quality));
    // The bound dicom-rs gives CharLS for a quality (NEAR, in stored values; MR has no rescale).
    let near = ((1i32 << (bits_stored - 4)) * (100 - i32::from(quality)) / 100).min(4096);
    assert!(near > 0, "a lossy encoding: NEAR {near}");
    let (original, decoded) = (values(&original), values(&decoded));
    assert_eq!(original.len(), decoded.len());
    let worst = original
        .iter()
        .zip(&decoded)
        .flat_map(|(a, b)| a.iter().zip(b).map(|(x, y)| (x - y).abs()))
        .fold(0.0f32, f32::max);
    assert!(worst <= near as f32, "largest error {worst}, NEAR {near}");
}
