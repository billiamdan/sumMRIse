//! Studies to compare: two MR studies of the same patient on one medium, a
//! year apart, with series whose positions are known exactly.
//!
//! [`write_comparison_studies`] writes a folder scan (no DICOMDIR). The studies
//! are in date order in the index, and series by number:
//!
//! | study | index | series | positions |
//! |---|---|---|---|
//! | [`EARLIER`] ([`EARLIER_DATE`]) | [`AXIAL_5MM`] | axial, slices every 5 mm | z = −60 … +60 mm |
//! | | [`AXIAL_2_5MM`] | axial, slices every 2.5 mm | z = −60 … +60 mm |
//! | | [`AXIAL_SHORT`] | axial, every 5 mm, less coverage | z = −20 … +20 mm |
//! | | [`SAGITTAL`] | sagittal, every 10 mm | x = −40 … +40 mm |
//! | [`LATER`] ([`LATER_DATE`]) | [`LATER_AXIAL`] | axial, every 5 mm | z = −53 … +67 mm |
//! | | [`LATER_SAGITTAL`] | sagittal, every 10 mm, the same description as [`SAGITTAL`] | x = −37 … +43 mm |
//! | [`OTHER_PATIENT`] (option) | 0 | another patient's axial series | z = −60 … +60 mm |
//!
//! Every image has 64 × 64 pixels of [`PIXEL_MM`] mm centred on the patient
//! axis, with the phantom's marker sphere. The series of one study share a
//! Frame of Reference, the studies do not. Axial columns run to the patient's
//! left and rows to the back; sagittal columns run to the back and rows to
//! the feet, so that row r of a sagittal image is at z = 63 − 2 r mm.
//!
//! With [`ComparisonOptions::matching`] off the later study holds a coronal CT
//! instead, which matches nothing in the earlier one.

use std::fs;
use std::path::Path;

use dicom_core::VR;
use dicom_dictionary_std::{tags, uids};

use crate::{
    FixtureError, Geometry, UID_ROOT, WrittenSeries, WrittenStudy, base_object, mr_signal, put_ds, put_geometry,
    put_mono16, put_str, write_file,
};

/// Index of each study in the indexed media, and their Study Dates.
pub const EARLIER: usize = 0;
pub const LATER: usize = 1;
/// Only with [`ComparisonOptions::other_patient`]; its date is the latest.
pub const OTHER_PATIENT: usize = 2;
pub const EARLIER_DATE: &str = "20250302";
pub const LATER_DATE: &str = "20260115";
pub const OTHER_PATIENT_DATE: &str = "20260601";

/// Index of each series in the earlier study.
pub const AXIAL_5MM: usize = 0;
pub const AXIAL_2_5MM: usize = 1;
pub const AXIAL_SHORT: usize = 2;
pub const SAGITTAL: usize = 3;
/// Index of each series in the later study.
pub const LATER_AXIAL: usize = 0;
pub const LATER_SAGITTAL: usize = 1;

/// Series descriptions.
pub const AXIAL_5MM_NAME: &str = "T1 AX 5mm (synthetic)";
pub const AXIAL_2_5MM_NAME: &str = "T2 AX 2.5mm (synthetic)";
pub const AXIAL_SHORT_NAME: &str = "T1 AX SHORT (synthetic)";
pub const SAGITTAL_NAME: &str = "T2 TSE SAG (synthetic)";
pub const CORONAL_CT_NAME: &str = "CT COR (synthetic)";

/// Pixel spacing of every image, in mm, and pixels per side.
pub const PIXEL_MM: f64 = 2.0;
pub const SIZE: u32 = 64;
/// How far the later study lies from the earlier one in its scanner coordinates, in mm.
pub const LATER_SHIFT_MM: [f64; 3] = [3.0, 0.0, 7.0];

