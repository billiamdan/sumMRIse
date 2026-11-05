//! Pixel buffers, 2-D frames and 3-D volumes in patient space.

use std::sync::Arc;

use glam::{DMat3, DVec3};

use crate::geometry::ImagePlane;

/// Grey-scale samples after the Modality LUT (e.g. Hounsfield units for CT),
/// stored in the most compact lossless representation.
#[derive(Debug, Clone, PartialEq)]
pub enum VoxelBuffer {
    I16(Vec<i16>),
    U16(Vec<u16>),
    F32(Vec<f32>),
}

impl VoxelBuffer {
    /// Stores `values` as `i16`/`u16` when every value is an integer in range,
    /// otherwise keeps `f32`. This halves memory for typical CT/MR data.
    pub fn compact(values: Vec<f32>) -> Self {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        let mut integral = true;
        for &v in &values {
            if v.fract() != 0.0 || !v.is_finite() {
                integral = false;
                break;
            }
            min = min.min(v);
            max = max.max(v);
        }
        if integral && !values.is_empty() {
            if min >= f32::from(i16::MIN) && max <= f32::from(i16::MAX) {
                return Self::I16(values.iter().map(|&v| v as i16).collect());
            }
            if min >= 0.0 && max <= f32::from(u16::MAX) {
                return Self::U16(values.iter().map(|&v| v as u16).collect());
            }
        }
        Self::F32(values)
    }

    pub fn len(&self) -> usize {
        match self {
            Self::I16(v) => v.len(),
            Self::U16(v) => v.len(),
            Self::F32(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Sample at linear index `i` (panics when out of bounds).
    #[inline]
    pub fn get(&self, i: usize) -> f32 {
        match self {
            Self::I16(v) => f32::from(v[i]),
            Self::U16(v) => f32::from(v[i]),
            Self::F32(v) => v[i],
        }
    }

    pub fn byte_size(&self) -> usize {
        match self {
            Self::I16(v) => v.len() * 2,
            Self::U16(v) => v.len() * 2,
            Self::F32(v) => v.len() * 4,
        }
    }

    /// Minimum and maximum finite sample.
    pub fn min_max(&self) -> Option<(f32, f32)> {
        fn fold(it: impl Iterator<Item = f32>) -> Option<(f32, f32)> {
            it.filter(|v| v.is_finite()).fold(None, |acc, v| match acc {
                None => Some((v, v)),
                Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
            })
        }
        match self {
            Self::I16(v) => fold(v.iter().map(|&x| f32::from(x))),
            Self::U16(v) => fold(v.iter().map(|&x| f32::from(x))),
            Self::F32(v) => fold(v.iter().copied()),
        }
    }
}

/// Pixel payload of a decoded frame.
#[derive(Debug, Clone, PartialEq)]
pub enum FramePixels {
    /// One grey-scale sample per pixel, row-major.
    Gray(VoxelBuffer),
    /// Interleaved 8-bit RGB, row-major.
    Rgb(Vec<u8>),
}

/// A decoded 2-D image.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: FramePixels,
}

impl Frame {
    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    pub fn byte_size(&self) -> usize {
        match &self.pixels {
            FramePixels::Gray(b) => b.byte_size(),
            FramePixels::Rgb(b) => b.len(),
        }
    }

    /// Grey value at (`x`, `y`), if this is a grey-scale frame.
    pub fn gray_at(&self, x: u32, y: u32) -> Option<f32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        match &self.pixels {
            FramePixels::Gray(b) => Some(b.get(y as usize * self.width as usize + x as usize)),
            FramePixels::Rgb(_) => None,
        }
    }
}

/// Why a stack of slices cannot be treated as a regular volume.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum VolumeError {
    #[error("at least two slices are required")]
    TooFewSlices,
    #[error("slices have different size, orientation or pixel spacing")]
    InconsistentGrid,
    #[error("several images share the same slice position (multi-echo, dynamic or repeated scan)")]
    DuplicatePositions,
    #[error("slices are not evenly spaced (largest deviation {max_deviation_mm:.2} mm); a slice may be missing")]
    IrregularSpacing { max_deviation_mm: f64 },
    #[error("slice geometry is degenerate")]
    Degenerate,
    #[error("images carry no patient geometry (e.g. X-ray, scanned document or screenshot)")]
    NoGeometry,
    #[error("colour images cannot be reformatted")]
    NotGrayscale,
}

