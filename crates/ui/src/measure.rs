//! Measuring in the viewer: the toolbar menu "Measure", its keys, the pointer
//! on a picture, the lines, handles and labels, the one-time notice and the
//! scale bar's note. What a measurement is and what it reads is in
//! `summrise_core::measure`.
//!
//! Everything here works per picture: [`show`] takes the picture's placement
//! and what it displays, and the measurements shown are those that belong to
//! that picture, so a measurement on image 33 appears wherever image 33 is
//! drawn. All positions go through [`Placement`], so lines follow zoom, pan,
//! turn and mirror; labels are drawn upright at their screen position.

use egui::containers::menu::MenuButton;
use egui::{
    Align2, Button, Color32, Context, CursorIcon, Id, Key, Modifiers, Painter, PointerButton, Pos2, Rect, Response,
    RichText, Role, Sense, Shape, Stroke, Ui, WidgetInfo, pos2, vec2,
};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::{
    ICON_ARCHITECTURE, ICON_CROP_SQUARE, ICON_DELETE, ICON_DELETE_SWEEP, ICON_RADIO_BUTTON_UNCHECKED, ICON_SQUARE_FOOT,
    ICON_STRAIGHTEN,
};
use summrise_core::dicom::Series;
use summrise_core::imaging::Calibration;
use summrise_core::measure::approximate_notice;
use summrise_core::{
    Grip, MeasureKind, MeasuredPicture, Measurement, Measurements, PictureValues, SeriesData, Settings, Tool, ViewKind,
    ViewState,
};
use summrise_i18n::{tr, tr_ctx};

use crate::placement::Placement;
use crate::render::{Displayed, Picture, Rendered};
use crate::theme;
use crate::viewport::ViewportInput;
use crate::widgets::{elide_lines, icon, overlay_block, text_width};

/// Distance from a handle at which it is taken hold of, in points (D6).
const HANDLE_GRIP: f32 = 8.0;
/// Distance from a line at which it is taken hold of, in points.
const LINE_GRIP: f32 = 5.0;
const HANDLE_RADIUS: f32 = 3.5;
const LABEL_SIZE: f32 = 12.0;
const LABEL_LINE_HEIGHT: f32 = LABEL_SIZE + 3.0;
/// Corners of the polygon an ellipse is drawn and taken hold of as.
const ELLIPSE_STEPS: usize = 96;

/// Measurements are cyan: unlike the crosshair lines (red, green, yellow), the
/// orientation letters (amber) and a PET in its usual colours.
const COLOR: Color32 = Color32::from_rgb(0x4D, 0xE3, 0xE8);
const COLOR_SELECTED: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const HALO: Color32 = Color32::from_black_alpha(170);

fn kind_icon(kind: MeasureKind) -> MaterialIcon {
    match kind {
        MeasureKind::Length => ICON_STRAIGHTEN,
        MeasureKind::Angle => ICON_ARCHITECTURE,
        MeasureKind::Ellipse => ICON_RADIO_BUTTON_UNCHECKED,
        MeasureKind::Rectangle => ICON_CROP_SQUARE,
    }
}

/// The key of each measuring tool.
const KEYS: [(Key, &str, MeasureKind); 4] = [
    (Key::D, "D", MeasureKind::Length),
    (Key::Y, "Y", MeasureKind::Angle),
    (Key::E, "E", MeasureKind::Ellipse),
    (Key::Q, "Q", MeasureKind::Rectangle),
];

fn kind_hint(kind: MeasureKind) -> &'static str {
    match kind {
        MeasureKind::Length => tr!("Length: drag from one point to the other (D)"),
        MeasureKind::Angle => tr!("Angle: click three points, the corner second (Y)"),
        MeasureKind::Ellipse => tr!("Ellipse: drag across a region for its area and values (E)"),
        MeasureKind::Rectangle => tr!("Rectangle: drag across a region for its area and values (Q)"),
    }
}

