//! Series that hold several sets of images at the same slice positions — time
//! points, echoes, diffusion weightings — and a cine of one slice: the data
//! that the viewer splits into stacks, or deliberately does not.
//!
//! [`write_stacks_study`] writes one MR study (a folder scan, no DICOMDIR) of
//! seven series on one axial grid: images of [`SIZE`] × [`SIZE`] pixels of
//! [`PIXEL_MM`] mm whose first pixel lies at R 120, A 120 mm ([`ORIGIN_MM`]),
//! so that the marker sphere at [`MARKER_POSITION`] (L 40, A 35, S 50 mm) has a
//! voxel of its own at its centre (column 32, row 17). Its value tells the sets
//! apart. In index order (series are sorted by number):
//!
//! | index | series | images |
//! |---|---|---|
//! | [`DYNAMIC`] | 3 time points × 12 slices | Temporal Position Identifier 1–3 and Trigger Time; marker [`DYNAMIC_MARKER`] |
//! | [`DUAL_ECHO`] | 2 echoes × 12 slices | Echo Number 1–2, Echo Time [`ECHO_TIMES_MS`], a stored window each ([`ECHO_WINDOWS`]) |
//! | [`DIFFUSION`] | 2 b-values × 12 slices | Diffusion b-value [`B_VALUES`], one echo time |
//! | [`REPEATED`] | 3 × 12 slices | the data of [`DYNAMIC`] without anything that tells the sets apart |
//! | [`INCOMPLETE`] | 35 images | [`DYNAMIC`] with the image of time point 3 at slice 5 missing |
//! | [`CINE`] | 20 images of one slice | Trigger Time 0–760 ms, Frame Time [`CINE_FRAME_TIME_MS`] |
//! | [`ENHANCED`] | 1 Enhanced MR file | 2 echoes ([`ENHANCED_ECHO_TIMES_MS`]) × 6 slices, frames interleaved |
//!
//! The 12 slices lie at S 20 to 75 mm, 5 mm apart ([`slice_height`]); the six of
//! the Enhanced object at S 20 to 70 mm, 10 mm apart. Every file of a series is
//! written in a shuffled order.

use std::fs;
use std::path::Path;

