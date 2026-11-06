//! The lines of the two other reformat planes in a reformat view, and the
//! handles that tilt them.
//!
//! A line is the true cut of another plane with the plane of this view,
//! through the crosshair. It is drawn through the same mapping as the picture
//! ([`Placement`]), so it is right in every display orientation. Dragging a
//! handle turns the two other planes about this view's normal: the line
//! follows the pointer and the picture of this view stays as it is.

use std::f64::consts::{PI, TAU};

use egui::accesskit::Action;
use egui::{Color32, CursorIcon, Id, Painter, Pos2, Rect, Role, Sense, Stroke, Ui, Vec2, WidgetInfo};
use summrise_core::imaging::MprAxis;
use summrise_core::{ViewKind, ViewState};
use summrise_i18n::tr;

use crate::orient::tilt_on_screen;
use crate::placement::Placement;
use crate::render::Displayed;
use crate::theme;

/// Free space around the crosshair, so that it does not hide what it points at.
const GAP: f32 = 12.0;
const HANDLE_RADIUS: f32 = 4.5;
/// Size of the area in which a handle is grabbed.
const HANDLE_GRIP: f32 = 22.0;
/// Closer to the crosshair than this, the direction of the pointer says nothing.
const DEAD_ZONE: f32 = 6.0;

/// Draws the plane lines and their handles in the `axis` view and lets the
/// user tilt the planes with the handles. Returns whether the user worked
/// with a handle.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    ui: &Ui,
    painter: &Painter,
    viewport: Rect,
    place: &Placement,
    displayed: &Displayed,
    id: Id,
    axis: MprAxis,
    state: &mut ViewState,
) -> bool {
    let Some((cx, cy)) = displayed.patient_to_pixel(state.cursor) else {
        return false;
    };
    let centre = place.to_screen(cx, cy);
    let clip = place.rect.intersect(viewport);
    let reach = (0.3 * clip.width().min(clip.height())).clamp(40.0, 130.0);
    let others = MprAxis::ALL.map(|other| (other != axis).then_some(other));
    // Direction on screen of the line in which `other`'s plane cuts this view's plane.
    let direction = |state: &ViewState, other: MprAxis| -> Option<Vec2> {
        let line = state.plane_line(axis, other)?;
        let (x, y) = displayed.patient_to_pixel(state.cursor + line * 100.0)?;
        let on_screen = place.to_screen(x, y) - centre;
        (on_screen.length() > 1e-3).then(|| on_screen.normalized())
    };
    // A handle sits on the drawn part of its line: some way from the crosshair, on the
    // side `side`, and not beyond the end of the line.
    let handle = |dir: Vec2, side: f32| {
        let (start, end) = clip_line(centre, dir, clip)?;
        let (near, far) = if side > 0.0 { (start, end) } else { (-end, -start) };
        let (near, far) = (near.max(GAP + HANDLE_RADIUS + 2.0), far - HANDLE_RADIUS - 3.0);
        (far >= near).then(|| centre + dir * (side * reach.clamp(near, far)))
    };

    // The handles first, so that the lines are drawn where the pointer has taken them.
    let mut turn = 0.0;
    let mut used = false;
    let mut held = [[false; 2]; 3];
    for (n, other) in others.into_iter().enumerate() {
        let Some(other) = other else { continue };
        let Some(dir) = direction(state, other) else { continue };
        for (k, side) in [1.0, -1.0].into_iter().enumerate() {
            let Some(pos) = handle(dir, side) else { continue };
            let grip = Rect::from_center_size(pos, Vec2::splat(HANDLE_GRIP));
            let response = ui.interact(grip, id.with(("tilt", n, k)), Sense::click_and_drag());
            response.widget_info(|| {
                let label = tr!(
                    "Tilt handle of the {plane} plane in the {view} view",
                    plane = other.label(),
                    view = axis.label()
                );
                WidgetInfo::labeled(Role::Slider, true, label)
            });
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.add_action(Action::Increment);
                node.add_action(Action::Decrement);
            });
            // Assistive technology turns the planes in steps of one degree.
            let steps = ui.input(|i| {
                i.num_accesskit_action_requests(response.id, Action::Increment) as f64
                    - i.num_accesskit_action_requests(response.id, Action::Decrement) as f64
            });
            if steps != 0.0 {
                tilt_on_screen(state, axis, steps);
                used = true;
            }
            if response.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                used = true;
                if let Some(now) = response.interact_pointer_pos() {
                    // A drag is recognised only after the pointer has moved a little:
                    // its first step covers the whole way from where the handle was pressed.
                    let from_press = response.total_drag_delta().filter(|_| response.drag_started());
                    let step = from_press.unwrap_or_else(|| response.drag_delta());
                    turn += angle_travelled(place, (cx, cy), centre, now - step, now);
                }
            }
            held[n][k] = response.dragged() || response.hovered();
            used |= response.clicked() || response.drag_started();
            if !response.dragged() {
                response
                    .on_hover_cursor(CursorIcon::Grab)
                    .on_hover_text(tr!("Drag to tilt the planes about the crosshair"));
            }
        }
    }
    if turn != 0.0 {
        state.tilt_planes(axis, turn);
        // The views drawn before this one show the planes as they were.
        ui.ctx().request_repaint();
    }

    for (n, other) in others.into_iter().enumerate() {
        let Some(other) = other else { continue };
        let Some(dir) = direction(state, other) else { continue };
        let color = theme::view_color(ViewKind::Mpr(other));
        let stroke = Stroke::new(1.0, color.gamma_multiply(0.85));
        if let Some((start, end)) = clip_line(centre, dir, clip) {
            if start < -GAP {
                painter.line_segment([centre + dir * start, centre + dir * end.min(-GAP)], stroke);
            }
            if end > GAP {
                painter.line_segment([centre + dir * start.max(GAP), centre + dir * end], stroke);
            }
        }
        for (k, side) in [1.0, -1.0].into_iter().enumerate() {
            let Some(pos) = handle(dir, side) else { continue };
            if held[n][k] {
                painter.circle(pos, HANDLE_RADIUS + 2.0, color, Stroke::new(1.5, Color32::WHITE));
            } else {
                painter.circle(pos, HANDLE_RADIUS, color, Stroke::new(1.0, Color32::from_black_alpha(190)));
            }
        }
    }
    used
}

