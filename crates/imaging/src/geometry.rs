//! Patient-space geometry (DICOM PS3.3 C.7.6.2).
//!
//! All coordinates use the DICOM patient coordinate system (LPS):
//! `+X` points to the patient's **L**eft, `+Y` to **P**osterior and `+Z` to
//! **S**uperior (head). Distances are millimetres.

use glam::DVec3;
use serde::{Deserialize, Serialize};
use summrise_i18n::tr;

/// Geometry of a single 2-D image in patient space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImagePlane {
    /// Centre of the first transmitted pixel (Image Position (Patient)).
    pub position: DVec3,
    /// Unit vector along a row, i.e. the direction of increasing column index
    /// (first triplet of Image Orientation (Patient)).
    pub row_dir: DVec3,
    /// Unit vector along a column, i.e. the direction of increasing row index
    /// (second triplet of Image Orientation (Patient)).
    pub col_dir: DVec3,
    /// Distance between the centres of adjacent rows in mm (Pixel Spacing\[0\]).
    pub row_spacing: f64,
    /// Distance between the centres of adjacent columns in mm (Pixel Spacing\[1\]).
    pub col_spacing: f64,
    pub rows: u32,
    pub cols: u32,
}

impl ImagePlane {
    /// Builds a plane from raw DICOM attribute values, normalising the
    /// direction cosines. Returns `None` when the values are degenerate.
    pub fn from_dicom(
        position: [f64; 3],
        orientation: [f64; 6],
        pixel_spacing: [f64; 2],
        rows: u32,
        cols: u32,
    ) -> Option<Self> {
        let row_dir = DVec3::new(orientation[0], orientation[1], orientation[2]).try_normalize()?;
        let col_dir = DVec3::new(orientation[3], orientation[4], orientation[5]).try_normalize()?;
        let position = DVec3::from_array(position);
        let plane = Self {
            position,
            row_dir,
            col_dir,
            row_spacing: pixel_spacing[0],
            col_spacing: pixel_spacing[1],
            rows,
            cols,
        };
        plane.is_valid().then_some(plane)
    }

    fn is_valid(&self) -> bool {
        self.position.is_finite()
            && self.row_spacing.is_finite()
            && self.col_spacing.is_finite()
            && self.row_spacing > 0.0
            && self.col_spacing > 0.0
            // Direction cosines must be (nearly) orthogonal.
            && self.row_dir.dot(self.col_dir).abs() < 1e-2
    }

    /// Unit normal of the image plane (row × column).
    pub fn normal(&self) -> DVec3 {
        self.row_dir.cross(self.col_dir).normalize_or_zero()
    }

    /// Patient position of the pixel centre at (`col`, `row`).
    pub fn pixel_to_patient(&self, col: f64, row: f64) -> DVec3 {
        self.position + self.row_dir * (col * self.col_spacing) + self.col_dir * (row * self.row_spacing)
    }

    /// Whether two planes share orientation and pixel grid (but not
    /// necessarily position).
    pub fn same_grid(&self, other: &Self) -> bool {
        const DIR_TOL: f64 = 1e-3;
        let rel = |a: f64, b: f64| (a - b).abs() <= 1e-3 * a.abs().max(b.abs()).max(1.0);
        self.rows == other.rows
            && self.cols == other.cols
            && (1.0 - self.row_dir.dot(other.row_dir)) < DIR_TOL
            && (1.0 - self.col_dir.dot(other.col_dir)) < DIR_TOL
            && rel(self.row_spacing, other.row_spacing)
            && rel(self.col_spacing, other.col_spacing)
    }

    /// Signed distance of the plane origin along `normal`.
    pub fn distance_along(&self, normal: DVec3) -> f64 {
        self.position.dot(normal)
    }
}

/// The anatomical plane an image (or view) lies in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnatomicalPlane {
    Axial,
    Coronal,
    Sagittal,
    Oblique,
}

impl AnatomicalPlane {
    /// Classifies a plane by its normal. A plane is considered pure when its
    /// normal is within ~37° of a patient axis, otherwise it is oblique.
    pub fn from_normal(normal: DVec3) -> Self {
        let a = normal.abs();
        let (max, plane) = if a.z >= a.x && a.z >= a.y {
            (a.z, Self::Axial)
        } else if a.y >= a.x {
            (a.y, Self::Coronal)
        } else {
            (a.x, Self::Sagittal)
        };
        if max < 0.8 { Self::Oblique } else { plane }
    }