/// The toolbar menu "Measure": it shows the chosen tool while one is chosen.
/// Not offered for the 3D view, which has no measurements.
pub(crate) fn menu(ui: &mut Ui, tool: &mut Tool, state: Option<&mut ViewState>, views: &[ViewKind]) {
    if views.contains(&ViewKind::Volume) {
        return;
    }
    let chosen = if let Tool::Measure(kind) = *tool { Some(kind) } else { None };
    let glyph = chosen.map_or(ICON_SQUARE_FOOT, kind_icon);
    // Translators: the toolbar menu with the tools that measure lengths, angles and regions.
    let measure = tr!("Measure");
    let text = format!("{} {}", icon(glyph), chosen.map_or(measure, MeasureKind::label));
    let button = MenuButton::from_button(Button::selectable(chosen.is_some(), text));
    let (response, _) = button.ui(ui, |ui| {
        ui.set_min_width(210.0);
        for (_, key, kind) in KEYS {
            let entry = Button::selectable(chosen == Some(kind), format!("{} {}", icon(kind_icon(kind)), kind.label()))
                .shortcut_text(key);
            if ui.add(entry).accessible_name(kind.label()).on_hover_text(kind_hint(kind)).clicked() {
                *tool = Tool::Measure(kind);
            }
        }
        ui.separator();
        let store = state.map(|state| &mut state.measurements);
        delete_entries(ui, store);
    });
    let name = match chosen {
        Some(kind) => tr!("Measure: {tool}", tool = kind.label()),
        None => measure.to_owned(),
    };
    response
        .accessible_name(name)
        .on_hover_text(tr!("Measure lengths, angles and regions (D, Y, E, Q). Approximate — not for diagnosis."));
}

fn delete_entries(ui: &mut Ui, mut store: Option<&mut Measurements>) {
    // Translators: the Delete key, as printed on keyboards of your language.
    let key = tr_ctx!("key name", "Del");
    let name = tr!("Delete the selected one");
    let any_selected = store.as_deref().is_some_and(|store| store.selected().is_some());
    let entry = Button::new(format!("{} {name}", icon(ICON_DELETE))).shortcut_text(key);
    let clicked = ui
        .add_enabled(any_selected, entry)
        .accessible_name(name)
        .on_hover_text(tr!("Remove the selected measurement (Delete or Backspace)"))
        .on_disabled_hover_text(tr!("With a measuring tool chosen, click a measurement to select it"))
        .clicked();
    if clicked && let Some(store) = store.as_deref_mut() {
        store.delete_selected();
    }
    let name = tr!("Delete all in this series");
    let any = store.as_deref().is_some_and(|store| !store.is_empty());
    let clicked = ui
        .add_enabled(any, Button::new(format!("{} {name}", icon(ICON_DELETE_SWEEP))))
        .accessible_name(name)
        .on_hover_text(tr!("Remove every measurement on every image of this series"))
        .clicked();
    if clicked && let Some(store) = store {
        store.clear();
    }
}

/// D, Y, E and Q choose a measuring tool; Delete or Backspace removes the
/// selected measurement; Esc cancels what is being drawn or edited (and is
/// left to others otherwise). None of this applies to the 3D view.
pub(crate) fn keys(ctx: &Context, tool: &mut Tool, state: &mut ViewState, active: ViewKind) {
    if active == ViewKind::Volume {
        return;
    }
    let pressed = |key: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    for (key, _, kind) in KEYS {
        if pressed(key) {
            *tool = Tool::Measure(kind);
        }
    }
    if pressed(Key::Delete) || pressed(Key::Backspace) {
        state.measurements.delete_selected();
    }
    if state.measurements.is_busy() && pressed(Key::Escape) {
        state.measurements.cancel();
    }
}

/// The hint in the status bar while a measuring tool is chosen.
pub(crate) fn status_hint(tool: Tool) -> &'static str {
    if tool == Tool::Measure(MeasureKind::Angle) {
        tr!("Click three points, the corner second · Drag a handle to adjust · Delete: remove · Esc: cancel")
    } else {
        tr!("Drag to draw · Drag a handle to adjust · Delete: remove · Esc: cancel")
    }
}

/// While a measuring tool is chosen the status bar says that results are
/// approximate, also while the pointer is on a picture (D7).
pub(crate) fn status_notice(ui: &mut Ui, tool: Tool, active: ViewKind) {
    if tool.is_measuring() && active != ViewKind::Volume {
        let warn = theme::palette(ui).warn;
        ui.label(RichText::new(approximate_notice()).small().color(warn));
    }
}

