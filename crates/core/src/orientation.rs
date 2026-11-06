//! How the picture of a view is turned and mirrored on screen.
//!
//! This is a property of one view of one series ([`ViewTransform`]): the
//! image itself is not changed, nothing is saved, and reset restores the
//! standard orientation. Whatever shows the picture must show everything
//! else through the same orientation — edge letters, crosshair, the value
//! under the pointer — so that what is on screen stays true.

use summrise_imaging::DVec3;

use crate::view::ViewTransform;

/// Orientation of a picture on screen: mirrored left to right first (if at
/// all), then turned clockwise in quarter turns. These are the eight ways a
/// picture fits upright into a rectangle; turning and flipping lead from one
/// to another.
///
/// Points are given as fractions of the picture and of its box on screen:
/// (0, 0) is the top left corner, (1, 1) the bottom right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayOrientation {
    mirrored: bool,
    quarter_turns: u8,
}

impl DisplayOrientation {
    /// As acquired or reformatted: not mirrored, not turned.
    pub const STANDARD: Self = Self::new(false, 0);

    /// All eight orientations.
    pub const ALL: [Self; 8] = [
        Self::new(false, 0),
        Self::new(false, 1),
        Self::new(false, 2),
        Self::new(false, 3),
        Self::new(true, 0),
        Self::new(true, 1),
        Self::new(true, 2),
        Self::new(true, 3),
    ];

    /// The picture mirrored left to right (if `mirrored`) and then turned
    /// clockwise by `quarter_turns` quarter turns.
    pub const fn new(mirrored: bool, quarter_turns: u8) -> Self {
        Self { mirrored, quarter_turns: quarter_turns % 4 }
    }

    /// Whether the picture is a mirror image of the standard one.
    pub const fn is_mirrored(self) -> bool {
        self.mirrored
    }

    /// Quarter turns clockwise (0 to 3), applied after the mirror.
    pub const fn quarter_turns(self) -> u8 {
        self.quarter_turns
    }

    pub fn is_standard(self) -> bool {
        self == Self::STANDARD
    }

    /// Whether the width and the height of the picture change places on screen.
    pub const fn swaps_axes(self) -> bool {
        self.quarter_turns % 2 == 1
    }

    /// Turned a quarter turn clockwise, as seen on screen.
    #[must_use]
    pub const fn turned_right(self) -> Self {
        Self::new(self.mirrored, self.quarter_turns + 1)
    }

    /// Turned a quarter turn counter-clockwise, as seen on screen.
    #[must_use]
    pub const fn turned_left(self) -> Self {
        Self::new(self.mirrored, self.quarter_turns + 3)
    }

    /// What is on screen mirrored left to right.
    #[must_use]
    pub const fn flipped_horizontally(self) -> Self {
        // Mirroring after the turns equals mirroring first and turning the other way.
        Self::new(!self.mirrored, 4 - self.quarter_turns)
    }

    /// What is on screen mirrored top to bottom (a horizontal flip and a half turn).
    #[must_use]
    pub const fn flipped_vertically(self) -> Self {
        Self::new(!self.mirrored, 6 - self.quarter_turns)
    }

    /// Where the point (`u`, `v`) of the picture is shown in its box on screen.
    pub fn to_screen(self, u: f64, v: f64) -> (f64, f64) {
        let u = if self.mirrored { 1.0 - u } else { u };
        match self.quarter_turns {
            0 => (u, v),
            1 => (1.0 - v, u),
            2 => (1.0 - u, 1.0 - v),
            _ => (v, 1.0 - u),
        }
    }

    /// The point of the picture that is shown at (`s`, `t`) of its box on screen.
    pub fn to_image(self, s: f64, t: f64) -> (f64, f64) {
        let (u, v) = match self.quarter_turns {
            0 => (s, t),
            1 => (t, 1.0 - s),
            2 => (1.0 - s, 1.0 - t),
            _ => (1.0 - t, s),
        };
        (if self.mirrored { 1.0 - u } else { u }, v)
    }

    /// The direction on screen (x to the right, y down) of a direction given
    /// along the picture's own right and down.
    pub fn screen_direction(self, right: f64, down: f64) -> (f64, f64) {
        let right = if self.mirrored { -right } else { right };
        match self.quarter_turns {
            0 => (right, down),
            1 => (-down, right),
            2 => (-right, -down),
            _ => (down, -right),
        }
    }

    /// The patient directions that point to the right and down on screen,
    /// for a picture whose own `right` and `down` directions are given.
    /// Edge letters are derived from these, so they describe what is shown.
    pub fn screen_axes(self, right: DVec3, down: DVec3) -> (DVec3, DVec3) {
        let right = if self.mirrored { -right } else { right };
        match self.quarter_turns {
            0 => (right, down),
            1 => (-down, right),
            2 => (-right, -down),
            _ => (down, -right),
        }
    }
}

