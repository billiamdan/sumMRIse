//! Tests of the ray caster with synthetic volumes: marker volumes built here
//! and the CT phantom of the testkit.

use glam::{DQuat, DVec3};
use summrise_testkit::{CT_BONE_AXIS_MM, CT_VESSEL_AXIS_MM, CtPhantom, PhantomOptions};

use super::*;
use crate::volume::{Volume, VolumeGeometry, VoxelBuffer};
use crate::window::Window;

const SIZE: [usize; 2] = [96, 96];

fn never() -> bool {
    false
}

/// A marker towards the patient's left and one towards the head (mm, LPS).
const LEFT_MARKER: DVec3 = DVec3::new(12.0, 0.0, 0.0);
const HEAD_MARKER: DVec3 = DVec3::new(0.0, 0.0, 12.0);

/// 40 × 40 × 40 voxels of 1 mm centred on the patient origin, 0 everywhere
/// but in spheres of radius 3.5 mm around `markers` (position, value). With
/// `shear_degrees` the slices are stacked along a direction tilted by that
/// angle from the slice normal, as with CT gantry tilt.
fn marker_volume(markers: &[(DVec3, f32)], shear_degrees: f64) -> Volume {
    let n = 40usize;
    let (sin, cos) = shear_degrees.to_radians().sin_cos();
    let axes = [DVec3::X, DVec3::Y, DVec3::new(0.0, sin, cos)];
    let origin = -(axes[0] + axes[1] + axes[2]) * ((n - 1) as f64 / 2.0);
    let geometry = VolumeGeometry::new([n, n, n], origin, axes).expect("geometry");
    let slices = (0..n)
        .map(|k| {
            let values = (0..n * n)
                .map(|index| {
                    let p = geometry.to_patient(DVec3::new((index % n) as f64, (index / n) as f64, k as f64));
                    markers.iter().find(|(at, _)| (p - *at).length() < 3.5).map_or(0.0, |(_, value)| *value)
                })
                .collect();
            VoxelBuffer::compact(values)
        })
        .collect();
    Volume::new(geometry, slices).expect("volume")
}

/// The CT phantom of the testkit, in memory, in Hounsfield units.
fn ct(size: u32, slices: u32) -> Volume {
    let phantom = CtPhantom::new(&PhantomOptions { ct_size: size, ct_slices: slices, ..PhantomOptions::small() });
    let [sx, sy, sz] = phantom.spacing();
    let geometry = VolumeGeometry::new(
        phantom.dims(),
        DVec3::from_array(phantom.origin()),
        [DVec3::X * sx, DVec3::Y * sy, DVec3::Z * sz],
    )
    .expect("geometry");
    let slices = (0..phantom.dims()[2]).map(|k| VoxelBuffer::I16(phantom.slice(k))).collect();
    Volume::new(geometry, slices).expect("volume")
}

/// An MR-like volume without Hounsfield units: a body of signal 320 with a
/// brighter inner region (820) and a marker of 1500 at L 12 A 8 S 10.
fn mr_like() -> Volume {
    let n = 36usize;
    let axes = [DVec3::X * 2.0, DVec3::Y * 2.0, DVec3::Z * 3.0];
    let origin = -(axes[0] + axes[1] + axes[2]) * ((n - 1) as f64 / 2.0);
    let geometry = VolumeGeometry::new([n, n, n], origin, axes).expect("geometry");
    let inside = |p: DVec3, c: DVec3, r: DVec3| ((p - c) / r).length_squared() <= 1.0;
    let slices = (0..n)
        .map(|k| {
            let values = (0..n * n)
                .map(|index| {
                    let p = geometry.to_patient(DVec3::new((index % n) as f64, (index / n) as f64, k as f64));
                    if (p - DVec3::new(12.0, -8.0, 10.0)).length() < 5.0 {
                        1500.0
                    } else if inside(p, DVec3::new(0.0, 0.0, 8.0), DVec3::new(15.0, 12.0, 14.0)) {
                        820.0
                    } else if inside(p, DVec3::ZERO, DVec3::new(30.0, 25.0, 45.0)) {
                        320.0
                    } else {
                        0.0
                    }
                })
                .collect();
            VoxelBuffer::compact(values)
        })
        .collect();
    Volume::new(geometry, slices).expect("volume")
}

fn style(look: Look, window: Window) -> Style {
    Style { look, window, invert: false }
}

/// The values from 0 to 1000 mapped linearly onto black to white.
fn mip_window() -> Window {
    Window::from_range(0.0, 1000.0)
}

fn draw(volume: &RenderVolume, camera: &Camera, style: &Style, size: [usize; 2], quality: Quality) -> Rendering {
    render(volume, camera, style, size, quality, &never).expect("not cancelled")
}

/// Centroid of the pixels whose red channel is at least `threshold`.
fn centroid(picture: &Rendering, threshold: u8) -> (f64, f64) {
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for y in 0..picture.height {
        for x in 0..picture.width {
            if picture.pixel(x, y).expect("pixel")[0] >= threshold {
                (sx, sy, n) = (sx + x as f64, sy + y as f64, n + 1.0);
            }
        }
    }
    assert!(n > 0.0, "nothing at or above {threshold}");
    (sx / n, sy / n)
}