use dicom_core::value::{DataSetSequence, PrimitiveValue};
use dicom_core::{DataElement, Length, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;

use crate::{
    FixtureError, Geometry, MARKER_POSITION, UID_ROOT, WrittenSeries, WrittenStudy, base_object, in_marker, noise,
    put_ds, put_geometry, put_mono16, put_str, write_file,
};

/// Pixels per side of every image.
pub const SIZE: u32 = 48;
/// Pixel spacing of every image, in mm (rows and columns).
pub const PIXEL_MM: f64 = 5.0;
/// Patient position (x, y in mm, LPS) of the first pixel of every image: R 120, A 120.
pub const ORIGIN_MM: [f64; 2] = [-120.0, -120.0];
/// Slices of the series with repeated positions.
pub const SLICES: u32 = 12;
/// Height (S, mm) of the first slice and the distance between slices.
pub const FIRST_SLICE_MM: f64 = 20.0;
pub const SLICE_GAP_MM: f64 = 5.0;

/// Index of each series in the indexed study.
pub const DYNAMIC: usize = 0;
pub const DUAL_ECHO: usize = 1;
pub const DIFFUSION: usize = 2;
pub const REPEATED: usize = 3;
pub const INCOMPLETE: usize = 4;
pub const CINE: usize = 5;
pub const ENHANCED: usize = 6;

/// Signal of the body (outside the marker) in the dynamic series and its copies.
pub const DYNAMIC_BODY: f64 = 200.0;
/// Value of the marker in time points 1, 2 and 3.
pub const DYNAMIC_MARKER: [f64; 3] = [600.0, 900.0, 1200.0];
/// Trigger Time of the three time points, in ms.
pub const DYNAMIC_TRIGGER_MS: [f64; 3] = [0.0, 15_000.0, 30_000.0];
/// Echo Time of echoes 1 and 2, in ms.
pub const ECHO_TIMES_MS: [f64; 2] = [15.0, 90.0];
/// Body and marker signal of echoes 1 and 2: the second echo is much darker.
pub const ECHO_SIGNAL: [(f64, f64); 2] = [(2000.0, 3500.0), (300.0, 600.0)];
/// The window stored with each echo: (centre, width).
pub const ECHO_WINDOWS: [(f64, f64); 2] = [(2000.0, 4000.0), (350.0, 700.0)];
/// Diffusion b-values of the diffusion series, in s/mm².
pub const B_VALUES: [f64; 2] = [0.0, 1000.0];
/// Body and marker signal at b = 0 and b = 1000.
pub const DIFFUSION_SIGNAL: [(f64, f64); 2] = [(900.0, 1400.0), (250.0, 700.0)];
/// Images of the cine of one slice, and the time between them.
pub const CINE_FRAMES: u32 = 20;
pub const CINE_FRAME_TIME_MS: f64 = 40.0;
/// Effective Echo Time of the two echoes of the Enhanced object, in ms.
pub const ENHANCED_ECHO_TIMES_MS: [f64; 2] = [30.0, 100.0];
/// Body and marker signal of the two echoes of the Enhanced object.
pub const ENHANCED_SIGNAL: [(f64, f64); 2] = [(1500.0, 2500.0), (400.0, 900.0)];
/// Slices of the Enhanced object, 10 mm apart from S 20 mm.
pub const ENHANCED_SLICES: u32 = 6;
/// Time point and slice (both from 0) of the image missing from [`INCOMPLETE`].
pub const MISSING_IMAGE: (u32, u32) = (2, 4);

/// Height (S, mm) of slice `k` (from 0) of the series with 12 positions.
pub fn slice_height(k: u32) -> f64 {
    FIRST_SLICE_MM + f64::from(k) * SLICE_GAP_MM
}

/// One series of the study.
struct Spec {
    number: u32,
    description: &'static str,
}

impl Spec {
    fn uid(&self) -> String {
        format!("{UID_ROOT}.5.{}", self.number)
    }
}

/// The body: an ellipsoid around the marker.
fn in_body(p: [f64; 3]) -> bool {
    ((p[0] / 95.0).powi(2) + ((p[1] + 10.0) / 75.0).powi(2) + ((p[2] - 47.5) / 80.0).powi(2)) <= 1.0
}

/// Signal at `p`: `body` (with a little noise) inside the body, `marker` in the marker.
fn signal(p: [f64; 3], body: f64, marker: f64) -> f64 {
    if in_marker(p) {
        marker
    } else if in_body(p) {
        body + 10.0 * noise(p)
    } else {
        0.0
    }
}

/// An axial image at height `z` with pixel values from `value`.
fn axial_image(
    study_uid: &str,
    spec: &Spec,
    instance: u32,
    z: f64,
    value: impl Fn([f64; 3]) -> f64,
) -> InMemDicomObject {
    let geom = Geometry {
        position: [ORIGIN_MM[0], ORIGIN_MM[1], z],
        orientation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        spacing: PIXEL_MM,
    };
    let pixels = (0..SIZE * SIZE).map(|i| value(geom.pixel(i % SIZE, i / SIZE)).round().clamp(0.0, 4095.0) as u16);
    let uid = spec.uid();
    let mut obj = base_object(
        study_uid,
        &uid,
        &format!("{uid}.{instance}"),
        uids::MR_IMAGE_STORAGE,
        "MR",
        spec.number,
        spec.description,
        instance,
    );
    put_geometry(&mut obj, &geom, SIZE, SIZE);
    put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "M", "ND"]);
    put_ds(&mut obj, tags::SLICE_THICKNESS, &[SLICE_GAP_MM]);
    put_ds(&mut obj, tags::SLICE_LOCATION, &[z]);
    put_str(&mut obj, tags::BODY_PART_EXAMINED, VR::CS, &["ABDOMEN"]);
    put_mono16(&mut obj, SIZE, SIZE, 12, pixels.collect());
    obj
}