/// The notice shown the first time a measuring tool is chosen. Only the fact
/// that it was shown is remembered.
pub(crate) fn notice(ctx: &Context, tool: Tool, settings: &mut Settings) {
    let id = Id::unique("measure notice");
    if tool.is_measuring() && !settings.measure_notice_shown {
        settings.measure_notice_shown = true;
        ctx.data_mut(|d| d.insert_temp(id, true));
    }
    if !ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        return;
    }
    let response = egui::Modal::new(id.with("modal")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading(tr!("Measurements are approximate"));
        ui.add_space(6.0);
        let warn = theme::palette(ui).warn;
        // Translators: keep this statement unambiguous and at least as strong as the English.
        ui.label(RichText::new(tr!("They are not for diagnosis.")).color(warn));
        ui.add_space(6.0);
        ui.label(tr!(
            "A result depends on where you place the points, on the contrast you see them in and on the scale recorded in the images. Each label says where its scale comes from. Ask your doctor about any size that matters to you."
        ));
        ui.add_space(6.0);
        ui.label(tr!("Measurements are not saved: they are gone when the study is closed."));
        ui.add_space(10.0);
        ui.vertical_centered(|ui| ui.button(tr!("Understood")).clicked()).inner
    });
    if response.inner || response.should_close() {
        ctx.data_mut(|d| d.remove::<bool>(id));
    }
}

/// The label of the scale bar: images calibrated only at the detector say so.
pub(crate) fn scale_label(length: f32, displayed: &Displayed, series: &SeriesData, meta: &Series) -> String {
    let length = format!("{length:.0}");
    let at_detector = match displayed {
        Displayed::Frame { index, .. } => calibration(series, meta, *index) == Calibration::ImagerPixelSpacing,
        Displayed::Mpr(_) => false,
    };
    if at_detector {
        // Translators: the label of the scale bar on an X-ray image, whose scale holds at the detector only.
        tr!("{length} mm at detector", length = length)
    } else {
        format!("{length} mm")
    }
}

/// Where the scale of acquired image `index` comes from.
fn calibration(series: &SeriesData, meta: &Series, index: usize) -> Calibration {
    series
        .frames()
        .get(index)
        .and_then(|info| meta.instances.get(info.instance))
        .map_or(Calibration::Uncalibrated, |header| header.calibration)
}

/// The picture as measuring sees it: `None` only if its volume has gone.
fn picture_of(displayed: &Displayed, input: &ViewportInput<'_>, state: &ViewState) -> Option<MeasuredPicture> {
    match displayed {
        Displayed::Frame { index, width, height, plane, spacing } => Some(MeasuredPicture::acquired(
            *index,
            (*width, *height),
            plane.as_ref(),
            *spacing,
            calibration(input.series, input.meta, *index),
        )),
        Displayed::Mpr(image) => {
            let volume = input.series.stack_volume(state.stack())?;
            Some(MeasuredPicture::reformat(image, state.stack(), &volume.geometry))
        }
    }
}

fn values_of<'a>(displayed: &'a Displayed, series: &'a SeriesData, state: &ViewState) -> PictureValues<'a> {
    match displayed {
        Displayed::Frame { index, .. } => PictureValues::of_frame(series, *index),
        Displayed::Mpr(image) => PictureValues::Reformat { image, slab: state.slab.map(|slab| slab.mode) },
    }
}

/// One picture's measurements: the pointer while a measuring tool is chosen
/// (`pointer_here`: when it is meant for this picture, not for another tile of
/// the grid), and the measurements that belong to this picture. Returns whether
/// the user worked with them. Call it for every picture drawn.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    ui: &Ui,
    response: &Response,
    painter: &Painter,
    viewport: Rect,
    place: &Placement,
    rendered: &Rendered,
    input: &ViewportInput<'_>,
    state: &mut ViewState,
    pointer_here: bool,
) -> bool {
    let Some(picture) = picture_of(&rendered.displayed, input, state) else {
        return false;
    };
    let kind = if let Tool::Measure(kind) = input.tool { Some(kind) } else { None };
    state.measurements.tool_changed(kind);
    let used = kind.filter(|_| pointer_here).is_some_and(|kind| {
        let can_draw = rendered.picture == Picture::Image;
        pointer(ui, response, place, &picture, can_draw, kind, &mut state.measurements)
    });
    let units = input.series.units();
    let values = values_of(&rendered.displayed, input.series, state);
    let store = &state.measurements;
    let selected = store.selected();
    let shown = store.on_picture(&picture).map(|m| (m, selected == Some(m.id())));
    let draft = store.draft_on(&picture).map(|m| (m, true));
    for (measurement, highlighted) in shown.chain(draft) {
        let outline = screen_outline(measurement, place, &picture);
        draw_outline(painter, &outline, highlighted);
        if kind.is_some() || highlighted {
            for (x, y) in measurement.handles(&picture) {
                draw_handle(painter, place.to_screen(x, y), highlighted);
            }
        }
        let readout = measurement.readout(&picture, &values, units);
        if !readout.lines.is_empty() {
            let id = input.id.with(("measurement", measurement.id()));
            draw_label(ui, painter, viewport, &outline, id, &readout.lines, &readout.details);
        }
    }
    used
}

