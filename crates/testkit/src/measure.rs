//! Images of known size and known values for testing measurements.
//!
//! [`write_measure_study`] writes one study (a folder scan, no DICOMDIR) of
//! four series. In index order (series are sorted by number):
//!
//! | index | series | what it holds |
//! |---|---|---|
//! | [`NON_SQUARE`] | 5 axial images, 200 × 200 | Pixel Spacing 1.0 \ 0.5: rows 1.0 mm apart, columns 0.5 mm apart; value [`NON_SQUARE_VALUE`] |
//! | [`SQUARE`] | 1 axial image, 64 × 64 | Pixel Spacing 0.5 \ 0.5; value `10 × column + row` ([`square_value`]) |
//! | [`PROJECTION`] | 1 radiograph, 100 × 100 | Imager Pixel Spacing 0.25 \ 0.25 only, no position; value `column` |
//! | [`NO_POSITION`] | 1 image, 100 × 100 | Pixel Spacing 0.5 \ 0.25, no position; value `row` |

use std::fs;
use std::path::Path;

use dicom_core::value::PrimitiveValue;
use dicom_core::{DataElement, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;

use crate::{
    FixtureError, Geometry, UID_ROOT, WrittenSeries, WrittenStudy, base_object, put_ds, put_geometry, put_str, put_u16,
    write_file,
};

/// Index of each series in the indexed study.
pub const NON_SQUARE: usize = 0;
pub const SQUARE: usize = 1;
pub const PROJECTION: usize = 2;
pub const NO_POSITION: usize = 3;

/// The value of every pixel of the series [`NON_SQUARE`].
pub const NON_SQUARE_VALUE: u16 = 100;
/// Height of the series [`NON_SQUARE`]: the first image at 0 mm, 3 mm apart.
pub const NON_SQUARE_STEP_MM: f64 = 3.0;

/// The value at column `x`, row `y` of the series [`SQUARE`].
pub fn square_value(x: u32, y: u32) -> u16 {
    (10 * x + y) as u16
}

struct Spec {
    number: u32,
    modality: &'static str,
    description: &'static str,
    sop_class: &'static str,
}

impl Spec {
    fn uid(&self) -> String {
        format!("{UID_ROOT}.8.{}", self.number)
    }

    fn object(&self, study_uid: &str, instance: u32) -> InMemDicomObject {
        let uid = self.uid();
        base_object(
            study_uid,
            &uid,
            &format!("{uid}.{instance}"),
            self.sop_class,
            self.modality,
            self.number,
            self.description,
            instance,
        )
    }

    fn write(&self, dir: &Path, images: Vec<InMemDicomObject>) -> Result<WrittenSeries, FixtureError> {
        let dir = dir.join(format!("SE{:06}", self.number));
        fs::create_dir_all(&dir)?;
        let mut files = Vec::new();
        for (k, obj) in images.into_iter().enumerate() {
            let path = dir.join(format!("IM{:06}", k + 1));
            write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
            files.push(path);
        }
        Ok(WrittenSeries {
            uid: self.uid(),
            number: self.number,
            modality: self.modality,
            description: self.description,
            files,
        })
    }
}

/// 16-bit grey pixels with the value `value(column, row)`.
fn put_grey(obj: &mut InMemDicomObject, cols: u32, rows: u32, value: impl Fn(u32, u32) -> u16) {
    put_u16(obj, tags::SAMPLES_PER_PIXEL, 1);
    put_str(obj, tags::PHOTOMETRIC_INTERPRETATION, VR::CS, &["MONOCHROME2"]);
    put_u16(obj, tags::ROWS, rows as u16);
    put_u16(obj, tags::COLUMNS, cols as u16);
    put_u16(obj, tags::BITS_ALLOCATED, 16);
    put_u16(obj, tags::BITS_STORED, 16);
    put_u16(obj, tags::HIGH_BIT, 15);
    put_u16(obj, tags::PIXEL_REPRESENTATION, 0);
    let pixels: Vec<u16> = (0..rows).flat_map(|y| (0..cols).map(move |x| (x, y))).map(|(x, y)| value(x, y)).collect();
    obj.put(DataElement::new(tags::PIXEL_DATA, VR::OW, PrimitiveValue::U16(pixels.into())));
}

fn axial(position: [f64; 3], spacing: f64) -> Geometry {
    Geometry { position, orientation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], spacing }
}

fn non_square(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 1,
        modality: "CT",
        description: "NON-SQUARE PIXELS (synthetic)",
        sop_class: uids::CT_IMAGE_STORAGE,
    };
    let n = 200;
    let mut images = Vec::new();
    for k in 0..5u32 {
        let mut obj = spec.object(study_uid, k + 1);
        let z = f64::from(k) * NON_SQUARE_STEP_MM;
        put_geometry(&mut obj, &axial([-50.0, -100.0, z], 1.0), n, n);
        put_ds(&mut obj, tags::PIXEL_SPACING, &[1.0, 0.5]);
        put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "AXIAL"]);
        put_grey(&mut obj, n, n, |_, _| NON_SQUARE_VALUE);
        images.push(obj);
    }
    spec.write(dir, images)
}

fn square(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec =
        Spec { number: 2, modality: "CT", description: "SQUARE PIXELS (synthetic)", sop_class: uids::CT_IMAGE_STORAGE };
    let n = 64;
    let mut obj = spec.object(study_uid, 1);
    put_geometry(&mut obj, &axial([0.0, 0.0, 0.0], 0.5), n, n);
    put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "AXIAL"]);
    put_grey(&mut obj, n, n, square_value);
    spec.write(dir, vec![obj])
}

fn projection(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 3,
        modality: "DX",
        description: "RADIOGRAPH (synthetic)",
        sop_class: uids::DIGITAL_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
    };
    let n = 100;
    let mut obj = spec.object(study_uid, 1);
    put_ds(&mut obj, tags::IMAGER_PIXEL_SPACING, &[0.25, 0.25]);
    put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY"]);
    put_grey(&mut obj, n, n, |x, _| x as u16);
    spec.write(dir, vec![obj])
}

fn no_position(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 4,
        modality: "OT",
        description: "SPACING WITHOUT POSITION (synthetic)",
        sop_class: uids::SECONDARY_CAPTURE_IMAGE_STORAGE,
    };
    let n = 100;
    let mut obj = spec.object(study_uid, 1);
    put_ds(&mut obj, tags::PIXEL_SPACING, &[0.5, 0.25]);
    put_grey(&mut obj, n, n, |_, y| y as u16);
    spec.write(dir, vec![obj])
}

/// Writes the study (see the module documentation) below `root`.
pub fn write_measure_study(root: &Path) -> Result<WrittenStudy, FixtureError> {
    let study_uid = format!("{UID_ROOT}.8");
    let dir = root.join("DICOM").join("ST000008");
    let series = vec![
        non_square(&dir, &study_uid)?,
        square(&dir, &study_uid)?,
        projection(&dir, &study_uid)?,
        no_position(&dir, &study_uid)?,
    ];
    Ok(WrittenStudy { root: root.to_path_buf(), study_uid, series, dicomdir: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_four_series_of_known_size() {
        let dir = std::env::temp_dir().join(format!("summrise-testkit-measure-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let study = write_measure_study(&dir).expect("fixture");
        let counts: Vec<usize> = study.series.iter().map(|s| s.files.len()).collect();
        assert_eq!(counts, [5, 1, 1, 1]);
        assert_eq!(square_value(3, 2), 32);
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}