impl ViewTransform {
    /// Fits the picture into the viewport again; its orientation is kept.
    pub fn fit(&mut self) {
        self.zoom = 1.0;
        self.pan = [0.0, 0.0];
    }

    /// Turns the picture a quarter turn clockwise about the centre of the viewport.
    pub fn turn_right(&mut self) {
        self.orientation = self.orientation.turned_right();
        self.pan = [-self.pan[1], self.pan[0]];
    }

    /// Turns the picture a quarter turn counter-clockwise about the centre of the viewport.
    pub fn turn_left(&mut self) {
        self.orientation = self.orientation.turned_left();
        self.pan = [self.pan[1], -self.pan[0]];
    }

    /// Mirrors what is on screen left to right, about the centre of the viewport.
    pub fn flip_horizontally(&mut self) {
        self.orientation = self.orientation.flipped_horizontally();
        self.pan = [-self.pan[0], self.pan[1]];
    }

    /// Mirrors what is on screen top to bottom, about the centre of the viewport.
    pub fn flip_vertically(&mut self) {
        self.orientation = self.orientation.flipped_vertically();
        self.pan = [self.pan[0], -self.pan[1]];
    }
}

#[cfg(test)]
mod tests {
    use summrise_imaging::{EdgeLabels, MprAxis};
    use summrise_testkit::PhantomOptions;

    use super::*;
    use crate::series::testing::{load, phantom};
    use crate::view::{ViewKind, ViewState};

