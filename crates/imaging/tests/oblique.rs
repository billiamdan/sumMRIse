//! Reslicing on tilted planes (`reslice_plane`): the three standard planes
//! give the same bits as before planes could be tilted, and a tilted plane
//! shows the volume where the geometry says.

use summrise_imaging::{
    DQuat, DVec3, ImagePlane, MprAxis, MprExtent, MprImage, PlaneBasis, Slab, SlabMode, Volume, VolumeGeometry,
    VoxelBuffer, reslice, reslice_plane,
};

/// Centre of the bright sphere: patient left, anterior, superior (mm).
const MARKER: DVec3 = DVec3::new(6.0, -6.0, 6.0);
const MARKER_RADIUS: f64 = 2.5;
const MARKER_VALUE: f32 = 1000.0;

/// A volume over `planes` with the marker sphere (1000) on a faint pattern
/// (0 to 49) that makes every voxel different from its neighbours.
fn phantom(planes: &[ImagePlane]) -> Volume {
    let g = VolumeGeometry::from_sorted_planes(planes).expect("regular stack");
    let [nx, ny, nz] = g.dims;
    let slices = (0..nz)
        .map(|k| {
            let mut v = vec![0.0f32; nx * ny];
            for j in 0..ny {
                for i in 0..nx {
                    let p = g.to_patient(DVec3::new(i as f64, j as f64, k as f64));
                    v[j * nx + i] = if (p - MARKER).length() < MARKER_RADIUS {
                        MARKER_VALUE
                    } else {
                        ((i * 7 + j * 13 + k * 29) % 50) as f32
                    };
                }
            }
            VoxelBuffer::compact(v)
        })
        .collect();
    Volume::new(g, slices).expect("volume")
}

/// `n` slices of `n × n` pixels of 1 mm, centred on the patient origin.
fn cube(orientation: [f64; 6]) -> Volume {
    let n = 20usize;
    let row = DVec3::new(orientation[0], orientation[1], orientation[2]);
    let col = DVec3::new(orientation[3], orientation[4], orientation[5]);
    let normal = row.cross(col);
    let planes: Vec<ImagePlane> = (0..n)
        .map(|k| {
            let pos = -(row + col + normal) * 9.5 + normal * k as f64;
            ImagePlane::from_dicom(pos.to_array(), orientation, [1.0, 1.0], n as u32, n as u32).expect("plane")
        })
        .collect();
    phantom(&planes)
}

fn axial() -> Volume {
    cube([1.0, 0.0, 0.0, 0.0, 1.0, 0.0])
}

/// Sagittal acquisition: rows run posterior, columns inferior.
fn sagittal() -> Volume {
    cube([0.0, 1.0, 0.0, 0.0, 0.0, -1.0])
}

/// CT with 20° gantry tilt: the images lean back, the table moves along the
/// patient axis, so the stack is sheared.
fn gantry_tilted() -> Volume {
    let n = 20usize;
    let (sin, cos) = 20f64.to_radians().sin_cos();
    let orientation = [1.0, 0.0, 0.0, 0.0, cos, sin];
    let (row, col) = (DVec3::X, DVec3::new(0.0, cos, sin));
    let planes: Vec<ImagePlane> = (0..n)
        .map(|k| {
            let pos = -(row + col) * 9.5 + DVec3::Z * (k as f64 - 9.5) * 1.2;
            ImagePlane::from_dicom(pos.to_array(), orientation, [1.0, 1.0], n as u32, n as u32).expect("plane")
        })
        .collect();
    let volume = phantom(&planes);
    assert!((volume.geometry.shear_degrees() - 20.0).abs() < 1e-9);
    volume
}

/// Voxels of 0.8 × 1.2 × 2.5 mm in a block that is not a cube.
fn anisotropic() -> Volume {
    let planes: Vec<ImagePlane> = (0..9)
        .map(|k| {
            let pos = [-9.2, -9.0, -10.0 + 2.5 * f64::from(k)];
            ImagePlane::from_dicom(pos, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [1.2, 0.8], 16, 24).expect("plane")
        })
        .collect();
    phantom(&planes)
}