/// What [`write_comparison_studies`] writes.
#[derive(Debug, Clone)]
pub struct ComparisonOptions {
    /// The later study holds series that match the earlier one's; otherwise only a coronal CT.
    pub matching: bool,
    /// Also writes a study of another patient.
    pub other_patient: bool,
}

impl Default for ComparisonOptions {
    fn default() -> Self {
        Self { matching: true, other_patient: false }
    }
}

const ROWS_DOWN_THE_BODY: [f64; 6] = [0.0, 1.0, 0.0, 0.0, 0.0, -1.0];
const AXIAL: [f64; 6] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
const CORONAL: [f64; 6] = [1.0, 0.0, 0.0, 0.0, 0.0, -1.0];

/// Half the width of an image between the centres of its outer pixels.
const HALF: f64 = (SIZE as f64 - 1.0) * PIXEL_MM / 2.0;

fn axial(z: f64, shift: [f64; 3]) -> Geometry {
    Geometry { position: [-HALF + shift[0], -HALF + shift[1], z + shift[2]], orientation: AXIAL, spacing: PIXEL_MM }
}

fn sagittal(x: f64, shift: [f64; 3]) -> Geometry {
    Geometry {
        position: [x + shift[0], -HALF + shift[1], HALF + shift[2]],
        orientation: ROWS_DOWN_THE_BODY,
        spacing: PIXEL_MM,
    }
}

fn coronal(y: f64, shift: [f64; 3]) -> Geometry {
    Geometry { position: [-HALF + shift[0], y + shift[1], HALF + shift[2]], orientation: CORONAL, spacing: PIXEL_MM }
}

/// What tells a study apart.
struct StudySpec<'a> {
    folder: &'a str,
    uid: String,
    date: &'a str,
    frame_of_reference: String,
    /// Patient Name and Patient ID instead of the fixture's own.
    patient: Option<(&'a str, &'a str)>,
    shift: [f64; 3],
}

/// One series: its images at `positions` (mm along the slice direction).
struct SeriesSpec<'a> {
    number: u32,
    modality: &'static str,
    description: &'static str,
    positions: Vec<f64>,
    place: &'a dyn Fn(f64, [f64; 3]) -> Geometry,
}

fn every(from: f64, step: f64, count: u32) -> Vec<f64> {
    (0..count).map(|k| from + step * f64::from(k)).collect()
}

/// Writes the studies below `root` (created if needed).
pub fn write_comparison_studies(root: &Path, options: &ComparisonOptions) -> Result<Vec<WrittenStudy>, FixtureError> {
    let study = |n: u32, folder, date, shift, patient| StudySpec {
        folder,
        uid: format!("{UID_ROOT}.70{n}"),
        date,
        frame_of_reference: format!("{UID_ROOT}.71{n}"),
        patient,
        shift,
    };
    let mut written = vec![write_study(
        root,
        &study(1, "ST000001", EARLIER_DATE, [0.0; 3], None),
        &[
            SeriesSpec {
                number: 1,
                modality: "MR",
                description: AXIAL_5MM_NAME,
                positions: every(-60.0, 5.0, 25),
                place: &axial,
            },
            SeriesSpec {
                number: 2,
                modality: "MR",
                description: AXIAL_2_5MM_NAME,
                positions: every(-60.0, 2.5, 49),
                place: &axial,
            },
            SeriesSpec {
                number: 3,
                modality: "MR",
                description: AXIAL_SHORT_NAME,
                positions: every(-20.0, 5.0, 9),
                place: &axial,
            },
            SeriesSpec {
                number: 4,
                modality: "MR",
                description: SAGITTAL_NAME,
                positions: every(-40.0, 10.0, 9),
                place: &sagittal,
            },
        ],
    )?];
    let later = study(2, "ST000002", LATER_DATE, LATER_SHIFT_MM, None);
    written.push(if options.matching {
        write_study(
            root,
            &later,
            &[
                SeriesSpec {
                    number: 1,
                    modality: "MR",
                    description: AXIAL_5MM_NAME,
                    positions: every(-60.0, 5.0, 25),
                    place: &axial,
                },
                SeriesSpec {
                    number: 2,
                    modality: "MR",
                    description: SAGITTAL_NAME,
                    positions: every(-40.0, 10.0, 9),
                    place: &sagittal,
                },
            ],
        )?
    } else {
        let series = SeriesSpec {
            number: 1,
            modality: "CT",
            description: CORONAL_CT_NAME,
            positions: every(-40.0, 10.0, 9),
            place: &coronal,
        };
        write_study(root, &later, &[series])?
    });
    if options.other_patient {
        let other = study(3, "ST000003", OTHER_PATIENT_DATE, [0.0; 3], Some(("OTHER^SYNTHETIC", "SYN-0002")));
        let series = SeriesSpec {
            number: 1,
            modality: "MR",
            description: SAGITTAL_NAME,
            positions: every(-40.0, 10.0, 9),
            place: &sagittal,
        };
        written.push(write_study(root, &other, &[series])?);
    }
    Ok(written)
}