fn letters(camera: &Camera) -> [String; 4] {
    let l = camera.edge_labels();
    [l.left, l.right, l.top, l.bottom]
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-9
}

// ---------------------------------------------------------------------------
// Camera and orientation

#[test]
fn the_first_view_is_from_the_front_with_the_head_up() {
    let volume = marker_volume(&[], 0.0);
    let camera = Camera::fit(&volume.geometry, DQuat::IDENTITY);
    // Bit for bit the coronal basis: screen-right is the patient's left, screen-down
    // the feet, and the view runs from the front to the back.
    let (right, down, normal) = MprAxis::Coronal.basis();
    assert_eq!(camera.basis(), PlaneBasis { right, down, normal });
    assert_eq!(letters(&camera), ["R", "L", "S", "I"]);
    // The patient's right is on the left of the screen, as in the coronal view.
    assert_eq!(camera.edge_labels(), MprAxis::Coronal.edge_labels());
}

#[test]
fn a_marker_on_the_left_appears_right_of_centre_and_one_towards_the_head_above_it() {
    let left = RenderVolume::new(&marker_volume(&[(LEFT_MARKER, 1000.0)], 0.0));
    let head = RenderVolume::new(&marker_volume(&[(HEAD_MARKER, 1000.0)], 0.0));
    let mip = style(Look::Mip, mip_window());
    let front = Camera::fit(&left.volume().geometry, DQuat::IDENTITY);
    let mm = front.mm_per_pixel(SIZE);
    let centre = (SIZE[0] as f64 / 2.0 - 0.5, SIZE[1] as f64 / 2.0 - 0.5);

    // From the front: the left marker 12 mm to the right of the centre, the head marker 12 mm above it.
    let (x, y) = centroid(&draw(&left, &front, &mip, SIZE, Quality::Full), 128);
    assert!(((x - centre.0) * mm - 12.0).abs() < 0.6 && ((y - centre.1) * mm).abs() < 0.6, "left marker at ({x}, {y})");
    let (x, y) = centroid(&draw(&head, &front, &mip, SIZE, Quality::Full), 128);
    assert!(((x - centre.0) * mm).abs() < 0.6 && ((centre.1 - y) * mm - 12.0).abs() < 0.6, "head marker at ({x}, {y})");

    // Turned by 180° about the vertical axis: the left marker is on the left, the head
    // marker still above the centre, and left and right have swapped their letters.
    let back = Camera { orientation: turned(DQuat::IDENTITY, 180.0, 0.0), ..front };
    assert_eq!(letters(&back), ["L", "R", "S", "I"]);
    let (x, y) = centroid(&draw(&left, &back, &mip, SIZE, Quality::Full), 128);
    assert!(((centre.0 - x) * mm - 12.0).abs() < 0.6 && ((y - centre.1) * mm).abs() < 0.6, "left marker at ({x}, {y})");
    let (_, y) = centroid(&draw(&head, &back, &mip, SIZE, Quality::Full), 128);
    assert!(((centre.1 - y) * mm - 12.0).abs() < 0.6, "head marker at y {y}");
    // The view from the back is the standard one.
    assert!(close(back.basis().right, StandardView::Back.orientation() * DVec3::X));
}

#[test]
fn standard_views_look_from_the_six_sides_with_true_letters() {
    let volume = marker_volume(&[], 0.0);
    let expected = [
        (StandardView::Front, ["R", "L", "S", "I"], DVec3::Y),
        (StandardView::Back, ["L", "R", "S", "I"], -DVec3::Y),
        (StandardView::Left, ["A", "P", "S", "I"], -DVec3::X),
        (StandardView::Right, ["P", "A", "S", "I"], DVec3::X),
        (StandardView::Top, ["L", "R", "A", "P"], -DVec3::Z),
        (StandardView::Bottom, ["R", "L", "A", "P"], DVec3::Z),
    ];
    for (view, edges, looking) in expected {
        let camera = Camera::fit(&volume.geometry, view.orientation());
        let basis = camera.basis();
        assert_eq!(letters(&camera), edges, "{view:?}");
        assert!(close(basis.normal, looking), "{view:?} looks along {:?}", basis.normal);
        // A proper rotation: orthonormal and right-handed, never mirrored.
        assert!(close(basis.right.cross(basis.down), basis.normal), "{view:?}");
        assert!((basis.right.length() - 1.0).abs() < 1e-12 && basis.right.dot(basis.down).abs() < 1e-12);
    }
    // The view from the left is the sagittal view, the one from below the feet the axial view.
    assert_eq!(
        Camera::fit(&volume.geometry, StandardView::Left.orientation()).edge_labels(),
        MprAxis::Sagittal.edge_labels()
    );
    assert_eq!(
        Camera::fit(&volume.geometry, StandardView::Bottom.orientation()).edge_labels(),
        MprAxis::Axial.edge_labels()
    );
}

