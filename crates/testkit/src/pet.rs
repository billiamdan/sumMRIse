//! A PET/CT study: the phantom CT and, in its Frame of Reference, PET series
//! whose uptake values are known exactly.
//!
//! [`write_pet_study`] writes one study (a folder scan, no DICOMDIR). In index
//! order (series are sorted by number):
//!
//! | index | series | images |
//! |---|---|---|
//! | [`CT`] | the phantom CT of [`crate::write_phantom_study`] | its sizes from [`PetStudyOptions::ct`]; the 300 HU marker at [`crate::MARKER_POSITION`] |
//! | [`PET`] | PET, attenuation corrected | [`SIZE`] × [`SIZE`] × [`SLICES`] voxels of [`VOXEL_MM`] mm, the hot sphere of [`MARKER_SUV`] at the marker |
//! | [`PET_NAC`] | the same activity, not attenuation corrected | Corrected Image without ATTN: SUV is refused |
//! | [`PET_ELSEWHERE`] | (option) the PET in another Frame of Reference | must not be offered over the CT |
//!
//! The PET records Units BQML, Corrected Image DECY\ATTN\SCAT\DTIM\RAN\NORM,
//! Decay Correction START, Patient Weight [`WEIGHT_KG`], and in the
//! Radiopharmaceutical Information Sequence F-18 FDG with
//! [`INJECTED_DOSE_BQ`], half-life [`HALF_LIFE_S`] and the injection at
//! [`INJECTION_TIME`], one half-life before the series time
//! [`SERIES_TIME`]. Half of the dose is left at the series time, so one SUV
//! is 185 MBq / 70 000 g = [`BQML_PER_SUV`] = 2642.857 Bq/ml, and the hot
//! sphere's 13 214.3 Bq/ml is SUV 5.0. Every slice has its own Rescale Slope
//! (its largest concentration / [`STORED_MAX`]), as PET files do.
//!
//! The PET grid starts at R 118, A 118, I 98 mm ([`ORIGIN_MM`]), so it is
//! offset from the CT grid and the marker lies between PET voxel centres in
//! the two in-plane directions.

use std::fs;
use std::path::Path;

use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, Length, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;

use crate::{
    FixtureError, Geometry, PhantomOptions, UID_ROOT, WrittenSeries, WrittenStudy, base_object, in_ellipsoid,
    in_marker, noise, put_ds, put_geometry, put_mono16, put_str, write_ct_series, write_file,
};

/// Index of each series in the indexed study.
pub const CT: usize = 0;
pub const PET: usize = 1;
pub const PET_NAC: usize = 2;
/// Only with [`PetStudyOptions::other_frame_of_reference`].
pub const PET_ELSEWHERE: usize = 3;

/// Voxels per side of every PET slice, and the number of slices.
pub const SIZE: u32 = 60;
pub const SLICES: u32 = 50;
/// Pixel spacing and slice spacing of the PET, in mm.
pub const VOXEL_MM: f64 = 4.0;
/// Patient position (LPS, mm) of the centre of the first PET voxel.
pub const ORIGIN_MM: [f64; 3] = [-118.0, -118.0, -98.0];
/// Slice left out with [`PetStudyOptions::missing_slice`] (from 0, inferior first).
pub const MISSING_SLICE: u32 = 25;

/// Patient Weight (0010,1030).
pub const WEIGHT_KG: f64 = 70.0;
/// Radionuclide Total Dose (0018,1074): 370 MBq.
pub const INJECTED_DOSE_BQ: f64 = 370e6;
/// Radionuclide Half Life (0018,1075) of F-18, in s.
pub const HALF_LIFE_S: f64 = 6586.2;
/// Series Date and Time, also the acquisition time of every image.
pub const SERIES_DATE: &str = "20260115";
pub const SERIES_TIME: &str = "101500";
/// Radiopharmaceutical Start Time: 10:15:00 − 6586.2 s = 08:25:13.8.
pub const INJECTION_TIME: &str = "082513.8";
/// Bq/ml for SUV 1: the dose left at the series time (half the injected
/// dose) per gram of body weight.
pub const BQML_PER_SUV: f64 = INJECTED_DOSE_BQ / 2.0 / (WEIGHT_KG * 1000.0);

/// Uptake of the hot sphere (at the CT marker, radius [`crate::MARKER_RADIUS_MM`]).
pub const MARKER_SUV: f64 = 5.0;
/// Uptake of the body (with ±5 % noise) and of the lungs; 0 outside the body.
pub const BODY_SUV: f64 = 1.0;
pub const LUNG_SUV: f64 = 0.4;
/// Stored value of the largest concentration of every slice.
pub const STORED_MAX: f64 = 30_000.0;

/// What [`write_pet_study`] writes.
#[derive(Debug, Clone)]
pub struct PetStudyOptions {
    /// Sizes of the CT (only `ct_size` and `ct_slices` are used).
    pub ct: PhantomOptions,
    /// Also writes the PET in another Frame of Reference ([`PET_ELSEWHERE`]).
    pub other_frame_of_reference: bool,
    /// Leaves out slice [`MISSING_SLICE`] of the PET: its images no longer
    /// form a volume.
    pub missing_slice: bool,
    /// Writes the PET without Patient Weight: SUV is refused.
    pub without_weight: bool,
}

