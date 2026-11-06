//! The interactive image viewport: display with correct physical aspect,
//! mouse/trackpad interaction, radiological overlays and MPR crosshair.
//!
//! While a series loads, an image that is not decoded yet is shown as a
//! placeholder at its own place, with its own number, position and
//! annotations — never as a blank viewport and never as another image.

use egui::{
    Align2, Color32, CursorIcon, FontId, Id, PointerButton, Pos2, Rect, Role, Sense, Stroke, StrokeKind, Ui,
    WidgetInfo, pos2, vec2,
};
use summrise_core::dicom::{Series, Study, format};
use summrise_core::imaging::DVec3;
use summrise_core::{SeriesData, Settings, Tool, ViewKind, ViewState};
use summrise_i18n::tr;

use crate::crosshair;
use crate::fidelity;
use crate::orient::{self, view_name, view_short_name};
use crate::placement::Placement;
use crate::render::{Picture, Rendered};
use crate::theme::{self, VIEWPORT_BG};
use crate::widgets::{elide_lines, overlay_block, overlay_text, share_row, text_width};

/// Scroll distance (points) per slice step.
const SCROLL_STEP: f32 = 18.0;
const OVERLAY_SIZE: f32 = 12.0;

pub(crate) struct ViewportInput<'a> {
    pub id: Id,
    pub kind: ViewKind,
    pub series: &'a SeriesData,
    pub meta: &'a Series,
    pub study: &'a Study,
    pub rendered: Option<&'a Rendered>,
    pub tool: Tool,
    pub settings: &'a Settings,
    pub is_active: bool,
    pub multi_view: bool,
    /// Show patient/study annotations (all views in single layout, first view otherwise).
    pub show_details: bool,
    /// What to say about a PET drawn over the images (see `pet`).
    pub fusion: Option<&'a crate::pet::Shown>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Hover {
    pub pixel: (usize, usize),
    pub patient: Option<DVec3>,
    pub value: Option<f32>,
    /// The pointer is outside the scanned area (padding, or outside a
    /// reformatted volume): there is no value to show.
    pub outside: bool,
}

#[derive(Default)]
pub(crate) struct ViewportResult {
    pub hover: Option<Hover>,
    pub activated: bool,
}