fn about(axis: DVec3, degrees: f64) -> DQuat {
    DQuat::from_axis_angle(axis.normalize(), degrees.to_radians())
}

/// 30° counter-clockwise in the axial view, then 25° clockwise in the
/// (tilted) coronal view: all three planes are oblique.
fn double_oblique() -> DQuat {
    let first = about(DVec3::Z, -30.0);
    about(PlaneBasis::tilted(MprAxis::Coronal, first).normal, 25.0) * first
}

/// Centre of the pixels brighter than half the marker value.
fn marker_centre(img: &MprImage) -> Option<(f64, f64)> {
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for (i, v) in img.values.iter().enumerate() {
        if *v > MARKER_VALUE / 2.0 {
            sx += (i % img.width) as f64;
            sy += (i / img.width) as f64;
            n += 1.0;
        }
    }
    (n > 0.0).then(|| (sx / n, sy / n))
}

fn assert_marker_at(img: &MprImage, expected: (f64, f64), tolerance: f64, what: &str) {
    let (x, y) = marker_centre(img).unwrap_or_else(|| panic!("{what}: no marker in the picture"));
    assert!(
        (x - expected.0).abs() <= tolerance && (y - expected.1).abs() <= tolerance,
        "{what}: marker at ({x:.2}, {y:.2}), geometry says ({:.2}, {:.2})",
        expected.0,
        expected.1
    );
}

/// Every pixel holds the value of the volume at the pixel's own patient
/// position: the picture and its geometry agree everywhere.
fn assert_pixels_match_their_positions(volume: &Volume, img: &MprImage, what: &str) {
    let mut inside = 0;
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img.pixel_to_patient(x as f64, y as f64);
            let direct = volume.sample(volume.geometry.to_index(p));
            // On the very border the two computations may round to different sides: only
            // pixels that both place inside the volume are compared.
            if let (Some(shown), Some(direct)) = (img.value_at(x, y), direct) {
                inside += 1;
                assert!((shown - direct).abs() < 1e-2, "{what}: pixel ({x}, {y}) shows {shown}, volume has {direct}");
            }
        }
    }
    assert!(inside > img.width * img.height / 4, "{what}: only {inside} pixels inside the volume");
}

/// `MprExtent::of` and `reslice` as they were before planes could be tilted
/// (canonical axes only), kept as the reference for the bit-identity test.
mod reference {
    use super::*;

    pub(super) struct Extent {
        pub(super) u: (f64, f64),
        pub(super) v: (f64, f64),
        pub(super) n: (f64, f64),
        pub(super) step: f64,
    }

    pub(super) fn extent(geometry: &VolumeGeometry, axis: MprAxis) -> Extent {
        let (right, down, normal) = axis.basis();
        let range = |dir: DVec3| {
            geometry.corners().iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
                let d = c.dot(dir);
                (lo.min(d), hi.max(d))
            })
        };
        let step = geometry.axes.iter().map(|a| a.dot(normal).abs()).fold(0.0, f64::max).max(1e-3);
        Extent { u: range(right), v: range(down), n: range(normal), step }
    }

    pub(super) fn reslice(
        volume: &Volume,
        axis: MprAxis,
        position: f64,
        slab: Option<Slab>,
        max_dim: usize,
    ) -> MprImage {
        let g = &volume.geometry;
        let (right, down, normal) = axis.basis();
        let ext = extent(g, axis);
        let max_dim = max_dim.max(16) as f64;
        let spacing =
            g.min_spacing().max((ext.u.1 - ext.u.0) / (max_dim - 1.0)).max((ext.v.1 - ext.v.0) / (max_dim - 1.0));
        let width = ((ext.u.1 - ext.u.0) / spacing).floor() as usize + 1;
        let height = ((ext.v.1 - ext.v.0) / spacing).floor() as usize + 1;
        let origin = right * ext.u.0 + down * ext.v.0 + normal * position;

        let o = g.to_index(origin);
        let du = g.vector_to_index(right * spacing);
        let dv = g.vector_to_index(down * spacing);

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
        for (y, row) in values.chunks_mut(width).enumerate() {
            let base = o + dv * y as f64;
            for (x, out) in row.iter_mut().enumerate() {
                let p = base + du * x as f64;
                *out = if offsets.len() == 1 {
                    volume.sample(p).unwrap_or(f32::NAN)
                } else {
                    combine(mode, offsets.iter().filter_map(|off| volume.sample(p + *off)))
                };
            }
        }
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
}

