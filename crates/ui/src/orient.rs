//! Turning, flipping and tilting from the toolbar and the keyboard, and the
//! words that tell the user about it: the names of tilted views and the
//! "Mirrored" badge.

use egui::containers::menu::{MenuButton, MenuConfig};
use egui::{
    Align, Button, Color32, Context, CornerRadius, FontId, Id, Key, Label, Layout, Modifiers, Painter,
    PopupCloseBehavior, Rect, RichText, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2,
};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::{
    ICON_FLIP, ICON_RESTART_ALT, ICON_ROTATE_90_DEGREES_CCW, ICON_ROTATE_90_DEGREES_CW, ICON_SCREEN_ROTATION_ALT,
    ICON_SWAP_HORIZ, ICON_SWAP_VERT,
};
use summrise_core::imaging::MprAxis;
use summrise_core::{ViewKind, ViewState};
use summrise_i18n::tr;

use crate::theme;
use crate::widgets::{alt_key, alt_shortcut, icon};

/// Degrees the planes turn per key press, and per key press with Shift.
const TILT_STEP: f64 = 1.0;
const TILT_STEP_COARSE: f64 = 5.0;

/// Tilts the planes in the `axis` view by `degrees` clockwise as seen on
/// screen (also when that view is displayed mirrored).
pub(crate) fn tilt_on_screen(state: &mut ViewState, axis: MprAxis, degrees: f64) {
    let mirrored = state.transform(ViewKind::Mpr(axis)).orientation.is_mirrored();
    state.tilt_planes(axis, (if mirrored { -degrees } else { degrees }).to_radians());
}

/// Keys that turn and flip the active view and tilt the planes:
/// `]` / `[` turn right / left, `H` / `V` flip, Alt+Right / Alt+Left tilt by
/// 1° (with Shift by 5°) in a reformat view, Alt+R resets the planes.
///
/// Must run before the plain arrow keys and `R` are handled: those bindings
/// ignore Alt and Shift and would take these keys.
///
/// None of this applies to the 3D view, which turns with its camera.
pub(crate) fn keys(ctx: &Context, state: &mut ViewState, active: ViewKind) {
    if active == ViewKind::Volume {
        return;
    }
    let presses = |modifiers: Modifiers, key: Key| ctx.input_mut(|i| i.count_and_consume_key(modifiers, key));
    if let ViewKind::Mpr(axis) = active {
        // The combination with Shift first: without it in the pattern, Shift is ignored.
        for (modifiers, step) in [(Modifiers::ALT | Modifiers::SHIFT, TILT_STEP_COARSE), (Modifiers::ALT, TILT_STEP)] {
            let steps = presses(modifiers, Key::ArrowRight) as f64 - presses(modifiers, Key::ArrowLeft) as f64;
            if steps != 0.0 {
                tilt_on_screen(state, axis, steps * step);
            }
        }
    }
    if presses(Modifiers::ALT, Key::R) > 0 {
        state.reset_planes();
    }
    for _ in 0..presses(Modifiers::NONE, Key::CloseBracket) {
        state.transform_mut(active).turn_right();
    }
    for _ in 0..presses(Modifiers::NONE, Key::OpenBracket) {
        state.transform_mut(active).turn_left();
    }
    for _ in 0..presses(Modifiers::NONE, Key::H) {
        state.transform_mut(active).flip_horizontally();
    }
    for _ in 0..presses(Modifiers::NONE, Key::V) {
        state.transform_mut(active).flip_vertically();
    }
}

/// The toolbar menu "Rotate and flip": turns and flips the active view and
/// returns tilted planes to the standard ones. It stays open while its
/// entries are used. With several `views` on screen the menu says which one
/// it acts on and lets the user pick another. Not shown for the 3D view,
/// which turns with its camera.
pub(crate) fn menu(ui: &mut Ui, state: &mut ViewState, active: &mut ViewKind, views: &[ViewKind]) {
    if views.contains(&ViewKind::Volume) {
        return;
    }
    let config = MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside);
    let (button, _) = MenuButton::new(icon(ICON_SCREEN_ROTATION_ALT))
        .config(config)
        .ui(ui, |ui| menu_entries(ui, state, active, views));
    button
        .accessible_name(tr!("Rotate and flip"))
        .on_hover_text(tr!("Rotate or flip the active view; return tilted planes to the standard ones"));
}

