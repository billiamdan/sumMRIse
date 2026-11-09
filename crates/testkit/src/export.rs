//! Images for the tests of saving, exporting and printing: a study (a folder
//! scan, no DICOMDIR) whose grey values and proportions can be checked in the
//! files that come out.
//!
//! | index | series | what it holds |
//! |---|---|---|
//! | [`LUNG_CT`] | 2 CT images of [`LUNG_SIZE`]² pixels | five vertical bands of [`LUNG_BANDS_HU`]; the stored window is the Lung window, W 1500 / L −600 |
//! | [`NON_SQUARE`] | 1 CT image, 256 columns × 128 rows, Pixel Spacing 1.0 \ 0.5 | a disc of [`DISC_RADIUS_MM`] mm: round only with the spacing applied |
//! | [`SIXTY`] | 60 CT images of 32² pixels | every pixel of image `n` (from 1, by instance number) holds [`sixty_hu`]`(n)`, so that each file tells which image it is |
//! | [`SCANNED`] | 1 secondary capture of a report page | 8-bit greyscale, no geometry |

use std::fs;
use std::path::Path;

use dicom_core::value::PrimitiveValue;
use dicom_core::{DataElement, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;

use crate::{
    FixtureError, Geometry, UID_ROOT, WrittenSeries, WrittenStudy, base_object, put_ds, put_geometry, put_mono16,
    put_str, put_u16, write_file,
};

/// Index of each series in the indexed study.
pub const LUNG_CT: usize = 0;
pub const NON_SQUARE: usize = 1;
pub const SIXTY: usize = 2;
pub const SCANNED: usize = 3;

/// Columns and rows of the images of [`LUNG_CT`].
pub const LUNG_SIZE: u32 = 512;
/// Hounsfield units of the five vertical bands of [`LUNG_CT`], from the left:
/// below the Lung window, its black end, its centre, its white end, above it.
pub const LUNG_BANDS_HU: [f64; 5] = [-1500.0, -1350.0, -600.0, 150.0, 900.0];
/// Field of view of [`LUNG_CT`] in millimetres.
pub const LUNG_FOV_MM: f64 = 240.0;
/// Radius of the disc of [`NON_SQUARE`].
pub const DISC_RADIUS_MM: f64 = 50.0;

/// The value of every pixel of image `n` (from 1) of [`SIXTY`].
pub fn sixty_hu(n: u32) -> f64 {
    -1000.0 + 30.0 * f64::from(n)
}

/// The column of [`LUNG_CT`] in the middle of band `band`.
pub fn lung_band_column(band: usize) -> usize {
    (2 * band + 1) * LUNG_SIZE as usize / (2 * LUNG_BANDS_HU.len())
}

fn axial(position: [f64; 3], spacing: f64) -> Geometry {
    Geometry { position, orientation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], spacing }
}

/// Writes the study below `root` (created if needed).
pub fn write_export_study(root: &Path) -> Result<WrittenStudy, FixtureError> {
    let study_uid = format!("{UID_ROOT}61");
    let study_dir = root.join("EXPORT");
    let series = vec![
        write_lung(&study_dir, &study_uid)?,
        write_non_square(&study_dir, &study_uid)?,
        write_sixty(&study_dir, &study_uid)?,
        write_scanned(&study_dir, &study_uid)?,
    ];
    Ok(WrittenStudy { root: root.to_path_buf(), study_uid, series, dicomdir: None })
}

/// Hounsfield units are stored with this intercept: values down to -2048 HU.
const INTERCEPT: f64 = -2048.0;

fn put_hu(obj: &mut InMemDicomObject, center: f64, width: f64, name: &str) {
    put_ds(obj, tags::RESCALE_INTERCEPT, &[INTERCEPT]);
    put_ds(obj, tags::RESCALE_SLOPE, &[1.0]);
    put_str(obj, tags::RESCALE_TYPE, VR::LO, &["HU"]);
    put_ds(obj, tags::WINDOW_CENTER, &[center]);
    put_ds(obj, tags::WINDOW_WIDTH, &[width]);
    put_str(obj, tags::WINDOW_CENTER_WIDTH_EXPLANATION, VR::LO, &[name]);
}

fn write_lung(study_dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let number = 61;
    let uid = format!("{UID_ROOT}{number}");
    let dir = study_dir.join(format!("SE{number:06}"));
    fs::create_dir_all(&dir)?;
    let size = LUNG_SIZE;
    let pixels: Vec<u16> = (0..size * size)
        .map(|i| {
            let band = ((i % size) as usize * LUNG_BANDS_HU.len()) / size as usize;
            (LUNG_BANDS_HU[band] - INTERCEPT) as u16
        })
        .collect();
    let description = "LUNG BANDS (synthetic)";
    let mut files = Vec::new();
    for k in 0..2u32 {
        let instance = k + 1;
        let mut obj = base_object(
            study_uid,
            &uid,
            &format!("{uid}.{instance}"),
            uids::CT_IMAGE_STORAGE,
            "CT",
            number,
            description,
            instance,
        );
        let geom = axial([-120.0, -120.0, f64::from(k) * 5.0], LUNG_FOV_MM / f64::from(size));
        put_geometry(&mut obj, &geom, size, size);
        put_hu(&mut obj, -600.0, 1500.0, "LUNG");
        put_mono16(&mut obj, size, size, 12, pixels.clone());
        let path = dir.join(format!("IM{instance:06}"));
        write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
        files.push(path);
    }
    Ok(WrittenSeries { uid, number, modality: "CT", description, files })
}

