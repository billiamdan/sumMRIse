//! Orientation of a reformat plane in patient space: the three standard
//! planes, and the same planes tilted about the crosshair.
//!
//! The three reformat planes are turned together by one rotation, so they
//! always stay mutually orthogonal. [`PlaneBasis::tilted`] is the orientation
//! of one view under that rotation.

use glam::{DQuat, DVec3};

use crate::geometry::AnatomicalPlane;
use crate::mpr::MprAxis;

/// Orientation of a reformat plane: the patient-space directions of
/// screen-right and screen-down and the viewing normal. The three vectors
/// are orthonormal and `normal == right × down`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneBasis {
    pub right: DVec3,
    pub down: DVec3,
    pub normal: DVec3,
}

impl PlaneBasis {
    /// The standard (radiological) orientation of `axis`: [`MprAxis::basis`].
    pub fn standard(axis: MprAxis) -> Self {
        let (right, down, normal) = axis.basis();
        Self { right, down, normal }
    }

    /// `rotation` applied to the standard basis of `axis` as a whole: the
    /// picture turns with the rotation, also about the plane's own normal.
    /// A rotation by nothing gives the standard basis bit for bit.
    pub fn rotated(axis: MprAxis, rotation: DQuat) -> Self {
        let standard = Self::standard(axis);
        let Some(rotation) = turning(rotation) else {
            return standard;
        };
        Self { right: rotation * standard.right, down: rotation * standard.down, normal: rotation * standard.normal }
    }

    /// The orientation of the `axis` view when the three planes are turned
    /// by `rotation`. The plane is the rotated plane (its normal is the
    /// rotated standard normal), but the picture does not spin about the
    /// plane's own normal: the in-plane axes are carried along by the
    /// shortest rotation that takes the standard normal to the rotated one.
    ///
    /// A rotation about the normal of `axis` therefore leaves this view as
    /// it is and turns only the two other planes; the identity gives the
    /// standard basis bit for bit. A plane turned over by exactly 180° has
    /// no shortest rotation: one of them is used.
    pub fn tilted(axis: MprAxis, rotation: DQuat) -> Self {
        let standard = Self::standard(axis);
        let Some(rotation) = turning(rotation) else {
            return standard;
        };
        let normal = (rotation * standard.normal).normalize_or(standard.normal);
        Self::rotated(axis, DQuat::from_rotation_arc(standard.normal, normal))
    }

    /// Unit direction of the line in which the plane `other` cuts this
    /// plane; `None` when the two planes are parallel.
    pub fn line_of(&self, other: &Self) -> Option<DVec3> {
        self.normal.cross(other.normal).try_normalize()
    }

    /// Angle in degrees (0 to 180) between this plane and the standard plane
    /// of `axis`, measured between their viewing normals: above 90° the
    /// plane is seen from the other side.
    pub fn tilt_degrees(&self, axis: MprAxis) -> f64 {
        let standard = axis.normal();
        standard.cross(self.normal).length().atan2(standard.dot(self.normal)).to_degrees()
    }

    /// The standard plane this plane is closest to.
    pub fn nearest_plane(&self) -> AnatomicalPlane {
        AnatomicalPlane::nearest(self.normal)
    }
}

