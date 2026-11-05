//! Multiplanar reformatting (MPR) and thick-slab projections (MIP/MinIP/mean).
//!
//! Views are resampled on canonical patient axes so that every volume —
//! whatever plane it was acquired in, including gantry-tilted CT — is shown
//! with radiological conventions: patient right on screen left, anterior or
//! superior at the top.

use glam::DVec3;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::geometry::{AnatomicalPlane, EdgeLabels};
use crate::volume::{Volume, VolumeGeometry};

/// One of the three orthogonal reformatting planes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MprAxis {
    Axial,
    Coronal,
    Sagittal,
}

impl MprAxis {
    pub const ALL: [Self; 3] = [Self::Axial, Self::Coronal, Self::Sagittal];

    /// (screen-right, screen-down, viewing normal) in patient space.
    /// `normal == right × down` for every axis.
    pub fn basis(self) -> (DVec3, DVec3, DVec3) {
        match self {
            Self::Axial => (DVec3::X, DVec3::Y, DVec3::Z),
            Self::Coronal => (DVec3::X, -DVec3::Z, DVec3::Y),
            Self::Sagittal => (DVec3::Y, -DVec3::Z, -DVec3::X),
        }
    }

    pub fn normal(self) -> DVec3 {
        self.basis().2
    }

    pub fn plane(self) -> AnatomicalPlane {
        match self {
            Self::Axial => AnatomicalPlane::Axial,
            Self::Coronal => AnatomicalPlane::Coronal,
            Self::Sagittal => AnatomicalPlane::Sagittal,
        }
    }

    pub fn label(self) -> &'static str {
        self.plane().label()
    }

    pub fn edge_labels(self) -> EdgeLabels {
        let (right, down, _) = self.basis();
        EdgeLabels::new(right, down)
    }

    /// Which views the vertical and horizontal crosshair lines represent.
    pub fn crosshair_partners(self) -> (Self, Self) {
        match self {
            Self::Axial => (Self::Sagittal, Self::Coronal),
            Self::Coronal => (Self::Sagittal, Self::Axial),
            Self::Sagittal => (Self::Coronal, Self::Axial),
        }
    }
}

/// Projection of a volume's bounding box onto a view's axes (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MprExtent {
    pub u: (f64, f64),
    pub v: (f64, f64),
    pub n: (f64, f64),
    /// Natural step between slices along the normal (mm).
    pub step: f64,
}

impl MprExtent {
    pub fn of(geometry: &VolumeGeometry, axis: MprAxis) -> Self {
        let (right, down, normal) = axis.basis();
        let range = |dir: DVec3| {
            geometry.corners().iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
                let d = c.dot(dir);
                (lo.min(d), hi.max(d))
            })
        };
        // Step along the normal: projection of the voxel axis most aligned with it.
        let step = geometry.axes.iter().map(|a| a.dot(normal).abs()).fold(0.0, f64::max).max(1e-3);
        Self { u: range(right), v: range(down), n: range(normal), step }
    }

    /// Number of distinct slice positions along the normal.
    pub fn slice_count(&self) -> usize {
        ((self.n.1 - self.n.0) / self.step + 1e-6).floor() as usize + 1
    }

    /// Position (mm along the normal) of slice `index`.
    pub fn position(&self, index: usize) -> f64 {
        self.n.0 + index as f64 * self.step
    }

    /// Nearest slice index for a position along the normal.
    pub fn index_of(&self, position: f64) -> usize {
        let i = ((position - self.n.0) / self.step).round();
        (i.max(0.0) as usize).min(self.slice_count() - 1)
    }
}

/// How samples are combined across a thick slab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SlabMode {
    /// Maximum intensity projection.
    Mip,
    /// Minimum intensity projection.
    MinIp,
    /// Average intensity projection.
    Mean,
}

impl SlabMode {
    pub const ALL: [Self; 3] = [Self::Mip, Self::MinIp, Self::Mean];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mip => "MIP",
            Self::MinIp => "MinIP",
            Self::Mean => "Average",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Slab {
    pub mode: SlabMode,
    pub thickness_mm: f64,
}

/// A resampled 2-D view. Values outside the volume are `NaN`.
#[derive(Debug, Clone)]
pub struct MprImage {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
    /// Isotropic pixel size in mm.
    pub spacing: f64,
    /// Patient position of pixel (0, 0).
    pub origin: DVec3,
    pub right: DVec3,
    pub down: DVec3,
}

impl MprImage {
    pub fn pixel_to_patient(&self, x: f64, y: f64) -> DVec3 {
        self.origin + self.right * (x * self.spacing) + self.down * (y * self.spacing)
    }

