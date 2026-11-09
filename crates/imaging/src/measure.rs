//! The maths of measuring on an image: distances and angles, the pixels of a
//! region and their statistics, and where the scale of an image comes from.
//!
//! Distances and angles are computed from positions in millimetres (patient
//! coordinates, or pixels scaled by the spacing), never from screen or pixel
//! distances, so they do not depend on zoom and are right for pixels that are
//! not square. Statistics are computed from the values of the image, so they
//! do not depend on the contrast window.

use glam::DVec3;

use crate::padding::{Padding, is_outside};

/// Where the scale of an image comes from (`measurement-tools` D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Calibration {
    /// Pixel Spacing (0028,0030): millimetres in the patient.
    PixelSpacing,
    /// Imager Pixel Spacing (0018,1164) only: millimetres at the detector of
    /// a projection image; what lies in front of it is magnified, so its true
    /// size may be smaller.
    ImagerPixelSpacing,
    /// Neither: only pixels can be counted.
    #[default]
    Uncalibrated,
}

impl Calibration {
    /// The ladder of D4: the spacing an image is displayed and measured with,
    /// and where it comes from. A Pixel Spacing that is present counts even
    /// when it is unusable (not positive): the image is then uncalibrated, it
    /// does not silently fall back to the detector's scale.
    pub fn ladder(pixel_spacing: Option<[f64; 2]>, imager_pixel_spacing: Option<[f64; 2]>) -> (Option<[f64; 2]>, Self) {
        let usable = |spacing: [f64; 2]| spacing[0] > 0.0 && spacing[1] > 0.0;
        match (pixel_spacing, imager_pixel_spacing) {
            (Some(spacing), _) if usable(spacing) => (Some(spacing), Self::PixelSpacing),
            (None, Some(spacing)) if usable(spacing) => (Some(spacing), Self::ImagerPixelSpacing),
            _ => (None, Self::Uncalibrated),
        }
    }
}

/// Straight-line distance between two positions.
pub fn distance(a: DVec3, b: DVec3) -> f64 {
    (a - b).length()
}

/// The angle in degrees (0 to 180) at `vertex` between the lines to `a` and
/// to `b`; `None` when an arm has no length.
pub fn angle_degrees(a: DVec3, vertex: DVec3, b: DVec3) -> Option<f64> {
    let first = (a - vertex).try_normalize()?;
    let second = (b - vertex).try_normalize()?;
    // Stable near 0° and 180°, where the arc cosine is not.
    Some(first.cross(second).length().atan2(first.dot(second)).to_degrees())
}

/// The shape of a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionShape {
    Ellipse,
    Rectangle,
}

/// A region of an image, axis-aligned in its pixel grid: a rectangle, or the
/// ellipse inscribed in it. Pixel coordinates are fractional, with pixel
/// centres at whole numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    shape: RegionShape,
    x: (f64, f64),
    y: (f64, f64),
}

impl Region {
    /// The region spanned by the opposite corners `a` and `b` (in any order).
    pub fn new(shape: RegionShape, a: (f64, f64), b: (f64, f64)) -> Self {
        Self { shape, x: (a.0.min(b.0), a.0.max(b.0)), y: (a.1.min(b.1), a.1.max(b.1)) }
    }

    /// Size in pixels (columns, rows) of the box around the region.
    pub fn size(&self) -> (f64, f64) {
        (self.x.1 - self.x.0, self.y.1 - self.y.0)
    }

    /// Area of the shape in square pixels: multiply by the area of one pixel.
    pub fn area_in_pixels(&self) -> f64 {
        let (width, height) = self.size();
        match self.shape {
            RegionShape::Rectangle => width * height,
            RegionShape::Ellipse => std::f64::consts::FRAC_PI_4 * width * height,
        }
    }

    /// Whether the point (`x`, `y`) lies inside the shape or on its edge.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        let in_box = x >= self.x.0 && x <= self.x.1 && y >= self.y.0 && y <= self.y.1;
        match self.shape {
            RegionShape::Rectangle => in_box,
            RegionShape::Ellipse => {
                let (width, height) = self.size();
                let along = |value: f64, range: (f64, f64), size: f64| {
                    if size > 0.0 { (value - (range.0 + range.1) / 2.0) / (size / 2.0) } else { 0.0 }
                };
                let (u, v) = (along(x, self.x, width), along(y, self.y, height));
                in_box && u * u + v * v <= 1.0
            }
        }
    }

    /// The pixels of an image of `width` × `height` whose centres lie in the shape.
    pub fn pixels(&self, width: usize, height: usize) -> impl Iterator<Item = (usize, usize)> + use<'_> {
        let columns = pixel_range(self.x, width);
        pixel_range(self.y, height)
            .flat_map(move |y| columns.clone().filter(move |&x| self.contains(x as f64, y as f64)).map(move |x| (x, y)))
    }
}

