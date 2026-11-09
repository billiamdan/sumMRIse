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
use summrise_core::{Gesture, SeriesData, SeriesKey, Settings, Tool, ViewKind, ViewState};
use summrise_i18n::tr;

use crate::crosshair;
use crate::fidelity;
use crate::orient::{self, view_name, view_short_name};
use crate::placement::Placement;
use crate::render::{Displayed, Picture, Rendered};
use crate::theme::{self, VIEWPORT_BG};
use crate::widgets::{elide_lines, overlay_block, overlay_text, share_row, text_width};

/// Scroll distance (points) per slice step.
const SCROLL_STEP: f32 = 18.0;
pub(crate) const OVERLAY_SIZE: f32 = 12.0;

pub(crate) struct ViewportInput<'a> {
    pub id: Id,
    pub key: SeriesKey,
    pub kind: ViewKind,
    pub series: &'a SeriesData,
    pub meta: &'a Series,
    pub study: &'a Study,
    pub rendered: Option<&'a Rendered>,
    pub tool: Tool,
    pub settings: &'a Settings,
    pub is_active: bool,
    pub multi_view: bool,
    /// Show patient/study annotations (all views in single layout, first view otherwise;
    /// every viewport of several series).
    pub show_details: bool,
    /// What to say about a PET drawn over the images (see `pet`).
    pub fusion: Option<&'a crate::pet::Shown>,
    /// The pictures of a tile grid, when the viewport shows several images (see `tiles`).
    pub tiles: Option<&'a crate::tiles::TileInput<'a>>,
    /// What a viewport of a multi-series layout adds (see `compare`).
    pub compare: Option<&'a crate::compare::ViewportInfo>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Hover {
    /// The series under the pointer: in a multi-series layout not always the active one.
    pub series: SeriesKey,
    /// The number of the image under the pointer, in a tile grid.
    pub image: Option<usize>,
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
    /// What the user did to zoom and pan, for the viewports linked to this one.
    pub gestures: Vec<Gesture>,
    /// A tile was double-clicked: the viewport goes back to one image, this
    /// one (index in its image set).
    pub open_image: Option<usize>,
    /// The link indicator was clicked (see `compare`).
    pub toggle_link: bool,
    /// Where cross-reference lines were drawn, on screen.
    pub cross_lines: Vec<[Pos2; 2]>,
}

/// One picture of a viewport: the rectangle it is drawn in and the image it
/// shows. A single image takes the whole viewport, a tile a part of it.
pub(crate) struct Cell<'a> {
    pub rect: Rect,
    /// `None`: a tile past the last image, which stays empty.
    pub rendered: Option<&'a Rendered>,
    /// The number of the image in a tile.
    pub number: Option<usize>,
}

/// What [`picture`] leaves to the viewport.
struct DrawnPicture {
    place: Placement,
    hover: Option<Hover>,
}