fn write_study(root: &Path, study: &StudySpec<'_>, series: &[SeriesSpec<'_>]) -> Result<WrittenStudy, FixtureError> {
    let study_dir = root.join("DICOM").join(study.folder);
    let written = series.iter().map(|spec| write_series(&study_dir, study, spec)).collect::<Result<Vec<_>, _>>()?;
    Ok(WrittenStudy { root: root.to_path_buf(), study_uid: study.uid.clone(), series: written, dicomdir: None })
}

fn write_series(study_dir: &Path, study: &StudySpec<'_>, spec: &SeriesSpec<'_>) -> Result<WrittenSeries, FixtureError> {
    let uid = format!("{}.{}", study.uid, spec.number);
    let dir = study_dir.join(format!("SE{:06}", spec.number));
    fs::create_dir_all(&dir)?;
    let sop_class = if spec.modality == "CT" { uids::CT_IMAGE_STORAGE } else { uids::MR_IMAGE_STORAGE };
    let mut files = Vec::with_capacity(spec.positions.len());
    for (k, &position) in spec.positions.iter().enumerate() {
        let geometry = (spec.place)(position, study.shift);
        // What the scanner saw is the same body, wherever the study lies in its coordinates.
        let body = |p: [f64; 3]| [p[0] - study.shift[0], p[1] - study.shift[1], p[2] - study.shift[2]];
        let pixels: Vec<u16> =
            (0..SIZE * SIZE).map(|i| mr_signal(body(geometry.pixel(i % SIZE, i / SIZE))).round() as u16).collect();
        let instance = k as u32 + 1;
        let mut obj = base_object(
            &study.uid,
            &uid,
            &format!("{uid}.{instance}"),
            sop_class,
            spec.modality,
            spec.number,
            spec.description,
            instance,
        );
        put_str(&mut obj, tags::STUDY_DATE, VR::DA, &[study.date]);
        put_str(&mut obj, tags::FRAME_OF_REFERENCE_UID, VR::UI, &[&study.frame_of_reference]);
        if let Some((name, id)) = study.patient {
            put_str(&mut obj, tags::PATIENT_NAME, VR::PN, &[name]);
            put_str(&mut obj, tags::PATIENT_ID, VR::LO, &[id]);
        }
        put_geometry(&mut obj, &geometry, SIZE, SIZE);
        put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "M", "ND"]);
        put_ds(&mut obj, tags::SLICE_THICKNESS, &[2.0]);
        put_ds(&mut obj, tags::WINDOW_CENTER, &[700.0]);
        put_ds(&mut obj, tags::WINDOW_WIDTH, &[1400.0]);
        put_mono16(&mut obj, SIZE, SIZE, 12, pixels);
        let path = dir.join(format!("IM{:06}", k + 1));
        write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
        files.push(path);
    }
    Ok(WrittenSeries { uid, number: spec.number, modality: spec.modality, description: spec.description, files })
}
