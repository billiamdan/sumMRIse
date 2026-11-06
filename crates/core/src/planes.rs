//! Tilting the three reformat planes about the crosshair (oblique and
//! double-oblique reformats).
//!
//! [`ViewState`] holds one rotation for all three planes, so they stay
//! mutually orthogonal. Every plane passes through the crosshair, which makes
//! the crosshair the centre of rotation: tilting never moves it. The
//! orientation of each view follows from the rotation
//! ([`PlaneBasis::tilted`]): a view shows the rotated plane but never spins
//! about its own normal, so tilting in a view turns the two other planes and
//! leaves the view itself as it is.

use summrise_imaging::{AnatomicalPlane, DQuat, DVec3, MprAxis, PlaneBasis};

use crate::view::ViewState;

/// Below this (sine of half the angle, about a hundred-thousandth of a
/// degree) a rotation counts as none, so that tilting forth and back gives
/// the standard planes again, exactly.
const NO_TILT: f64 = 1e-7;

/// How far a reformat view is tilted away from its standard plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneTilt {
    /// Angle between the view's plane and its standard plane, in degrees
    /// (above 90° the plane is seen from the other side).
    pub degrees: f64,
    /// The standard plane the view's plane is closest to.
    pub nearest: AnatomicalPlane,
}

impl ViewState {
    /// The rotation of the three reformat planes (identity: axial, coronal, sagittal).
    pub fn plane_rotation(&self) -> DQuat {
        self.plane_rotation
    }

    /// Whether the planes are tilted away from axial, coronal and sagittal.
    pub fn planes_tilted(&self) -> bool {
        self.plane_rotation != DQuat::IDENTITY
    }

    /// Orientation of the `axis` view: its standard basis while the planes
    /// are not tilted (bit for bit), else the tilted plane.
    pub fn plane_basis(&self, axis: MprAxis) -> PlaneBasis {
        PlaneBasis::tilted(axis, self.plane_rotation)
    }

    /// Turns the two other planes about the normal of the `about` view,
    /// through the crosshair, by `radians`. A positive angle turns their
    /// lines in that view from screen-right towards screen-down (clockwise
    /// on a screen that is not mirrored). The `about` view and the crosshair
    /// stay as they are.
    pub fn tilt_planes(&mut self, about: MprAxis, radians: f64) {
        if !radians.is_finite() {
            return;
        }
        let normal = self.plane_basis(about).normal;
        let rotation = (DQuat::from_axis_angle(normal, radians) * self.plane_rotation).normalize();
        self.plane_rotation =
            if rotation.is_finite() && rotation.xyz().length() >= NO_TILT { rotation } else { DQuat::IDENTITY };
    }

    /// Returns the planes to axial, coronal and sagittal; the crosshair stays.
    pub fn reset_planes(&mut self) {
        self.plane_rotation = DQuat::IDENTITY;
    }

    /// How far the `axis` view is tilted; `None` while it shows its standard plane.
    pub fn plane_tilt(&self, axis: MprAxis) -> Option<PlaneTilt> {
        let basis = self.plane_basis(axis);
        (basis != PlaneBasis::standard(axis))
            .then(|| PlaneTilt { degrees: basis.tilt_degrees(axis), nearest: basis.nearest_plane() })
    }

    /// Patient direction of the line in which the plane of the `other` view
    /// cuts the plane of the `view` view. The line runs through the crosshair.
    pub fn plane_line(&self, view: MprAxis, other: MprAxis) -> Option<DVec3> {
        self.plane_basis(view).line_of(&self.plane_basis(other))
    }
}

#[cfg(test)]
mod tests {
    use summrise_imaging::{EdgeLabels, MprExtent};
    use summrise_testkit::PhantomOptions;

    use super::*;
    use crate::series::SeriesData;
    use crate::series::testing::{load, phantom};
    use crate::view::ViewKind;

    const AXIAL: MprAxis = MprAxis::Axial;
    const CORONAL: MprAxis = MprAxis::Coronal;
    const SAGITTAL: MprAxis = MprAxis::Sagittal;