#[test]
fn dragging_turns_the_near_side_with_the_pointer() {
    let basis = |q: DQuat| PlaneBasis::rotated(MprAxis::Coronal, q);
    // A quarter turn to the right brings the patient's right side to the front.
    let right = basis(turned(DQuat::IDENTITY, 90.0, 0.0));
    let expected = basis(StandardView::Right.orientation());
    assert!(close(right.right, expected.right) && close(right.down, expected.down), "{right:?}");
    let left = basis(turned(DQuat::IDENTITY, -90.0, 0.0));
    assert!(close(left.normal, -DVec3::X) && close(left.right, DVec3::Y));
    // Dragging down by 90° tips the head towards the viewer: seen from above the head,
    // with the front of the body at the bottom of the screen.
    let tipped = basis(turned(DQuat::IDENTITY, 0.0, 90.0));
    assert!(close(tipped.normal, -DVec3::Z) && close(tipped.down, -DVec3::Y), "{tipped:?}");
    // Turning on from a turned view uses that view's screen axes.
    let twice = basis(turned(turned(DQuat::IDENTITY, 90.0, 0.0), 90.0, 0.0));
    assert!(close(twice.right, -DVec3::X) && close(twice.normal, -DVec3::Y));
    // Nothing to turn, and nonsense, leave the orientation alone; an orientation that
    // is no rotation turns from the first view.
    let q = turned(DQuat::IDENTITY, 30.0, 10.0);
    assert_eq!(turned(q, 0.0, 0.0), q);
    assert_eq!(turned(q, f64::NAN, 0.0), q);
    let from_nothing = basis(turned(DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0), 90.0, 0.0));
    assert!(close(from_nothing.right, expected.right) && close(from_nothing.normal, expected.normal));
}

#[test]
fn camera_fits_the_bounding_sphere_and_maps_positions_to_pixels() {
    let volume = marker_volume(&[], 0.0);
    let camera = Camera::fit(&volume.geometry, DQuat::IDENTITY);
    // 40 voxels of 1 mm fill a cube of 40 mm: its diagonal is 69.3 mm.
    assert!((camera.field_mm - 40.0 * 3f64.sqrt()).abs() < 1e-9, "{}", camera.field_mm);
    assert!(close(camera.center, DVec3::ZERO));
    assert_eq!(camera.mm_per_pixel([200, 100]), camera.field_mm / 100.0);
    assert_eq!(camera.to_pixel(camera.center, [200, 100]), (99.5, 49.5));
    let (x, y) = camera.to_pixel(DVec3::new(10.0, 5.0, 10.0), [200, 100]);
    assert!((x - (99.5 + 10.0 / camera.mm_per_pixel([200, 100]))).abs() < 1e-9);
    assert!((y - (49.5 - 10.0 / camera.mm_per_pixel([200, 100]))).abs() < 1e-9);
}

#[test]
fn a_gantry_tilted_volume_shows_its_marker_where_it_is() {
    let marker = DVec3::new(8.0, -6.0, 10.0);
    let volume = RenderVolume::new(&marker_volume(&[(marker, 1000.0)], 20.0));
    assert!((volume.volume().geometry.shear_degrees() - 20.0).abs() < 1e-9);
    let mip = style(Look::Mip, mip_window());
    for orientation in [DQuat::IDENTITY, turned(DQuat::IDENTITY, 35.0, -25.0), StandardView::Left.orientation()] {
        let camera = Camera::fit(&volume.volume().geometry, orientation);
        let (x, y) = centroid(&draw(&volume, &camera, &mip, SIZE, Quality::Full), 128);
        let (ex, ey) = camera.to_pixel(marker, SIZE);
        assert!((x - ex).abs() < 1.0 && (y - ey).abs() < 1.0, "{orientation:?}: at ({x}, {y}), expected ({ex}, {ey})");
    }
}

// ---------------------------------------------------------------------------
// Looks

/// The pixel through which the ray of `camera` through the patient position `p` passes.
fn pixel_of(camera: &Camera, p: DVec3) -> (usize, usize) {
    let (x, y) = camera.to_pixel(p, SIZE);
    (x.round() as usize, y.round() as usize)
}

/// The pixel of a front view through which the ray at patient x and z passes.
fn front_pixel(camera: &Camera, x: f64, z: f64) -> (usize, usize) {
    pixel_of(camera, DVec3::new(x, 0.0, z))
}

/// Seen from the patient's left side, the vessel (in front of the spine) and the spine
/// lie side by side: the ray through the vessel axis meets no bone.
fn side_pixel(camera: &Camera, axis: [f64; 2]) -> (usize, usize) {
    pixel_of(camera, DVec3::new(0.0, axis[1], 0.0))
}