    pub fn patient_to_pixel(&self, p: DVec3) -> (f64, f64) {
        let d = p - self.origin;
        (d.dot(self.right) / self.spacing, d.dot(self.down) / self.spacing)
    }

    pub fn value_at(&self, x: usize, y: usize) -> Option<f32> {
        (x < self.width && y < self.height).then(|| self.values[y * self.width + x]).filter(|v| v.is_finite())
    }
}

/// Resamples `volume` on the plane of `axis` located `position` mm along the
/// view normal. Output is limited to `max_dim` pixels per side.
pub fn reslice(volume: &Volume, axis: MprAxis, position: f64, slab: Option<Slab>, max_dim: usize) -> MprImage {
    let g = &volume.geometry;
    let (right, down, normal) = axis.basis();
    let ext = MprExtent::of(g, axis);
    let max_dim = max_dim.max(16) as f64;
    let spacing = g.min_spacing().max((ext.u.1 - ext.u.0) / (max_dim - 1.0)).max((ext.v.1 - ext.v.0) / (max_dim - 1.0));
    let width = ((ext.u.1 - ext.u.0) / spacing).floor() as usize + 1;
    let height = ((ext.v.1 - ext.v.0) / spacing).floor() as usize + 1;
    let origin = right * ext.u.0 + down * ext.v.0 + normal * position;

    let o = g.to_index(origin);
    let du = g.vector_to_index(right * spacing);
    let dv = g.vector_to_index(down * spacing);

    // Slab sample offsets along the normal (index space).
    let offsets: Vec<DVec3> = match slab {
        Some(s) if s.thickness_mm > g.min_spacing() => {
            let ds = g.min_spacing();
            let count = (s.thickness_mm / ds).round().max(1.0) as usize;
            let dn = g.vector_to_index(normal * ds);
            let mid = (count as f64 - 1.0) / 2.0;
            (0..count).map(|t| dn * (t as f64 - mid)).collect()
        }
        _ => vec![DVec3::ZERO],
    };
    let mode = slab.map_or(SlabMode::Mean, |s| s.mode);

    let mut values = vec![f32::NAN; width * height];
    values.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        let base = o + dv * y as f64;
        for (x, out) in row.iter_mut().enumerate() {
            let p = base + du * x as f64;
            *out = if offsets.len() == 1 {
                volume.sample(p).unwrap_or(f32::NAN)
            } else {
                combine(mode, offsets.iter().filter_map(|off| volume.sample(p + *off)))
            };
        }
    });

    MprImage { width, height, values, spacing, origin, right, down }
}

