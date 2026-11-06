//! Drawing a second series over the picture of a view (PET over CT or MR):
//! the overlay's volume is resampled in patient space at the centre of every
//! pixel of the picture shown, mapped through its own window and colour map,
//! and blended with an opacity. Values below a threshold are transparent.
//!
//! Fusion is positional only: the two series must share a Frame of
//! Reference, and the patient coordinates (LPS) of both are taken as the
//! scanners recorded them. Nothing is registered or moved.

use std::cmp::Ordering;

use glam::DVec3;
use rayon::prelude::*;

use crate::colormap::{Colormap, level};
use crate::geometry::ImagePlane;
use crate::mpr::MprImage;
use crate::volume::Volume;
use crate::window::Window;

/// The pixel grid of a picture in patient space: where the centre of pixel
/// (0, 0) lies and how far one column and one row step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelGrid {
    pub width: usize,
    pub height: usize,
    /// Patient position of the centre of pixel (0, 0), mm.
    pub origin: DVec3,
    /// Patient-space step from one column to the next, mm.
    pub column: DVec3,
    /// Patient-space step from one row to the next, mm.
    pub row: DVec3,
}

impl PixelGrid {
    /// The grid of an acquired image: columns run along the row direction
    /// (Pixel Spacing\[1\] apart), rows along the column direction (Pixel
    /// Spacing\[0\] apart).
    pub fn of_plane(plane: &ImagePlane) -> Self {
        Self {
            width: plane.cols as usize,
            height: plane.rows as usize,
            origin: plane.position,
            column: plane.row_dir * plane.col_spacing,
            row: plane.col_dir * plane.row_spacing,
        }
    }

    /// The grid of a reformatted picture, tilted or not.
    pub fn of_reformat(image: &MprImage) -> Self {
        Self {
            width: image.width,
            height: image.height,
            origin: image.origin,
            column: image.right * image.spacing,
            row: image.down * image.spacing,
        }
    }

    /// Patient position of the pixel (`x`, `y`) (pixel centres at whole numbers).
    pub fn to_patient(&self, x: f64, y: f64) -> DVec3 {
        self.origin + self.column * x + self.row * y
    }