    /// The standard plane a plane with this normal is closest to (never
    /// [`Self::Oblique`]), however far it is tilted.
    pub fn nearest(normal: DVec3) -> Self {
        let a = normal.abs();
        if a.z >= a.x && a.z >= a.y {
            Self::Axial
        } else if a.y >= a.x {
            Self::Coronal
        } else {
            Self::Sagittal
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            // Translators: the names of the image planes (views of the body), used on their own and before
            // an image number ("Axial 12 / 24").
            Self::Axial => tr!("Axial"),
            Self::Coronal => tr!("Coronal"),
            Self::Sagittal => tr!("Sagittal"),
            // Translators: an image plane that is neither axial, coronal nor sagittal.
            Self::Oblique => tr!("Oblique"),
        }
    }
}

impl std::fmt::Display for AnatomicalPlane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Radiological orientation label for a patient-space direction, e.g. `"L"`,
/// `"A"` or `"RS"` for oblique directions (components ordered by magnitude).
pub fn orientation_label(dir: DVec3) -> String {
    const THRESHOLD: f64 = 0.25;
    let mut comps = [(dir.x, 'L', 'R'), (dir.y, 'P', 'A'), (dir.z, 'S', 'I')];
    comps.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
    comps.iter().filter(|(v, ..)| v.abs() >= THRESHOLD).map(|&(v, pos, neg)| if v > 0.0 { pos } else { neg }).collect()
}

/// Labels for the four edges of a displayed image whose screen-right and
/// screen-down directions are given in patient space.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EdgeLabels {
    pub left: String,
    pub right: String,
    pub top: String,
    pub bottom: String,
}

impl EdgeLabels {
    pub fn new(screen_right: DVec3, screen_down: DVec3) -> Self {
        Self {
            left: orientation_label(-screen_right),
            right: orientation_label(screen_right),
            top: orientation_label(-screen_down),
            bottom: orientation_label(screen_down),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_standard_planes() {
        assert_eq!(AnatomicalPlane::from_normal(DVec3::Z), AnatomicalPlane::Axial);
        assert_eq!(AnatomicalPlane::from_normal(-DVec3::Y), AnatomicalPlane::Coronal);
        assert_eq!(AnatomicalPlane::from_normal(DVec3::X), AnatomicalPlane::Sagittal);
        let oblique = DVec3::new(1.0, 1.0, 1.0).normalize();
        assert_eq!(AnatomicalPlane::from_normal(oblique), AnatomicalPlane::Oblique);
        // 20° tilt is still axial.
        let tilted = DVec3::new(0.0, 20f64.to_radians().sin(), 20f64.to_radians().cos());
        assert_eq!(AnatomicalPlane::from_normal(tilted), AnatomicalPlane::Axial);
    }

    #[test]
    fn labels_follow_lps_convention() {
        assert_eq!(orientation_label(DVec3::X), "L");
        assert_eq!(orientation_label(-DVec3::X), "R");
        assert_eq!(orientation_label(DVec3::Y), "P");
        assert_eq!(orientation_label(-DVec3::Y), "A");
        assert_eq!(orientation_label(DVec3::Z), "S");
        assert_eq!(orientation_label(-DVec3::Z), "I");
        assert_eq!(orientation_label(DVec3::new(-0.8, 0.0, 0.6)), "RS");
        assert_eq!(orientation_label(DVec3::ZERO), "");
    }

    #[test]
    fn axial_image_edges() {
        // Standard axial: rows run to patient left, columns to posterior.
        let labels = EdgeLabels::new(DVec3::X, DVec3::Y);
        assert_eq!(labels.left, "R");
        assert_eq!(labels.right, "L");
        assert_eq!(labels.top, "A");
        assert_eq!(labels.bottom, "P");
    }

    #[test]
    fn pixel_to_patient_uses_dicom_spacing_order() {
        // PixelSpacing = [row spacing, column spacing] = [0.5, 0.8].
        let plane = ImagePlane::from_dicom([10.0, 20.0, 30.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [0.5, 0.8], 4, 4)
            .expect("valid plane");
        let p = plane.pixel_to_patient(2.0, 3.0);
        assert!((p - DVec3::new(10.0 + 2.0 * 0.8, 20.0 + 3.0 * 0.5, 30.0)).length() < 1e-9);
        assert_eq!(plane.normal(), DVec3::Z);
    }

    #[test]
    fn rejects_degenerate_orientation() {
        assert!(ImagePlane::from_dicom([0.0; 3], [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], [1.0, 1.0], 2, 2).is_none());
        assert!(ImagePlane::from_dicom([0.0; 3], [0.0; 6], [1.0, 1.0], 2, 2).is_none());
    }
}