fn write_non_square(study_dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let number = 62;
    let uid = format!("{UID_ROOT}{number}");
    let dir = study_dir.join(format!("SE{number:06}"));
    fs::create_dir_all(&dir)?;
    let (cols, rows) = (256u32, 128u32);
    let (row_mm, col_mm) = (1.0f64, 0.5f64);
    let pixels: Vec<u16> = (0..rows)
        .flat_map(|y| {
            (0..cols).map(move |x| {
                let dx = (f64::from(x) + 0.5 - f64::from(cols) / 2.0) * col_mm;
                let dy = (f64::from(y) + 0.5 - f64::from(rows) / 2.0) * row_mm;
                let hu = if dx.hypot(dy) <= DISC_RADIUS_MM { 500.0 } else { -500.0 };
                (hu - INTERCEPT) as u16
            })
        })
        .collect();
    let description = "NON-SQUARE PIXELS (synthetic)";
    let mut obj =
        base_object(study_uid, &uid, &format!("{uid}.1"), uids::CT_IMAGE_STORAGE, "CT", number, description, 1);
    put_geometry(&mut obj, &axial([-64.0, -64.0, 0.0], col_mm), rows, cols);
    put_ds(&mut obj, tags::PIXEL_SPACING, &[row_mm, col_mm]);
    put_hu(&mut obj, 0.0, 1000.0, "FULL");
    put_mono16(&mut obj, rows, cols, 12, pixels);
    let path = dir.join("IM000001");
    write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
    Ok(WrittenSeries { uid, number, modality: "CT", description, files: vec![path] })
}

fn write_sixty(study_dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let number = 63;
    let uid = format!("{UID_ROOT}{number}");
    let dir = study_dir.join(format!("SE{number:06}"));
    fs::create_dir_all(&dir)?;
    let size = 32u32;
    let description = "SIXTY SLICES (synthetic)";
    let mut files = Vec::new();
    // Written in a scrambled order: the display order comes from the positions.
    let mut order: Vec<u32> = (1..=60).collect();
    order.sort_by_key(|n| (n * 37) % 61);
    for (file, instance) in order.into_iter().enumerate() {
        let mut obj = base_object(
            study_uid,
            &uid,
            &format!("{uid}.{instance}"),
            uids::CT_IMAGE_STORAGE,
            "CT",
            number,
            description,
            instance,
        );
        // Instance 1 is the most superior, as in the phantom.
        let geom = axial([-60.0, -60.0, f64::from(60 - instance) * 2.5], 120.0 / f64::from(size));
        put_geometry(&mut obj, &geom, size, size);
        put_hu(&mut obj, 0.0, 2000.0, "WIDE");
        put_mono16(&mut obj, size, size, 12, vec![(sixty_hu(instance) - INTERCEPT) as u16; (size * size) as usize]);
        let path = dir.join(format!("IM{:06}", file + 1));
        write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
        files.push(path);
    }
    Ok(WrittenSeries { uid, number, modality: "CT", description, files })
}

fn write_scanned(study_dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let number = 64;
    let uid = format!("{UID_ROOT}{number}");
    let dir = study_dir.join(format!("SE{number:06}"));
    fs::create_dir_all(&dir)?;
    let (cols, rows) = (64u16, 48u16);
    let description = "Scanned report (synthetic)";
    let mut obj = base_object(
        study_uid,
        &uid,
        &format!("{uid}.1"),
        uids::SECONDARY_CAPTURE_IMAGE_STORAGE,
        "OT",
        number,
        description,
        1,
    );
    put_u16(&mut obj, tags::SAMPLES_PER_PIXEL, 1);
    put_str(&mut obj, tags::PHOTOMETRIC_INTERPRETATION, VR::CS, &["MONOCHROME2"]);
    put_u16(&mut obj, tags::ROWS, rows);
    put_u16(&mut obj, tags::COLUMNS, cols);
    put_u16(&mut obj, tags::BITS_ALLOCATED, 8);
    put_u16(&mut obj, tags::BITS_STORED, 8);
    put_u16(&mut obj, tags::HIGH_BIT, 7);
    put_u16(&mut obj, tags::PIXEL_REPRESENTATION, 0);
    let pixels: Vec<u8> = (0..usize::from(rows) * usize::from(cols)).map(|i| (i % 200) as u8 + 40).collect();
    obj.put(DataElement::new(tags::PIXEL_DATA, VR::OB, PrimitiveValue::U8(pixels.into())));
    let path = dir.join("IM000001");
    write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
    Ok(WrittenSeries { uid, number, modality: "OT", description, files: vec![path] })
}