#[test]
fn bone_shows_the_bone_and_leaves_out_soft_tissue_and_the_vessel() {
    let volume = RenderVolume::new(&ct(64, 32));
    let bone = style(Look::Bone, Window::new(40.0, 400.0));
    let front = Camera::fit(&volume.volume().geometry, DQuat::IDENTITY);
    let picture = draw(&volume, &front, &bone, SIZE, Quality::Full);
    let at = |x: f64, z: f64| {
        let (px, py) = front_pixel(&front, x, z);
        picture.pixel(px, py).expect("inside")
    };
    // Through the spine-like cylinder of 700 HU: opaque and ivory.
    let spine = at(CT_BONE_AXIS_MM[0], 0.0);
    assert!(spine[3] >= 250 && spine[0] > 120 && spine[0].abs_diff(spine[1]) < 40, "spine {spine:?}");
    // Through soft tissue (40 HU) and a lung (-820 HU) only: background.
    assert_eq!(at(-70.0, 0.0), [0, 0, 0, 0], "soft tissue");
    // Air around the body.
    assert_eq!(at(-110.0, 0.0), [0, 0, 0, 0], "air");
    // From the side, through the contrast-filled vessel of 300 HU: not bone.
    let side = Camera::fit(&volume.volume().geometry, StandardView::Left.orientation());
    let picture = draw(&volume, &side, &bone, SIZE, Quality::Full);
    let (x, y) = side_pixel(&side, CT_VESSEL_AXIS_MM);
    assert_eq!(picture.pixel(x, y), Some([0, 0, 0, 0]), "vessel");
    let (x, y) = side_pixel(&side, CT_BONE_AXIS_MM);
    assert!(picture.pixel(x, y).expect("pixel")[3] >= 250, "spine from the side");
}

#[test]
fn skin_shows_the_outline_of_the_body() {
    let volume = RenderVolume::new(&ct(64, 32));
    let camera = Camera::fit(&volume.volume().geometry, DQuat::IDENTITY);
    let skin = draw(&volume, &camera, &style(Look::Skin, Window::new(40.0, 400.0)), SIZE, Quality::Full);
    let at = |x: f64, z: f64| {
        let (px, py) = front_pixel(&camera, x, z);
        skin.pixel(px, py).expect("inside")
    };
    for (x, z) in [(-70.0, 0.0), (0.0, 0.0), (60.0, -80.0), (-90.0, 90.0)] {
        assert!(at(x, z)[3] >= 250, "body at x {x}, z {z}: {:?}", at(x, z));
    }
    // The body is 200 mm wide: beside it there is only air.
    for (x, z) in [(-110.0, 0.0), (110.0, 40.0)] {
        assert_eq!(at(x, z), [0, 0, 0, 0], "air at x {x}, z {z}");
    }
    // Skin coloured: more red than blue.
    let body = at(-70.0, 0.0);
    assert!(body[0] > body[2] + 30, "{body:?}");
}

#[test]
fn vessels_and_bone_shows_the_vessel_red_and_the_bone_ivory() {
    let volume = RenderVolume::new(&ct(64, 32));
    let look = style(Look::VesselsAndBone, Window::new(40.0, 400.0));
    let side = Camera::fit(&volume.volume().geometry, StandardView::Left.orientation());
    let picture = draw(&volume, &side, &look, SIZE, Quality::Full);
    let at = |axis: [f64; 2]| {
        let (x, y) = side_pixel(&side, axis);
        picture.pixel(x, y).expect("inside")
    };
    let green_share = |px: [u8; 4]| f64::from(px[1]) / f64::from(px[0].max(1));
    let vessel = at(CT_VESSEL_AXIS_MM);
    assert!(vessel[3] >= 200 && green_share(vessel) < 0.5, "vessel {vessel:?}");
    // The bone is ivory; its edge passes through the vessel range and is tinted, but
    // the bone is clearly less red than the vessel.
    let spine = at(CT_BONE_AXIS_MM);
    assert!(spine[3] >= 250 && green_share(spine) > green_share(vessel) + 0.25, "spine {spine:?}, vessel {vessel:?}");
    let front = Camera::fit(&volume.volume().geometry, DQuat::IDENTITY);
    let picture = draw(&volume, &front, &look, SIZE, Quality::Full);
    let (x, y) = front_pixel(&front, -70.0, 0.0);
    assert_eq!(picture.pixel(x, y), Some([0, 0, 0, 0]), "soft tissue and lung");
}