/// What the pointer does on a picture while a measuring tool is chosen (D6):
/// a drag takes hold of what is under it or draws; clicks select and place the
/// points of an angle. Right-drag, middle-drag and Shift-drag stay with the
/// viewport. Returns whether the pointer did something here.
fn pointer(
    ui: &Ui,
    response: &Response,
    place: &Placement,
    picture: &MeasuredPicture,
    can_draw: bool,
    kind: MeasureKind,
    store: &mut Measurements,
) -> bool {
    let shift = ui.input(|i| i.modifiers.shift);
    let mut used = false;
    if response.drag_started_by(PointerButton::Primary) && !shift {
        used = true;
        // A drag is recognised only after the pointer has moved a little: it starts where it was pressed.
        let from = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
        if let Some(from) = from.filter(|_| store.draft_on(picture).is_none()) {
            match hit_test(place, picture, store, from, false, kind) {
                Some((id, grip)) => store.grab(picture, id, grip, place.to_pixel(from)),
                None if can_draw && place.rect.contains(from) => store.begin(kind, picture, place.to_pixel(from)),
                None => store.select(None),
            }
        }
    }
    if response.dragged_by(PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        store.drag(picture, place.to_pixel(pos));
        store.follow(picture, place.to_pixel(pos));
    }
    if response.drag_stopped_by(PointerButton::Primary) {
        used = true;
        store.release();
        if kind != MeasureKind::Angle {
            store.finish(picture);
        } else if !shift
            && store.draft_on(picture).is_some()
            && let Some(pos) = response.interact_pointer_pos()
        {
            // Pressed and moved a little on its way to a click: a point all the same.
            store.place(picture, place.to_pixel(pos));
        }
    }
    if response.clicked_by(PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        used = true;
        if store.draft_on(picture).is_some() {
            store.place(picture, place.to_pixel(pos));
        } else if let Some((id, _)) = hit_test(place, picture, store, pos, true, kind) {
            store.select(Some(id));
        } else {
            store.select(None);
            if kind == MeasureKind::Angle && can_draw && place.rect.contains(pos) {
                store.begin(kind, picture, place.to_pixel(pos));
            }
        }
    }
    // The free end of an angle follows the pointer between the clicks.
    if response.hovered()
        && let Some(pos) = ui.input(|i| i.pointer.hover_pos())
    {
        store.follow(picture, place.to_pixel(pos));
        if !store.is_busy() {
            let hit = hit_test(place, picture, store, pos, false, kind);
            if let Some((_, grip)) = hit {
                ui.ctx().set_cursor_icon(if grip == Grip::Body { CursorIcon::Move } else { CursorIcon::Grab });
            }
        }
    }
    if response.dragged_by(PointerButton::Primary) && store.is_busy() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    used
}

/// What lies under the screen position `pos`: handles first, then lines, then
/// the inside of a region; among those the selected measurement, then the
/// newest. The inside of a region counts when it is selected, or for a click
/// that is not a point of an angle: dragging inside a region that is not
/// selected draws there, so that a length can be measured in an ellipse.
fn hit_test(
    place: &Placement,
    picture: &MeasuredPicture,
    store: &Measurements,
    pos: Pos2,
    click: bool,
    kind: MeasureKind,
) -> Option<(u64, Grip)> {
    let selected = store.selected();
    let mut candidates: Vec<&Measurement> = store.on_picture(picture).collect();
    candidates.reverse();
    candidates.sort_by_key(|m| selected != Some(m.id()));
    for m in &candidates {
        for (n, (x, y)) in m.handles(picture).into_iter().enumerate() {
            if (place.to_screen(x, y) - pos).length() <= HANDLE_GRIP {
                return Some((m.id(), Grip::Handle(n)));
            }
        }
    }
    for m in &candidates {
        let outline = screen_outline(m, place, picture);
        if outline.windows(2).any(|segment| segment_distance(segment[0], segment[1], pos) <= LINE_GRIP) {
            return Some((m.id(), Grip::Body));
        }
    }
    let (x, y) = place.to_pixel(pos);
    candidates
        .iter()
        .filter(|m| selected == Some(m.id()) || (click && kind != MeasureKind::Angle))
        .find(|m| m.region(picture).is_some_and(|region| region.contains(x, y)))
        .map(|m| (m.id(), Grip::Body))
}