impl Default for PetStudyOptions {
    fn default() -> Self {
        Self {
            ct: PhantomOptions::small(),
            other_frame_of_reference: false,
            missing_slice: false,
            without_weight: false,
        }
    }
}

/// Uptake (SUV) of the phantom at patient position `p`.
pub fn suv_at(p: [f64; 3]) -> f64 {
    let [x, y, _] = p;
    if in_marker(p) {
        return MARKER_SUV;
    }
    if (x / 100.0).powi(2) + (y / 80.0).powi(2) > 1.0 {
        return 0.0;
    }
    if [-1.0, 1.0].iter().any(|side| in_ellipsoid(p, [side * 45.0, -5.0, 0.0], [30.0, 45.0, 70.0])) {
        return LUNG_SUV;
    }
    BODY_SUV * (1.0 + 0.05 * noise(p))
}

/// Height (S, mm) of PET slice `k` (from 0).
pub fn slice_height(k: u32) -> f64 {
    ORIGIN_MM[2] + f64::from(k) * VOXEL_MM
}

/// A value as [`put_ds`] writes it, so that stored values are computed with
/// the slope a reader finds.
fn as_written(v: f64) -> f64 {
    let mut s = format!("{v:.6}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    s.truncate(16);
    s.parse().unwrap_or(v)
}

/// One PET series.
struct Spec {
    number: u32,
    description: &'static str,
    corrected: &'static [&'static str],
    frame_of_reference: Option<String>,
}

/// Writes the study below `root` (created if needed).
pub fn write_pet_study(root: &Path, opts: &PetStudyOptions) -> Result<WrittenStudy, FixtureError> {
    let study_uid = format!("{UID_ROOT}.6");
    let study_dir = root.join("DICOM").join("ST000001");
    let mut series = vec![write_ct_series(&study_dir, &study_uid, &opts.ct)?];
    let attenuation_corrected: &[&str] = &["DECY", "ATTN", "SCAT", "DTIM", "RAN", "NORM"];
    let mut specs = vec![
        Spec {
            number: 5,
            description: "PET WB FDG AC (synthetic)",
            corrected: attenuation_corrected,
            frame_of_reference: None,
        },
        Spec {
            number: 6,
            description: "PET WB FDG NAC (synthetic)",
            corrected: &["DECY", "DTIM", "RAN", "NORM"],
            frame_of_reference: None,
        },
    ];
    if opts.other_frame_of_reference {
        specs.push(Spec {
            number: 7,
            description: "PET other position (synthetic)",
            corrected: attenuation_corrected,
            frame_of_reference: Some(format!("{UID_ROOT}8888")),
        });
    }
    for spec in &specs {
        series.push(write_pet_series(&study_dir, &study_uid, spec, opts)?);
    }
    Ok(WrittenStudy { root: root.to_path_buf(), study_uid, series, dicomdir: None })
}

fn write_pet_series(
    study_dir: &Path,
    study_uid: &str,
    spec: &Spec,
    opts: &PetStudyOptions,
) -> Result<WrittenSeries, FixtureError> {
    let uid = format!("{UID_ROOT}.6.{}", spec.number);
    let dir = study_dir.join(format!("SE{:06}", spec.number));
    fs::create_dir_all(&dir)?;
    let mut files = Vec::new();
    for k in (0..SLICES).filter(|&k| !(opts.missing_slice && k == MISSING_SLICE)) {
        let geom = Geometry {
            position: [ORIGIN_MM[0], ORIGIN_MM[1], slice_height(k)],
            orientation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            spacing: VOXEL_MM,
        };
        let activity: Vec<f64> =
            (0..SIZE * SIZE).map(|i| suv_at(geom.pixel(i % SIZE, i / SIZE)) * BQML_PER_SUV).collect();
        let largest = activity.iter().copied().fold(0.0, f64::max);
        let slope = if largest > 0.0 { as_written(largest / STORED_MAX) } else { 1.0 };
        let pixels: Vec<u16> = activity.iter().map(|a| (a / slope).round().clamp(0.0, 65_535.0) as u16).collect();
        let instance_number = k + 1;
        let mut obj = base_object(
            study_uid,
            &uid,
            &format!("{uid}.{instance_number}"),
            uids::POSITRON_EMISSION_TOMOGRAPHY_IMAGE_STORAGE,
            "PT",
            spec.number,
            spec.description,
            instance_number,
        );
        if let Some(frame_of_reference) = &spec.frame_of_reference {
            put_str(&mut obj, tags::FRAME_OF_REFERENCE_UID, VR::UI, &[frame_of_reference]);
        }
        put_geometry(&mut obj, &geom, SIZE, SIZE);
        put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY"]);
        put_ds(&mut obj, tags::SLICE_THICKNESS, &[VOXEL_MM]);
        put_ds(&mut obj, tags::SLICE_LOCATION, &[slice_height(k)]);
        put_str(&mut obj, tags::SERIES_DATE, VR::DA, &[SERIES_DATE]);
        put_str(&mut obj, tags::SERIES_TIME, VR::TM, &[SERIES_TIME]);
        put_str(&mut obj, tags::ACQUISITION_DATE, VR::DA, &[SERIES_DATE]);
        put_str(&mut obj, tags::ACQUISITION_TIME, VR::TM, &[SERIES_TIME]);
        put_str(&mut obj, tags::BODY_PART_EXAMINED, VR::CS, &["CHEST"]);
        put_str(&mut obj, tags::UNITS, VR::CS, &["BQML"]);
        put_str(&mut obj, tags::CORRECTED_IMAGE, VR::CS, spec.corrected);
        put_str(&mut obj, tags::DECAY_CORRECTION, VR::CS, &["START"]);
        if !opts.without_weight {
            put_ds(&mut obj, tags::PATIENT_WEIGHT, &[WEIGHT_KG]);
        }
        obj.put(DataElement::new(
            tags::RADIOPHARMACEUTICAL_INFORMATION_SEQUENCE,
            VR::SQ,
            DataSetSequence::new(vec![radiopharmaceutical()], Length::UNDEFINED),
        ));
        put_ds(&mut obj, tags::RESCALE_INTERCEPT, &[0.0]);
        put_ds(&mut obj, tags::RESCALE_SLOPE, &[slope]);
        put_mono16(&mut obj, SIZE, SIZE, 16, pixels);
        let path = dir.join(format!("IM{:06}", k + 1));
        write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
        files.push(path);
    }
    Ok(WrittenSeries { uid, number: spec.number, modality: "PT", description: spec.description, files })
}