/// Draws one picture in its cell, the single image of a viewport or a tile of
/// a grid: the image or what stands in for it, its edge letters and the cross-
/// reference line. **Everything that belongs to a picture is drawn here**, so
/// that it also works in a tile, apart from measurements, which also need the
/// pointer of the whole viewport (`show` calls them for every picture). Anything drawn over a picture goes through
/// `place`, which maps between the screen and the pixels of this image, and
/// the image is `rendered.displayed` (`Displayed::Frame::index` for an
/// acquired one), which is not the current image in every tile. Also tells
/// what lies under the pointer. `tile` is the position in a tile grid.
fn picture(
    ui: &mut Ui,
    cell: &Cell<'_>,
    rendered: &Rendered,
    tile: Option<usize>,
    input: &ViewportInput<'_>,
    state: &mut ViewState,
    result: &mut ViewportResult,
) -> DrawnPicture {
    let rect = cell.rect;
    let painter = ui.painter_at(rect);
    let displayed = &rendered.displayed;
    let transform = state.transform(input.kind);
    // Where the picture is on screen: fitted, zoomed, panned, turned and mirrored.
    let place = Placement::of(rect, displayed, &transform);
    let img_rect = place.rect;
    match (rendered.texture(), cell.number) {
        (Some(texture), _) => place.paint(&painter, texture.id()),
        (None, Some(number)) => draw_tile_placeholder(ui, &painter, rect, &rendered.picture, number),
        (None, None) => {
            let description = describe(input, state, &rendered.picture);
            draw_placeholder(ui, &painter, rect, img_rect, &rendered.picture, &description);
        }
    }

    // Not while a menu or another window covers the view under the pointer.
    let (pw, ph) = displayed.pixel_size();
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut hover = None;
    if let Some(pos) = pointer.filter(|p| img_rect.contains(*p) && rect.contains(*p) && ui.rect_contains_pointer(rect))
    {
        let (x, y) = place.to_pixel(pos);
        let (px, py) = (x.round().max(0.0) as usize, y.round().max(0.0) as usize);
        if px < pw && py < ph {
            let outside = fidelity::is_outside_scan(displayed, input.series, px, py);
            hover = Some(Hover {
                series: input.key,
                image: cell.number,
                pixel: (px, py),
                patient: displayed.pixel_to_patient(x, y),
                value: displayed.value_at(input.series, px, py).filter(|_| !outside),
                outside,
            });
        }
    }

    if input.settings.show_overlays {
        // Clear of the lines at the top and the text at the bottom of a viewport of several series.
        let beside = input.compare.is_some().then(|| crate::compare::clear_of_text(rect));
        draw_edge_labels(&painter, rect, &place, displayed, cell.number.is_some(), beside);
        // Where the image of the active viewport cuts this one.
        if let Some(cutting) = input.compare.and_then(|info| info.cross.as_ref()) {
            result.cross_lines.extend(crate::compare::draw_cross_reference(&painter, &place, displayed, cutting));
        }
    }

    // What a tile says for itself, about its own image.
    if let (Some(number), Some(tile)) = (cell.number, tile) {
        let count = input.tiles.map_or(0, |grid| grid.count);
        let label = describe_image(input.kind, state, number, count, &rendered.picture);
        let id = input.id.with(("tile", tile));
        let response = ui.interact(rect, id, Sense::hover());
        response.widget_info(|| WidgetInfo::labeled(Role::Image, true, &label));
        let corner = rect.left_top() + vec2(5.0, 3.0);
        overlay_text(&painter, corner, Align2::LEFT_TOP, &number.to_string(), 11.5, theme::OVERLAY_TEXT);
        if let Displayed::Frame { index, .. } = displayed
            && let Some(mark) = fidelity::lossy_label_of_image(input.series, input.meta, *index)
        {
            fidelity::lossy_tile_mark(ui, &painter, rect, id, &mark);
        }
    }
    DrawnPicture { place, hover }
}

/// Stands in for the image of a tile that is not decoded (yet): small enough
/// for a tile, whose number stays above it.
fn draw_tile_placeholder(ui: &Ui, painter: &egui::Painter, rect: Rect, picture: &Picture, number: usize) {
    painter.rect_filled(rect, 0.0, Color32::from_gray(13));
    let center = rect.center();
    match picture {
        Picture::Failed(_) => {
            let text = tr!("Image {number} could not be decoded", number = number);
            let width = (rect.width() - 10.0).max(40.0);
            let galley = painter.layout(text, FontId::proportional(11.0), theme::DARK.warn, width);
            painter.galley(center - galley.size() / 2.0, galley, theme::DARK.warn);
        }
        Picture::Loading | Picture::Image => {
            let accent = theme::palette(ui).accent;
            egui::Spinner::new().color(accent).paint_at(ui, Rect::from_center_size(center, vec2(18.0, 18.0)));
        }
    }
}