/// Writes the files of one series in a shuffled order (file names follow the
/// writing order, so the index meets them shuffled as well).
fn write_series(study_dir: &Path, spec: &Spec, images: Vec<InMemDicomObject>) -> Result<WrittenSeries, FixtureError> {
    let dir = study_dir.join(format!("SE{:06}", spec.number));
    fs::create_dir_all(&dir)?;
    let n = images.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|k| (k * 7919) % n.max(1));
    let mut slots: Vec<Option<InMemDicomObject>> = images.into_iter().map(Some).collect();
    let mut files = Vec::with_capacity(n);
    for (file_index, k) in order.into_iter().enumerate() {
        let Some(obj) = slots[k].take() else { continue };
        let path = dir.join(format!("IM{:06}", file_index + 1));
        write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
        files.push(path);
    }
    Ok(WrittenSeries { uid: spec.uid(), number: spec.number, modality: "MR", description: spec.description, files })
}

/// Writes the study of series with repeated positions below `root` (see the
/// module documentation).
pub fn write_stacks_study(root: &Path) -> Result<WrittenStudy, FixtureError> {
    let study_uid = format!("{UID_ROOT}.5");
    let study_dir = root.join("DICOM").join("ST000005");
    let series = vec![
        dynamic(&study_dir, &study_uid, &Spec { number: 11, description: "DYNAMIC (synthetic)" }, true, None)?,
        dual_echo(&study_dir, &study_uid)?,
        diffusion(&study_dir, &study_uid)?,
        dynamic(&study_dir, &study_uid, &Spec { number: 14, description: "REPEATED (synthetic)" }, false, None)?,
        dynamic(
            &study_dir,
            &study_uid,
            &Spec { number: 15, description: "DYNAMIC INCOMPLETE (synthetic)" },
            true,
            Some(MISSING_IMAGE),
        )?,
        cine(&study_dir, &study_uid)?,
        enhanced(&study_dir, &study_uid)?,
    ];
    Ok(WrittenStudy { root: root.to_path_buf(), study_uid, series, dicomdir: None })
}

/// 3 time points × 12 slices, numbered time point by time point. With
/// `recorded`, every image carries its Temporal Position Identifier and Trigger
/// Time; without, nothing tells the time points apart. `missing` leaves out
/// one image (time point, slice).
fn dynamic(
    study_dir: &Path,
    study_uid: &str,
    spec: &Spec,
    recorded: bool,
    missing: Option<(u32, u32)>,
) -> Result<WrittenSeries, FixtureError> {
    let mut images = Vec::new();
    for t in 0..3u32 {
        for k in 0..SLICES {
            if missing == Some((t, k)) {
                continue;
            }
            let marker = DYNAMIC_MARKER[t as usize];
            let mut obj =
                axial_image(study_uid, spec, t * SLICES + k + 1, slice_height(k), |p| signal(p, DYNAMIC_BODY, marker));
            put_ds(&mut obj, tags::REPETITION_TIME, &[5.0]);
            put_ds(&mut obj, tags::ECHO_TIME, &[2.0]);
            put_str(&mut obj, tags::SCANNING_SEQUENCE, VR::CS, &["GR"]);
            if recorded {
                put_str(&mut obj, tags::TEMPORAL_POSITION_IDENTIFIER, VR::IS, &[&(t + 1).to_string()]);
                put_str(&mut obj, tags::NUMBER_OF_TEMPORAL_POSITIONS, VR::IS, &["3"]);
                put_ds(&mut obj, tags::TRIGGER_TIME, &[DYNAMIC_TRIGGER_MS[t as usize]]);
            }
            images.push(obj);
        }
    }
    write_series(study_dir, spec, images)
}

