//! What the viewer shows so that unusual images display correctly and
//! honestly (`openspec/changes/display-fidelity/design.md`): the acquired
//! picture through the window or a contrast table with padding as background,
//! the scanner graphics drawn over it, the read-out outside the scanned area,
//! the lossy mark, the burned-in text notice of privacy mode and the toolbar
//! toggle of the scanner graphics.

use egui::{
    Color32, CornerRadius, FontId, Id, Label, Painter, Rect, Response, RichText, Role, Sense, Stroke, StrokeKind, Ui,
    WidgetInfo, pos2, vec2,
};
use egui_material_icons::icons::{ICON_COMPRESS, ICON_GESTURE, ICON_PRIVACY_TIP};
use summrise_core::dicom::{FrameRef, LossyCompression, Series};
use summrise_core::guide;
use summrise_core::imaging::VoxelBuffer;
use summrise_core::imaging::graphics::{GRAPHICS_COLOR, composite};
use summrise_core::imaging::padding::{is_outside, visible};
use summrise_core::imaging::window::{gray_to_rgba, lut_to_rgba};
use summrise_core::{SeriesData, ViewKind, ViewState};
use summrise_i18n::tr;

use crate::orient;
use crate::render::Displayed;
use crate::theme;
use crate::widgets::icon;

/// The badge row of the lossy mark: its own row, under "Mirrored" (row 0),
/// so that the two never cover each other and the mark never moves.
const LOSSY_ROW: usize = 1;

/// The RGBA picture of acquired image `index` with grey values `data`:
/// through the contrast table the user chose, else through the window;
/// padding is black, also when inverted.
pub(crate) fn grey_rgba(series: &SeriesData, index: usize, data: &VoxelBuffer, state: &ViewState) -> Vec<u8> {
    let padding = series.padding(index);
    let value = |i| visible(data.get(i), padding);
    match state.active_lut(series) {
        Some(table) => lut_to_rgba(data.len(), value, &table.table, state.invert),
        None => gray_to_rgba(data.len(), value, &state.window, state.invert),
    }
}

/// Draws the scanner graphics of acquired image `index` over its RGBA
/// picture of `size` pixels, unless the user has hidden them. The values of
/// the image are not touched.
pub(crate) fn draw_graphics(rgba: &mut [u8], size: (u32, u32), series: &SeriesData, index: usize, state: &ViewState) {
    if state.show_graphics {
        composite(rgba, size.0, size.1, series.graphics(index).iter().map(|plane| &**plane), GRAPHICS_COLOR);
    }
}

/// Whether the point at pixel (`x`, `y`) of what a view shows lies outside
/// the scanned area: padding of an acquired image, or a point of a reformat
/// outside the volume or touching padding.
pub(crate) fn is_outside_scan(displayed: &Displayed, series: &SeriesData, x: usize, y: usize) -> bool {
    if let Displayed::Frame { index, .. } = displayed {
        match series.frame(*index) {
            Some(FrameRef::Gray { width, height, data }) if x < width as usize && y < height as usize => {
                is_outside(data.get(y * width as usize + x), series.padding(*index))
            }
            _ => false,
        }
    } else if let Displayed::Mpr(image) = displayed {
        x < image.width && y < image.height && !image.values[y * image.width + x].is_finite()
    } else {
        false
    }
}

/// The lossy mark of a view: of the image shown in the acquired view, of any
/// image of the series in a reformat; none in other views.
pub(crate) fn lossy_label(kind: ViewKind, series: &SeriesData, meta: &Series, state: &ViewState) -> Option<String> {
    if kind == ViewKind::Stack {
        let info = series.frames().get(state.slot_index(series))?;
        meta.instances.get(info.instance)?.fidelity.lossy.as_ref().map(LossyCompression::label)
    } else if let ViewKind::Mpr(_) = kind {
        meta.instances.iter().find_map(|header| header.fidelity.lossy.as_ref()).map(LossyCompression::label)
    } else {
        None
    }
}

/// Draws the lossy mark `label` with the badge helper, in its own row.
pub(crate) fn lossy_badge(
    ui: &Ui,
    painter: &Painter,
    viewport: Rect,
    picture: Rect,
    id: Id,
    multi_view: bool,
    label: &str,
) {
    let text = format!("{} {label}", icon(ICON_COMPRESS));
    let hint = tr!(
        "This image is or was compressed in a way that discards some detail to save space. Fine detail can differ from what the scanner recorded."
    );
    orient::badge(ui, painter, viewport, picture, id.with("lossy"), LOSSY_ROW, multi_view, &text, label, hint);
}

/// Whether the acquired view shows an image recorded with burned-in text.
pub(crate) fn shows_burned_in_text(kind: ViewKind, series: &SeriesData, meta: &Series, state: &ViewState) -> bool {
    kind == ViewKind::Stack
        && series
            .frames()
            .get(state.slot_index(series))
            .and_then(|info| meta.instances.get(info.instance))
            .is_some_and(|header| header.fidelity.burned_in_text)
}

/// The notice of privacy mode for an image with burned-in text, at the
/// top-left corner of the view, where the hidden patient details would be.
pub(crate) fn burned_in_notice(ui: &Ui, painter: &Painter, viewport: Rect, id: Id, multi_view: bool) {
    let text = tr!("Patient details may be part of the picture itself and cannot be hidden.");
    let color = theme::DARK.warn;
    let width = (viewport.width() - 40.0).clamp(80.0, 300.0);
    let galley = painter.layout(format!("{} {text}", icon(ICON_PRIVACY_TIP)), FontId::proportional(12.0), color, width);
    let top = viewport.top() + if multi_view { 28.0 } else { 10.0 };
    let rect = Rect::from_min_size(pos2(viewport.left() + 10.0, top), galley.size() + vec2(16.0, 8.0));
    painter.rect(
        rect,
        CornerRadius::same(6),
        Color32::from_black_alpha(205),
        Stroke::new(1.0, color),
        StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(8.0, 4.0), galley, color);
    let response = ui.interact(rect, id.with("burned-in"), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, text));
}

/// The tooltip of a contrast table in the contrast menu: where it comes
/// from and, with explanations on, what a contrast table is (as
/// `guide::PresetHint` does for windows).
pub(crate) fn table_hint(response: Response, explain: bool) -> Response {
    let hint = tr!("Contrast table stored in the images");
    if !explain {
        return response.on_hover_text(hint);
    }
    response.ctx.accesskit_node_builder(response.id, |node| node.set_description(hint));
    response.on_hover_ui(|ui| {
        let p = theme::palette(ui);
        ui.set_max_width(340.0);
        ui.add(Label::new(RichText::new(hint).small().color(p.text_weak)).wrap());
        ui.add(Label::new(guide::explain_contrast_table()).wrap());
    })
}

/// The status-bar toggle of the scanner graphics (key G), shown while the
/// series has scanner graphics: it also tells that the images have some.
/// (The toolbar is full at 1280 points: one more button there covers the
/// "Rotate and flip" menu.)
pub(crate) fn graphics_toggle(ui: &mut Ui, series: &SeriesData, state: &mut ViewState) {
    if !series.has_graphics() {
        return;
    }
    let name = tr!("Scanner graphics");
    ui.toggle_value(&mut state.show_graphics, RichText::new(format!("{} {name}", icon(ICON_GESTURE))).small())
        .accessible_name(name)
        .on_hover_text(tr!(
            "Scanner graphics (G): marks and text that the scanner stored with the image, drawn in colour over it"
        ));
}