fn vec_bits(v: DVec3) -> [u64; 3] {
    v.to_array().map(f64::to_bits)
}

fn assert_bit_identical(a: &MprImage, b: &MprImage, what: &str) {
    assert_eq!((a.width, a.height), (b.width, b.height), "{what}: size");
    assert_eq!(a.spacing.to_bits(), b.spacing.to_bits(), "{what}: spacing");
    assert_eq!(vec_bits(a.origin), vec_bits(b.origin), "{what}: origin");
    assert_eq!(vec_bits(a.right), vec_bits(b.right), "{what}: right");
    assert_eq!(vec_bits(a.down), vec_bits(b.down), "{what}: down");
    let differing = a.values.iter().zip(&b.values).filter(|(x, y)| x.to_bits() != y.to_bits()).count();
    assert_eq!(differing, 0, "{what}: {differing} of {} values differ", a.values.len());
}

#[test]
fn standard_planes_are_bit_identical_to_the_previous_implementation() {
    let volumes = [("axial", axial()), ("sagittal", sagittal()), ("gantry", gantry_tilted()), ("aniso", anisotropic())];
    let slabs = [
        None,
        Some(Slab { mode: SlabMode::Mip, thickness_mm: 16.0 }),
        Some(Slab { mode: SlabMode::MinIp, thickness_mm: 5.0 }),
        Some(Slab { mode: SlabMode::Mean, thickness_mm: 7.5 }),
    ];
    let mut compared = 0;
    for (name, volume) in &volumes {
        for axis in MprAxis::ALL {
            let before = reference::extent(&volume.geometry, axis);
            let now = MprExtent::of(&volume.geometry, axis);
            assert_eq!((now.u, now.v, now.n, now.step), (before.u, before.v, before.n, before.step), "{name} {axis:?}");
            assert_eq!(MprExtent::of_plane(&volume.geometry, &PlaneBasis::standard(axis)), now);
            // The identity rotation, however it is written, is the standard plane.
            let bases = [
                PlaneBasis::standard(axis),
                PlaneBasis::tilted(axis, DQuat::IDENTITY),
                PlaneBasis::rotated(axis, DQuat::IDENTITY),
                PlaneBasis::tilted(axis, about(DVec3::X, 17.0) * about(DVec3::X, -17.0)),
            ];
            for position in [before.n.0, 0.0, 0.37, MARKER.dot(axis.normal()), before.n.1] {
                for slab in slabs {
                    for max_dim in [512, 16] {
                        let what = format!("{name} {axis:?} at {position} mm, slab {slab:?}, {max_dim} px");
                        let expected = reference::reslice(volume, axis, position, slab, max_dim);
                        assert_bit_identical(&reslice(volume, axis, position, slab, max_dim), &expected, &what);
                        for basis in &bases {
                            let tilted = reslice_plane(volume, basis, position, slab, max_dim);
                            assert_bit_identical(&tilted, &expected, &what);
                        }
                        compared += 1;
                    }
                }
            }
        }
    }
    assert_eq!(compared, 4 * 3 * 5 * 4 * 2);
}