/// Two echoes of every slice, numbered slice by slice (echo 1 and 2 alternate).
fn dual_echo(study_dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec { number: 12, description: "DUAL ECHO (synthetic)" };
    let mut images = Vec::new();
    for k in 0..SLICES {
        for e in 0..2usize {
            let (body, marker) = ECHO_SIGNAL[e];
            let instance = k * 2 + e as u32 + 1;
            let mut obj = axial_image(study_uid, &spec, instance, slice_height(k), |p| signal(p, body, marker));
            put_ds(&mut obj, tags::REPETITION_TIME, &[3000.0]);
            put_ds(&mut obj, tags::ECHO_TIME, &[ECHO_TIMES_MS[e]]);
            put_str(&mut obj, tags::ECHO_NUMBERS, VR::IS, &[&(e + 1).to_string()]);
            put_str(&mut obj, tags::SCANNING_SEQUENCE, VR::CS, &["SE"]);
            let (center, width) = ECHO_WINDOWS[e];
            put_ds(&mut obj, tags::WINDOW_CENTER, &[center]);
            put_ds(&mut obj, tags::WINDOW_WIDTH, &[width]);
            images.push(obj);
        }
    }
    write_series(study_dir, &spec, images)
}

/// b = 0 and b = 1000 of every slice, one echo time for all.
fn diffusion(study_dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec { number: 13, description: "DWI (synthetic)" };
    let mut images = Vec::new();
    for (b, &b_value) in B_VALUES.iter().enumerate() {
        let (body, marker) = DIFFUSION_SIGNAL[b];
        for k in 0..SLICES {
            let instance = b as u32 * SLICES + k + 1;
            let mut obj = axial_image(study_uid, &spec, instance, slice_height(k), |p| signal(p, body, marker));
            put_ds(&mut obj, tags::REPETITION_TIME, &[5000.0]);
            put_ds(&mut obj, tags::ECHO_TIME, &[90.0]);
            put_str(&mut obj, tags::SCANNING_SEQUENCE, VR::CS, &["EP"]);
            put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "DIFFUSION", "NONE"]);
            obj.put(DataElement::new(tags::DIFFUSION_B_VALUE, VR::FD, PrimitiveValue::from(b_value)));
            images.push(obj);
        }
    }
    write_series(study_dir, &spec, images)
}

/// 20 images of the slice through the marker, 40 ms apart; the marker grows brighter.
fn cine(study_dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec { number: 16, description: "CINE 1 SLICE (synthetic)" };
    let mut images = Vec::new();
    for t in 0..CINE_FRAMES {
        let marker = 400.0 + 30.0 * f64::from(t);
        let mut obj = axial_image(study_uid, &spec, t + 1, MARKER_POSITION[2], |p| signal(p, DYNAMIC_BODY, marker));
        put_ds(&mut obj, tags::TRIGGER_TIME, &[f64::from(t) * CINE_FRAME_TIME_MS]);
        put_ds(&mut obj, tags::FRAME_TIME, &[CINE_FRAME_TIME_MS]);
        put_str(&mut obj, tags::SCANNING_SEQUENCE, VR::CS, &["GR"]);
        images.push(obj);
    }
    write_series(study_dir, &spec, images)
}

fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> DataElement<InMemDicomObject> {
    DataElement::new(tag, VR::SQ, DataSetSequence::new(items, Length::UNDEFINED))
}

fn item(elements: Vec<DataElement<InMemDicomObject>>) -> InMemDicomObject {
    InMemDicomObject::from_element_iter(elements)
}

/// A decimal string element, written as the other fixtures write them.
fn decimals(tag: Tag, values: &[f64]) -> DataElement<InMemDicomObject> {
    let mut holder = InMemDicomObject::new_empty();
    put_ds(&mut holder, tag, values);
    holder.get(tag).cloned().unwrap_or_else(|| DataElement::new(tag, VR::DS, PrimitiveValue::Empty))
}

