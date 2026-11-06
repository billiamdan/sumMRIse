//! Where the picture of a view is on screen: fitted, zoomed, panned, turned
//! and mirrored.
//!
//! Everything that is drawn over the picture or read under the pointer goes
//! through this one mapping — the texture, the value and position under the
//! pointer, clicks, the crosshair lines and their handles, the edge letters
//! and the scale bar — so they agree in every display orientation, also for
//! images whose pixels are not square.

use egui::epaint::{Mesh, Vertex};
use egui::{Color32, Painter, Pos2, Rect, Shape, TextureId, Vec2, pos2, vec2};
use summrise_core::imaging::EdgeLabels;
use summrise_core::{DisplayOrientation, ViewTransform};

use crate::render::Displayed;

/// The mapping between the screen and the pixels of the displayed image.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Placement {
    /// The box the picture occupies on screen, after turning (points).
    pub rect: Rect,
    /// Columns and rows of the image.
    pixels: (usize, usize),
    orientation: DisplayOrientation,
}

impl Placement {
    /// Places an image of `pixels` (columns, rows) with the physical size
    /// `physical` (any unit, right aspect) in `viewport`: fitted, then
    /// zoomed, panned and turned as `transform` says.
    pub(crate) fn new(viewport: Rect, physical: Vec2, pixels: (usize, usize), transform: &ViewTransform) -> Self {
        let orientation = transform.orientation;
        let size = if orientation.swaps_axes() { vec2(physical.y, physical.x) } else { physical };
        let fit = (viewport.width() / size.x).min(viewport.height() / size.y).max(1e-3);
        let center = viewport.center() + vec2(transform.pan[0], transform.pan[1]);
        Self { rect: Rect::from_center_size(center, size * fit * transform.zoom), pixels, orientation }
    }

    pub(crate) fn of(viewport: Rect, displayed: &Displayed, transform: &ViewTransform) -> Self {
        Self::new(viewport, displayed.physical_size(), displayed.pixel_size(), transform)
    }

    /// The image pixel (column, row; fractional, pixel centres at whole
    /// numbers) shown at the screen position `pos`.
    pub(crate) fn to_pixel(self, pos: Pos2) -> (f64, f64) {
        let s = f64::from((pos.x - self.rect.min.x) / self.rect.width());
        let t = f64::from((pos.y - self.rect.min.y) / self.rect.height());
        let (u, v) = self.orientation.to_image(s, t);
        (u * self.pixels.0 as f64 - 0.5, v * self.pixels.1 as f64 - 0.5)
    }

    /// The screen position of the image pixel (`x`, `y`).
    pub(crate) fn to_screen(self, x: f64, y: f64) -> Pos2 {
        let (s, t) = self.orientation.to_screen((x + 0.5) / self.pixels.0 as f64, (y + 0.5) / self.pixels.1 as f64);
        pos2(self.rect.min.x + s as f32 * self.rect.width(), self.rect.min.y + t as f32 * self.rect.height())
    }

    /// Draws the texture of the image turned and mirrored into its box. The
    /// image itself is not rendered again.
    pub(crate) fn paint(&self, painter: &Painter, texture: TextureId) {
        let mut mesh = Mesh::with_texture(texture);
        let r = self.rect;
        let corners = [
            (r.left_top(), 0.0, 0.0),
            (r.right_top(), 1.0, 0.0),
            (r.right_bottom(), 1.0, 1.0),
            (r.left_bottom(), 0.0, 1.0),
        ];
        for (pos, s, t) in corners {
            let (u, v) = self.orientation.to_image(s, t);
            mesh.vertices.push(Vertex { pos, uv: pos2(u as f32, v as f32), color: Color32::WHITE });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        painter.add(Shape::mesh(mesh));
    }

    /// Screen points per millimetre (the same in both directions), when the
    /// pixel spacing is known.
    pub(crate) fn points_per_mm(&self, displayed: &Displayed) -> Option<f32> {
        // What runs across the screen is the image's height after a quarter turn.
        let width_mm = if self.orientation.swaps_axes() {
            self.pixels.1 as f64 * displayed.mm_per_pixel_y()?
        } else {
            self.pixels.0 as f64 * displayed.mm_per_pixel_x()?
        };
        Some(self.rect.width() / width_mm as f32)
    }

    /// Orientation letters for the four edges of the picture as it is shown:
    /// from the patient directions that point right and down on screen.
    pub(crate) fn edge_labels(&self, displayed: &Displayed) -> Option<EdgeLabels> {
        let (right, down) = displayed.screen_axes()?;
        let (right, down) = self.orientation.screen_axes(right, down);
        Some(EdgeLabels::new(right, down))
    }
}

#[cfg(test)]
mod tests {
    use summrise_core::imaging::ImagePlane;