/// The whole numbers in `range` that are pixels of a row or column of `count`.
fn pixel_range(range: (f64, f64), count: usize) -> std::ops::Range<usize> {
    let last = range.1.floor();
    if count == 0 || last < 0.0 || last.is_nan() || range.0.is_nan() {
        return 0..0;
    }
    (range.0.ceil().max(0.0) as usize)..(last as usize).saturating_add(1).min(count)
}

/// Mean, standard deviation, minimum and maximum of some values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValueSummary {
    pub mean: f64,
    /// Standard deviation of these values as a whole (the population form):
    /// they are all the pixels of the region, not a sample of them.
    pub std_dev: f64,
    pub min: f32,
    pub max: f32,
}

/// The values of a region.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionStatistics {
    /// Pixels whose value counts.
    pub included: usize,
    /// Pixels of the region that hold no measurement (padding, or outside a
    /// reformatted volume) and are left out.
    pub left_out: usize,
    /// `None` when no value counts.
    pub summary: Option<ValueSummary>,
}

/// The statistics of the values inside `region` of an image of `width` ×
/// `height`, without interpolation: `value(x, y)` gives the value of a pixel.
/// Padding and values that are not finite are left out.
pub fn region_statistics(
    region: &Region,
    width: usize,
    height: usize,
    value: impl Fn(usize, usize) -> f32,
    padding: Option<Padding>,
) -> RegionStatistics {
    let (mut included, mut left_out) = (0usize, 0usize);
    let (mut mean, mut squares) = (0.0f64, 0.0f64);
    let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
    for (x, y) in region.pixels(width, height) {
        let v = value(x, y);
        if is_outside(v, padding) {
            left_out += 1;
            continue;
        }
        included += 1;
        // Welford's running form: exact for constant values, as air in a CT.
        let delta = f64::from(v) - mean;
        mean += delta / included as f64;
        squares += delta * (f64::from(v) - mean);
        min = min.min(v);
        max = max.max(v);
    }
    let summary =
        (included > 0).then(|| ValueSummary { mean, std_dev: (squares.max(0.0) / included as f64).sqrt(), min, max });
    RegionStatistics { included, left_out, summary }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::ImagePlane;

    /// 200 columns × 100 rows of an axial image with Pixel Spacing 1.0 \ 0.5:
    /// rows 1.0 mm apart, columns 0.5 mm apart.
    fn non_square() -> ImagePlane {
        ImagePlane::from_dicom([10.0, -20.0, 5.0], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [1.0, 0.5], 100, 200).expect("plane")
    }

    #[test]
    fn distances_follow_the_spacing_of_rows_and_columns() {
        let plane = non_square();
        let origin = plane.pixel_to_patient(10.0, 10.0);
        // 100 columns of 0.5 mm, 80 rows of 1.0 mm (the image has 100 rows).
        assert!((distance(origin, plane.pixel_to_patient(110.0, 10.0)) - 50.0).abs() < 1e-9);
        assert!((distance(origin, plane.pixel_to_patient(10.0, 90.0)) - 80.0).abs() < 1e-9);
        // 40 columns and 30 rows: 20 mm and 30 mm.
        let diagonal = distance(origin, plane.pixel_to_patient(50.0, 40.0));
        assert!((diagonal - (20f64 * 20.0 + 30.0 * 30.0).sqrt()).abs() < 1e-9, "{diagonal}");
    }

    #[test]
    fn the_scenario_of_the_specification_reads_50_100_and_90() {
        let plane =
            ImagePlane::from_dicom([0.0; 3], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [1.0, 0.5], 200, 200).expect("plane");
        let at = |x, y| plane.pixel_to_patient(x, y);
        assert_eq!(format!("{:.1}", distance(at(10.0, 10.0), at(110.0, 10.0))), "50.0");
        assert_eq!(format!("{:.1}", distance(at(10.0, 10.0), at(10.0, 110.0))), "100.0");
        let angle = angle_degrees(at(110.0, 10.0), at(10.0, 10.0), at(10.0, 110.0)).expect("angle");
        assert_eq!(format!("{angle:.1}"), "90.0");
    }

    #[test]
    fn angles_are_measured_in_patient_space_from_0_to_180() {
        let plane = non_square();
        let at = |x, y| plane.pixel_to_patient(x, y);
        // In the pixel grid this looks like 45°: 40 columns (20 mm) and 40 rows (40 mm).
        let angle = angle_degrees(at(50.0, 10.0), at(10.0, 10.0), at(50.0, 50.0)).expect("angle");
        assert!((angle - (40f64 / 20.0).atan().to_degrees()).abs() < 1e-9, "{angle}");
        assert!((angle - 45.0).abs() > 15.0, "not the angle in pixels: {angle}");

        let v = DVec3::ZERO;
        let degrees = |a: DVec3, b: DVec3| angle_degrees(a, v, b).expect("angle");
        assert_eq!(degrees(DVec3::X, DVec3::X), 0.0);
        assert!((degrees(DVec3::X, -DVec3::X) - 180.0).abs() < 1e-9);
        assert!((degrees(DVec3::X, DVec3::Y) - 90.0).abs() < 1e-9);
        assert!((degrees(DVec3::X, DVec3::new(-1.0, 1.0, 0.0)) - 135.0).abs() < 1e-9);
        // The order of the arms does not matter, and the plane they lie in neither.
        assert!((degrees(DVec3::new(-1.0, 1.0, 0.0), DVec3::X) - 135.0).abs() < 1e-9);
        assert!((degrees(DVec3::Z, DVec3::new(0.0, 1.0, 1.0)) - 45.0).abs() < 1e-9);
        // An arm without length has no angle.
        assert_eq!(angle_degrees(v, v, DVec3::X), None);
        assert_eq!(angle_degrees(DVec3::X, v, v), None);
    }

    fn pixels(region: &Region, width: usize, height: usize) -> Vec<(usize, usize)> {
        region.pixels(width, height).collect()
    }

    #[test]
    fn a_rectangle_holds_the_pixels_whose_centres_lie_in_it() {
        let region = Region::new(RegionShape::Rectangle, (1.5, 1.0), (3.5, 2.2));
        assert_eq!(pixels(&region, 10, 10), [(2, 1), (3, 1), (2, 2), (3, 2)]);
        // Corners in any order; centres on the edge count.
        let swapped = Region::new(RegionShape::Rectangle, (3.0, 2.0), (1.0, 1.0));
        assert_eq!(pixels(&swapped, 10, 10), [(1, 1), (2, 1), (3, 1), (1, 2), (2, 2), (3, 2)]);
        // Only pixels of the image, whatever the region.
        let beyond = Region::new(RegionShape::Rectangle, (-3.0, -3.0), (1.0, 20.0));
        assert_eq!(pixels(&beyond, 2, 2), [(0, 0), (1, 0), (0, 1), (1, 1)]);
        let outside = Region::new(RegionShape::Rectangle, (-3.0, -3.0), (-1.0, -1.0));
        assert_eq!(pixels(&outside, 4, 4), []);
        let also_outside = Region::new(RegionShape::Rectangle, (5.0, 0.0), (9.0, 1.0));
        assert_eq!(pixels(&also_outside, 4, 4), []);
        // A region between the pixel centres holds none.
        let between = Region::new(RegionShape::Rectangle, (1.2, 1.2), (1.8, 1.8));
        assert_eq!(pixels(&between, 4, 4), []);
    }

    #[test]
    fn an_ellipse_holds_the_pixels_whose_centres_lie_in_it() {
        // A circle of radius 2 about (5, 5).
        let circle = Region::new(RegionShape::Ellipse, (3.0, 3.0), (7.0, 7.0));
        assert_eq!(
            pixels(&circle, 20, 20),
            [(5, 3), (4, 4), (5, 4), (6, 4), (3, 5), (4, 5), (5, 5), (6, 5), (7, 5), (4, 6), (5, 6), (6, 6), (5, 7)]
        );
        assert!(circle.contains(5.0, 5.0) && !circle.contains(3.0, 3.0));
        // The number of pixels approaches the area of the shape.
        let large = Region::new(RegionShape::Ellipse, (10.0, 20.0), (210.0, 120.0));
        let count = large.pixels(300, 300).count() as f64;
        let area = large.area_in_pixels();
        assert!((area - std::f64::consts::PI * 100.0 * 50.0).abs() < 1e-6, "{area}");
        assert!((count - area).abs() / area < 0.01, "{count} pixels, {area} square pixels");
    }

    #[test]
    fn the_area_of_a_shape_does_not_count_pixels() {
        let rectangle = Region::new(RegionShape::Rectangle, (10.0, 10.0), (50.0, 30.0));
        assert_eq!(rectangle.area_in_pixels(), 800.0);
        // With 0.5 mm square pixels: 200 mm².
        assert_eq!(rectangle.area_in_pixels() * 0.5 * 0.5, 200.0);
        // Pixel centres on the edges count for the values: 41 × 21 of them.
        assert_eq!(rectangle.pixels(100, 100).count(), 41 * 21);
    }

    #[test]
    fn statistics_are_those_of_the_values_inside_without_interpolation() {
        // Values 0, 1, 2, 3 in a row of pixels; a value at every other pixel is far away.
        let value = |x: usize, y: usize| if y == 2 { x as f32 } else { 1000.0 };
        let region = Region::new(RegionShape::Rectangle, (0.0, 2.0), (3.0, 2.0));
        let stats = region_statistics(&region, 8, 8, value, None);
        assert_eq!((stats.included, stats.left_out), (4, 0));
        let summary = stats.summary.expect("values");
        assert_eq!((summary.mean, summary.min, summary.max), (1.5, 0.0, 3.0));
        assert!((summary.std_dev - 1.25f64.sqrt()).abs() < 1e-12, "{}", summary.std_dev);
    }

    #[test]
    fn constant_values_have_no_deviation() {
        let region = Region::new(RegionShape::Ellipse, (0.0, 0.0), (50.0, 50.0));
        let stats = region_statistics(&region, 64, 64, |_, _| -1000.0, None);
        let summary = stats.summary.expect("values");
        assert_eq!((summary.mean, summary.std_dev, summary.min, summary.max), (-1000.0, 0.0, -1000.0, -1000.0));
    }

    #[test]
    fn padding_and_values_outside_a_reformat_are_left_out() {
        let region = Region::new(RegionShape::Rectangle, (0.0, 0.0), (3.0, 0.0));
        let values = [40.0, -2000.0, f32::NAN, 60.0];
        let stats = region_statistics(&region, 4, 1, |x, _| values[x], Some(Padding::new(-2000.0, -2000.0)));
        assert_eq!((stats.included, stats.left_out), (2, 2));
        let summary = stats.summary.expect("values");
        assert_eq!((summary.mean, summary.min, summary.max), (50.0, 40.0, 60.0));
        assert_eq!(summary.std_dev, 10.0);
        // Nothing but padding: no values at all, and no NaN in their place.
        let none = region_statistics(&region, 4, 1, |_, _| f32::NAN, None);
        assert_eq!((none.included, none.left_out, none.summary), (0, 4, None));
    }

    #[test]
    fn the_calibration_ladder_takes_pixel_spacing_then_imager_pixel_spacing_then_nothing() {
        let (ps, imager) = (Some([0.5, 0.25]), Some([0.2, 0.2]));
        assert_eq!(Calibration::ladder(ps, None), (ps, Calibration::PixelSpacing));
        assert_eq!(Calibration::ladder(ps, imager), (ps, Calibration::PixelSpacing));
        assert_eq!(Calibration::ladder(None, imager), (imager, Calibration::ImagerPixelSpacing));
        assert_eq!(Calibration::ladder(None, None), (None, Calibration::Uncalibrated));
        // Values that are no spacing give no scale; an unusable Pixel Spacing is not replaced.
        assert_eq!(Calibration::ladder(None, Some([0.0, 0.2])), (None, Calibration::Uncalibrated));
        assert_eq!(Calibration::ladder(Some([0.0, 0.0]), imager), (None, Calibration::Uncalibrated));
        assert_eq!(Calibration::ladder(Some([f64::NAN, 1.0]), None), (None, Calibration::Uncalibrated));
    }
}
