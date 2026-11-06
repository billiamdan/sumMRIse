//! The 3D view of a series: the orientation of its camera (turned by
//! dragging, by the arrow keys or to one of six standard views), its look,
//! and the camera for a viewport of a given size.
//!
//! The camera starts in front of the patient with the head up, as in the
//! coronal view. Turning it is a change made by the user (`is_pristine`),
//! and the general reset returns it to that first view and to the first look.
//! Zoom and pan are those of the view's `ViewTransform`, as in every other
//! view; its display orientation is not used (the camera covers rotation).

use summrise_imaging::render3d::{Camera, Look, StandardView, Style, turned};
use summrise_imaging::{DQuat, VolumeGeometry};

use crate::series::SeriesData;
use crate::view::{ViewKind, ViewState, ViewTransform};

/// Below this (sine of half the angle, about a hundred-thousandth of a
/// degree) a turn of the camera counts as none, so that turning forth and
/// back gives the first view again, exactly.
const NO_TURN: f64 = 1e-7;

impl ViewState {
    /// Orientation of the 3D view's camera: a rotation of the coronal basis
    /// (identity: from the front, head up).
    pub fn camera_orientation(&self) -> DQuat {
        self.camera
    }

    /// Turns the object in the 3D view as a drag on screen by `right_degrees`
    /// to the right and `down_degrees` downwards does: its near side follows
    /// the pointer.
    pub fn turn_camera(&mut self, right_degrees: f64, down_degrees: f64) {
        let rotation = turned(self.camera, right_degrees, down_degrees);
        self.camera =
            if rotation.is_finite() && rotation.xyz().length() >= NO_TURN { rotation } else { DQuat::IDENTITY };
    }

    /// Looks at the volume from one of the six standard directions.
    pub fn show_standard_view(&mut self, view: StandardView) {
        self.camera = view.orientation();
    }

    /// The standard view the camera is in, if it is in one.
    pub fn standard_view(&self) -> Option<StandardView> {
        StandardView::ALL.into_iter().find(|view| {
            let q = view.orientation();
            // `q` and `-q` are the same rotation.
            (self.camera.dot(q).abs() - 1.0).abs() < 1e-9
        })
    }

    /// The looks offered for `series`: the Hounsfield looks for CT, the
    /// others otherwise.
    pub fn looks(series: &SeriesData) -> &'static [Look] {
        Look::available(series.units() == Some("HU"))
    }

    /// Chooses `look`, if it is offered for `series`.
    pub fn set_look(&mut self, look: Look, series: &SeriesData) {
        if Self::looks(series).contains(&look) {
            self.look = look;
        }
    }

    /// Chooses the look after the current one, in the order offered.
    pub fn next_look(&mut self, series: &SeriesData) {
        let looks = Self::looks(series);
        let next = looks.iter().position(|look| *look == self.look).map_or(0, |i| (i + 1) % looks.len());
        if let Some(look) = looks.get(next) {
            self.look = *look;
        }
    }

    /// Everything about the 3D picture but the camera and the size: the look
    /// and the contrast settings it may follow.
    pub fn volume_style(&self) -> Style {
        Style { look: self.look, window: self.window, invert: self.invert }
    }

    /// The camera of the 3D view in a viewport of `viewport` (width, height)
    /// points: fitted so that the turning volume always stays inside, then
    /// zoomed and panned as the view's transform says. Pan is in points on
    /// screen, as in the other views.
    pub fn volume_camera(&self, geometry: &VolumeGeometry, viewport: [f32; 2]) -> Camera {
        let fit = Camera::fit(geometry, self.camera);
        let transform = self.transform(ViewKind::Volume);
        let zoom = f64::from(transform.zoom.clamp(ViewTransform::MIN_ZOOM, ViewTransform::MAX_ZOOM));
        let field_mm = fit.field_mm / zoom;
        let mm_per_point = field_mm / f64::from(viewport[0].min(viewport[1]).max(1.0));
        let basis = fit.basis();
        let center = fit.center
            - basis.right * (f64::from(transform.pan[0]) * mm_per_point)
            - basis.down * (f64::from(transform.pan[1]) * mm_per_point);
        Camera { orientation: self.camera, center, field_mm }
    }
}

#[cfg(test)]
mod tests {
    use summrise_imaging::{DVec3, MprAxis, Window};
    use summrise_testkit::{MARKER_POSITION, PhantomOptions};

    use super::*;
    use crate::series::testing::{load, phantom};

    fn letters(camera: &Camera) -> [String; 4] {
        let l = camera.edge_labels();
        [l.left, l.right, l.top, l.bottom]
    }