pub(crate) fn show(ui: &mut Ui, rect: Rect, input: &ViewportInput<'_>, state: &mut ViewState) -> ViewportResult {
    let mut result = ViewportResult::default();
    crate::export::note_view(ui.ctx(), rect, input.is_active);
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

    // One picture, or the pictures of a tile grid: every one through `picture`.
    let cells = match input.tiles {
        Some(grid) => crate::tiles::cells(rect, grid, input.multi_view),
        None => vec![Cell { rect, rendered: Some(rendered), number: None }],
    };
    let in_grid = cells.len() > 1;
    // Every picture drawn, with where it lies on screen.
    let mut drawn = Vec::with_capacity(cells.len());
    for (tile, cell) in cells.iter().enumerate() {
        if let Some(shown) = cell.rendered {
            let result_of_cell = picture(ui, cell, shown, in_grid.then_some(tile), input, state, &mut result);
            result.hover = result.hover.or(result_of_cell.hover);
            drawn.push((cell, shown, result_of_cell.place));
        }
        if in_grid {
            let border = Stroke::new(1.0, Color32::from_gray(44));
            painter.rect_stroke(cell.rect, 0.0, border, StrokeKind::Outside);
        }
    }
    // The picture of the current image: the first (or only) one.
    let displayed = &rendered.displayed;
    let place = drawn.first().map_or_else(|| Placement::of(rect, displayed, &state.transform(input.kind)), |d| d.2);
    let img_rect = if in_grid { rect } else { place.rect };
    let description = match input.tiles {
        Some(grid) => crate::tiles::describe(grid),
        None => describe(input, state, &rendered.picture),
    };
    // What the viewport shows, for assistive technology (and UI tests); the
    // stack of a series split into stacks is part of it.
    let progress = loading_progress(input);
    let stack = input.series.stack_label(state.stack());
    response.widget_info(|| {
        let parts = [Some(&description), stack.as_ref(), progress.as_ref()];
        let label = parts.into_iter().flatten().map(String::as_str).collect::<Vec<_>>().join(", ");
        WidgetInfo::labeled(Role::Image, true, label)
    });

    // ---- Interaction -------------------------------------------------------
    let (modifiers, scroll, zoom, pointer) =
        ui.input(|i| (i.modifiers, i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
    // Zoom keeps the point under the pointer in place, in the picture it is in.
    let centre_of =
        |pos: Pos2| cells.iter().find(|cell| cell.rect.contains(pos)).map_or(rect, |cell| cell.rect).center();
    if response.hovered() {
        if (zoom - 1.0).abs() > f32::EPSILON {
            let anchor = pointer.map_or([0.0, 0.0], |p| [p.x - centre_of(p).x, p.y - centre_of(p).y]);
            zoom_view(state, input.kind, zoom, anchor, &mut result);
            result.activated = true;
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
                result.activated = true;
            }
        }
        ui.ctx().set_cursor_icon(match input.tool {
            Tool::WindowLevel => CursorIcon::Crosshair,
            Tool::Pan => CursorIcon::Grab,
            Tool::Zoom => CursorIcon::ZoomIn,
            Tool::Measure(_) => CursorIcon::Crosshair,
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
            result.gestures.push(Gesture::Pan { delta: [delta.x, delta.y] });
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if response.dragged_by(PointerButton::Secondary) || (primary && input.tool == Tool::WindowLevel) {
            if !input.series.is_color() {
                state.drag_window(f64::from(delta.x), f64::from(delta.y), input.series);
            }
        } else if primary && input.tool == Tool::Zoom {
            let origin = response.interact_pointer_pos().unwrap_or(rect.center()) - delta;
            let centre = centre_of(origin);
            let anchor = [origin.x - centre.x, origin.y - centre.y];
            zoom_view(state, input.kind, (-delta.y * 0.01).exp(), anchor, &mut result);
        }
    }
    // Clicks of a measuring tool place and select measurements (see `measure`).
    let measuring = input.tool.is_measuring();
    if response.double_clicked() && !measuring {
        // A tile opens as one image; a single image goes back to the fitted view.
        if in_grid {
            let pressed = response.interact_pointer_pos();
            let tile = pressed.and_then(|pos| cells.iter().find(|cell| cell.rect.contains(pos)));
            result.open_image = tile.and_then(|cell| cell.number).map(|number| number - 1);
        } else {
            let before = state.transform(input.kind);
            state.transform_mut(input.kind).fit();
            result.gestures.push(Gesture::Zoom { factor: 1.0 / before.zoom, anchor: [0.0, 0.0] });
            result.gestures.push(Gesture::Pan { delta: [-before.pan[0] / before.zoom, -before.pan[1] / before.zoom] });
        }
    } else if response.clicked() {
        result.activated = true;
        // In a grid only the first tile shows the current image: another one is no place to go to.
        let target =
            response.interact_pointer_pos().filter(|pos| !measuring && (!in_grid || cells[0].rect.contains(*pos)));
        if let Some(pos) = target {
            let (x, y) = place.to_pixel(pos);
            if let Some(p) = displayed.pixel_to_patient(x, y) {
                state.set_cursor(p, input.series);
            }
        }
    }
    if response.clicked_by(PointerButton::Secondary) || response.drag_started() {
        result.activated = true;
    }

    // ---- Crosshair: the lines of the two other planes, with their tilt handles ----
    if let ViewKind::Mpr(axis) = input.kind
        && input.settings.show_overlays
        && !measuring
        && crosshair::show(ui, &painter, rect, &place, displayed, input.id, axis, state)
    {
        result.activated = true;
    }

    // ---- Overlays -----------------------------------------------------------
    if input.settings.show_overlays {
        match input.tiles {
            Some(grid) => {
                crate::tiles::annotations(&painter, rect, &place, input, grid, state, rendered, progress.as_deref());
            }
            None => draw_overlays(&painter, rect, &place, input, state, rendered, progress.as_deref()),
        }
    } else if let Some(progress) = &progress {
        // Loading progress is a status, not an annotation: it stays when annotations are hidden.
        let corner = rect.left_bottom() + vec2(10.0, -10.0);
        overlay_text(&painter, corner, Align2::LEFT_BOTTOM, progress, OVERLAY_SIZE, theme::OVERLAY_TEXT);
    }
    // Measurements belong to each picture and are not annotations: they stay when those are hidden.
    // In a grid the pointer goes to the tile it was pressed in or, before that, is over.
    let pointer_at =
        ui.input(|i| i.pointer.press_origin().or(i.pointer.hover_pos())).or(response.interact_pointer_pos());
    for (cell, shown, place) in &drawn {
        let pointer_here = !in_grid || pointer_at.is_some_and(|pos| cell.rect.contains(pos));
        let painter = ui.painter_at(cell.rect);
        if crate::measure::show(ui, &response, &painter, cell.rect, place, shown, input, state, pointer_here) {
            result.activated = true;
        }
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
    // So does a lossy one (in the row under "Mirrored"; in a grid each tile marks its own image),
    // and in privacy mode one with burned-in text.
    if !in_grid && let Some(label) = fidelity::lossy_label(input.kind, input.series, input.meta, state) {
        fidelity::lossy_badge(ui, &painter, rect, img_rect, input.id, input.multi_view, &label);
    }
    let burned_in = if in_grid {
        cells.iter().filter_map(|cell| cell.rendered).any(|shown| match shown.displayed {
            Displayed::Frame { index, .. } => fidelity::image_has_burned_in_text(input.series, input.meta, index),
            Displayed::Mpr(_) => false,
        })
    } else {
        fidelity::shows_burned_in_text(input.kind, input.series, input.meta, state)
    };
    if input.settings.hide_patient_details && burned_in {
        fidelity::burned_in_notice(ui, &painter, rect, input.id, input.multi_view);
    }
    if let Some(info) = input.compare {
        crate::compare::outside_badge(ui, &painter, rect, img_rect, input.id, info);
    }
    if input.multi_view {
        let color = theme::view_color(input.kind);
        if let Some(info) = input.compare {
            // Which series and which date, whether annotations are shown or not.
            crate::compare::header(ui, &painter, rect, input.id, info, color, &mut result);
        } else {
            let label = format!(
                "{}  {}/{}",
                view_name(input.kind, state).to_uppercase(),
                state.slice_index(input.kind, input.series) + 1,
                state.slice_count(input.kind, input.series)
            );
            // Shortened ("…") in a view too narrow for it.
            let label = elide_lines(&painter, vec![label], 11.5, rect.width() - 20.0).concat();
            overlay_text(&painter, pos2(rect.left() + 10.0, rect.top() + 8.0), Align2::LEFT_TOP, &label, 11.5, color);
        }
        painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(2.0, color.gamma_multiply(0.8)));
        if input.is_active {
            painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(1.5, accent), StrokeKind::Inside);
        }
    }
    result
}

/// Zooms the view by `factor` about `anchor` and notes it for the viewports
/// linked to this one, by the factor it came to within the limits of the zoom.
fn zoom_view(state: &mut ViewState, kind: ViewKind, factor: f32, anchor: [f32; 2], result: &mut ViewportResult) {
    let transform = state.transform_mut(kind);
    let before = transform.zoom;
    transform.zoom_about(factor, anchor);
    result.gestures.push(Gesture::Zoom { factor: transform.zoom / before, anchor });
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
    describe_image(input.kind, state, number, count, picture)
}

/// What a picture shows, in words: image `number` of `count`.
fn describe_image(kind: ViewKind, state: &ViewState, number: usize, count: usize, picture: &Picture) -> String {
    match (picture, kind) {
        (Picture::Loading, _) => tr!("Loading image {number} / {count}", number = number, count = count),
        (Picture::Failed(_), _) => tr!("Image {number} could not be decoded", number = number),
        (Picture::Image, ViewKind::Stack) => tr!("Image {number} / {count}", number = number, count = count),
        // Translators: {view} is the name of a view, such as Axial or Coronal.
        (Picture::Image, ViewKind::Mpr(_)) => {
            tr!("{view} {number} / {count}", view = view_name(kind, state), number = number, count = count)
        }
        // Shown by `volume_view`.
        (Picture::Image, ViewKind::Volume) => view_name(kind, state),
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
pub(crate) const CORNER_GAP: f32 = 12.0;

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
    // In a multi-series layout the top line says which series this is.
    let top_right = (input.show_details && input.compare.is_none()).then_some(lines);
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

    // Scale bar.
    draw_scale_bar(painter, rect, place, &rendered.displayed, input, free);
}

/// Orientation labels at the edges of a picture as it is shown (kept inside
/// its cell), smaller in a tile. `clear` is how far the top and the bottom
/// letter stay from the edges of the cell, where other text may be.
fn draw_edge_labels(
    painter: &egui::Painter,
    rect: Rect,
    place: &Placement,
    displayed: &Displayed,
    tile: bool,
    clear: Option<(f32, f32)>,
) {
    let img_rect = place.rect;
    if let Some(labels) = place.edge_labels(displayed) {
        let m = if tile { 2.0 } else { 4.0 };
        let r = img_rect.intersect(rect.shrink(m));
        if r.is_positive() {
            let size = if tile { 11.0 } else { 15.0 };
            let color = Color32::from_rgb(0xFF, 0xD3, 0x6E);
            overlay_text(painter, pos2(r.left() + m, r.center().y), Align2::LEFT_CENTER, &labels.left, size, color);
            overlay_text(painter, pos2(r.right() - m, r.center().y), Align2::RIGHT_CENTER, &labels.right, size, color);
            // Clear of the text and the scale bar at the bottom of a single picture.
            let (from_top, from_bottom) = clear.filter(|_| !tile).unwrap_or((0.0, if tile { 0.0 } else { 32.0 }));
            let top = (r.top() + m).max(rect.top() + from_top);
            overlay_text(painter, pos2(r.center().x, top), Align2::CENTER_TOP, &labels.top, size, color);
            let bottom = (r.bottom() - m).min(rect.bottom() - from_bottom);
            overlay_text(painter, pos2(r.center().x, bottom), Align2::CENTER_BOTTOM, &labels.bottom, size, color);
        }
    }
}

/// The scale bar at the bottom of a viewport `rect`, between the x positions
/// `free`, to the scale of the picture `place`.
pub(crate) fn draw_scale_bar(
    painter: &egui::Painter,
    rect: Rect,
    place: &Placement,
    displayed: &Displayed,
    input: &ViewportInput<'_>,
    free: (f32, f32),
) {
    if let Some(points_per_mm) = place.points_per_mm(displayed) {
        let max_len = (rect.width() * 0.25).clamp(40.0, 160.0);
        if let Some(&len) = [1.0f32, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0]
            .iter()
            .rev()
            .find(|&&l| (30.0..=max_len).contains(&(l * points_per_mm)))
        {
            // Left out in a view too narrow for it and the text beside it.
            let w = len * points_per_mm;
            let label = crate::measure::scale_label(len, displayed, input.series, input.meta);
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