    use super::*;

    /// 200 columns × 100 rows with Pixel Spacing 2.0 \ 0.5: rows are 2 mm
    /// apart and columns 0.5 mm, so the image is 100 mm wide and 200 mm high.
    const PIXELS: (usize, usize) = (200, 100);
    const ROW_SPACING: f64 = 2.0;
    const COL_SPACING: f64 = 0.5;

    fn viewport() -> Rect {
        Rect::from_min_size(pos2(300.0, 50.0), vec2(640.0, 480.0))
    }

    /// A standard axial image with pixels that are not square.
    fn axial() -> Displayed {
        let plane = ImagePlane::from_dicom(
            [-50.0, -100.0, 0.0],
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            [ROW_SPACING, COL_SPACING],
            PIXELS.1 as u32,
            PIXELS.0 as u32,
        );
        Displayed::Frame {
            index: 0,
            width: PIXELS.0 as u32,
            height: PIXELS.1 as u32,
            plane,
            spacing: Some([ROW_SPACING, COL_SPACING]),
        }
    }

    fn transform(orientation: DisplayOrientation) -> ViewTransform {
        ViewTransform { orientation, ..ViewTransform::default() }
    }

    fn place(t: &ViewTransform) -> Placement {
        Placement::of(viewport(), &axial(), t)
    }

    fn letters(l: &EdgeLabels) -> [&str; 4] {
        [&l.left, &l.right, &l.top, &l.bottom]
    }

    fn same(a: Rect, b: Rect) -> bool {
        (a.min - b.min).length() < 1e-3 && (a.max - b.max).length() < 1e-3
    }

    #[test]
    fn standard_orientation_is_the_fitted_centred_picture() {
        let p = place(&ViewTransform::default());
        // 100 mm × 200 mm fitted into 640 × 480 points: 2.4 points per mm.
        assert!(same(p.rect, Rect::from_center_size(viewport().center(), vec2(240.0, 480.0))), "{:?}", p.rect);
        assert!((p.points_per_mm(&axial()).expect("spacing") - 2.4).abs() < 1e-5);
        // Pixel centres: the first pixel sits half a pixel inside the top left corner.
        assert!((p.to_screen(-0.5, -0.5) - p.rect.left_top()).length() < 1e-3);
        assert!((p.to_screen(199.5, 99.5) - p.rect.right_bottom()).length() < 1e-3);
        let (x, y) = p.to_pixel(p.rect.left_top());
        assert!((x + 0.5).abs() < 1e-3 && (y + 0.5).abs() < 1e-3, "({x}, {y})");
    }