pub(crate) fn show(ui: &mut Ui, rect: Rect, input: &ViewportInput<'_>, state: &mut ViewState) -> ViewportResult {
    let mut result = ViewportResult::default();
    let response = ui.interact(rect, input.id, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, VIEWPORT_BG);
    let accent = theme::palette(ui).accent;

    let Some(rendered) = input.rendered else {
        let message = match (input.kind, input.series.volume_issue()) {
            (ViewKind::Mpr(_), Some(reason)) => tr!("Reformatting is not available:\n{reason}", reason = reason),
            _ => tr!("No image").to_owned(),
        };
        painter.text(rect.center(), Align2::CENTER_CENTER, message, FontId::proportional(13.0), Color32::GRAY);
        return result;
    };

    let displayed = &rendered.displayed;
    let transform = state.transform(input.kind);
    // Where the picture is on screen: fitted, zoomed, panned, turned and mirrored.
    let place = Placement::of(rect, displayed, &transform);
    let img_rect = place.rect;
    let description = describe(input, state, &rendered.picture);
    match rendered.texture() {
        Some(texture) => place.paint(&painter, texture.id()),
        None => draw_placeholder(ui, &painter, rect, img_rect, &rendered.picture, &description),
    }
    // What the viewport shows, for assistive technology (and UI tests); the
    // stack of a series split into stacks is part of it.
    let progress = loading_progress(input);
    let stack = input.series.stack_label(state.stack());
    response.widget_info(|| {
        let parts = [Some(&description), stack.as_ref(), progress.as_ref()];
        let label = parts.into_iter().flatten().map(String::as_str).collect::<Vec<_>>().join(", ");
        WidgetInfo::labeled(Role::Image, true, label)
    });

    let (pw, ph) = displayed.pixel_size();
    let to_pixel = |pos: Pos2| place.to_pixel(pos);

    // ---- Interaction -------------------------------------------------------
    let (modifiers, scroll, zoom, pointer) =
        ui.input(|i| (i.modifiers, i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
    if response.hovered() {
        if (zoom - 1.0).abs() > f32::EPSILON {
            let anchor = pointer.map_or([0.0, 0.0], |p| [p.x - rect.center().x, p.y - rect.center().y]);
            state.transform_mut(input.kind).zoom_about(zoom, anchor);
        } else if scroll.y.abs() > 0.0 {
            let steps = ui.data_mut(|d| {
                let acc = d.get_temp_mut_or_default::<f32>(input.id.with("scroll"));
                *acc += scroll.y;
                let steps = (*acc / SCROLL_STEP).trunc();
                *acc -= steps * SCROLL_STEP;
                steps
            });
            if steps != 0.0 {
                state.scroll(input.kind, input.series, -(steps as i64));
            }
        }
        ui.ctx().set_cursor_icon(match input.tool {
            Tool::WindowLevel => CursorIcon::Crosshair,
            Tool::Pan => CursorIcon::Grab,
            Tool::Zoom => CursorIcon::ZoomIn,
        });
    }
    let delta = response.drag_delta();
    let primary = response.dragged_by(PointerButton::Primary);
    if delta != egui::Vec2::ZERO {
        result.activated = true;
        let pan =
            response.dragged_by(PointerButton::Middle) || (primary && (input.tool == Tool::Pan || modifiers.shift));
        if pan {
            let t = state.transform_mut(input.kind);
            t.pan = [t.pan[0] + delta.x, t.pan[1] + delta.y];
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if response.dragged_by(PointerButton::Secondary) || (primary && input.tool == Tool::WindowLevel) {
            if !input.series.is_color() {
                state.drag_window(f64::from(delta.x), f64::from(delta.y), input.series);
            }
        } else if primary && input.tool == Tool::Zoom {
            let origin = response.interact_pointer_pos().unwrap_or(rect.center()) - delta;
            state
                .transform_mut(input.kind)
                .zoom_about((-delta.y * 0.01).exp(), [origin.x - rect.center().x, origin.y - rect.center().y]);
        }
    }
    if response.double_clicked() {
        state.transform_mut(input.kind).fit();
    } else if response.clicked() {
        result.activated = true;
        if let Some(pos) = response.interact_pointer_pos() {
            let (x, y) = to_pixel(pos);
            if let Some(p) = displayed.pixel_to_patient(x, y) {
                state.set_cursor(p, input.series);
            }
        }
    }
    if response.clicked_by(PointerButton::Secondary) || response.drag_started() {
        result.activated = true;
    }

    // Not while a menu or another window covers the view under the pointer.
    if let Some(pos) = pointer.filter(|p| img_rect.contains(*p) && rect.contains(*p) && ui.rect_contains_pointer(rect))
    {
        let (x, y) = to_pixel(pos);
        let (px, py) = (x.round().max(0.0) as usize, y.round().max(0.0) as usize);
        if px < pw && py < ph {
            let outside = fidelity::is_outside_scan(displayed, input.series, px, py);
            result.hover = Some(Hover {
                pixel: (px, py),
                patient: displayed.pixel_to_patient(x, y),
                value: displayed.value_at(input.series, px, py).filter(|_| !outside),
                outside,
            });
        }
    }

    // ---- Crosshair: the lines of the two other planes, with their tilt handles ----
    if let ViewKind::Mpr(axis) = input.kind
        && input.settings.show_overlays
        && crosshair::show(ui, &painter, rect, &place, displayed, input.id, axis, state)
    {
        result.activated = true;
    }

    // ---- Overlays -----------------------------------------------------------
    if input.settings.show_overlays {
        draw_overlays(&painter, rect, &place, input, state, rendered, progress.as_deref());
    } else if let Some(progress) = &progress {
        // Loading progress is a status, not an annotation: it stays when annotations are hidden.
        let corner = rect.left_bottom() + vec2(10.0, -10.0);
        overlay_text(&painter, corner, Align2::LEFT_BOTTOM, progress, OVERLAY_SIZE, theme::OVERLAY_TEXT);
    }
    if progress.is_some() {
        let done = input.series.loaded() as f32 / input.series.len().max(1) as f32;
        let bar = Rect::from_min_size(rect.left_top(), vec2(rect.width() * done.clamp(0.0, 1.0), 2.0));
        painter.rect_filled(bar, 0.0, accent);
    }
    // A PET overlay that is loading or unavailable says so, whether annotations are shown or not.
    crate::pet::draw_status(ui, &painter, rect, img_rect, input);
    // A mirrored picture says so, whether annotations are shown or not.
    if state.transform(input.kind).orientation.is_mirrored() {
        orient::mirrored_badge(ui, &painter, rect, img_rect, input.id, input.multi_view);
    }
    // So does a lossy one (in the row under "Mirrored"), and in privacy mode one with burned-in text.
    if let Some(label) = fidelity::lossy_label(input.kind, input.series, input.meta, state) {
        fidelity::lossy_badge(ui, &painter, rect, img_rect, input.id, input.multi_view, &label);
    }
    if input.settings.hide_patient_details
        && fidelity::shows_burned_in_text(input.kind, input.series, input.meta, state)
    {
        fidelity::burned_in_notice(ui, &painter, rect, input.id, input.multi_view);
    }
    if input.multi_view {
        let color = theme::view_color(input.kind);
        let label = format!(
            "{}  {}/{}",
            view_name(input.kind, state).to_uppercase(),
            state.slice_index(input.kind, input.series) + 1,
            state.slice_count(input.kind, input.series)
        );
        // Shortened ("…") in a view too narrow for it.
        let label = elide_lines(&painter, vec![label], 11.5, rect.width() - 20.0).concat();
        overlay_text(&painter, pos2(rect.left() + 10.0, rect.top() + 8.0), Align2::LEFT_TOP, &label, 11.5, color);
        painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(2.0, color.gamma_multiply(0.8)));
        if input.is_active {
            painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(1.5, accent), StrokeKind::Inside);
        }
    }
    result
}

/// "Loading 300 / 600" while the series of the acquired view still loads.
fn loading_progress(input: &ViewportInput<'_>) -> Option<String> {
    let series = input.series;
    (input.kind == ViewKind::Stack && !series.is_complete())
        .then(|| tr!("Loading {loaded} / {total}", loaded = series.loaded(), total = series.len()))
}

/// What the viewport shows, in words: the image's number, or what stands in
/// for an image that is not there.
fn describe(input: &ViewportInput<'_>, state: &ViewState, picture: &Picture) -> String {
    let number = state.slice_index(input.kind, input.series) + 1;
    let count = state.slice_count(input.kind, input.series);
    match (picture, input.kind) {
        (Picture::Loading, _) => tr!("Loading image {number} / {count}", number = number, count = count),
        (Picture::Failed(_), _) => tr!("Image {number} could not be decoded", number = number),
        (Picture::Image, ViewKind::Stack) => tr!("Image {number} / {count}", number = number, count = count),
        // Translators: {view} is the name of a view, such as Axial or Coronal.
        (Picture::Image, ViewKind::Mpr(_)) => {
            tr!("{view} {number} / {count}", view = view_name(input.kind, state), number = number, count = count)
        }
        // Shown by `volume_view`.
        (Picture::Image, ViewKind::Volume) => view_name(input.kind, state),
    }
}

/// Stands in for an image that is not decoded (yet), at the place and with
/// the size the image has, so that annotations, zoom and pan stay put.
fn draw_placeholder(ui: &Ui, painter: &egui::Painter, rect: Rect, img_rect: Rect, picture: &Picture, title: &str) {
    let p = theme::palette(ui);
    painter.rect_filled(img_rect, 0.0, Color32::from_gray(13));
    painter.rect_stroke(img_rect, 0.0, Stroke::new(1.0, Color32::from_gray(34)), StrokeKind::Inside);
    let visible = img_rect.intersect(rect);
    let center = if visible.is_positive() { visible.center() } else { rect.center() };
    match picture {
        Picture::Failed(reason) => {
            overlay_text(painter, center, Align2::CENTER_BOTTOM, title, 14.0, theme::DARK.warn);
            let width = (visible.width() - 24.0).clamp(120.0, 420.0);
            let galley = painter.layout(reason.clone(), FontId::proportional(11.5), theme::DARK.text_weak, width);
            painter.galley(pos2(center.x - galley.size().x / 2.0, center.y + 6.0), galley, theme::DARK.text_weak);
        }
        Picture::Loading | Picture::Image => {
            egui::Spinner::new()
                .color(p.accent)
                .paint_at(ui, Rect::from_center_size(center - vec2(0.0, 20.0), vec2(24.0, 24.0)));
            overlay_text(painter, center + vec2(0.0, 2.0), Align2::CENTER_TOP, title, 14.0, theme::OVERLAY_TEXT);
        }
    }
}

/// Least space between text blocks in opposite corners of a view, in points.
const CORNER_GAP: f32 = 12.0;

fn draw_overlays(
    painter: &egui::Painter,
    rect: Rect,
    place: &Placement,
    input: &ViewportInput<'_>,
    state: &ViewState,
    rendered: &Rendered,
    progress: Option<&str>,
) {
    let m = 10.0;
    let top = if input.multi_view { 28.0 } else { m };
    let compact = input.multi_view;
    let study = input.study;
    let series = input.series;

    // Top-left: patient (hidden in privacy mode).
    let top_left = (input.show_details && !input.settings.hide_patient_details).then(|| {
        let p = &study.patient;
        let mut lines =
            vec![p.name.as_deref().map(format::person_name).unwrap_or_else(|| tr!("Unknown patient").into())];
        if let Some(id) = p.id.as_ref().filter(|_| !compact) {
            // Translators: the patient ID on the image.
            lines.push(tr!("ID {id}", id = id));
        }
        let demographics: Vec<String> = [
            p.birth_date.as_deref().and_then(format::date).map(|d| tr!("Born {date}", date = d)),
            p.age.as_deref().and_then(format::age),
            p.sex.as_deref().map(format::sex),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !demographics.is_empty() && !compact {
            lines.push(demographics.join(" · "));
        }
        lines
    });

    // Top-right: study and series.
    let mut lines = Vec::new();
    if !compact {
        if let Some(inst) = &study.institution {
            lines.push(inst.clone());
        }
        lines.push(study.title());
        let date = study.date.as_deref().and_then(format::date);
        let time = study.time.as_deref().and_then(format::time);
        if let Some(d) = date {
            lines.push(time.map_or_else(|| d.clone(), |t| format!("{d} {t}")));
        }
    }
    lines.push(match input.meta.number {
        // Translators: S is short for series, followed by the series number; {title} is the series' name.
        Some(number) => tr!("S{number}: {title}", number = number, title = input.meta.title()),
        None => input.meta.title(),
    });
    let top_right = input.show_details.then_some(lines);
    // Blocks in the left and right corners of the same rows share the width of
    // the view: where both do not fit, the text is shortened ("…").
    let available = rect.width() - 2.0 * m - CORNER_GAP;
    let width = |lines: &Option<Vec<String>>| lines.as_deref().map_or(0.0, |l| text_width(painter, l, OVERLAY_SIZE));
    let (left_max, right_max) = share_row(width(&top_left), width(&top_right), available);
    if let Some(lines) = top_left {
        let lines = elide_lines(painter, lines, OVERLAY_SIZE, left_max);
        overlay_block(painter, rect.left_top() + vec2(m, top), Align2::LEFT_TOP, &lines, OVERLAY_SIZE);
    }
    if let Some(lines) = top_right {
        let lines = elide_lines(painter, lines, OVERLAY_SIZE, right_max);
        overlay_block(painter, rect.right_top() + vec2(-m, top), Align2::RIGHT_TOP, &lines, OVERLAY_SIZE);
    }

    // Bottom-left: loading progress and position.
    let index = state.slice_index(input.kind, series);
    let count = state.slice_count(input.kind, series);
    let mut lines = Vec::new();
    lines.extend(progress.map(str::to_owned));
    match input.kind {
        ViewKind::Stack => {
            lines.push(tr!("Image {number} / {count}", number = index + 1, count = count));
            if let Some(info) = series.frames().get(state.slot_index(series)) {
                if let Some(n) = info.instance_number {
                    // Translators: the number of the image file (DICOM Instance Number) and of the frame in it.
                    lines.push(if info.frame > 0 {
                        tr!("Instance {number} · frame {frame}", number = n, frame = info.frame + 1)
                    } else {
                        tr!("Instance {number}", number = n)
                    });
                }
                if let Some(plane) = info.plane {
                    let n = plane.normal();
                    let label = summrise_core::imaging::orientation_label(n);
                    lines.push(format!("{label} {} mm", one_decimal(plane.position.dot(n))));
                }
            }
        }
        ViewKind::Mpr(axis) => {
            let view = view_short_name(input.kind, state);
            lines.push(tr!("{view} {number} / {count}", view = view, number = index + 1, count = count));
            lines.push(format!("{} mm", one_decimal(state.mpr_position(axis))));
            if let Some(slab) = state.slab {
                // Translators: {mode} is MIP, MinIP or Average; the slab's thickness follows in millimetres.
                let thickness = format!("{:.0}", slab.thickness_mm);
                lines.push(tr!("{mode} slab {thickness} mm", mode = slab.mode.label(), thickness = thickness));
            }
        }
        // Shown by `volume_view`.
        ViewKind::Volume => {}
    }
    // Every view of a series split into stacks says which stack it shows, under the image number.
    if let Some(label) = series.stack_label(state.stack()) {
        lines.insert((usize::from(progress.is_some()) + 1).min(lines.len()), label);
    }
    let bottom_left = lines;

    // Bottom-right: contrast and zoom.
    // The lines about a PET come first: above the rest, clear of the scale bar.
    let mut lines = crate::pet::annotation_lines(series, state, input.fusion);
    let table = state.active_lut(series).filter(|_| input.kind == ViewKind::Stack);
    if let Some(table) = table.filter(|_| !series.is_color()) {
        // A contrast table applies to the acquired images only; reformats show their window.
        lines.push(summrise_i18n::tr!("Contrast table").to_owned());
        lines.push(table.name.clone());
    } else if !series.is_color() {
        lines.push(crate::pet::window_annotation(series.units(), &state.window));
        lines.push(state.preset_label(series).map_or_else(|| tr!("Custom window").to_owned(), str::to_owned));
    }
    let percent = format!("{:.0}", state.transform(input.kind).zoom * 100.0);
    lines.push(if state.invert {
        tr!("Zoom {percent}% · Inverted", percent = percent)
    } else {
        tr!("Zoom {percent}%", percent = percent)
    });
    let (left_max, right_max) = share_row(
        text_width(painter, &bottom_left, OVERLAY_SIZE),
        text_width(painter, &lines, OVERLAY_SIZE),
        available,
    );
    let bottom_left = elide_lines(painter, bottom_left, OVERLAY_SIZE, left_max);
    let bottom_right = elide_lines(painter, lines, OVERLAY_SIZE, right_max);
    overlay_block(painter, rect.left_bottom() + vec2(m, -m), Align2::LEFT_BOTTOM, &bottom_left, OVERLAY_SIZE);
    overlay_block(painter, rect.right_bottom() + vec2(-m, -m), Align2::RIGHT_BOTTOM, &bottom_right, OVERLAY_SIZE);
    // The scale bar sits between the two bottom blocks, level with their last two lines.
    let last_two = |lines: &[String]| text_width(painter, &lines[lines.len().saturating_sub(2)..], OVERLAY_SIZE);
    let free = (
        rect.left() + m + last_two(&bottom_left) + CORNER_GAP,
        rect.right() - m - last_two(&bottom_right) - CORNER_GAP,
    );

    // Orientation labels at the edges of the picture as it is shown (kept inside the viewport).
    let img_rect = place.rect;
    if let Some(labels) = place.edge_labels(&rendered.displayed) {
        let r = img_rect.intersect(rect.shrink(4.0));
        if r.is_positive() {
            let size = 15.0;
            let color = Color32::from_rgb(0xFF, 0xD3, 0x6E);
            overlay_text(painter, pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, &labels.left, size, color);
            overlay_text(
                painter,
                pos2(r.right() - 4.0, r.center().y),
                Align2::RIGHT_CENTER,
                &labels.right,
                size,
                color,
            );
            overlay_text(painter, pos2(r.center().x, r.top() + 4.0), Align2::CENTER_TOP, &labels.top, size, color);
            overlay_text(
                painter,
                pos2(r.center().x, (r.bottom() - 4.0).min(rect.bottom() - 32.0)),
                Align2::CENTER_BOTTOM,
                &labels.bottom,
                size,
                color,
            );
        }
    }

    // Scale bar.
    if let Some(points_per_mm) = place.points_per_mm(&rendered.displayed) {
        let max_len = (rect.width() * 0.25).clamp(40.0, 160.0);
        if let Some(&len) = [1.0f32, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0]
            .iter()
            .rev()
            .find(|&&l| (30.0..=max_len).contains(&(l * points_per_mm)))
        {
            // Left out in a view too narrow for it and the text beside it.
            let w = len * points_per_mm;
            let label = format!("{len:.0} mm");
            let half = w.max(text_width(painter, std::slice::from_ref(&label), 11.0)) / 2.0;
            let fits = rect.center().x - half >= free.0 && rect.center().x + half <= free.1;
            let y = rect.bottom() - 12.0;
            let x0 = rect.center().x - w / 2.0;
            let stroke = Stroke::new(1.5, theme::OVERLAY_TEXT);
            let shadow = Stroke::new(3.5, Color32::from_black_alpha(150));
            for s in [shadow, stroke].into_iter().filter(|_| fits) {
                painter.line_segment([pos2(x0, y), pos2(x0 + w, y)], s);
                painter.line_segment([pos2(x0, y - 4.0), pos2(x0, y + 4.0)], s);
                painter.line_segment([pos2(x0 + w, y - 4.0), pos2(x0 + w, y + 4.0)], s);
            }
            if fits {
                overlay_text(
                    painter,
                    pos2(rect.center().x, y - 5.0),
                    Align2::CENTER_BOTTOM,
                    &label,
                    11.0,
                    theme::OVERLAY_TEXT,
                );
            }
        }
    }
}

/// One decimal place without printing negative zero ("-0.0").
fn one_decimal(v: f64) -> String {
    format!("{:.1}", (v * 10.0).round() / 10.0 + 0.0)
}
