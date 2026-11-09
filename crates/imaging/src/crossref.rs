//! Where the slices of different series lie relative to each other, from
//! their header geometry alone: the slice nearest to a patient position
//! (synchronised scrolling) and the line along which one image's plane cuts
//! another image (cross-reference lines). Everything is in patient
//! coordinates and millimetres; image numbers play no part.

use glam::DVec3;

use crate::geometry::ImagePlane;

/// Slice directions that differ by less than this many degrees scroll
/// together by position.
pub const LINK_ANGLE_DEGREES: f64 = 30.0;

/// Planes within this many degrees of parallel have no cross-reference line.
pub const PARALLEL_DEGREES: f64 = 10.0;

/// The angle between two slice directions in degrees, from 0 to 90: normals
/// that point in opposite directions belong to the same slice direction.
pub fn slice_angle_degrees(a: DVec3, b: DVec3) -> f64 {
    a.dot(b).abs().min(1.0).acos().to_degrees()
}

/// Patient position of the centre of an image.
pub fn centre(plane: &ImagePlane) -> DVec3 {
    plane.pixel_to_patient((f64::from(plane.cols) - 1.0) / 2.0, (f64::from(plane.rows) - 1.0) / 2.0)
}

/// The slice of a series nearest to a patient position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NearestSlice {
    /// Index into the planes the slice was looked for in.
    pub index: usize,
    /// Distance of the position from the slice's plane, along its normal.
    pub distance_mm: f64,
    /// The position lies beyond the slices at the ends: further from the end
    /// slice than half the average distance between slices.
    pub outside: bool,
}

/// The slice whose plane is nearest to `point`, each measured along its own
/// normal; the first one of those equally near. Images without a plane do
/// not count; `None` when no image has one.
pub fn nearest_slice(planes: &[Option<ImagePlane>], point: DVec3) -> Option<NearestSlice> {
    let distance = |plane: &ImagePlane| (point - plane.position).dot(plane.normal()).abs();
    let (index, plane) = planes
        .iter()
        .enumerate()
        .filter_map(|(i, plane)| Some((i, plane.as_ref()?)))
        .reduce(|best, candidate| if distance(candidate.1) < distance(best.1) { candidate } else { best })?;
    let normal = plane.normal();
    let along = planes.iter().flatten().map(|other| other.position.dot(normal));
    let (count, low, high) =
        along.fold((0usize, f64::INFINITY, f64::NEG_INFINITY), |(n, lo, hi), p| (n + 1, lo.min(p), hi.max(p)));
    // One image alone covers its own plane, within half a millimetre.
    let half_gap = if count > 1 { (high - low) / (count - 1) as f64 / 2.0 } else { 0.0 }.max(0.5);
    let position = point.dot(normal);
    let outside = position < low - half_gap - 1e-6 || position > high + half_gap + 1e-6;
    Some(NearestSlice { index, distance_mm: distance(plane), outside })
}