#[test]
fn marker_appears_where_geometry_says_after_45_degrees_about_the_superior_axis() {
    let volume = axial();
    // The coronal plane turned so that screen-right points to left-anterior.
    let basis = PlaneBasis::tilted(MprAxis::Coronal, about(DVec3::Z, -45.0));
    let h = std::f64::consts::FRAC_1_SQRT_2;
    assert!((basis.right - DVec3::new(h, -h, 0.0)).length() < 1e-12);
    assert!((basis.down + DVec3::Z).length() < 1e-12 && (basis.normal - DVec3::new(h, h, 0.0)).length() < 1e-12);

    // Through the centre of the volume this plane contains the marker.
    assert!(MARKER.dot(basis.normal).abs() < 1e-12);
    let img = reslice_plane(&volume, &basis, 0.0, None, 512);
    // Extent: the diagonal of the 19 mm square, 19 mm high.
    assert_eq!((img.width, img.height, img.spacing), (27, 20, 1.0));
    let centre = img.patient_to_pixel(DVec3::ZERO);
    assert!((centre.0 - 19.0 * h).abs() < 1e-9 && (centre.1 - 9.5).abs() < 1e-9);
    // 8.5 mm to the right of the centre (6 mm left and 6 mm anterior), 6 mm above it.
    let expected = (centre.0 + 12.0 * h, centre.1 - 6.0);
    let computed = img.patient_to_pixel(MARKER);
    assert!((computed.0 - expected.0).abs() < 1e-9 && (computed.1 - expected.1).abs() < 1e-9, "{computed:?}");
    assert_marker_at(&img, expected, 0.75, "left-anterior plane");
    assert_pixels_match_their_positions(&volume, &img, "left-anterior plane");

    // Turned the other way the plane through the centre misses the marker by 8.5 mm;
    // moved to the marker, it shows it on its vertical centre line.
    let basis = PlaneBasis::tilted(MprAxis::Coronal, about(DVec3::Z, 45.0));
    assert!((basis.right - DVec3::new(h, h, 0.0)).length() < 1e-12);
    let through_centre = reslice_plane(&volume, &basis, 0.0, None, 512);
    assert_eq!(marker_centre(&through_centre), None);
    let position = MARKER.dot(basis.normal);
    assert!((position + 12.0 * h).abs() < 1e-12);
    let img = reslice_plane(&volume, &basis, position, None, 512);
    let centre = img.patient_to_pixel(DVec3::ZERO);
    assert_marker_at(&img, (centre.0, centre.1 - 6.0), 0.75, "left-posterior plane");
    assert_pixels_match_their_positions(&volume, &img, "left-posterior plane");
}

#[test]
fn marker_appears_where_geometry_says_on_double_oblique_planes() {
    let rotation = double_oblique();
    for (name, volume) in [("axial", axial()), ("sagittal", sagittal())] {
        for axis in MprAxis::ALL {
            let basis = PlaneBasis::tilted(axis, rotation);
            assert!(basis.tilt_degrees(axis) > 20.0, "{axis:?} is tilted");
            let what = format!("{name} acquisition, {axis:?} view");
            let img = reslice_plane(&volume, &basis, MARKER.dot(basis.normal), None, 512);
            assert_eq!((img.right, img.down), (basis.right, basis.down));
            assert_marker_at(&img, img.patient_to_pixel(MARKER), 0.75, &what);
            assert_pixels_match_their_positions(&volume, &img, &what);
            // The marker is to the patient's left, anterior and superior of the centre:
            // on screen it lies on the side the basis says.
            let (mx, my) = img.patient_to_pixel(MARKER);
            let (cx, cy) = img.patient_to_pixel(DVec3::ZERO);
            assert_eq!(mx > cx, MARKER.dot(basis.right) > 0.0, "{what}");
            assert_eq!(my > cy, MARKER.dot(basis.down) > 0.0, "{what}");
        }
    }
}