fn combine(mode: SlabMode, samples: impl Iterator<Item = f32>) -> f32 {
    match mode {
        SlabMode::Mip => samples.fold(f32::NAN, f32::max),
        SlabMode::MinIp => samples.fold(f32::NAN, f32::min),
        SlabMode::Mean => {
            let (sum, n) = samples.fold((0.0f64, 0u32), |(s, n), v| (s + f64::from(v), n + 1));
            if n == 0 { f32::NAN } else { (sum / f64::from(n)) as f32 }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::ImagePlane;
    use crate::volume::VoxelBuffer;

    /// 20×20×20 volume (1 mm voxels) with a bright voxel cluster in the
    /// patient's left-anterior-superior octant.
    fn marker_volume(orientation: [f64; 6]) -> Volume {
        let n = 20usize;
        let planes: Vec<ImagePlane> = (0..n)
            .map(|k| {
                let row = DVec3::new(orientation[0], orientation[1], orientation[2]);
                let col = DVec3::new(orientation[3], orientation[4], orientation[5]);
                let normal = row.cross(col);
                // Centre the grid on the origin.
                let pos = -(row + col + normal) * 9.5 + normal * k as f64;
                ImagePlane::from_dicom(pos.to_array(), orientation, [1.0, 1.0], n as u32, n as u32)
                    .expect("valid plane")
            })
            .collect();
        let g = VolumeGeometry::from_sorted_planes(&planes).expect("regular");
        let marker = DVec3::new(6.0, -6.0, 6.0); // Left, Anterior, Superior
        let slices = (0..n)
            .map(|k| {
                let mut v = vec![0.0f32; n * n];
                for j in 0..n {
                    for i in 0..n {
                        let p = g.to_patient(DVec3::new(i as f64, j as f64, k as f64));
                        if (p - marker).length() < 2.5 {
                            v[j * n + i] = 1000.0;
                        }
                    }
                }
                VoxelBuffer::compact(v)
            })
            .collect();
        Volume::new(g, slices).expect("volume")
    }

    fn brightest_pixel(img: &MprImage) -> (usize, usize) {
        let (idx, _) = img
            .values
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_finite())
            .fold((0, f32::MIN), |acc, (i, &v)| if v > acc.1 { (i, v) } else { acc });
        (idx % img.width, idx / img.width)
    }

    fn assert_marker_quadrants(volume: &Volume) {
        let marker = DVec3::new(6.0, -6.0, 6.0);
        for axis in MprAxis::ALL {
            let pos = marker.dot(axis.normal());
            let img = reslice(volume, axis, pos, None, 512);
            let (x, y) = brightest_pixel(&img);
            let left_half = x < img.width / 2;
            let top_half = y < img.height / 2;
            match axis {
                // Patient left → screen right; anterior → top.
                MprAxis::Axial => assert!(!left_half && top_half, "axial {x},{y}"),
                // Patient left → screen right; superior → top.
                MprAxis::Coronal => assert!(!left_half && top_half, "coronal {x},{y}"),
                // Anterior → screen left; superior → top.
                MprAxis::Sagittal => assert!(left_half && top_half, "sagittal {x},{y}"),
            }
        }
    }

    #[test]
    fn mpr_follows_radiological_convention_for_axial_acquisition() {
        assert_marker_quadrants(&marker_volume([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]));
    }

    #[test]
    fn mpr_is_independent_of_acquisition_plane() {
        // Sagittal acquisition: rows run posterior, columns inferior.
        assert_marker_quadrants(&marker_volume([0.0, 1.0, 0.0, 0.0, 0.0, -1.0]));
        // Coronal acquisition: rows run left, columns inferior.
        assert_marker_quadrants(&marker_volume([1.0, 0.0, 0.0, 0.0, 0.0, -1.0]));
    }

    #[test]
    fn slab_projections_combine_samples() {
        let volume = marker_volume([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        // A thin slice far from the marker misses it; a thick MIP slab catches it.
        let thin = reslice(&volume, MprAxis::Axial, 0.0, None, 512);
        assert!(thin.values.iter().all(|v| v.is_nan() || *v == 0.0));
        let slab = Slab { mode: SlabMode::Mip, thickness_mm: 16.0 };
        let mip = reslice(&volume, MprAxis::Axial, 0.0, Some(slab), 512);
        assert!(mip.values.iter().any(|v| *v > 900.0));
        let mean = reslice(&volume, MprAxis::Axial, 0.0, Some(Slab { mode: SlabMode::Mean, thickness_mm: 16.0 }), 512);
        let peak = mean.values.iter().copied().filter(|v| v.is_finite()).fold(0.0, f32::max);
        assert!(peak > 0.0 && peak < 900.0);
    }

    #[test]
    fn extent_steps_follow_native_spacing() {
        let volume = marker_volume([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let ext = MprExtent::of(&volume.geometry, MprAxis::Axial);
        assert_eq!(ext.slice_count(), 20);
        assert!((ext.step - 1.0).abs() < 1e-9);
        assert_eq!(ext.index_of(ext.position(7)), 7);
    }

    #[test]
    fn pixel_patient_round_trip() {
        let volume = marker_volume([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let img = reslice(&volume, MprAxis::Coronal, 0.0, None, 512);
        let p = img.pixel_to_patient(3.0, 4.0);
        let (x, y) = img.patient_to_pixel(p);
        assert!((x - 3.0).abs() < 1e-9 && (y - 4.0).abs() < 1e-9);
    }
}