/// Affine mapping between voxel indices (i = column, j = row, k = slice) and
/// patient coordinates. Shear (e.g. CT gantry tilt) is represented exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeGeometry {
    /// Number of voxels along i, j, k.
    pub dims: [usize; 3],
    /// Patient position of voxel (0, 0, 0).
    pub origin: DVec3,
    /// Patient-space step for +1 along i, j and k (mm vectors).
    pub axes: [DVec3; 3],
    inverse: DMat3,
}

impl VolumeGeometry {
    pub fn new(dims: [usize; 3], origin: DVec3, axes: [DVec3; 3]) -> Option<Self> {
        let m = DMat3::from_cols(axes[0], axes[1], axes[2]);
        let det = m.determinant();
        if !det.is_finite() || det.abs() < 1e-9 || dims.contains(&0) {
            return None;
        }
        Some(Self { dims, origin, axes, inverse: m.inverse() })
    }

    /// Builds the geometry of a stack of planes that are already sorted by
    /// position. Validates that the stack is a regular, evenly spaced grid.
    pub fn from_sorted_planes(planes: &[ImagePlane]) -> Result<Self, VolumeError> {
        let (first, last) = match planes {
            [first, .., last] => (first, last),
            _ => return Err(VolumeError::TooFewSlices),
        };
        if planes.iter().any(|p| !p.same_grid(first)) {
            return Err(VolumeError::InconsistentGrid);
        }
        let normal = first.normal();
        let distances: Vec<f64> = planes.iter().map(|p| p.distance_along(normal)).collect();
        let steps: Vec<f64> = distances.windows(2).map(|w| w[1] - w[0]).collect();
        if steps.iter().any(|s| s.abs() < 1e-3) {
            return Err(VolumeError::DuplicatePositions);
        }
        let mean = (distances[distances.len() - 1] - distances[0]) / steps.len() as f64;
        let max_deviation_mm = steps.iter().map(|s| (s - mean).abs()).fold(0.0, f64::max);
        let tolerance = (0.1 * mean.abs()).max(0.05);
        if max_deviation_mm > tolerance || steps.iter().any(|s| s.signum() != mean.signum()) {
            return Err(VolumeError::IrregularSpacing { max_deviation_mm });
        }
        let axis_k = (last.position - first.position) / steps.len() as f64;
        Self::new(
            [first.cols as usize, first.rows as usize, planes.len()],
            first.position,
            [first.row_dir * first.col_spacing, first.col_dir * first.row_spacing, axis_k],
        )
        .ok_or(VolumeError::Degenerate)
    }

    #[inline]
    pub fn to_patient(&self, ijk: DVec3) -> DVec3 {
        self.origin + self.axes[0] * ijk.x + self.axes[1] * ijk.y + self.axes[2] * ijk.z
    }

    #[inline]
    pub fn to_index(&self, p: DVec3) -> DVec3 {
        self.inverse * (p - self.origin)
    }

    /// Converts a patient-space displacement into an index-space displacement.
    #[inline]
    pub fn vector_to_index(&self, v: DVec3) -> DVec3 {
        self.inverse * v
    }

    /// Voxel size along i, j, k in mm.
    pub fn spacing(&self) -> DVec3 {
        DVec3::new(self.axes[0].length(), self.axes[1].length(), self.axes[2].length())
    }

    pub fn min_spacing(&self) -> f64 {
        self.spacing().min_element()
    }

    /// Patient positions of the eight corner voxel centres.
    pub fn corners(&self) -> [DVec3; 8] {
        let [nx, ny, nz] = self.dims.map(|d| (d - 1) as f64);
        let mut out = [DVec3::ZERO; 8];
        for (n, corner) in out.iter_mut().enumerate() {
            let ijk = DVec3::new(
                if n & 1 == 0 { 0.0 } else { nx },
                if n & 2 == 0 { 0.0 } else { ny },
                if n & 4 == 0 { 0.0 } else { nz },
            );
            *corner = self.to_patient(ijk);
        }
        out
    }