/// The segment along which the plane of `cutting` crosses the image `image`,
/// as two points in pixel coordinates of `image` (column, row; pixel centres
/// at whole numbers), on the outer edge of its picture. From the signed
/// distances of the image's four corners to the plane: the ends lie where the
/// sign changes along the edges. `None` when the planes are within
/// [`PARALLEL_DEGREES`] of parallel or the plane misses the image.
pub fn intersection_segment(cutting: &ImagePlane, image: &ImagePlane) -> Option<[(f64, f64); 2]> {
    let normal = cutting.normal();
    if slice_angle_degrees(normal, image.normal()) < PARALLEL_DEGREES {
        return None;
    }
    let (right, bottom) = (f64::from(image.cols) - 0.5, f64::from(image.rows) - 0.5);
    let corners = [(-0.5, -0.5), (right, -0.5), (right, bottom), (-0.5, bottom)];
    let distances = corners.map(|(x, y)| (image.pixel_to_patient(x, y) - cutting.position).dot(normal));
    let mut ends = Vec::with_capacity(2);
    for i in 0..4 {
        let j = (i + 1) % 4;
        let (a, b) = (distances[i], distances[j]);
        if a == 0.0 {
            ends.push(corners[i]);
        } else if a * b < 0.0 {
            let t = a / (a - b);
            let (from, to) = (corners[i], corners[j]);
            ends.push((from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t));
        }
    }
    match ends[..] {
        [first, second, ..] => Some([first, second]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An axial image of 64 × 64 pixels of 2 mm at height `z`, centred on the
    /// patient axis; columns run to the left, rows to the back.
    fn axial(z: f64) -> ImagePlane {
        ImagePlane::from_dicom([-63.0, -63.0, z], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [2.0, 2.0], 64, 64).expect("plane")
    }

    /// A sagittal image of 64 × 64 pixels of 2 mm at x = `x`: columns run to
    /// the back, rows to the feet.
    fn sagittal(x: f64) -> ImagePlane {
        ImagePlane::from_dicom([x, -63.0, 63.0], [0.0, 1.0, 0.0, 0.0, 0.0, -1.0], [2.0, 2.0], 64, 64).expect("plane")
    }

    fn stack(heights: impl Iterator<Item = f64>) -> Vec<Option<ImagePlane>> {
        heights.map(|z| Some(axial(z))).collect()
    }

    #[test]
    fn the_nearest_slice_is_found_in_millimetres_whatever_the_spacing() {
        // 2.5 mm apart from -60 to +60 mm.
        let fine = stack((0..49).map(|k| -60.0 + 2.5 * f64::from(k)));
        // The slice at -40 mm of a 5 mm series: its centre is the reference point.
        let at_40 = centre(&axial(-40.0));
        assert_eq!(at_40, DVec3::new(0.0, 0.0, -40.0));
        let nearest = nearest_slice(&fine, at_40).expect("slices");
        assert_eq!((nearest.index, nearest.distance_mm, nearest.outside), (8, 0.0, false));
        // One step of the 5 mm series, to -45 mm: two slices of the fine one, to -45 mm.
        let nearest = nearest_slice(&fine, centre(&axial(-45.0))).expect("slices");
        assert_eq!(nearest.index, 6);
        assert_eq!(fine[6].expect("plane").position.z, -45.0);

        // The other way round: the coarse series shows its nearest slice, 1.25 mm away at most.
        let coarse = stack((0..25).map(|k| -60.0 + 5.0 * f64::from(k)));
        let nearest = nearest_slice(&coarse, centre(&axial(-41.0))).expect("slices");
        assert_eq!(coarse[nearest.index].expect("plane").position.z, -40.0);
        assert!((nearest.distance_mm - 1.0).abs() < 1e-9);
        // Exactly between two slices: the first of them, and it stays the same every time.
        let nearest = nearest_slice(&coarse, centre(&axial(-42.5))).expect("slices");
        assert_eq!(coarse[nearest.index].expect("plane").position.z, -45.0);
    }

    #[test]
    fn the_distance_is_measured_along_the_normal_of_the_slice() {
        // A point far off to the side still belongs to the plane at its height.
        let slices = stack((0..5).map(|k| 10.0 * f64::from(k)));
        let nearest = nearest_slice(&slices, DVec3::new(500.0, -300.0, 21.0)).expect("slices");
        assert_eq!(nearest.index, 2);
        assert!((nearest.distance_mm - 1.0).abs() < 1e-9);
        // A stack whose normals point the other way (rows reversed) is measured just the same.
        let flipped: Vec<Option<ImagePlane>> = (0..5)
            .map(|k| {
                ImagePlane::from_dicom(
                    [-63.0, 63.0, 10.0 * f64::from(k)],
                    [1.0, 0.0, 0.0, 0.0, -1.0, 0.0],
                    [2.0, 2.0],
                    64,
                    64,
                )
            })
            .collect();
        assert_eq!(flipped[0].expect("plane").normal().z, -1.0);
        assert_eq!(nearest_slice(&flipped, DVec3::new(0.0, 0.0, 21.0)).expect("slices").index, 2);
    }

    #[test]
    fn a_position_beyond_half_a_gap_past_the_end_is_outside() {
        // -20 to +20 mm every 5 mm.
        let short = stack((0..9).map(|k| -20.0 + 5.0 * f64::from(k)));
        let at = |z: f64| nearest_slice(&short, DVec3::new(0.0, 0.0, z)).expect("slices");
        assert!(!at(20.0).outside);
        assert!(!at(22.4).outside, "still within half a gap of the end slice");
        let beyond = at(25.0);
        assert!(beyond.outside);
        assert_eq!((beyond.index, beyond.distance_mm), (8, 5.0), "it stays on its nearest end slice");
        let below = at(-40.0);
        assert!(below.outside);
        assert_eq!((below.index, below.distance_mm), (0, 20.0));
        // Inside the coverage never outside, also where images are missing.
        let with_gap = stack([-20.0, -15.0, 15.0, 20.0].into_iter());
        assert!(!nearest_slice(&with_gap, DVec3::new(0.0, 0.0, 0.0)).expect("slices").outside);
    }

    #[test]
    fn images_without_a_plane_do_not_count() {
        assert_eq!(nearest_slice(&[], DVec3::ZERO), None);
        assert_eq!(nearest_slice(&[None, None], DVec3::ZERO), None);
        let nearest = nearest_slice(&[None, Some(axial(4.0)), None], DVec3::ZERO).expect("one image");
        assert_eq!((nearest.index, nearest.distance_mm, nearest.outside), (1, 4.0, true));
        assert!(!nearest_slice(&[Some(axial(0.25))], DVec3::ZERO).expect("one image").outside);
    }

    #[test]
    fn the_axial_slice_at_12_mm_cuts_a_sagittal_image_along_its_row() {
        let image = sagittal(0.0);
        let [a, b] = intersection_segment(&axial(12.0), &image).expect("a line");
        // Rows run to the feet from z = +63 mm in steps of 2 mm: z = 12 mm is row 25.5.
        assert!((a.1 - 25.5).abs() < 1e-9 && (b.1 - 25.5).abs() < 1e-9, "{a:?} {b:?}");
        // Across the whole picture, from its outer edge to its outer edge.
        let mut columns = [a.0, b.0];
        columns.sort_by(f64::total_cmp);
        assert_eq!(columns, [-0.5, 63.5]);
        // Towards the head the line moves up: to a smaller row.
        let [higher, _] = intersection_segment(&axial(32.0), &image).expect("a line");
        assert!((higher.1 - 15.5).abs() < 1e-9, "{higher:?}");
        // The patient position of the line is at the height of the slice.
        assert!((image.pixel_to_patient(a.0, a.1).z - 12.0).abs() < 1e-9);
    }

    #[test]
    fn parallel_planes_have_no_line_and_a_plane_that_misses_has_none() {
        assert_eq!(intersection_segment(&axial(12.0), &axial(20.0)), None);
        // Tilted by 9°: within 10° of parallel. Tilted by 11°: a line.
        let tilted = |degrees: f64| {
            let (s, c) = degrees.to_radians().sin_cos();
            ImagePlane::from_dicom([-63.0, -63.0, 0.0], [1.0, 0.0, 0.0, 0.0, c, s], [2.0, 2.0], 64, 64).expect("plane")
        };
        assert_eq!(intersection_segment(&tilted(9.0), &axial(0.0)), None);
        assert!(intersection_segment(&tilted(11.0), &axial(0.0)).is_some());
        // The sagittal image reaches from z = -63 to +63 mm (and half a pixel more): z = 80 mm misses it.
        assert_eq!(intersection_segment(&axial(80.0), &sagittal(0.0)), None);
        assert!(intersection_segment(&axial(63.5), &sagittal(0.0)).is_some());
    }

    #[test]
    fn an_oblique_plane_gives_a_slanted_line() {
        // A plane through the centre, at 45° between axial and coronal.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let oblique = ImagePlane::from_dicom([-63.0, -63.0, -63.0], [1.0, 0.0, 0.0, 0.0, s, s], [2.0, 2.0], 64, 64)
            .expect("plane");
        let [a, b] = intersection_segment(&oblique, &sagittal(0.0)).expect("a line");
        // On the sagittal image y = z in this plane: from the bottom left to the top right corner.
        let mut ends = [a, b];
        ends.sort_by(|p, q| p.0.total_cmp(&q.0));
        assert!((ends[0].0 + 0.5).abs() < 1e-6 && (ends[0].1 - 63.5).abs() < 1e-6, "{ends:?}");
        assert!((ends[1].0 - 63.5).abs() < 1e-6 && (ends[1].1 + 0.5).abs() < 1e-6, "{ends:?}");
    }

    #[test]
    fn slice_directions_are_compared_without_their_sign() {
        assert_eq!(slice_angle_degrees(DVec3::Z, -DVec3::Z), 0.0);
        assert!((slice_angle_degrees(DVec3::Z, DVec3::X) - 90.0).abs() < 1e-9);
        let tilted = DVec3::new(0.0, 25f64.to_radians().sin(), -(25f64.to_radians().cos()));
        assert!((slice_angle_degrees(DVec3::Z, tilted) - 25.0).abs() < 1e-9);
        assert!(slice_angle_degrees(DVec3::Z, tilted) < LINK_ANGLE_DEGREES);
    }
}