    #[test]
    fn screen_and_pixel_round_trip_in_all_eight_orientations_with_non_square_pixels() {
        let pixels = [(0.0, 0.0), (199.0, 0.0), (0.0, 99.0), (199.0, 99.0), (17.25, 63.5), (-0.5, -0.5), (199.5, 99.5)];
        for orientation in DisplayOrientation::ALL {
            let t = ViewTransform { zoom: 1.7, pan: [23.0, -11.0], orientation };
            let p = place(&t);
            let points_per_mm = p.points_per_mm(&axial()).expect("spacing is known");
            // The box has the physical aspect of the image, on its side after a quarter turn.
            let (w_mm, h_mm) = if orientation.swaps_axes() { (200.0, 100.0) } else { (100.0, 200.0) };
            assert!((p.rect.width() / points_per_mm - w_mm).abs() < 1e-3, "{orientation:?}");
            assert!((p.rect.height() / points_per_mm - h_mm).abs() < 1e-3, "{orientation:?}");
            assert!((p.rect.center() - (viewport().center() + vec2(23.0, -11.0))).length() < 1e-3);

            for (x, y) in pixels {
                let pos = p.to_screen(x, y);
                assert!(p.rect.expand(0.01).contains(pos), "{orientation:?}: pixel ({x}, {y}) is outside the picture");
                let (bx, by) = p.to_pixel(pos);
                assert!((bx - x).abs() < 1e-3 && (by - y).abs() < 1e-3, "{orientation:?}: ({x}, {y}) -> ({bx}, {by})");
            }
            // The same scale in both directions although the pixels are not square: one
            // column is 0.5 mm, one row 2 mm, wherever they point on screen.
            let origin = p.to_screen(40.0, 40.0);
            let column = p.to_screen(41.0, 40.0) - origin;
            let row = p.to_screen(40.0, 41.0) - origin;
            assert!((column.length() / points_per_mm - COL_SPACING as f32).abs() < 1e-3, "{orientation:?}");
            assert!((row.length() / points_per_mm - ROW_SPACING as f32).abs() < 1e-3, "{orientation:?}");
            assert!(column.dot(row).abs() < 1e-3, "rows and columns stay perpendicular");
            // Mirrored pictures are left-handed on screen.
            let handedness = column.x * row.y - column.y * row.x;
            assert_eq!(handedness < 0.0, orientation.is_mirrored(), "{orientation:?}");
        }
    }

    #[test]
    fn a_right_turn_puts_the_first_pixel_in_the_top_right_corner() {
        let standard = place(&ViewTransform::default());
        let mut t = ViewTransform::default();
        t.turn_right();
        let turned = place(&t);
        // 200 mm wide and 100 mm high now, fitted to the width: 3.2 points per mm.
        assert!(
            same(turned.rect, Rect::from_center_size(viewport().center(), vec2(640.0, 320.0))),
            "{:?}",
            turned.rect
        );
        assert!((turned.points_per_mm(&axial()).expect("spacing") - 3.2).abs() < 1e-5);
        assert!((turned.to_screen(-0.5, -0.5) - turned.rect.right_top()).length() < 1e-3);
        assert!((turned.to_screen(199.5, -0.5) - turned.rect.right_bottom()).length() < 1e-3);
        assert!((turned.to_screen(-0.5, 99.5) - turned.rect.left_top()).length() < 1e-3);
        // One step along a row of the image (0.5 mm) goes down on screen.
        let step = turned.to_screen(11.0, 5.0) - turned.to_screen(10.0, 5.0);
        assert!(step.x.abs() < 1e-3 && (step.y - 0.5 * 3.2).abs() < 1e-3, "{step:?}");
        // The pixel under a point of the picture box is another one than before the turn.
        let inside = turned.rect.left_top() + vec2(1.6, 1.6);
        let (x, y) = turned.to_pixel(inside);
        assert!((x - 0.5).abs() < 1e-3 && (y - 99.25).abs() < 1e-3, "({x}, {y})");
        assert_ne!(standard.rect, turned.rect);
    }