    pub fn center(&self) -> DVec3 {
        let half = DVec3::new((self.dims[0] - 1) as f64, (self.dims[1] - 1) as f64, (self.dims[2] - 1) as f64) * 0.5;
        self.to_patient(half)
    }

    /// Angle between the slice stacking direction and the slice normal, in
    /// degrees (non-zero for CT gantry tilt).
    pub fn shear_degrees(&self) -> f64 {
        let n = self.axes[0].cross(self.axes[1]).normalize_or_zero();
        let k = self.axes[2].normalize_or_zero();
        n.dot(k).abs().clamp(0.0, 1.0).acos().to_degrees()
    }
}

/// A regular grey-scale volume. Each slice is stored separately (and in its
/// own compact type), avoiding one huge allocation while loading. Slices are
/// reference-counted so that a volume can be assembled over images that are
/// already held elsewhere (the acquired stack) without copying them.
#[derive(Debug, Clone)]
pub struct Volume {
    pub geometry: VolumeGeometry,
    pub slices: Vec<Arc<VoxelBuffer>>,
}

impl Volume {
    pub fn new(geometry: VolumeGeometry, slices: Vec<VoxelBuffer>) -> Option<Self> {
        Self::from_shared(geometry, slices.into_iter().map(Arc::new).collect())
    }

    /// Like [`Self::new`], sharing the slices with their other owners.
    pub fn from_shared(geometry: VolumeGeometry, slices: Vec<Arc<VoxelBuffer>>) -> Option<Self> {
        let per_slice = geometry.dims[0] * geometry.dims[1];
        (slices.len() == geometry.dims[2] && slices.iter().all(|s| s.len() == per_slice))
            .then_some(Self { geometry, slices })
    }

    pub fn byte_size(&self) -> usize {
        self.slices.iter().map(|s| s.byte_size()).sum()
    }

    #[inline]
    fn voxel(&self, i: usize, j: usize, k: usize) -> f32 {
        self.slices[k].get(j * self.geometry.dims[0] + i)
    }

    /// Trilinear interpolation at a fractional voxel index. Returns `None`
    /// outside the volume (with half a voxel tolerance at the borders).
    #[inline]
    pub fn sample(&self, ijk: DVec3) -> Option<f32> {
        let [nx, ny, nz] = self.geometry.dims;
        let max = DVec3::new((nx - 1) as f64, (ny - 1) as f64, (nz - 1) as f64);
        if ijk.cmplt(DVec3::splat(-0.5)).any() || ijk.cmpgt(max + 0.5).any() {
            return None;
        }
        let p = ijk.clamp(DVec3::ZERO, max);
        let i0 = p.x.floor() as usize;
        let j0 = p.y.floor() as usize;
        let k0 = p.z.floor() as usize;
        let i1 = (i0 + 1).min(nx - 1);
        let j1 = (j0 + 1).min(ny - 1);
        let k1 = (k0 + 1).min(nz - 1);
        let fx = (p.x - i0 as f64) as f32;
        let fy = (p.y - j0 as f64) as f32;
        let fz = (p.z - k0 as f64) as f32;
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let c00 = lerp(self.voxel(i0, j0, k0), self.voxel(i1, j0, k0), fx);
        let c10 = lerp(self.voxel(i0, j1, k0), self.voxel(i1, j1, k0), fx);
        let c01 = lerp(self.voxel(i0, j0, k1), self.voxel(i1, j0, k1), fx);
        let c11 = lerp(self.voxel(i0, j1, k1), self.voxel(i1, j1, k1), fx);
        Some(lerp(lerp(c00, c10, fy), lerp(c01, c11, fy), fz))
    }

    /// Value of the nearest voxel to a patient-space point.
    pub fn value_at_patient(&self, p: DVec3) -> Option<f32> {
        let ijk = self.geometry.to_index(p).round();
        let [nx, ny, nz] = self.geometry.dims;
        if ijk.cmplt(DVec3::ZERO).any() || ijk.x >= nx as f64 || ijk.y >= ny as f64 || ijk.z >= nz as f64 {
            return None;
        }
        Some(self.voxel(ijk.x as usize, ijk.y as usize, ijk.z as usize))
    }
}