#[test]
fn pixel_patient_round_trip_on_oblique_images() {
    let volume = anisotropic();
    for axis in MprAxis::ALL {
        let basis = PlaneBasis::tilted(axis, double_oblique());
        let position = 1.75;
        let img = reslice_plane(&volume, &basis, position, None, 512);
        for (x, y) in [(0.0, 0.0), (3.25, 4.5), (img.width as f64 - 1.0, img.height as f64 - 1.0), (-2.0, 40.0)] {
            let p = img.pixel_to_patient(x, y);
            // Every pixel lies on the plane…
            assert!((p.dot(basis.normal) - position).abs() < 1e-9, "{axis:?} ({x}, {y})");
            // …and maps back to itself.
            let (bx, by) = img.patient_to_pixel(p);
            assert!((bx - x).abs() < 1e-9 && (by - y).abs() < 1e-9, "{axis:?} ({x}, {y}) -> ({bx}, {by})");
            // A point off the plane maps to the pixel straight below it along the normal.
            let (ox, oy) = img.patient_to_pixel(p + basis.normal * 7.0);
            assert!((ox - x).abs() < 1e-9 && (oy - y).abs() < 1e-9);
        }
        // One pixel is `spacing` mm, in both directions.
        let step = img.pixel_to_patient(1.0, 0.0) - img.pixel_to_patient(0.0, 0.0);
        assert!((step - basis.right * img.spacing).length() < 1e-12);
        let step = img.pixel_to_patient(0.0, 1.0) - img.pixel_to_patient(0.0, 0.0);
        assert!((step - basis.down * img.spacing).length() < 1e-12);
    }
}

#[test]
fn slab_mip_projects_along_the_rotated_normal() {
    let volume = axial();
    let basis = PlaneBasis::tilted(MprAxis::Coronal, about(DVec3::Z, -45.0));
    // A plane 6 mm from the marker along the tilted normal.
    let position = MARKER.dot(basis.normal) + 6.0;
    let thin = reslice_plane(&volume, &basis, position, None, 512);
    assert_eq!(marker_centre(&thin), None, "a thin slice 6 mm away misses the marker");

    let slab = |mode| Some(Slab { mode, thickness_mm: 16.0 });
    let mip = reslice_plane(&volume, &basis, position, slab(SlabMode::Mip), 512);
    // The marker shows where the tilted normal through it meets the plane…
    let along_tilted_normal = mip.patient_to_pixel(MARKER);
    assert_marker_at(&mip, along_tilted_normal, 0.75, "MIP slab");
    // …not where the normal of the standard coronal plane would put it (6 pixels away).
    let coronal = MprAxis::Coronal.normal();
    let t = (position - MARKER.dot(basis.normal)) / coronal.dot(basis.normal);
    let along_standard_normal = mip.patient_to_pixel(MARKER + coronal * t);
    assert!((along_standard_normal.0 - along_tilted_normal.0).abs() > 5.9);
    let (x, _) = marker_centre(&mip).expect("marker");
    assert!((x - along_standard_normal.0).abs() > 5.0);

    // The average over the slab holds a part of the marker; the minimum none of it.
    let mean = reslice_plane(&volume, &basis, position, slab(SlabMode::Mean), 512);
    let (x, y) = (along_tilted_normal.0.round() as usize, along_tilted_normal.1.round() as usize);
    let (peak, background) = (mean.value_at(x, y).expect("inside"), mean.value_at(2, 10).expect("inside"));
    assert!(peak > 200.0 && peak < 900.0 && background < 50.0, "peak {peak}, background {background}");
    let min = reslice_plane(&volume, &basis, position, slab(SlabMode::MinIp), 512);
    assert_eq!(marker_centre(&min), None);
    // A slab thinner than the distance does not reach the marker.
    let narrow = reslice_plane(&volume, &basis, position, Some(Slab { mode: SlabMode::Mip, thickness_mm: 4.0 }), 512);
    assert_eq!(marker_centre(&narrow), None);
}