    #[test]
    fn the_3d_view_opens_from_the_front_with_the_look_for_the_data() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let ct = load(&index.studies[0].series[0]);
        let mr = load(&index.studies[0].series[1]);
        let state = ViewState::new(&ct);
        assert_eq!(state.camera_orientation(), DQuat::IDENTITY);
        assert_eq!(state.standard_view(), Some(StandardView::Front));
        assert_eq!(state.look, Look::Bone);
        assert_eq!(ViewState::looks(&ct), [Look::Bone, Look::Skin, Look::VesselsAndBone, Look::Mip]);
        // MR has no Hounsfield units: the looks that need them are not offered.
        assert_eq!(ViewState::new(&mr).look, Look::Surface);
        assert_eq!(ViewState::looks(&mr), [Look::Surface, Look::Mip]);
        let geometry = ct.volume().expect("volume").geometry;
        let camera = state.volume_camera(&geometry, [800.0, 600.0]);
        assert_eq!(letters(&camera), ["R", "L", "S", "I"]);
        assert_eq!(camera.basis(), summrise_imaging::PlaneBasis::standard(MprAxis::Coronal));
    }

    #[test]
    fn turning_and_choosing_a_look_count_as_changes_and_reset_undoes_them() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let ct = load(&index.studies[0].series[0]);
        let mut state = ViewState::new(&ct);
        assert!(state.is_pristine(&ct));
        state.turn_camera(30.0, 0.0);
        assert!(!state.is_pristine(&ct), "turning the object is the user's doing");
        assert_eq!(state.standard_view(), None);
        // Turning back gives the first view again, exactly.
        state.turn_camera(-30.0, 0.0);
        assert_eq!(state.camera_orientation(), DQuat::IDENTITY);
        assert!(state.is_pristine(&ct));

        state.next_look(&ct);
        assert_eq!(state.look, Look::Skin);
        assert!(!state.is_pristine(&ct));
        // A look that is not offered for the data is not taken.
        state.set_look(Look::Surface, &ct);
        assert_eq!(state.look, Look::Skin);
        state.set_look(Look::Mip, &ct);
        state.next_look(&ct);
        assert_eq!(state.look, Look::Bone, "the list wraps around");

        state.show_standard_view(StandardView::Left);
        state.set_look(Look::VesselsAndBone, &ct);
        state.transform_mut(ViewKind::Volume).zoom_about(2.0, [10.0, 0.0]);
        let cursor = state.cursor;
        state.reset(&ct);
        assert_eq!((state.camera_orientation(), state.look), (DQuat::IDENTITY, Look::Bone));
        assert_eq!(state.transform(ViewKind::Volume), ViewTransform::default());
        assert_eq!(state.cursor, cursor, "reset keeps the position");
        assert!(state.is_pristine(&ct));
    }

    #[test]
    fn standard_views_and_letters() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let ct = load(&index.studies[0].series[0]);
        let geometry = ct.volume().expect("volume").geometry;
        let mut state = ViewState::new(&ct);
        for (view, expected) in [
            (StandardView::Back, ["L", "R", "S", "I"]),
            (StandardView::Left, ["A", "P", "S", "I"]),
            (StandardView::Right, ["P", "A", "S", "I"]),
            (StandardView::Top, ["L", "R", "A", "P"]),
            (StandardView::Bottom, ["R", "L", "A", "P"]),
            (StandardView::Front, ["R", "L", "S", "I"]),
        ] {
            state.show_standard_view(view);
            assert_eq!(state.standard_view(), Some(view));
            assert_eq!(letters(&state.volume_camera(&geometry, [640.0, 480.0])), expected, "{view:?}");
        }
        // Turning by 180° about the vertical axis is the view from the back.
        state.turn_camera(90.0, 0.0);
        state.turn_camera(90.0, 0.0);
        assert_eq!(state.standard_view(), Some(StandardView::Back));
        assert_eq!(letters(&state.volume_camera(&geometry, [640.0, 480.0])), ["L", "R", "S", "I"]);
    }

    #[test]
    fn zoom_and_pan_move_the_camera_as_in_the_other_views() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let ct = load(&index.studies[0].series[0]);
        let geometry = ct.volume().expect("volume").geometry;
        let mut state = ViewState::new(&ct);
        let viewport = [800.0, 600.0];
        let fitted = state.volume_camera(&geometry, viewport);
        assert_eq!(fitted, Camera::fit(&geometry, DQuat::IDENTITY));
        // In a picture of the viewport's size (one pixel per point) the marker is where
        // it is drawn; panning by (30, -20) points moves it by just that.
        let size = [800, 600];
        let marker = DVec3::from_array(MARKER_POSITION);
        let (x, y) = fitted.to_pixel(marker, size);
        state.transform_mut(ViewKind::Volume).pan = [30.0, -20.0];
        let (px, py) = state.volume_camera(&geometry, viewport).to_pixel(marker, size);
        assert!((px - x - 30.0).abs() < 1e-9 && (py - y + 20.0).abs() < 1e-9, "({x}, {y}) -> ({px}, {py})");
        // Zooming about a point keeps it where it is on screen.
        let anchor = [px as f32 - 399.5, py as f32 - 299.5];
        state.transform_mut(ViewKind::Volume).zoom_about(2.5, anchor);
        let camera = state.volume_camera(&geometry, viewport);
        assert!((camera.field_mm - fitted.field_mm / 2.5).abs() < 1e-9);
        let (zx, zy) = camera.to_pixel(marker, size);
        assert!((zx - px).abs() < 1e-3 && (zy - py).abs() < 1e-3, "({px}, {py}) -> ({zx}, {zy})");
    }

    #[test]
    fn the_style_carries_the_look_and_the_window() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let mr = load(&index.studies[0].series[1]);
        let mut state = ViewState::new(&mr);
        state.window = Window::new(700.0, 1400.0);
        state.invert = true;
        state.set_look(Look::Mip, &mr);
        assert_eq!(state.volume_style(), Style { look: Look::Mip, window: Window::new(700.0, 1400.0), invert: true });
    }
}