/// Orders planes by their position along the common slice normal. The
/// returned indices are stable for equal positions.
pub fn sort_by_position(planes: &[ImagePlane]) -> Vec<usize> {
    let Some(first) = planes.first() else {
        return Vec::new();
    };
    let normal = first.normal();
    let mut order: Vec<usize> = (0..planes.len()).collect();
    order.sort_by(|&a, &b| planes[a].distance_along(normal).total_cmp(&planes[b].distance_along(normal)));
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axial_plane(z: f64) -> ImagePlane {
        ImagePlane::from_dicom([-10.0, -20.0, z], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [0.5, 0.5], 8, 6)
            .expect("valid plane")
    }

    #[test]
    fn compact_storage_picks_smallest_lossless_type() {
        assert!(matches!(VoxelBuffer::compact(vec![-1024.0, 3071.0]), VoxelBuffer::I16(_)));
        assert!(matches!(VoxelBuffer::compact(vec![0.0, 65535.0]), VoxelBuffer::U16(_)));
        assert!(matches!(VoxelBuffer::compact(vec![0.5, 1.0]), VoxelBuffer::F32(_)));
        assert!(matches!(VoxelBuffer::compact(vec![-1.0, 40000.0]), VoxelBuffer::F32(_)));
        let b = VoxelBuffer::compact(vec![-5.0, 7.0]);
        assert_eq!(b.get(0), -5.0);
        assert_eq!(b.min_max(), Some((-5.0, 7.0)));
    }

    #[test]
    fn geometry_round_trips() {
        let planes: Vec<_> = (0..5).map(|k| axial_plane(2.5 * k as f64)).collect();
        let g = VolumeGeometry::from_sorted_planes(&planes).expect("regular stack");
        assert_eq!(g.dims, [6, 8, 5]);
        assert!((g.spacing() - DVec3::new(0.5, 0.5, 2.5)).length() < 1e-9);
        let p = g.to_patient(DVec3::new(1.0, 2.0, 3.0));
        assert!((g.to_index(p) - DVec3::new(1.0, 2.0, 3.0)).length() < 1e-9);
        assert!(g.shear_degrees() < 1e-6);
    }

    #[test]
    fn detects_missing_slice_and_duplicates() {
        let mut planes: Vec<_> = (0..6).map(|k| axial_plane(2.0 * k as f64)).collect();
        planes.remove(3);
        assert!(matches!(VolumeGeometry::from_sorted_planes(&planes), Err(VolumeError::IrregularSpacing { .. })));
        let dup = vec![axial_plane(0.0), axial_plane(0.0), axial_plane(1.0)];
        assert_eq!(VolumeGeometry::from_sorted_planes(&dup), Err(VolumeError::DuplicatePositions));
        assert_eq!(VolumeGeometry::from_sorted_planes(&dup[..1]), Err(VolumeError::TooFewSlices));
    }

    #[test]
    fn sorts_shuffled_slices_spatially() {
        let planes = vec![axial_plane(4.0), axial_plane(0.0), axial_plane(2.0)];
        assert_eq!(sort_by_position(&planes), vec![1, 2, 0]);
    }

    #[test]
    fn trilinear_sampling_interpolates() {
        let planes: Vec<_> = (0..2).map(|k| axial_plane(k as f64)).collect();
        let g = VolumeGeometry::from_sorted_planes(&planes).expect("regular stack");
        let slices = vec![VoxelBuffer::F32(vec![0.0; 48]), VoxelBuffer::F32(vec![100.0; 48])];
        let v = Volume::new(g, slices).expect("consistent volume");
        assert_eq!(v.sample(DVec3::new(2.0, 3.0, 0.5)), Some(50.0));
        assert_eq!(v.sample(DVec3::new(2.0, 3.0, 5.0)), None);
        assert_eq!(v.value_at_patient(g.to_patient(DVec3::new(1.0, 1.0, 1.0))), Some(100.0));
    }
}