    /// Number of pixels.
    pub fn len(&self) -> usize {
        self.width * self.height
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// How the overlay is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayLook {
    /// The colour scale: overlay values from the low end of the window (entry
    /// 0 of the map) to its high end (entry 255).
    pub window: Window,
    pub colormap: Colormap,
    /// 0 leaves the picture unchanged, 1 covers it.
    pub opacity: f32,
    /// Overlay values below this are not drawn (transparent).
    pub threshold: f64,
}

/// The overlay's value at the patient position `p`: trilinear, `None`
/// outside its volume.
pub fn sample_at(volume: &Volume, p: DVec3) -> Option<f32> {
    volume.sample(volume.geometry.to_index(p))
}

/// `volume` sampled trilinearly at the centre of every pixel of `grid`, row
/// by row; NaN where the pixel lies outside the volume.
pub fn resample(volume: &Volume, grid: &PixelGrid) -> Vec<f32> {
    let g = &volume.geometry;
    let origin = g.to_index(grid.origin);
    let column = g.vector_to_index(grid.column);
    let row = g.vector_to_index(grid.row);
    let mut values = vec![f32::NAN; grid.len()];
    if grid.width == 0 {
        return values;
    }
    values.par_chunks_mut(grid.width).enumerate().for_each(|(y, line)| {
        let start = origin + row * y as f64;
        for (x, value) in line.iter_mut().enumerate() {
            *value = volume.sample(start + column * x as f64).unwrap_or(f32::NAN);
        }
    });
    values
}

/// Draws `volume` over the picture `rgba` (RGBA8 of the size of `grid`):
/// every pixel whose overlay value is at or above the threshold gets the
/// colour of that value mixed in, `(1 − opacity) × picture + opacity ×
/// colour`. Pixels below the threshold or outside the overlay's volume are
/// left exactly as they are, and so is every pixel with opacity 0.
pub fn blend(rgba: &mut [u8], grid: &PixelGrid, volume: &Volume, look: &OverlayLook) {
    // No opacity (or none that is a number): nothing to draw.
    let visible = look.opacity.partial_cmp(&0.0) == Some(Ordering::Greater);
    if !visible || grid.is_empty() || rgba.len() != grid.len() * 4 {
        return;
    }
    let alpha = look.opacity.min(1.0);
    let values = resample(volume, grid);
    let table = look.colormap.table();
    rgba.par_chunks_mut(4 * grid.width).zip(values.par_chunks(grid.width)).for_each(|(pixels, line)| {
        for (pixel, &value) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(line) {
            let value = f64::from(value);
            // Outside the overlay (NaN) or below the threshold: transparent.
            if matches!(value.partial_cmp(&look.threshold), None | Some(Ordering::Less)) {
                continue;
            }
            let colour = table[usize::from(level(look.window.apply(value)))];
            for (channel, over) in pixel.iter_mut().zip(colour) {
                *channel = (f32::from(*channel) * (1.0 - alpha) + f32::from(over) * alpha).round() as u8;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpr::{MprAxis, reslice};
    use crate::volume::{VolumeGeometry, VoxelBuffer};

    /// 5 × 5 × 5 voxels of 2 mm centred on the patient origin, 0 everywhere
    /// but the centre voxel (at L 0, A 0, S 0 mm), which holds 100.
    fn dot() -> Volume {
        let geometry =
            VolumeGeometry::new([5, 5, 5], DVec3::splat(-4.0), [DVec3::X * 2.0, DVec3::Y * 2.0, DVec3::Z * 2.0])
                .expect("geometry");
        let slices = (0..5)
            .map(|k| {
                let mut v = vec![0.0f32; 25];
                if k == 2 {
                    v[2 * 5 + 2] = 100.0;
                }
                VoxelBuffer::compact(v)
            })
            .collect();
        Volume::new(geometry, slices).expect("volume")
    }

    /// An axial image at height `z` of 9 × 9 pixels of 1 mm centred on the
    /// patient origin, with screen-right towards `right` and screen-down
    /// towards `down`.
    fn axial(z: f64, right: DVec3, down: DVec3) -> PixelGrid {
        PixelGrid {
            width: 9,
            height: 9,
            origin: DVec3::new(0.0, 0.0, z) - (right + down) * 4.0,
            column: right,
            row: down,
        }
    }

    fn grey(grid: &PixelGrid, level: u8) -> Vec<u8> {
        [level, level, level, 255].repeat(grid.len())
    }

    #[test]
    fn the_overlay_is_sampled_at_true_patient_positions() {
        let volume = dot();
        let grid = axial(0.0, DVec3::X, DVec3::Y);
        let values = resample(&volume, &grid);
        // Pixel (4, 4) lies on the bright voxel; 1 mm to the patient's left is half-way to the next voxel.
        assert_eq!(values[4 * 9 + 4], 100.0);
        assert_eq!(values[4 * 9 + 5], 50.0);
        assert_eq!(values[5 * 9 + 4], 50.0, "1 mm towards posterior");
        assert_eq!(values[4 * 9 + 6], 0.0);
        // The same patient position, whatever the picture's orientation: here screen-right
        // points to the patient's right and screen-down to anterior (a turned, mirrored grid).
        let turned = axial(0.0, -DVec3::X, -DVec3::Y);
        let at = |grid: &PixelGrid, p: DVec3| {
            let values = resample(&volume, grid);
            let found =
                (0..grid.len()).find(|&i| (grid.to_patient((i % 9) as f64, (i / 9) as f64) - p).length() < 1e-9);
            found.map(|i| values[i])
        };
        for p in [DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0), DVec3::new(0.0, -1.0, 0.0), DVec3::new(-3.0, 2.0, 0.0)] {
            assert_eq!(at(&grid, p), at(&turned, p), "{p:?}");
            assert_eq!(at(&grid, p), sample_at(&volume, p), "{p:?}");
        }
        // 1 mm above the voxel: half of it; outside the volume: nothing.
        assert_eq!(resample(&volume, &axial(1.0, DVec3::X, DVec3::Y))[4 * 9 + 4], 50.0);
        assert!(resample(&volume, &axial(20.0, DVec3::X, DVec3::Y)).iter().all(|v| v.is_nan()));
        assert_eq!(sample_at(&volume, DVec3::new(30.0, 0.0, 0.0)), None);
    }

    #[test]
    fn grids_of_acquired_images_and_reformats() {
        let plane = ImagePlane::from_dicom([-10.0, -20.0, 5.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [0.5, 0.8], 6, 4)
            .expect("plane");
        let grid = PixelGrid::of_plane(&plane);
        assert_eq!((grid.width, grid.height), (4, 6));
        // Pixel Spacing is rows \ columns: a column step is 0.8 mm, a row step 0.5 mm.
        assert!((grid.to_patient(2.0, 3.0) - plane.pixel_to_patient(2.0, 3.0)).length() < 1e-12);
        let reformat = reslice(&dot(), MprAxis::Coronal, 0.0, None, 64);
        let grid = PixelGrid::of_reformat(&reformat);
        assert_eq!((grid.width, grid.height, grid.len()), (reformat.width, reformat.height, reformat.values.len()));
        assert!((grid.to_patient(3.0, 2.0) - reformat.pixel_to_patient(3.0, 2.0)).length() < 1e-12);
        // Sampling the volume on the reformat's grid gives the reformat itself.
        assert_eq!(
            resample(&dot(), &grid).iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            reformat.values.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn opacity_threshold_and_colour() {
        let volume = dot();
        let grid = axial(0.0, DVec3::X, DVec3::Y);
        let look = OverlayLook {
            window: Window::from_range(0.0, 100.0),
            colormap: Colormap::HotIron,
            opacity: 0.5,
            threshold: 40.0,
        };
        let base = grey(&grid, 100);

        // Opacity 0 changes nothing at all.
        let mut picture = base.clone();
        blend(&mut picture, &grid, &volume, &OverlayLook { opacity: 0.0, ..look });
        assert_eq!(picture, base);

        let mut picture = base.clone();
        blend(&mut picture, &grid, &volume, &look);
        let pixel = |i: usize| [picture[4 * i], picture[4 * i + 1], picture[4 * i + 2], picture[4 * i + 3]];
        // The bright voxel (100, the top of the colour scale): half grey 100, half (255, 254, 252).
        assert_eq!(pixel(4 * 9 + 4), [178, 177, 176, 255]);
        // 50 is above the threshold of 40: the middle of the scale, red.
        assert_eq!(pixel(4 * 9 + 5), [178, 50, 50, 255]);
        // 0 is below the threshold: transparent, the picture as it was.
        assert_eq!(pixel(0), [100, 100, 100, 255]);
        let changed = (0..grid.len()).filter(|&i| pixel(i) != [100, 100, 100, 255]).count();
        assert_eq!(changed, 5, "the voxel and its four neighbours at 50");

        // Fully opaque: the colour itself.
        let mut picture = base.clone();
        blend(&mut picture, &grid, &volume, &OverlayLook { opacity: 1.0, colormap: Colormap::Rainbow, ..look });
        assert_eq!(&picture[4 * (4 * 9 + 4)..4 * (4 * 9 + 4) + 4], &[255, 0, 0, 255]);

        // Outside the overlay's volume and with a picture of the wrong size nothing changes.
        let mut picture = base.clone();
        blend(&mut picture, &axial(20.0, DVec3::X, DVec3::Y), &volume, &look);
        assert_eq!(picture, base);
        let mut short = vec![7u8; 8];
        blend(&mut short, &grid, &volume, &look);
        assert_eq!(short, vec![7u8; 8]);
    }
}