    /// The synthetic CT: 48 × 48 × 24 voxels of 5 × 5 × 8.33 mm, axial.
    fn ct() -> (tempfile::TempDir, SeriesData) {
        let (dir, index) = phantom(&PhantomOptions::small());
        let series = load(&index.studies[0].series[0]);
        (dir, series)
    }

    fn close(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-9
    }

    /// Edge letters (left, right, top, bottom) of a reformat view.
    fn letters(state: &ViewState, axis: MprAxis) -> [String; 4] {
        let basis = state.plane_basis(axis);
        let l = EdgeLabels::new(basis.right, basis.down);
        [l.left, l.right, l.top, l.bottom]
    }

    #[test]
    fn a_series_opens_with_the_standard_planes() {
        let (_d, series) = ct();
        let state = ViewState::new(&series);
        assert_eq!(state.plane_rotation(), DQuat::IDENTITY);
        assert!(!state.planes_tilted());
        for axis in MprAxis::ALL {
            assert_eq!(state.plane_basis(axis), PlaneBasis::standard(axis));
            assert_eq!(state.plane_tilt(axis), None);
            assert_eq!(state.mpr_position(axis), state.cursor.dot(axis.normal()));
        }
        assert_eq!(letters(&state, AXIAL), ["R", "L", "A", "P"]);
        assert_eq!(letters(&state, CORONAL), ["R", "L", "S", "I"]);
        assert_eq!(letters(&state, SAGITTAL), ["A", "P", "S", "I"]);
    }