/// The angle (radians) by which the pointer went around the crosshair on its
/// way from `before` to `now`, measured in the image: positive from the
/// image's right towards its down, whatever the display orientation.
fn angle_travelled(place: &Placement, crosshair: (f64, f64), centre: Pos2, before: Pos2, now: Pos2) -> f64 {
    if (before - centre).length() < DEAD_ZONE || (now - centre).length() < DEAD_ZONE {
        return 0.0;
    }
    let angle = |pos: Pos2| {
        let (x, y) = place.to_pixel(pos);
        (y - crosshair.1).atan2(x - crosshair.0)
    };
    let travelled = angle(now) - angle(before);
    // The short way round.
    if travelled > PI {
        travelled - TAU
    } else if travelled < -PI {
        travelled + TAU
    } else {
        travelled
    }
}

/// The part of the line `centre + s · dir` that lies inside `rect`, as the
/// range of `s`; `None` when the line misses the rectangle.
fn clip_line(centre: Pos2, dir: Vec2, rect: Rect) -> Option<(f32, f32)> {
    let (mut start, mut end) = (f32::NEG_INFINITY, f32::INFINITY);
    for (c, d, min, max) in [(centre.x, dir.x, rect.left(), rect.right()), (centre.y, dir.y, rect.top(), rect.bottom())]
    {
        if d.abs() < 1e-6 {
            if c < min || c > max {
                return None;
            }
        } else {
            let (a, b) = ((min - c) / d, (max - c) / d);
            start = start.max(a.min(b));
            end = end.min(a.max(b));
        }
    }
    (start < end).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use egui::{pos2, vec2};
    use summrise_core::imaging::{DVec3, MprImage};
    use summrise_core::{DisplayOrientation, ViewTransform};

    use super::*;

    fn rect() -> Rect {
        Rect::from_min_max(pos2(100.0, 50.0), pos2(500.0, 350.0))
    }

    #[test]
    fn lines_are_clipped_to_the_picture() {
        let centre = pos2(200.0, 150.0);
        // Horizontal and vertical lines run from edge to edge.
        assert_eq!(clip_line(centre, vec2(1.0, 0.0), rect()), Some((-100.0, 300.0)));
        assert_eq!(clip_line(centre, vec2(0.0, -1.0), rect()), Some((-200.0, 100.0)));
        // A line at 45° ends where it first meets an edge.
        let (start, end) = clip_line(centre, vec2(1.0, 1.0).normalized(), rect()).expect("crosses");
        let (a, b) = (centre + vec2(1.0, 1.0).normalized() * start, centre + vec2(1.0, 1.0).normalized() * end);
        assert!((a - pos2(100.0, 50.0)).length() < 1e-3 && (b - pos2(400.0, 350.0)).length() < 1e-3, "{a:?} {b:?}");
        // A crosshair outside the picture: the part of the line that crosses it, or nothing.
        let (start, end) = clip_line(pos2(50.0, 100.0), vec2(1.0, 0.0), rect()).expect("crosses");
        assert_eq!((start, end), (50.0, 450.0));
        assert_eq!(clip_line(pos2(50.0, 100.0), vec2(0.0, 1.0), rect()), None);
        assert_eq!(clip_line(pos2(50.0, 400.0), vec2(1.0, 0.0), rect()), None);
    }

    #[test]
    fn a_handle_follows_the_pointer_in_every_display_orientation() {
        // A 100 × 100 pixel reformat of 1 mm pixels; the crosshair at pixel (40, 60).
        let image = MprImage {
            width: 100,
            height: 100,
            values: Vec::new(),
            spacing: 1.0,
            origin: DVec3::ZERO,
            right: DVec3::X,
            down: DVec3::Y,
        };
        let displayed = Displayed::Mpr(Box::new(image));
        let crosshair = (40.0, 60.0);
        for orientation in DisplayOrientation::ALL {
            let transform = ViewTransform { zoom: 1.5, pan: [10.0, -20.0], orientation };
            let place = Placement::of(rect(), &displayed, &transform);
            let centre = place.to_screen(crosshair.0, crosshair.1);
            // The pointer goes a quarter of a circle around the crosshair, clockwise on screen.
            let radius = 80.0;
            let at = |degrees: f32| centre + radius * Vec2::angled(degrees.to_radians());
            let mut travelled = 0.0;
            for step in 0..9 {
                travelled +=
                    angle_travelled(&place, crosshair, centre, at(step as f32 * 10.0), at((step + 1) as f32 * 10.0));
            }
            // In the image that is a quarter turn too: clockwise, or counter-clockwise
            // when the picture is mirrored. A line turned by this angle stays under the pointer.
            let expected = if orientation.is_mirrored() { -90.0 } else { 90.0 };
            assert!((travelled.to_degrees() - expected).abs() < 1e-2, "{orientation:?}: {}", travelled.to_degrees());
            // Across the ±180° seam the short way is taken.
            let across = angle_travelled(&place, crosshair, centre, at(175.0), at(-175.0));
            assert!((across.to_degrees().abs() - 10.0).abs() < 1e-2, "{orientation:?}: {}", across.to_degrees());
            // At the crosshair itself the pointer has no direction.
            assert_eq!(angle_travelled(&place, crosshair, centre, centre + vec2(2.0, 1.0), at(40.0)), 0.0);
        }
    }
}