/// One Enhanced MR file: 6 slices × 2 echoes, the frames of one slice next to
/// each other (slice 1 echo 1, slice 1 echo 2, slice 2 echo 1 …). Positions
/// and echo times are in the per-frame functional groups, orientation and
/// pixel spacing in the shared one.
fn enhanced(study_dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec { number: 17, description: "ENHANCED DUAL ECHO (synthetic)" };
    let uid = spec.uid();
    let mut obj = base_object(
        study_uid,
        &uid,
        &format!("{uid}.1"),
        uids::ENHANCED_MR_IMAGE_STORAGE,
        "MR",
        spec.number,
        spec.description,
        1,
    );
    let mut frames = Vec::new();
    let mut pixels: Vec<u16> = Vec::new();
    for k in 0..ENHANCED_SLICES {
        let z = FIRST_SLICE_MM + f64::from(k) * 10.0;
        let geom = Geometry {
            position: [ORIGIN_MM[0], ORIGIN_MM[1], z],
            orientation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            spacing: PIXEL_MM,
        };
        for (e, &echo_time) in ENHANCED_ECHO_TIMES_MS.iter().enumerate() {
            let (body, marker) = ENHANCED_SIGNAL[e];
            pixels.extend(
                (0..SIZE * SIZE)
                    .map(|i| signal(geom.pixel(i % SIZE, i / SIZE), body, marker).round().clamp(0.0, 4095.0) as u16),
            );
            frames.push(item(vec![
                sequence(
                    tags::PLANE_POSITION_SEQUENCE,
                    vec![item(vec![decimals(tags::IMAGE_POSITION_PATIENT, &geom.position)])],
                ),
                sequence(
                    tags::MR_ECHO_SEQUENCE,
                    vec![item(vec![DataElement::new(
                        tags::EFFECTIVE_ECHO_TIME,
                        VR::FD,
                        PrimitiveValue::from(echo_time),
                    )])],
                ),
                sequence(
                    tags::FRAME_CONTENT_SEQUENCE,
                    vec![item(vec![DataElement::new(
                        tags::IN_STACK_POSITION_NUMBER,
                        VR::UL,
                        PrimitiveValue::from(k + 1),
                    )])],
                ),
            ]));
        }
    }
    let shared = item(vec![
        sequence(
            tags::PIXEL_MEASURES_SEQUENCE,
            vec![item(vec![
                decimals(tags::PIXEL_SPACING, &[PIXEL_MM, PIXEL_MM]),
                decimals(tags::SLICE_THICKNESS, &[5.0]),
            ])],
        ),
        sequence(
            tags::PLANE_ORIENTATION_SEQUENCE,
            vec![item(vec![decimals(tags::IMAGE_ORIENTATION_PATIENT, &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0])])],
        ),
    ]);
    obj.put(sequence(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, vec![shared]));
    let count = frames.len();
    obj.put(sequence(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE, frames));
    put_str(&mut obj, tags::NUMBER_OF_FRAMES, VR::IS, &[&count.to_string()]);
    put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "M", "NONE"]);
    put_str(&mut obj, tags::ECHO_PULSE_SEQUENCE, VR::CS, &["SPIN"]);
    put_str(&mut obj, tags::BODY_PART_EXAMINED, VR::CS, &["ABDOMEN"]);
    put_mono16(&mut obj, SIZE, SIZE, 12, pixels);
    write_series(study_dir, &spec, vec![obj])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_seven_series_with_their_images() {
        let dir = std::env::temp_dir().join(format!("summrise-testkit-stacks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let study = write_stacks_study(&dir).expect("fixture");
        let counts: Vec<usize> = study.series.iter().map(|s| s.files.len()).collect();
        assert_eq!(counts, [36, 24, 24, 36, 35, 20, 1]);
        assert!(study.dicomdir.is_none());
        // The marker sits on a pixel centre: column 32, row 17 of the slice at S 50 mm.
        let (col, row) =
            ((MARKER_POSITION[0] - ORIGIN_MM[0]) / PIXEL_MM, (MARKER_POSITION[1] - ORIGIN_MM[1]) / PIXEL_MM);
        assert_eq!((col, row), (32.0, 17.0));
        assert_eq!(slice_height(6), MARKER_POSITION[2]);
        let bytes = fs::read(&study.series[ENHANCED].files[0]).expect("read");
        assert_eq!(&bytes[128..132], b"DICM");
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}