#[test]
fn mip_is_the_largest_value_along_each_ray() {
    let marker = marker_volume(&[(LEFT_MARKER, 1000.0), (HEAD_MARKER, 600.0)], 0.0);
    let ct = ct(48, 24);
    for (volume, window) in [(&marker, mip_window()), (&ct, Window::new(300.0, 1600.0))] {
        let prepared = RenderVolume::new(volume);
        for orientation in [DQuat::IDENTITY, turned(DQuat::IDENTITY, 40.0, 25.0)] {
            let camera = Camera::fit(&volume.geometry, orientation);
            let mip = style(Look::Mip, window);
            let picture = draw(&prepared, &camera, &mip, SIZE, Quality::Full);
            let setup = Setup::new(&prepared, &camera, &mip, SIZE, Quality::Full, Shortcuts::ALL);
            let mut compared = 0;
            for y in (0..SIZE[1]).step_by(3) {
                for x in (0..SIZE[0]).step_by(3) {
                    let start = setup.start(x, y);
                    // Brute force: every sample of the ray through `Volume::sample`.
                    let reference = setup.samples(start).map(|(first, last)| {
                        (first..=last)
                            .map(|k| start + setup.direction * (k as f32 * setup.step))
                            .filter_map(|p| volume.sample(p.as_dvec3()))
                            .fold(f32::NEG_INFINITY, f32::max)
                    });
                    let found = setup.mip(&Voxels::new(volume), start, f32::INFINITY);
                    match (reference, found) {
                        (Some(expected), Some(max)) => {
                            assert!((expected - max).abs() < 0.05, "({x}, {y}): {max} instead of {expected}");
                            let grey = (window.apply(f64::from(expected)).clamp(0.0, 1.0) * 255.0).round() as u8;
                            let shown = picture.pixel(x, y).expect("pixel");
                            assert!(shown[0].abs_diff(grey) <= 1 && shown[3] == 255, "({x}, {y}): {shown:?}, {grey}");
                            compared += 1;
                        }
                        (None, None) => assert_eq!(picture.pixel(x, y), Some([0; 4])),
                        other => panic!("({x}, {y}): {other:?}"),
                    }
                }
            }
            // The volume covers about a third of the picture.
            assert!(compared > 250, "{compared} rays compared");
        }
    }
    // Through the marker the projection is white.
    let prepared = RenderVolume::new(&marker);
    let camera = Camera::fit(&marker.geometry, DQuat::IDENTITY);
    let picture = draw(&prepared, &camera, &style(Look::Mip, mip_window()), SIZE, Quality::Full);
    let (x, y) = camera.to_pixel(LEFT_MARKER, SIZE);
    assert_eq!(picture.pixel(x.round() as usize, y.round() as usize), Some([255, 255, 255, 255]));
}

#[test]
fn mip_follows_the_window_and_can_be_inverted() {
    let volume = RenderVolume::new(&marker_volume(&[(LEFT_MARKER, 1000.0), (HEAD_MARKER, 600.0)], 0.0));
    let camera = Camera::fit(&volume.volume().geometry, DQuat::IDENTITY);
    let (x, y) = camera.to_pixel(HEAD_MARKER, SIZE);
    let (x, y) = (x.round() as usize, y.round() as usize);
    let grey = |window: Window, invert: bool| {
        let picture = draw(&volume, &camera, &Style { look: Look::Mip, window, invert }, SIZE, Quality::Full);
        picture.pixel(x, y).expect("pixel")[0]
    };
    // 600 in a window from 0 to 1000 is 60 % grey, in one from 0 to 600 white.
    assert_eq!(grey(mip_window(), false), 153);
    assert_eq!(grey(Window::from_range(0.0, 600.0), false), 255);
    assert_eq!(grey(mip_window(), true), 102);
}

#[test]
fn surface_opacity_follows_the_window() {
    let volume = RenderVolume::new(&mr_like());
    let camera = Camera::fit(&volume.volume().geometry, DQuat::IDENTITY);
    let alpha = |window: Window| {
        let picture = draw(&volume, &camera, &style(Look::Surface, window), SIZE, Quality::Full);
        let (x, y) = camera.to_pixel(DVec3::new(-20.0, 0.0, -20.0), SIZE);
        picture.pixel(x.round() as usize, y.round() as usize).expect("pixel")[3]
    };
    // A window from 0 to 400 makes the body (320) nearly opaque; one from 1000 to 2000
    // leaves it out, so that only the marker (1500) remains.
    assert!(alpha(Window::from_range(0.0, 400.0)) >= 250);
    assert_eq!(alpha(Window::from_range(1000.0, 2000.0)), 0);
    let picture =
        draw(&volume, &camera, &style(Look::Surface, Window::from_range(1000.0, 2000.0)), SIZE, Quality::Full);
    let (x, y) = camera.to_pixel(DVec3::new(12.0, -8.0, 10.0), SIZE);
    assert!(picture.pixel(x.round() as usize, y.round() as usize).expect("pixel")[3] >= 250, "the marker");
}

#[test]
fn looks_on_offer_depend_on_hounsfield_units() {
    assert_eq!(Look::available(true), [Look::Bone, Look::Skin, Look::VesselsAndBone, Look::Mip]);
    assert_eq!(Look::available(false), [Look::Surface, Look::Mip]);
    assert_eq!((Look::default_for(true), Look::default_for(false)), (Look::Bone, Look::Surface));
    let following: Vec<Look> = [Look::Bone, Look::Skin, Look::VesselsAndBone, Look::Surface, Look::Mip]
        .into_iter()
        .filter(|look| look.follows_window())
        .collect();
    assert_eq!(following, [Look::Surface, Look::Mip]);
    // The documented ranges: Bone from 300 HU, fully 0.9 per mm at 500 HU.
    assert_eq!(piecewise(Look::Bone.points(), 300.0).0, 0.0);
    assert_eq!(piecewise(Look::Bone.points(), 500.0).0, 0.9);
    assert!((piecewise(Look::Bone.points(), 400.0).0 - 0.45).abs() < 1e-6);
    assert_eq!(piecewise(Look::Skin.points(), -600.0).0, 0.0);
    assert_eq!(piecewise(Look::Skin.points(), 40.0).0, 0.9);
    assert_eq!(piecewise(Look::VesselsAndBone.points(), 150.0).0, 0.0);
    assert!((piecewise(Look::VesselsAndBone.points(), 300.0).0 - 0.2833).abs() < 1e-3);
    assert_eq!(piecewise(Look::VesselsAndBone.points(), 450.0).0, 0.9);
}