/// `rotation` as a unit quaternion, or `None` when it turns nothing (or is
/// not a rotation at all).
fn turning(rotation: DQuat) -> Option<DQuat> {
    (rotation.is_finite() && rotation.xyz() != DVec3::ZERO).then(|| rotation.normalize()).filter(|r| r.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(basis: &PlaneBasis) -> [u64; 9] {
        let mut out = [0; 9];
        for (slot, value) in out
            .iter_mut()
            .zip(basis.right.to_array().into_iter().chain(basis.down.to_array()).chain(basis.normal.to_array()))
        {
            *slot = value.to_bits();
        }
        out
    }

    fn close(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-12
    }

    fn about(axis: DVec3, degrees: f64) -> DQuat {
        DQuat::from_axis_angle(axis.normalize(), degrees.to_radians())
    }

    /// A few rotations: about one patient axis, double-oblique, large.
    fn rotations() -> Vec<DQuat> {
        vec![
            about(DVec3::Z, 30.0),
            about(DVec3::X, -20.0),
            about(DVec3::new(0.5, 0.866, 0.0), 25.0) * about(DVec3::Z, -30.0),
            about(DVec3::new(1.0, 2.0, 3.0), 77.0),
            about(DVec3::Y, 135.0) * about(DVec3::X, 60.0),
        ]
    }

    #[test]
    fn no_rotation_gives_the_standard_basis_bit_for_bit() {
        // Tiny residues, as left by tilting forth and back, count as no rotation.
        let residue = DQuat::from_xyzw(1e-17, -1e-17, 0.0, 1.0).normalize();
        for axis in MprAxis::ALL {
            let standard = PlaneBasis::standard(axis);
            assert_eq!((standard.right, standard.down, standard.normal), axis.basis());
            for rotation in [DQuat::IDENTITY, -DQuat::IDENTITY, residue] {
                assert_eq!(bits(&PlaneBasis::tilted(axis, rotation)), bits(&standard), "{axis:?} {rotation:?}");
            }
            for rotation in [DQuat::IDENTITY, -DQuat::IDENTITY] {
                assert_eq!(bits(&PlaneBasis::rotated(axis, rotation)), bits(&standard), "{axis:?} {rotation:?}");
            }
        }
    }

    #[test]
    fn tilted_planes_stay_orthonormal_and_mutually_orthogonal() {
        for rotation in rotations() {
            let bases = MprAxis::ALL.map(|axis| PlaneBasis::tilted(axis, rotation));
            for (axis, b) in MprAxis::ALL.into_iter().zip(&bases) {
                assert!((b.right.length() - 1.0).abs() < 1e-12 && (b.down.length() - 1.0).abs() < 1e-12);
                assert!(b.right.dot(b.down).abs() < 1e-12);
                assert!(close(b.right.cross(b.down), b.normal), "normal is right × down");
                // The plane is the rotated standard plane.
                assert!(close(b.normal, rotation * axis.normal()), "{axis:?}");
            }
            for (i, j) in [(0, 1), (0, 2), (1, 2)] {
                assert!(bases[i].normal.dot(bases[j].normal).abs() < 1e-12, "planes {i} and {j} are perpendicular");
            }
        }
    }

    #[test]
    fn a_view_does_not_spin_about_its_own_normal() {
        // Turning about the axial normal: the axial view stays exactly as it is…
        let turn = about(DVec3::Z, 30.0);
        assert_eq!(bits(&PlaneBasis::tilted(MprAxis::Axial, turn)), bits(&PlaneBasis::standard(MprAxis::Axial)));
        // …whereas the whole rotation would spin its picture by 30°.
        let spun = PlaneBasis::rotated(MprAxis::Axial, turn);
        assert!(close(spun.right, DVec3::new(30f64.to_radians().cos(), 30f64.to_radians().sin(), 0.0)));
        // The two other planes turn with it, upright (down stays inferior).
        let coronal = PlaneBasis::tilted(MprAxis::Coronal, turn);
        assert!(close(coronal.right, spun.right) && close(coronal.down, -DVec3::Z));
        assert!(close(coronal.normal, DVec3::new(-0.5, 30f64.to_radians().cos(), 0.0)));
        let sagittal = PlaneBasis::tilted(MprAxis::Sagittal, turn);
        assert!(close(sagittal.right, DVec3::new(-0.5, 30f64.to_radians().cos(), 0.0)));
        assert!(close(sagittal.down, -DVec3::Z));

        // The same holds for a plane that is tilted already: a further turn about its
        // own normal changes the two other views only.
        for first in rotations() {
            for axis in MprAxis::ALL {
                let before = PlaneBasis::tilted(axis, first);
                let after = PlaneBasis::tilted(axis, about(before.normal, 40.0) * first);
                assert!(close(before.right, after.right) && close(before.down, after.down), "{axis:?}");
                let other = MprAxis::ALL[(MprAxis::ALL.iter().position(|a| *a == axis).expect("axis") + 1) % 3];
                let turned = PlaneBasis::tilted(other, about(before.normal, 40.0) * first);
                assert!(!close(turned.normal, PlaneBasis::tilted(other, first).normal));
            }
        }
    }

    #[test]
    fn lines_of_intersection_follow_the_planes() {
        let [axial, coronal, sagittal] = MprAxis::ALL.map(PlaneBasis::standard);
        // Standard planes: the coronal plane cuts the axial one along left-right, the
        // sagittal one along anterior-posterior.
        let line = axial.line_of(&coronal).expect("line");
        assert_eq!((line.dot(axial.right).abs(), line.dot(axial.down)), (1.0, 0.0));
        let line = axial.line_of(&sagittal).expect("line");
        assert_eq!((line.dot(axial.right), line.dot(axial.down).abs()), (0.0, 1.0));
        assert_eq!(axial.line_of(&axial), None);

        for rotation in rotations() {
            let bases = MprAxis::ALL.map(|axis| PlaneBasis::tilted(axis, rotation));
            for view in 0..3 {
                let a = bases[view].line_of(&bases[(view + 1) % 3]).expect("line");
                let b = bases[view].line_of(&bases[(view + 2) % 3]).expect("line");
                // Both lines lie in the view's plane and are perpendicular to each other.
                assert!(a.dot(bases[view].normal).abs() < 1e-12 && b.dot(bases[view].normal).abs() < 1e-12);
                assert!(a.dot(b).abs() < 1e-12);
            }
        }

        // Turned by 30° about the axial normal, the lines in the axial view turn by 30°
        // from screen-right towards screen-down; those in the other views do not.
        let turn = about(DVec3::Z, 30.0);
        let [axial, coronal, sagittal] = MprAxis::ALL.map(|axis| PlaneBasis::tilted(axis, turn));
        let line = axial.line_of(&coronal).expect("line");
        let angle = line.dot(axial.down).atan2(line.dot(axial.right)).to_degrees();
        assert!((angle - 30.0).abs() < 1e-9 || (angle + 150.0).abs() < 1e-9, "{angle}");
        let line = coronal.line_of(&axial).expect("line");
        assert!(line.dot(coronal.down).abs() < 1e-12, "still horizontal in the coronal view");
        let line = sagittal.line_of(&coronal).expect("line");
        assert!(line.dot(sagittal.right).abs() < 1e-12, "still vertical in the sagittal view");
    }

    #[test]
    fn tilt_angle_and_nearest_plane() {
        for axis in MprAxis::ALL {
            let standard = PlaneBasis::standard(axis);
            assert_eq!(standard.tilt_degrees(axis), 0.0);
            assert_eq!(standard.nearest_plane(), axis.plane());
        }
        let coronal = PlaneBasis::tilted(MprAxis::Coronal, about(DVec3::Z, 30.0));
        assert!((coronal.tilt_degrees(MprAxis::Coronal) - 30.0).abs() < 1e-9);
        assert_eq!(coronal.nearest_plane(), AnatomicalPlane::Coronal);
        // Beyond 45° the plane is closer to another standard plane.
        let coronal = PlaneBasis::tilted(MprAxis::Coronal, about(DVec3::Z, 60.0));
        assert!((coronal.tilt_degrees(MprAxis::Coronal) - 60.0).abs() < 1e-9);
        assert_eq!(coronal.nearest_plane(), AnatomicalPlane::Sagittal);
        // Far from every standard plane there is still a nearest one.
        let skew = PlaneBasis::tilted(MprAxis::Axial, about(DVec3::new(1.0, -1.0, 0.0), 50.0));
        assert_eq!(AnatomicalPlane::from_normal(skew.normal), AnatomicalPlane::Oblique);
        assert_eq!(skew.nearest_plane(), AnatomicalPlane::Axial);
        // Small angles are not lost in rounding.
        let slight = PlaneBasis::tilted(MprAxis::Axial, about(DVec3::X, 1e-4));
        assert!((slight.tilt_degrees(MprAxis::Axial) - 1e-4).abs() < 1e-9);
    }

    #[test]
    fn a_plane_turned_over_is_still_a_valid_basis() {
        for axis in MprAxis::ALL {
            let standard = PlaneBasis::standard(axis);
            let over = PlaneBasis::tilted(axis, about(standard.right, 180.0));
            assert!(close(over.normal, -standard.normal));
            assert!(close(over.right.cross(over.down), over.normal));
            assert!((over.tilt_degrees(axis) - 180.0).abs() < 1e-6);
        }
        // Rotations that are no rotations leave the view alone instead of spoiling it.
        let nan = DQuat::from_xyzw(f64::NAN, 0.0, 0.0, 1.0);
        let zero = DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0);
        for broken in [nan, zero] {
            assert_eq!(PlaneBasis::tilted(MprAxis::Coronal, broken), PlaneBasis::standard(MprAxis::Coronal));
            assert_eq!(PlaneBasis::rotated(MprAxis::Coronal, broken), PlaneBasis::standard(MprAxis::Coronal));
        }
    }
}