#[test]
fn gantry_tilted_volume_is_resliced_without_distortion() {
    let volume = gantry_tilted();
    // Standard planes: the marker is where the radiological convention puts it.
    for axis in MprAxis::ALL {
        let img = reslice(&volume, axis, MARKER.dot(axis.normal()), None, 512);
        assert_marker_at(&img, img.patient_to_pixel(MARKER), 0.75, &format!("{axis:?}"));
    }
    // Tilted planes, single and double oblique.
    for rotation in [about(DVec3::X, 20.0), about(DVec3::Z, -45.0), double_oblique()] {
        for axis in MprAxis::ALL {
            let basis = PlaneBasis::tilted(axis, rotation);
            let what = format!("{axis:?} view turned by {rotation:?}");
            let img = reslice_plane(&volume, &basis, MARKER.dot(basis.normal), None, 512);
            assert_marker_at(&img, img.patient_to_pixel(MARKER), 0.75, &what);
            assert_pixels_match_their_positions(&volume, &img, &what);
            // The sphere stays round: as wide as it is high (5 mm across, 1 mm pixels).
            let bright: Vec<(usize, usize)> = (0..img.values.len())
                .filter(|&i| img.values[i] > MARKER_VALUE / 2.0)
                .map(|i| (i % img.width, i / img.width))
                .collect();
            let span = |f: fn(&(usize, usize)) -> usize| {
                let (lo, hi) = bright.iter().map(f).fold((usize::MAX, 0), |(lo, hi), v| (lo.min(v), hi.max(v)));
                (hi - lo) as f64 * img.spacing
            };
            let (w, h) = (span(|p| p.0), span(|p| p.1));
            assert!((w - h).abs() <= 1.5 && (3.0..=6.0).contains(&w), "{what}: marker {w} × {h} mm");
        }
    }
}

#[test]
fn extent_of_a_tilted_plane_is_the_bounding_box_of_the_corners() {
    let volume = axial();
    let g = &volume.geometry;
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let basis = PlaneBasis::tilted(MprAxis::Coronal, about(DVec3::Z, 45.0));
    let ext = MprExtent::of_plane(g, &basis);
    // The 19 mm square seen along its diagonal: 19·√2 wide and deep, 19 high.
    for (range, half) in [(ext.u, 19.0 * h), (ext.v, 9.5), (ext.n, 19.0 * h)] {
        assert!((range.0 + half).abs() < 1e-9 && (range.1 - half).abs() < 1e-9, "{range:?}");
    }
    // Slices follow the tilted normal: one step is the largest projection of a voxel edge.
    assert!((ext.step - h).abs() < 1e-12);
    assert_eq!(ext.slice_count(), 39);
    assert_eq!(ext.index_of(ext.position(17)), 17);
    assert!((ext.position(1) - ext.position(0) - h).abs() < 1e-12);

    // Every corner of the volume is inside the picture of every tilted view.
    for axis in MprAxis::ALL {
        let basis = PlaneBasis::tilted(axis, double_oblique());
        let ext = MprExtent::of_plane(g, &basis);
        let img = reslice_plane(&volume, &basis, 0.0, None, 512);
        for corner in g.corners() {
            let (x, y) = img.patient_to_pixel(corner);
            assert!((-1e-9..=(img.width - 1) as f64 + 1.0).contains(&x), "{axis:?} x {x}");
            assert!((-1e-9..=(img.height - 1) as f64 + 1.0).contains(&y), "{axis:?} y {y}");
            let n = corner.dot(basis.normal);
            assert!(n >= ext.n.0 - 1e-9 && n <= ext.n.1 + 1e-9);
        }
        // A large volume is reduced to the pixel limit, as for the standard planes.
        let small = reslice_plane(&volume, &basis, 0.0, None, 16);
        assert!(small.width <= 16 && small.height <= 16 && small.spacing > 1.0);
    }
}