    #[test]
    fn tilting_keeps_the_crosshair_fixed_and_on_all_three_planes() {
        let (_d, series) = ct();
        let mut state = ViewState::new(&series);
        state.set_cursor(DVec3::new(22.0, -31.0, 12.0), &series);
        let cursor = state.cursor;
        let stack = state.slice;

        // 30° counter-clockwise in the axial view, then 25° clockwise in the coronal view.
        state.tilt_planes(AXIAL, (-30f64).to_radians());
        state.tilt_planes(CORONAL, 25f64.to_radians());
        assert!(state.planes_tilted());
        assert_eq!(state.cursor, cursor, "the crosshair is the centre of rotation");
        assert_eq!(state.slice, stack, "the acquired stack stays on its image");

        let bases = MprAxis::ALL.map(|axis| state.plane_basis(axis));
        for (axis, basis) in MprAxis::ALL.into_iter().zip(&bases) {
            // Each view shows the plane through the crosshair…
            assert_eq!(state.mpr_position(axis), cursor.dot(basis.normal));
            // …and its lines, the cuts of the two other planes, run through it too.
            for other in MprAxis::ALL.into_iter().filter(|o| *o != axis) {
                let line = state.plane_line(axis, other).expect("the planes are not parallel");
                let on_line = cursor + line * 40.0;
                assert!((on_line.dot(basis.normal) - state.mpr_position(axis)).abs() < 1e-9);
                assert!((on_line.dot(state.plane_basis(other).normal) - state.mpr_position(other)).abs() < 1e-9);
            }
        }
        // The planes are still mutually perpendicular.
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            assert!(bases[i].normal.dot(bases[j].normal).abs() < 1e-12);
        }
        // Double oblique: the axial plane is tilted by 25°, the coronal one still by 30°.
        let tilt = |axis| state.plane_tilt(axis).expect("tilted");
        assert!((tilt(AXIAL).degrees - 25.0).abs() < 1e-9 && tilt(AXIAL).nearest == AnatomicalPlane::Axial);
        assert!((tilt(CORONAL).degrees - 30.0).abs() < 1e-9 && tilt(CORONAL).nearest == AnatomicalPlane::Coronal);
        assert!((tilt(SAGITTAL).degrees - 38.29).abs() < 0.01 && tilt(SAGITTAL).nearest == AnatomicalPlane::Sagittal);
    }

    #[test]
    fn tilting_in_a_view_turns_the_two_other_planes_only() {
        let (_d, series) = ct();
        let mut state = ViewState::new(&series);
        state.tilt_planes(AXIAL, (-30f64).to_radians());
        // The axial view is exactly as it was: same plane, same picture, same letters.
        assert_eq!(state.plane_basis(AXIAL), PlaneBasis::standard(AXIAL));
        assert_eq!(state.plane_tilt(AXIAL), None);
        assert_eq!(letters(&state, AXIAL), ["R", "L", "A", "P"]);
        // The coronal and sagittal planes are turned by 30° about the superior-inferior axis.
        let (sin, cos) = 30f64.to_radians().sin_cos();
        let coronal = state.plane_basis(CORONAL);
        assert!(close(coronal.right, DVec3::new(cos, -sin, 0.0)) && close(coronal.down, -DVec3::Z));
        assert!(close(coronal.normal, DVec3::new(sin, cos, 0.0)));
        let tilt = state.plane_tilt(CORONAL).expect("tilted");
        assert!((tilt.degrees - 30.0).abs() < 1e-9 && tilt.nearest == AnatomicalPlane::Coronal);
        assert_eq!(letters(&state, CORONAL), ["RP", "LA", "S", "I"]);
        let sagittal = state.plane_basis(SAGITTAL);
        assert!(close(sagittal.right, DVec3::new(sin, cos, 0.0)) && close(sagittal.down, -DVec3::Z));
        assert_eq!(letters(&state, SAGITTAL), ["AR", "PL", "S", "I"]);

        // In the axial view the lines of the two other planes have turned by 30° from
        // screen-right towards screen-up (counter-clockwise); they stay perpendicular.
        let axial = state.plane_basis(AXIAL);
        let on_screen = |line: DVec3| (line.dot(axial.right), line.dot(axial.down));
        let (x, y) = on_screen(state.plane_line(AXIAL, CORONAL).expect("line"));
        assert!((y.atan2(x).to_degrees() + 30.0).abs() < 1e-9 || (y.atan2(x).to_degrees() - 150.0).abs() < 1e-9);
        let (sx, sy) = on_screen(state.plane_line(AXIAL, SAGITTAL).expect("line"));
        assert!((x * sx + y * sy).abs() < 1e-12);
        // In the two other views the lines are still horizontal and vertical.
        assert!(state.plane_line(CORONAL, AXIAL).expect("line").dot(coronal.down).abs() < 1e-12);
        assert!(state.plane_line(CORONAL, SAGITTAL).expect("line").dot(coronal.right).abs() < 1e-12);

        // A second tilt, in the coronal view, leaves the coronal view as it is.
        state.tilt_planes(CORONAL, 25f64.to_radians());
        let after = state.plane_basis(CORONAL);
        assert!(close(after.right, coronal.right) && close(after.down, coronal.down));
        assert!(state.plane_tilt(AXIAL).is_some(), "the axial plane is tilted now");
        assert_eq!(letters(&state, AXIAL), ["RS", "LI", "A", "P"]);
        assert_eq!(letters(&state, SAGITTAL), ["AR", "PL", "SL", "IR"]);

        // Three letters appear where a direction has a share of all three patient axes.
        let mut state = ViewState::new(&series);
        state.tilt_planes(AXIAL, 45f64.to_radians());
        state.tilt_planes(SAGITTAL, 35f64.to_radians());
        state.tilt_planes(CORONAL, (-40f64).to_radians());
        assert_eq!(letters(&state, SAGITTAL), ["LAS", "RPI", "SRA", "ILP"]);
        let right = state.plane_basis(SAGITTAL).right;
        assert!(right.x < -0.25 && right.y > 0.25 && right.z < -0.25, "right, posterior and inferior: {right:?}");
    }

    #[test]
    fn reset_restores_the_standard_planes() {
        let (_d, series) = ct();
        let mut state = ViewState::new(&series);
        assert!(state.is_pristine(&series));
        state.tilt_planes(SAGITTAL, 0.4);
        state.tilt_planes(AXIAL, -1.1);
        let cursor = state.cursor;
        assert!(state.planes_tilted());
        assert!(!state.is_pristine(&series), "tilting is the user's doing");

        // "Reset planes" touches nothing else.
        state.drag_window(40.0, 0.0, &series);
        let window = state.window;
        state.reset_planes();
        assert_eq!(state.plane_rotation(), DQuat::IDENTITY);
        assert_eq!((state.cursor, state.window), (cursor, window));
        for axis in MprAxis::ALL {
            assert_eq!(state.plane_basis(axis), PlaneBasis::standard(axis));
        }

        // The general reset restores the planes too.
        state.tilt_planes(CORONAL, 0.2);
        state.reset(&series);
        assert!(!state.planes_tilted());
        assert_eq!(state.cursor, cursor, "reset keeps the position");
        assert!(state.is_pristine(&series));
    }

    #[test]
    fn tilting_forth_and_back_gives_the_standard_planes_exactly() {
        let (_d, series) = ct();
        let mut state = ViewState::new(&series);
        let degree = 1f64.to_radians();
        for _ in 0..7 {
            state.tilt_planes(AXIAL, degree);
        }
        state.tilt_planes(CORONAL, 5.0 * degree);
        assert!((state.plane_tilt(SAGITTAL).expect("tilted").degrees - 8.6).abs() < 0.05);
        state.tilt_planes(CORONAL, -5.0 * degree);
        for _ in 0..7 {
            state.tilt_planes(AXIAL, -degree);
        }
        assert_eq!(state.plane_rotation(), DQuat::IDENTITY);
        assert!(state.is_pristine(&series));
        // A whole turn is no tilt either, and nonsense is ignored.
        for _ in 0..4 {
            state.tilt_planes(SAGITTAL, std::f64::consts::FRAC_PI_2);
        }
        assert!(!state.planes_tilted());
        state.tilt_planes(AXIAL, f64::NAN);
        state.tilt_planes(AXIAL, f64::INFINITY);
        assert!(!state.planes_tilted());
    }

    #[test]
    fn scrolling_a_tilted_view_moves_the_crosshair_along_the_tilted_normal() {
        let (_d, series) = ct();
        let volume = series.volume().expect("volume");
        let mut state = ViewState::new(&series);
        let coronal = ViewKind::Mpr(CORONAL);
        let untilted_count = state.slice_count(coronal, &series);
        state.tilt_planes(AXIAL, 30f64.to_radians());
        let basis = state.plane_basis(CORONAL);
        let normal = basis.normal;
        assert!(close(normal, DVec3::new(-0.5, 30f64.to_radians().cos(), 0.0)));

        // The slice step is the largest projection of a voxel edge on the tilted normal:
        // 5 mm × cos 30° = 4.33 mm.
        let ext = MprExtent::of_plane(&volume.geometry, &basis);
        assert!((ext.step - 5.0 * 30f64.to_radians().cos()).abs() < 1e-9, "{}", ext.step);
        // The tilted plane sweeps the volume along its diagonal: more slices than before.
        let count = state.slice_count(coronal, &series);
        assert_eq!(count, ext.slice_count());
        assert!(count > untilted_count, "{count} slices, {untilted_count} untilted");

        // The first step brings the crosshair onto the slice grid…
        state.scroll(coronal, &series, 1);
        let on_grid = state.cursor;
        let index = state.slice_index(coronal, &series);
        assert!((state.mpr_position(CORONAL) - ext.position(index)).abs() < 1e-9);
        // …and every further step moves it by one slice step along the tilted normal,
        // not along the patient's anterior-posterior axis.
        state.scroll(coronal, &series, 1);
        assert!(close(state.cursor - on_grid, normal * ext.step), "moved by {:?}", state.cursor - on_grid);
        assert_eq!(state.slice_index(coronal, &series), index + 1);
        state.scroll(coronal, &series, -3);
        assert!(close(state.cursor - on_grid, normal * (-2.0 * ext.step)));
        assert_eq!(state.slice_index(coronal, &series), index - 2);
        // The two other views follow: their planes still pass through the crosshair.
        assert_eq!(state.mpr_position(SAGITTAL), state.cursor.dot(state.plane_basis(SAGITTAL).normal));

        // Home and End reach the first and the last slice of the tilted view.
        state.set_slice(coronal, &series, 0);
        assert!((state.mpr_position(CORONAL) - ext.n.0).abs() < 1e-9);
        state.set_slice(coronal, &series, usize::MAX);
        assert_eq!(state.slice_index(coronal, &series), count - 1);
    }
}