// ---------------------------------------------------------------------------
// Shortcuts, determinism, cancellation

#[test]
fn skipping_empty_space_and_stopping_early_do_not_change_the_picture() {
    let plain = Shortcuts { skip_empty: false, stop_when_opaque: false };
    let skipping = Shortcuts { skip_empty: true, stop_when_opaque: false };
    let ct = RenderVolume::new(&ct(48, 24));
    let mr = RenderVolume::new(&mr_like());
    let mut cases = Vec::new();
    for look in [Look::Bone, Look::Skin, Look::VesselsAndBone, Look::Mip] {
        cases.push((&ct, style(look, Window::new(300.0, 1600.0))));
    }
    for look in [Look::Surface, Look::Mip] {
        cases.push((&mr, style(look, Window::new(700.0, 1400.0))));
    }
    let mut skipped_somewhere = false;
    for (volume, style) in cases {
        for orientation in [DQuat::IDENTITY, turned(DQuat::IDENTITY, 35.0, -20.0), StandardView::Top.orientation()] {
            let camera = Camera::fit(&volume.volume().geometry, orientation);
            for quality in [Quality::Preview, Quality::Full] {
                let draw = |shortcuts| {
                    render_with(volume, &camera, &style, [64, 48], quality, shortcuts, &never).expect("picture")
                };
                let reference = draw(plain);
                // Skipping blocks in which the look sees nothing changes no bit.
                assert_eq!(draw(skipping), reference, "{:?} {orientation:?} {quality:?}", style.look);
                // Stopping when a ray is opaque changes a channel by one grey level at most.
                let fast = draw(Shortcuts::ALL);
                let worst = fast.rgba.iter().zip(&reference.rgba).map(|(a, b)| a.abs_diff(*b)).max();
                assert!(worst <= Some(1), "{:?} {orientation:?} {quality:?}: {worst:?}", style.look);
            }
        }
        skipped_somewhere |= match &Setup::new(
            volume,
            &Camera::fit(&volume.volume().geometry, DQuat::IDENTITY),
            &style,
            [8, 8],
            Quality::Full,
            Shortcuts::ALL,
        )
        .kind
        {
            Kind::Composite { empty, .. } => empty.iter().any(|e| *e) && !empty.iter().all(|e| *e),
            Kind::Mip { .. } => false,
        };
    }
    assert!(skipped_somewhere, "the phantoms have empty and non-empty blocks");
}

#[test]
fn two_renders_are_identical_whatever_the_number_of_threads() {
    let volume = RenderVolume::new(&ct(48, 24));
    let camera = Camera::fit(&volume.volume().geometry, turned(DQuat::IDENTITY, 30.0, 15.0));
    for look in [Look::VesselsAndBone, Look::Mip] {
        let style = style(look, Window::new(300.0, 1600.0));
        let first = draw(&volume, &camera, &style, SIZE, Quality::Full);
        assert_eq!(draw(&volume, &camera, &style, SIZE, Quality::Full), first);
        let single = rayon::ThreadPoolBuilder::new().num_threads(1).build().expect("thread pool");
        assert_eq!(single.install(|| draw(&volume, &camera, &style, SIZE, Quality::Full)), first);
        assert!(first.rgba.chunks(4).any(|px| px[3] > 0), "something is drawn");
    }
}

#[test]
fn a_cancelled_render_gives_no_picture() {
    let volume = RenderVolume::new(&ct(48, 24));
    let camera = Camera::fit(&volume.volume().geometry, DQuat::IDENTITY);
    let style = style(Look::Bone, Window::new(40.0, 400.0));
    assert_eq!(render(&volume, &camera, &style, SIZE, Quality::Full, &|| true), None);
    // Degenerate requests give an empty picture instead of failing.
    let empty = draw(&volume, &camera, &style, [0, 10], Quality::Full);
    assert_eq!((empty.width, empty.height, empty.rgba.len()), (0, 10, 0));
    let broken = Camera { field_mm: f64::NAN, ..camera };
    assert!(draw(&volume, &broken, &style, [4, 4], Quality::Full).rgba.iter().all(|&b| b == 0));
}