/// The item of the Radiopharmaceutical Information Sequence: F-18 FDG.
fn radiopharmaceutical() -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    put_str(&mut item, tags::RADIOPHARMACEUTICAL, VR::LO, &["Fluorodeoxyglucose"]);
    put_str(&mut item, tags::RADIOPHARMACEUTICAL_START_TIME, VR::TM, &[INJECTION_TIME]);
    put_ds(&mut item, tags::RADIONUCLIDE_TOTAL_DOSE, &[INJECTED_DOSE_BQ]);
    put_ds(&mut item, tags::RADIONUCLIDE_HALF_LIFE, &[HALF_LIFE_S]);
    put_ds(&mut item, tags::RADIONUCLIDE_POSITRON_FRACTION, &[0.9686]);
    let mut code = InMemDicomObject::new_empty();
    put_str(&mut code, tags::CODE_VALUE, VR::SH, &["C-111A1"]);
    put_str(&mut code, tags::CODING_SCHEME_DESIGNATOR, VR::SH, &["SRT"]);
    put_str(&mut code, tags::CODE_MEANING, VR::LO, &["^18^Fluorine"]);
    item.put(DataElement::new(
        tags::RADIONUCLIDE_CODE_SEQUENCE,
        VR::SQ,
        DataSetSequence::new(vec![code], Length::UNDEFINED),
    ));
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MARKER_POSITION;

    #[test]
    fn the_numbers_give_a_round_suv() {
        // 185 MBq left at the series time, 70 000 g: 2642.857 Bq/ml per SUV.
        assert!((BQML_PER_SUV - 2_642.857_142_857).abs() < 1e-6);
        assert!((MARKER_SUV * BQML_PER_SUV - 13_214.285_714).abs() < 1e-3, "13 214.3 Bq/ml is SUV 5");
        // The injection is one half-life before the series time.
        let seconds = |t: &str| -> f64 {
            let (h, m, s) = (&t[0..2], &t[2..4], &t[4..]);
            h.parse::<f64>().unwrap_or(0.0) * 3600.0
                + m.parse::<f64>().unwrap_or(0.0) * 60.0
                + s.parse::<f64>().unwrap_or(0.0)
        };
        assert!((seconds(SERIES_TIME) - seconds(INJECTION_TIME) - HALF_LIFE_S).abs() < 1e-9);
        assert_eq!(suv_at(MARKER_POSITION), MARKER_SUV);
        assert_eq!(suv_at([0.0, 150.0, 0.0]), 0.0, "outside the body");
        assert_eq!(suv_at([45.0, -5.0, -30.0]), LUNG_SUV);
        assert_eq!(as_written(13_214.285_714 / STORED_MAX), 0.440476);
    }

    #[test]
    fn writes_the_ct_and_the_pet_series() {
        let dir = tempfile_dir("pet");
        let options =
            PetStudyOptions { other_frame_of_reference: true, missing_slice: true, ..PetStudyOptions::default() };
        let study = write_pet_study(&dir, &options).expect("fixture");
        let counts: Vec<(&str, usize)> = study.series.iter().map(|s| (s.modality, s.files.len())).collect();
        assert_eq!(counts, [("CT", 24), ("PT", 49), ("PT", 49), ("PT", 49)]);
        assert!(study.series.iter().all(|s| s.files.iter().all(|f| f.exists())));
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// A fresh folder in the temporary directory (the testkit has no
    /// dependency on `tempfile`).
    fn tempfile_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("summrise-testkit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }
}