    #[test]
    fn edge_letters_follow_the_picture_in_every_orientation() {
        let image = axial();
        let labels = |t: &ViewTransform| place(t).edge_labels(&image).expect("the image has a plane");
        let mut t = ViewTransform::default();
        // left, right, top, bottom
        assert_eq!(letters(&labels(&t)), ["R", "L", "A", "P"]);
        t.turn_right();
        assert_eq!(letters(&labels(&t)), ["P", "A", "R", "L"]);
        t.turn_right();
        assert_eq!(letters(&labels(&t)), ["L", "R", "P", "A"]);
        t.turn_right();
        assert_eq!(letters(&labels(&t)), ["A", "P", "L", "R"]);
        t.turn_right();
        assert_eq!(letters(&labels(&t)), ["R", "L", "A", "P"]);
        t.flip_horizontally();
        assert_eq!(letters(&labels(&t)), ["L", "R", "A", "P"]);
        t.turn_left();
        assert_eq!(letters(&labels(&t)), ["A", "P", "R", "L"]);
        t.flip_vertically();
        assert_eq!(letters(&labels(&t)), ["A", "P", "L", "R"]);
        t.flip_horizontally();
        assert_eq!(letters(&labels(&t)), ["P", "A", "L", "R"]);

        // The letters are those of what is on screen: the patient position under a point
        // near the edge that reads "L" is to the patient's left of the centre, and so on.
        for orientation in DisplayOrientation::ALL {
            let p = place(&transform(orientation));
            let l = labels(&transform(orientation));
            let patient = |pos: Pos2| {
                let (x, y) = p.to_pixel(pos);
                image.pixel_to_patient(x, y).expect("plane")
            };
            let centre = patient(p.rect.center());
            for (edge, pos) in [
                (&l.left, p.rect.left_center()),
                (&l.right, p.rect.right_center()),
                (&l.top, p.rect.center_top()),
                (&l.bottom, p.rect.center_bottom()),
            ] {
                let d = patient(pos) - centre;
                let expected = match edge.as_str() {
                    "L" => d.x > 1.0,
                    "R" => d.x < -1.0,
                    "P" => d.y > 1.0,
                    "A" => d.y < -1.0,
                    other => panic!("unexpected letter {other:?}"),
                };
                assert!(expected, "{orientation:?}: edge {edge} lies towards {d:?}");
            }
        }
        // No letters without geometry.
        let no_plane = Displayed::Frame { index: 0, width: 4, height: 4, plane: None, spacing: None };
        assert!(Placement::of(viewport(), &no_plane, &ViewTransform::default()).edge_labels(&no_plane).is_none());
        assert_eq!(Placement::of(viewport(), &no_plane, &ViewTransform::default()).points_per_mm(&no_plane), None);
    }

    #[test]
    fn zooming_about_the_pointer_keeps_its_pixel_in_place_in_every_orientation() {
        let pointer = viewport().center() + vec2(-70.0, 55.0);
        let anchor = [pointer.x - viewport().center().x, pointer.y - viewport().center().y];
        for orientation in DisplayOrientation::ALL {
            let mut t = ViewTransform { zoom: 1.0, pan: [12.0, -7.0], orientation };
            let before = place(&t).to_pixel(pointer);
            t.zoom_about(2.5, anchor);
            let after = place(&t).to_pixel(pointer);
            assert!((before.0 - after.0).abs() < 0.02 && (before.1 - after.1).abs() < 0.02, "{orientation:?}");
            // Panning moves the picture with the pointer, whatever its orientation.
            let grabbed = place(&t).to_pixel(pointer);
            t.pan = [t.pan[0] + 31.0, t.pan[1] - 18.0];
            let moved = place(&t).to_pixel(pointer + vec2(31.0, -18.0));
            assert!((grabbed.0 - moved.0).abs() < 0.02 && (grabbed.1 - moved.1).abs() < 0.02, "{orientation:?}");
        }
    }

    #[test]
    fn turning_a_view_keeps_the_centre_of_a_square_picture_in_place() {
        // What is at the centre of the viewport stays there when the picture is turned or
        // flipped (exactly so for a picture whose fit does not change).
        let square = Displayed::Frame { index: 0, width: 64, height: 64, plane: None, spacing: Some([1.5, 1.5]) };
        let at_centre = |t: &ViewTransform| Placement::of(viewport(), &square, t).to_pixel(viewport().center());
        let mut t = ViewTransform { zoom: 3.0, pan: [40.0, -25.0], ..ViewTransform::default() };
        let before = at_centre(&t);
        for step in [
            ViewTransform::turn_right,
            ViewTransform::flip_vertically,
            ViewTransform::turn_left,
            ViewTransform::flip_horizontally,
        ] {
            step(&mut t);
            let now = at_centre(&t);
            assert!((before.0 - now.0).abs() < 1e-3 && (before.1 - now.1).abs() < 1e-3, "{:?}: {now:?}", t.orientation);
        }
    }
}