#[test]
fn rays_take_a_bounded_number_of_steps() {
    // Voxels of 0.01 mm across but slices 50 mm apart: half a voxel per step would be
    // about 80 000 steps along a ray through the 400 mm long volume.
    let axes = [DVec3::X * 0.01, DVec3::Y * 0.01, DVec3::Z * 50.0];
    let geometry = VolumeGeometry::new([4, 4, 8], DVec3::ZERO, axes).expect("geometry");
    let volume = Volume::new(geometry, (0..8).map(|_| VoxelBuffer::F32(vec![100.0; 16])).collect()).expect("volume");
    let prepared = RenderVolume::new(&volume);
    let camera = Camera::fit(&geometry, StandardView::Bottom.orientation());
    let mip = style(Look::Mip, mip_window());
    // An odd size puts the centre pixel on the axis of the thin volume.
    let setup = Setup::new(&prepared, &camera, &mip, [9, 9], Quality::Full, Shortcuts::ALL);
    assert!((f64::from(setup.step) - camera.field_mm / 4096.0).abs() < 1e-6, "{} mm", setup.step);
    let mut crossing = 0;
    for y in 0..9 {
        for x in 0..9 {
            if let Some((first, last)) = setup.samples(setup.start(x, y)) {
                assert!(last - first <= 4096, "{} steps", last - first);
                crossing += 1;
            }
        }
    }
    assert!(crossing > 0, "rays cross the volume");
    let picture = draw(&prepared, &camera, &mip, [9, 9], Quality::Full);
    assert_eq!(picture.pixel(4, 4), Some([26, 26, 26, 255]), "100 in a window from 0 to 1000");
}

#[test]
fn preview_and_full_pictures_show_the_same_thing() {
    let volume = RenderVolume::new(&ct(48, 24));
    let camera = Camera::fit(&volume.volume().geometry, DQuat::IDENTITY);
    let style = style(Look::Bone, Window::new(40.0, 400.0));
    let preview = draw(&volume, &camera, &style, [32, 32], Quality::Preview);
    let full = draw(&volume, &camera, &style, [96, 96], Quality::Full);
    let (px, py) = camera.to_pixel(DVec3::new(CT_BONE_AXIS_MM[0], 0.0, 0.0), [32, 32]);
    let (fx, fy) = camera.to_pixel(DVec3::new(CT_BONE_AXIS_MM[0], 0.0, 0.0), [96, 96]);
    assert!(preview.pixel(px.round() as usize, py.round() as usize).expect("pixel")[3] >= 250);
    assert!(full.pixel(fx.round() as usize, fy.round() as usize).expect("pixel")[3] >= 250);
}

// ---------------------------------------------------------------------------
// Prepared volume and sampling

#[test]
fn the_grid_holds_the_range_of_every_block_with_one_voxel_overlap() {
    let dims = [19usize, 11, 10];
    let geometry = VolumeGeometry::new(dims, DVec3::ZERO, [DVec3::X, DVec3::Y, DVec3::Z]).expect("geometry");
    let value = |i: usize, j: usize, k: usize| ((i * 7 + j * 13 + k * 29) % 31) as f32 - 9.0;
    let slices = (0..dims[2])
        .map(|k| VoxelBuffer::F32((0..dims[0] * dims[1]).map(|n| value(n % dims[0], n / dims[0], k)).collect()))
        .collect();
    let volume = Volume::new(geometry, slices).expect("volume");
    let prepared = RenderVolume::new(&volume);
    let grid = &prepared.grid;
    assert_eq!(grid.blocks, [3, 2, 2]);
    for bk in 0..2 {
        for bj in 0..2 {
            for bi in 0..3 {
                let span = |b: usize, n: usize| (b * 8)..=((b * 8 + 8).min(n - 1));
                let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
                for k in span(bk, dims[2]) {
                    for j in span(bj, dims[1]) {
                        for i in span(bi, dims[0]) {
                            lo = lo.min(value(i, j, k));
                            hi = hi.max(value(i, j, k));
                        }
                    }
                }
                let index = grid.index([bi, bj, bk]);
                assert_eq!((grid.min[index], grid.max[index]), (lo, hi), "block {bi} {bj} {bk}");
            }
        }
    }
    assert_eq!(prepared.value_range(), (-9.0, 21.0));
    assert!(prepared.is_for(&volume) && prepared.is_for(&volume.clone()), "the same slices");
    let other = Volume::new(geometry, (0..dims[2]).map(|_| VoxelBuffer::F32(vec![0.0; dims[0] * dims[1]])).collect());
    assert!(!prepared.is_for(&other.expect("volume")));
    // The prepared volume shares the slices: nothing is copied.
    assert!(prepared.volume().slices.iter().zip(&volume.slices).all(|(a, b)| Arc::ptr_eq(a, b)));
}

#[test]
fn the_renderer_samples_as_the_volume_does() {
    let volume = ct(48, 24);
    let voxels = Voxels::new(&volume);
    for n in 0..500 {
        let t = n as f32;
        let p = Vec3A::new((t * 0.37).rem_euclid(47.0), (t * 0.61).rem_euclid(47.0), (t * 0.23).rem_euclid(23.0));
        let (sample, reference) = (voxels.sample(p), volume.sample(p.as_dvec3()).expect("inside"));
        assert!((sample - reference).abs() < 1e-2, "{p:?}: {sample} instead of {reference}");
    }
    // Within half a voxel outside the voxel centres the samples are clamped, as
    // `Volume::sample` does; further out the renderer keeps clamping.
    let edge = Vec3A::new(-0.4, 47.4, 23.4);
    assert!((voxels.sample(edge) - volume.sample(edge.as_dvec3()).expect("inside")).abs() < 1e-2);
    assert_eq!(voxels.sample(Vec3A::new(-3.0, 50.0, 30.0)), voxels.sample(Vec3A::new(0.0, 47.0, 23.0)));
}