    const POINTS: [(f64, f64); 5] = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.25, 0.75), (0.5, 0.125)];

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-12 && (a.1 - b.1).abs() < 1e-12
    }

    /// Edge letters (left, right, top, bottom) of a standard axial image in `o`.
    fn axial_letters(o: DisplayOrientation) -> [String; 4] {
        let (right, down) = o.screen_axes(DVec3::X, DVec3::Y);
        let l = EdgeLabels::new(right, down);
        [l.left, l.right, l.top, l.bottom]
    }

    #[test]
    fn there_are_eight_orientations_and_the_default_is_the_standard_one() {
        assert_eq!(DisplayOrientation::default(), DisplayOrientation::STANDARD);
        assert!(DisplayOrientation::STANDARD.is_standard() && !DisplayOrientation::STANDARD.is_mirrored());
        for (i, a) in DisplayOrientation::ALL.iter().enumerate() {
            assert!(a.quarter_turns() < 4);
            assert_eq!(a.swaps_axes(), a.quarter_turns() % 2 == 1);
            assert_eq!(a.is_standard(), i == 0);
            for b in &DisplayOrientation::ALL[i + 1..] {
                assert_ne!(a, b);
                assert!(POINTS.iter().any(|&(u, v)| !close(a.to_screen(u, v), b.to_screen(u, v))), "{a:?} = {b:?}");
            }
        }
        assert_eq!(DisplayOrientation::new(true, 7), DisplayOrientation::new(true, 3));
    }

    #[test]
    fn picture_and_screen_points_map_back_and_forth() {
        for o in DisplayOrientation::ALL {
            for (u, v) in POINTS {
                let (s, t) = o.to_screen(u, v);
                assert!((0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t), "{o:?} leaves the box");
                assert!(close(o.to_image(s, t), (u, v)), "{o:?} ({u}, {v})");
            }
            // Directions follow the points.
            let (s0, t0) = o.to_screen(0.25, 0.5);
            let (s1, t1) = o.to_screen(0.5, 0.5);
            assert!(close(o.screen_direction(0.25, 0.0), (s1 - s0, t1 - t0)), "{o:?} right");
            let (s1, t1) = o.to_screen(0.25, 0.75);
            assert!(close(o.screen_direction(0.0, 0.25), (s1 - s0, t1 - t0)), "{o:?} down");
        }
    }

    #[test]
    fn turning_and_flipping_do_on_screen_what_their_names_say() {
        for o in DisplayOrientation::ALL {
            for (u, v) in POINTS {
                let (s, t) = o.to_screen(u, v);
                // A right turn takes the top edge to the right-hand side.
                assert!(close(o.turned_right().to_screen(u, v), (1.0 - t, s)), "{o:?} right");
                assert!(close(o.turned_left().to_screen(u, v), (t, 1.0 - s)), "{o:?} left");
                assert!(close(o.flipped_horizontally().to_screen(u, v), (1.0 - s, t)), "{o:?} horizontal");
                assert!(close(o.flipped_vertically().to_screen(u, v), (s, 1.0 - t)), "{o:?} vertical");
            }
            assert_eq!(o.turned_right().turned_left(), o);
            assert_eq!(o.turned_right().turned_right().turned_right().turned_right(), o);
            assert_eq!(o.flipped_horizontally().flipped_horizontally(), o);
            assert_eq!(o.flipped_vertically().flipped_vertically(), o);
            // Two different flips are a half turn; one flip changes the handedness.
            assert_eq!(o.flipped_horizontally().flipped_vertically(), o.turned_right().turned_right());
            assert_ne!(o.flipped_horizontally().is_mirrored(), o.is_mirrored());
            assert_ne!(o.flipped_vertically().is_mirrored(), o.is_mirrored());
            assert_eq!(o.turned_right().is_mirrored(), o.is_mirrored());
        }
    }

    #[test]
    fn edge_letters_of_an_axial_image_in_all_eight_orientations() {
        let s = DisplayOrientation::STANDARD;
        // left, right, top, bottom
        assert_eq!(axial_letters(s), ["R", "L", "A", "P"]);
        assert_eq!(axial_letters(s.turned_right()), ["P", "A", "R", "L"]);
        assert_eq!(axial_letters(s.turned_right().turned_right()), ["L", "R", "P", "A"]);
        assert_eq!(axial_letters(s.turned_left()), ["A", "P", "L", "R"]);
        assert_eq!(axial_letters(s.flipped_horizontally()), ["L", "R", "A", "P"]);
        assert_eq!(axial_letters(s.flipped_vertically()), ["R", "L", "P", "A"]);
        assert_eq!(axial_letters(s.flipped_horizontally().turned_right()), ["P", "A", "L", "R"]);
        assert_eq!(axial_letters(s.flipped_horizontally().turned_left()), ["A", "P", "R", "L"]);
        // Eight orientations, eight different sets of letters.
        let mut all: Vec<[String; 4]> = DisplayOrientation::ALL.into_iter().map(axial_letters).collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 8);
        // The letters are those of the directions shown: screen axes stay perpendicular,
        // and a mirrored picture is left-handed.
        for o in DisplayOrientation::ALL {
            let (right, down) = o.screen_axes(DVec3::X, DVec3::Y);
            assert_eq!(right.dot(down), 0.0);
            assert_eq!(right.cross(down).z, if o.is_mirrored() { -1.0 } else { 1.0 }, "{o:?}");
            let (x, y) = o.screen_direction(1.0, 0.0);
            assert_eq!(right * x + down * y, DVec3::X, "{o:?}: screen axes and directions agree");
        }
    }

    #[test]
    fn view_transform_turns_about_the_viewport_centre_and_fit_keeps_the_orientation() {
        let mut t = ViewTransform::default();
        assert!(t.orientation.is_standard());
        t.zoom_about(2.0, [0.0, 0.0]);
        t.pan = [10.0, 4.0];
        // What was right of the centre is below it after a right turn.
        t.turn_right();
        assert_eq!((t.pan, t.zoom, t.orientation.quarter_turns()), ([-4.0, 10.0], 2.0, 1));
        t.turn_left();
        assert_eq!((t.pan, t.orientation), ([10.0, 4.0], DisplayOrientation::STANDARD));
        t.flip_horizontally();
        assert_eq!((t.pan, t.orientation.is_mirrored()), ([-10.0, 4.0], true));
        t.flip_vertically();
        assert_eq!((t.pan, t.orientation), ([-10.0, -4.0], DisplayOrientation::new(false, 2)));
        // Fitting resets zoom and pan only.
        t.fit();
        assert_eq!((t.zoom, t.pan, t.orientation), (1.0, [0.0, 0.0], DisplayOrientation::new(false, 2)));
        assert_ne!(t, ViewTransform::default());
        t.turn_right();
        t.turn_right();
        assert_eq!(t, ViewTransform::default());
    }

    #[test]
    fn turning_or_flipping_belongs_to_one_view_counts_as_a_change_and_is_undone_by_reset() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let series = load(&index.studies[0].series[0]);
        let coronal = ViewKind::Mpr(MprAxis::Coronal);
        let mut state = ViewState::new(&series);
        assert!(state.is_pristine(&series));
        assert!(state.transform(ViewKind::Stack).orientation.is_standard());

        state.transform_mut(ViewKind::Stack).turn_right();
        assert!(!state.is_pristine(&series), "turning a view is the user's doing");
        state.transform_mut(ViewKind::Stack).turn_left();
        assert!(state.is_pristine(&series));

        // One view of the series; the others are untouched.
        state.transform_mut(coronal).flip_vertically();
        assert!(state.transform(coronal).orientation.is_mirrored());
        assert!(state.transform(ViewKind::Stack).orientation.is_standard());
        assert!(state.transform(ViewKind::Mpr(MprAxis::Axial)).orientation.is_standard());
        assert!(!state.is_pristine(&series));

        // Reset restores the standard orientation; a series always opens in it.
        state.reset(&series);
        assert!(state.transform(coronal).orientation.is_standard());
        assert!(state.is_pristine(&series));
        assert!(ViewState::new(&series).transform(coronal).orientation.is_standard());
    }
}