fn segment_distance(a: Pos2, b: Pos2, pos: Pos2) -> f32 {
    let along = b - a;
    let t = if along.length_sq() > 0.0 { ((pos - a).dot(along) / along.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + along * t - pos).length()
}

/// The lines of a measurement on screen: through its points, around the four
/// corners of a rectangle or around an ellipse. Computed in pixels, so they
/// are right in every display orientation and for pixels that are not square.
fn screen_outline(measurement: &Measurement, place: &Placement, picture: &MeasuredPicture) -> Vec<Pos2> {
    let handles = measurement.handles(picture);
    let on_screen = |(x, y): (f64, f64)| place.to_screen(x, y);
    match (measurement.kind(), measurement.corners(picture)) {
        (MeasureKind::Rectangle, _) => handles.iter().chain(handles.first()).copied().map(on_screen).collect(),
        (MeasureKind::Ellipse, Some((a, b))) => {
            let (centre, radius) = (((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0), ((b.0 - a.0) / 2.0, (b.1 - a.1) / 2.0));
            (0..=ELLIPSE_STEPS)
                .map(|step| {
                    let (sin, cos) = (std::f64::consts::TAU * step as f64 / ELLIPSE_STEPS as f64).sin_cos();
                    on_screen((centre.0 + radius.0 * cos, centre.1 + radius.1 * sin))
                })
                .collect()
        }
        _ => handles.into_iter().map(on_screen).collect(),
    }
}

fn draw_outline(painter: &Painter, outline: &[Pos2], highlighted: bool) {
    let color = if highlighted { COLOR_SELECTED } else { COLOR };
    let width = if highlighted { 1.75 } else { 1.25 };
    for stroke in [Stroke::new(width + 2.0, HALO), Stroke::new(width, color)] {
        painter.add(Shape::line(outline.to_vec(), stroke));
    }
}

fn draw_handle(painter: &Painter, at: Pos2, highlighted: bool) {
    let fill = if highlighted { COLOR_SELECTED } else { COLOR };
    painter.circle(at, HANDLE_RADIUS, fill, Stroke::new(1.0, HALO));
}

/// The label beside a measurement: upright, on the right of it unless it would
/// not fit there, and inside the viewport. It is known to assistive
/// technology, with its tooltip as description.
fn draw_label(
    ui: &Ui,
    painter: &Painter,
    viewport: Rect,
    outline: &[Pos2],
    id: Id,
    lines: &[String],
    details: &[String],
) {
    let bounds = Rect::from_points(outline);
    let limits = viewport.shrink(6.0);
    let shown = elide_lines(painter, lines.to_vec(), LABEL_SIZE, limits.width());
    let size = vec2(text_width(painter, &shown, LABEL_SIZE), shown.len() as f32 * LABEL_LINE_HEIGHT);
    let gap = 10.0;
    let middle = bounds.center() - size / 2.0;
    let beside = [
        pos2(bounds.right() + gap, middle.y),
        pos2(bounds.left() - gap - size.x, middle.y),
        pos2(middle.x, bounds.bottom() + gap),
        pos2(middle.x, bounds.top() - gap - size.y),
    ];
    let fits = |at: &Pos2| limits.contains_rect(Rect::from_min_size(*at, size));
    let top_left = beside.iter().copied().find(|at| fits(at)).unwrap_or_else(|| {
        pos2(
            beside[0].x.min(limits.right() - size.x).max(limits.left()),
            beside[0].y.min(limits.bottom() - size.y).max(limits.top()),
        )
    });
    let rect = Rect::from_min_size(top_left, size);
    painter.rect_filled(rect.expand(3.0), 3.0, Color32::from_black_alpha(120));
    overlay_block(painter, top_left, Align2::LEFT_TOP, &shown, LABEL_SIZE);
    let text = lines.join(", ");
    let response = ui.interact(rect, id, Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, &text));
    let details = details.join("\n");
    response.ctx.accesskit_node_builder(response.id, |node| node.set_description(details.clone()));
    response.on_hover_text(details);
}