// ---------------------------------------------------------------------------
// Timing (run by hand: `cargo test --release -p summrise-imaging --lib timing -- --ignored --nocapture`)

/// Samples through the generic `Volume::sample`, for comparison with the fast path.
struct ViaVolume<'a>(&'a Volume);

impl Sample for ViaVolume<'_> {
    fn sample(&self, p: Vec3A) -> f32 {
        self.0.sample(p.as_dvec3()).unwrap_or(0.0)
    }
}

/// The CT phantom of the testkit at `size` × `size` × `slices`, built in parallel.
fn big_ct(size: u32, slices: u32) -> Volume {
    let phantom = CtPhantom::new(&PhantomOptions { ct_size: size, ct_slices: slices, ..PhantomOptions::small() });
    let [sx, sy, sz] = phantom.spacing();
    let geometry = VolumeGeometry::new(
        phantom.dims(),
        DVec3::from_array(phantom.origin()),
        [DVec3::X * sx, DVec3::Y * sy, DVec3::Z * sz],
    )
    .expect("geometry");
    let slices = (0..phantom.dims()[2]).into_par_iter().map(|k| VoxelBuffer::I16(phantom.slice(k))).collect();
    Volume::new(geometry, slices).expect("volume")
}

/// Median time of `runs` runs of `f`, in milliseconds.
fn median_ms(runs: usize, mut f: impl FnMut()) -> f64 {
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = std::time::Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[runs / 2]
}

#[test]
#[ignore = "timing measurement; run by hand in release"]
#[allow(clippy::print_stderr)]
fn timing() {
    let threads = rayon::current_num_threads();
    eprintln!("rayon threads: {threads}");
    for size in [256u32, 512] {
        let volume = big_ct(size, 300);
        let mut prepared = None;
        let grid_ms = median_ms(3, || prepared = Some(RenderVolume::new(&volume)));
        let prepared = prepared.expect("prepared");
        eprintln!("CT {size} x {size} x 300 ({:.0} MB): grid {grid_ms:.1} ms", volume.byte_size() as f64 / 1e6);
        let window = Window::new(300.0, 1600.0);
        for (name, orientation) in [("front", DQuat::IDENTITY), ("oblique", turned(DQuat::IDENTITY, 30.0, -20.0))] {
            let camera = Camera::fit(&volume.geometry, orientation);
            for look in [Look::Bone, Look::Skin, Look::VesselsAndBone, Look::Mip] {
                let style = style(look, window);
                let time = |pixels: usize, quality: Quality, runs: usize| {
                    median_ms(runs, || {
                        render(&prepared, &camera, &style, [pixels, pixels], quality, &never).expect("picture");
                    })
                };
                let preview = time(192, Quality::Preview, 7);
                let preview_256 = time(256, Quality::Preview, 7);
                let full_512 = time(512, Quality::Full, 3);
                let full_1024 = time(1024, Quality::Full, 3);
                eprintln!(
                    "  {name:7} {:16} preview 192: {preview:7.1} ms  preview 256: {preview_256:7.1} ms  full 512: {full_512:7.1} ms  full 1024: {full_1024:7.1} ms",
                    format!("{look:?}")
                );
            }
        }
        // The generic `Volume::sample` against the fast path, full 512 from the front.
        let camera = Camera::fit(&volume.geometry, DQuat::IDENTITY);
        for look in [Look::Bone, Look::Mip] {
            let style = style(look, window);
            let setup = Setup::new(&prepared, &camera, &style, [512, 512], Quality::Full, Shortcuts::ALL);
            let mut rgba = vec![0u8; 512 * 512 * 4];
            let generic =
                median_ms(3, || Rows { setup: &setup, rgba: &mut rgba, cancelled: &never }.run(&ViaVolume(&volume)));
            let voxels = Voxels::new(&volume);
            let own = median_ms(3, || Rows { setup: &setup, rgba: &mut rgba, cancelled: &never }.run(&voxels));
            eprintln!("  sampler {look:?} full 512: Volume::sample {generic:.1} ms, renderer's sampler {own:.1} ms");
        }
        // Without the shortcuts, for the record.
        for look in [Look::Bone, Look::Mip] {
            let style = style(look, window);
            let plain = Shortcuts { skip_empty: false, stop_when_opaque: false };
            let ms = median_ms(3, || {
                render_with(&prepared, &camera, &style, [512, 512], Quality::Full, plain, &never).expect("picture");
            });
            eprintln!("  no shortcuts {look:?} full 512: {ms:.1} ms");
        }
    }
}