fn menu_entries(ui: &mut Ui, state: &mut ViewState, active: &mut ViewKind, views: &[ViewKind]) {
    let weak = theme::palette(ui).text_weak;
    ui.set_min_width(230.0);
    ui.set_max_width(340.0);
    if views.len() > 1 {
        // The view the entries below act on: the active one, or the one picked here.
        ui.horizontal(|ui| {
            // Translators: which of the views (axial, coronal, sagittal) the entries below turn and flip.
            ui.label(RichText::new(tr!("View")).small().color(weak));
            for &kind in views {
                ui.selectable_value(active, kind, kind.label()).on_hover_text(tr!("Rotate and flip this view"));
            }
        });
        ui.separator();
    }
    let active = *active;

    let entry = |ui: &mut Ui, glyph: MaterialIcon, name: &str, key: &str, hint: &str| {
        ui.add(Button::new(format!("{} {name}", icon(glyph))).shortcut_text(key.to_owned()))
            .accessible_name(name)
            .on_hover_text(hint)
            .clicked()
    };
    let turn_left = tr!("Turn the picture a quarter turn to the left");
    if entry(ui, ICON_ROTATE_90_DEGREES_CCW, tr!("Rotate left"), "[", turn_left) {
        state.transform_mut(active).turn_left();
    }
    let turn_right = tr!("Turn the picture a quarter turn to the right");
    if entry(ui, ICON_ROTATE_90_DEGREES_CW, tr!("Rotate right"), "]", turn_right) {
        state.transform_mut(active).turn_right();
    }
    let flip_horizontal = tr!("Mirror the picture left to right; a badge marks it as mirrored");
    if entry(ui, ICON_SWAP_HORIZ, tr!("Flip horizontally"), "H", flip_horizontal) {
        state.transform_mut(active).flip_horizontally();
    }
    let flip_vertical = tr!("Mirror the picture top to bottom; a badge marks it as mirrored");
    if entry(ui, ICON_SWAP_VERT, tr!("Flip vertically"), "V", flip_vertical) {
        state.transform_mut(active).flip_vertically();
    }

    ui.separator();
    let reset_name = tr!("Reset planes");
    let reset = Button::new(format!("{} {reset_name}", icon(ICON_RESTART_ALT))).shortcut_text(alt_shortcut("R"));
    let reset = ui
        .add_enabled(state.planes_tilted(), reset)
        .accessible_name(reset_name)
        .on_hover_text(tr!("Return the tilted planes to axial, coronal and sagittal"))
        .on_disabled_hover_text(tr!("The planes are not tilted"));
    if reset.clicked() {
        state.reset_planes();
    }
    let hint = tr!(
        "To tilt the planes, drag a round handle on a line in a reformat view, or press {alt}+Left and {alt}+Right (5° steps with Shift).",
        alt = alt_key()
    );
    // Left-aligned: a menu stretches its entries, which would space the words out.
    ui.with_layout(Layout::top_down(Align::Min), |ui| {
        ui.add(Label::new(RichText::new(hint).small().color(weak)).wrap());
    });
}

/// What a view shows, in words: its standard plane, or for a tilted plane
/// that it is oblique, by how much, and which standard plane it is nearest
/// to. A tilted view is never named like a standard plane alone.
pub(crate) fn view_name(kind: ViewKind, state: &ViewState) -> String {
    let tilt = if let ViewKind::Mpr(axis) = kind { state.plane_tilt(axis) } else { None };
    match tilt {
        Some(tilt) => tr!(
            "Oblique {degrees}° · near {plane}",
            degrees = format!("{:.1}", tilt.degrees),
            plane = tilt.nearest.label()
        ),
        None => kind.label().to_owned(),
    }
}

/// The short form of [`view_name`], for the line that counts the slices.
pub(crate) fn view_short_name(kind: ViewKind, state: &ViewState) -> &'static str {
    match kind {
        ViewKind::Mpr(axis) if state.plane_tilt(axis).is_some() => tr!("Oblique"),
        _ => kind.label(),
    }
}

/// The badge of a mirrored view. It is part of the picture, not an
/// annotation: it is shown whenever the picture is mirrored.
pub(crate) fn mirrored_badge(ui: &Ui, painter: &Painter, viewport: Rect, picture: Rect, id: Id, multi_view: bool) {
    let text = format!("{} {}", icon(ICON_FLIP), tr!("Mirrored"));
    let hint = tr!(
        "This picture is shown mirrored. The letters at its edges give the true directions. Flip it again or reset (R) to return to the standard view."
    );
    badge(ui, painter, viewport, picture, id.with("mirrored"), 0, multi_view, &text, tr!("Mirrored"), hint);
}

/// A status badge on the picture of a view, for something the user must not
/// miss: it does not depend on the annotations being shown. Badges are
/// centred under the top edge letter of the picture, one `row` below the
/// other, and stay inside the viewport.
#[allow(clippy::too_many_arguments)]
pub(crate) fn badge(
    ui: &Ui,
    painter: &Painter,
    viewport: Rect,
    picture: Rect,
    id: Id,
    row: usize,
    multi_view: bool,
    text: &str,
    name: &str,
    hint: &str,
) {
    const ROW_HEIGHT: f32 = 24.0;
    let color = theme::DARK.warn;
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(12.0), color);
    let size = galley.size() + vec2(18.0, 6.0);
    // Below the title and the first annotation line of a small view, below the edge letter.
    let visible = picture.intersect(viewport);
    let below_text = viewport.top() + if multi_view { 46.0 } else { 26.0 };
    let (x, top) = if visible.is_positive() {
        (visible.center().x, below_text.max(visible.top() + 26.0))
    } else {
        (viewport.center().x, below_text)
    };
    let half = size.x / 2.0 + 4.0;
    let x = if viewport.width() > 2.0 * half { x.clamp(viewport.left() + half, viewport.right() - half) } else { x };
    let top = (top + row as f32 * ROW_HEIGHT).min(viewport.bottom() - size.y - 4.0).max(viewport.top());
    let rect = Rect::from_min_size(pos2(x - size.x / 2.0, top), size);
    painter.rect(
        rect,
        CornerRadius::same(10),
        Color32::from_black_alpha(205),
        Stroke::new(1.0, color),
        StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(9.0, 3.0), galley, color);
    // Known to assistive technology, with an explanation on hover.
    let response = ui.interact(rect, id.with(row), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, name));
    response.on_hover_text(hint);
}
