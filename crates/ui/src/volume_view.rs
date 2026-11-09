//! The 3D view: the volume as a shaded object that turns with the pointer
//! and the arrow keys, its toolbar controls (look and standard views), its
//! overlays, its keys and its status-bar hint.
//!
//! Pictures come from `summrise_core::VolumePictures`: a preview while the
//! user turns or zooms, the full picture from a background job afterwards.
//! Everything drawn over the picture — the edge letters above all — comes
//! from the camera of the picture actually shown, so it describes what is on
//! screen. The view shows no value read-out, crosshair or scale bar, and no
//! display orientation is applied: the camera covers rotation.

use std::borrow::Cow;
use std::sync::Arc;

use egui::containers::menu::{MenuButton, MenuConfig};
use egui::{
    Align, Align2, Button, Color32, ColorImage, Context, CornerRadius, CursorIcon, FontId, Key, Label, Layout,
    Modifiers, PointerButton, PopupCloseBehavior, Rect, RichText, Role, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Ui, Vec2, WidgetInfo, pos2, vec2,
};
use egui_material_icons::icons::ICON_3D_ROTATION;
use summrise_core::export::Picture;
use summrise_core::imaging::colormap::Colormap;
use summrise_core::imaging::render3d::{Look, Quality, StandardView, Style};
use summrise_core::imaging::{DVec3, EdgeLabels, Rendering, Volume};
use summrise_core::{PictureRequest, SeriesData, SeriesKey, Ticket, ViewKind, ViewState, VolumePictures, Waker};
use summrise_i18n::tr;

use crate::theme::{self, VIEWPORT_BG};
use crate::viewport::{ViewportInput, ViewportResult};
use crate::widgets::{command_key, icon, overlay_block, overlay_text};

/// Degrees the object turns per point the pointer is dragged.
const DEGREES_PER_POINT: f64 = 0.5;
/// Degrees the object turns per arrow key press, and with Shift.
const KEY_STEP: f64 = 10.0;
const KEY_STEP_COARSE: f64 = 45.0;
const OVERLAY_SIZE: f32 = 12.0;
/// Colour of the edge letters, as in the other views.
const LETTERS: Color32 = Color32::from_rgb(0xFF, 0xD3, 0x6E);

/// The standard views and their keys (with Shift): the letter of the side
/// the view looks from, as the edge letters name it.
const VIEW_KEYS: [(StandardView, Key, &str); 6] = [
    (StandardView::Front, Key::A, "A"),
    (StandardView::Back, Key::P, "P"),
    (StandardView::Left, Key::L, "L"),
    (StandardView::Right, Key::R, "R"),
    (StandardView::Top, Key::S, "S"),
    (StandardView::Bottom, Key::I, "I"),
];

/// The pictures of the 3D view of the displayed series.
#[derive(Default)]
pub(crate) struct VolumeView {
    slot: Option<Slot>,
}

struct Slot {
    series: SeriesKey,
    pictures: VolumePictures,
    texture: Option<TextureHandle>,
    /// The picture in `texture`, and the colour map it is shown in.
    uploaded: Option<(Ticket, Colormap)>,
}

impl VolumeView {
    /// Lets go of the pictures (and the shared volume) of a series other than `keep`.
    pub(crate) fn retain_series(&mut self, keep: Option<SeriesKey>) {
        if self.slot.as_ref().is_some_and(|slot| Some(slot.series) != keep) {
            self.slot = None;
        }
    }

    /// Patient directions of screen-right and screen-down of the 3D picture
    /// on screen for `series`, when `view` is the 3D view.
    pub(crate) fn shown_axes(&self, series: SeriesKey, view: ViewKind) -> Option<(DVec3, DVec3)> {
        let slot = self.slot.as_ref().filter(|slot| slot.series == series && view == ViewKind::Volume)?;
        let basis = slot.pictures.shown()?.request.camera.basis();
        Some((basis.right, basis.down))
    }

    /// The picture on screen, its camera and its quality (for tests).
    #[cfg(test)]
    pub(crate) fn shown(&self) -> Option<&summrise_core::Shown<Arc<summrise_core::imaging::Rendering>>> {
        self.slot.as_ref()?.pictures.shown()
    }

    /// The colour map of the picture on screen (for tests).
    #[cfg(test)]
    pub(crate) fn shown_colormap(&self) -> Option<Colormap> {
        self.slot.as_ref()?.uploaded.map(|(_, colormap)| colormap)
    }

    /// The 3D picture on screen for `series`, when `view` is the 3D view: what
    /// saving and printing take.
    pub(crate) fn shown_picture(&self, series: SeriesKey, view: ViewKind, state: &ViewState) -> Option<ShownPicture> {
        let slot = self.slot.as_ref().filter(|slot| slot.series == series && view == ViewKind::Volume)?;
        let shown = slot.pictures.shown()?;
        let colormap = if shown.request.style.look == Look::Mip { state.colormap() } else { Colormap::Grey };
        Some(ShownPicture {
            rendering: shown.picture.clone(),
            colormap,
            request: shown.request.clone(),
            quality: shown.quality,
        })
    }

    /// Shows the 3D view of `volume` in `rect` and handles its pointer input.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        input: &ViewportInput<'_>,
        series: SeriesKey,
        state: &mut ViewState,
        volume: Option<&Volume>,
    ) -> ViewportResult {
        let mut result = ViewportResult::default();
        crate::export::note_view(ui.ctx(), rect, input.is_active);
        let response = ui.interact(rect, input.id, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, VIEWPORT_BG);
        let Some(volume) = volume else {
            let message = tr!("There is no volume to show in 3D.");
            painter.text(rect.center(), Align2::CENTER_CENTER, message, FontId::proportional(13.0), Color32::GRAY);
            response.widget_info(|| WidgetInfo::labeled(Role::Image, true, message));
            return result;
        };
        interact(ui, &response, rect, input, state, &mut result);

        // The picture of this frame: a preview at once if anything changed.
        let pixels_per_point = ui.ctx().pixels_per_point();
        let camera = state.volume_camera(&volume.geometry, [rect.width(), rect.height()]);
        let pixels = [rect.width() * pixels_per_point, rect.height() * pixels_per_point];
        let request = PictureRequest::new(camera, state.volume_style(), pixels);
        let slot = match &mut self.slot {
            Some(slot) if slot.series == series => slot,
            other => other.insert(Slot { series, pictures: VolumePictures::default(), texture: None, uploaded: None }),
        };
        let ctx = ui.ctx().clone();
        let waker: Waker = Arc::new(move || ctx.request_repaint());
        if let Some(shown) = slot.pictures.update(volume, &request, Some(waker)) {
            // A MIP of a PET or nuclear-medicine series is shown in the colour map of its slices.
            let colormap = if shown.request.style.look == Look::Mip { state.colormap() } else { Colormap::Grey };
            if slot.uploaded != Some((shown.ticket, colormap)) {
                let size = [shown.picture.width, shown.picture.height];
                let image = ColorImage::from_rgba_premultiplied(size, &coloured(&shown.picture.rgba, colormap));
                match &mut slot.texture {
                    Some(texture) => texture.set(image, TextureOptions::LINEAR),
                    None => slot.texture = Some(ui.ctx().load_texture("volume-3d", image, TextureOptions::LINEAR)),
                }
                slot.uploaded = Some((shown.ticket, colormap));
            }
        }
        if slot.pictures.wants_frame() {
            // The next frame starts the full picture of the preview on screen.
            ui.ctx().request_repaint();
        }
        if let Some(texture) = &slot.texture {
            painter.image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }

        let shown = slot.pictures.shown();
        let look = shown.map_or(state.look, |shown| shown.request.style.look);
        response.widget_info(|| WidgetInfo::labeled(Role::Image, true, tr!("3D view: {look}", look = look.label())));
        if input.settings.show_overlays
            && let Some(shown) = shown
        {
            let refining = shown.quality == Quality::Preview;
            draw_overlays(&painter, rect, input, state, &shown.request, refining);
        }
        caption(ui, &painter, rect, input);
        result
    }
}

/// A 3D picture as it was on screen, for saving and printing.
#[derive(Clone)]
pub(crate) struct ShownPicture {
    rendering: Arc<Rendering>,
    colormap: Colormap,
    request: PictureRequest,
    quality: Quality,
}

impl ShownPicture {
    /// Whether it is the full picture, not a preview on the way to it.
    pub(crate) fn is_final(&self) -> bool {
        self.quality == Quality::Full
    }

    /// The picture, on black.
    pub(crate) fn picture(&self) -> Option<Picture> {
        let rgba = coloured(&self.rendering.rgba, self.colormap);
        Picture::from_rgba(self.rendering.width, self.rendering.height, &rgba)
    }

    /// The letters at its edges, from the camera it was made with.
    pub(crate) fn letters(&self) -> EdgeLabels {
        self.request.camera.edge_labels()
    }
}

/// Draws the picture `shown` as the view would, with its overlays and its
/// caption: the export pass. Keep the texture until the pass has been painted.
pub(crate) fn paint_shown(
    ui: &mut Ui,
    rect: Rect,
    input: &ViewportInput<'_>,
    state: &ViewState,
    shown: &ShownPicture,
) -> TextureHandle {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, VIEWPORT_BG);
    let size = [shown.rendering.width, shown.rendering.height];
    let image = ColorImage::from_rgba_premultiplied(size, &coloured(&shown.rendering.rgba, shown.colormap));
    let texture = ui.ctx().load_texture("volume-3d-export", image, TextureOptions::LINEAR);
    painter.image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    draw_overlays(&painter, rect, input, state, &shown.request, false);
    caption(ui, &painter, rect, input);
    texture
}

/// Pointer input: a primary-button drag turns the object whatever the tool;
/// Shift-drag and middle-drag pan, right-drag changes the contrast, pinch
/// and Command/Ctrl-scroll zoom about the pointer, double-click fits.
fn interact(
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    input: &ViewportInput<'_>,
    state: &mut ViewState,
    result: &mut ViewportResult,
) {
    let (modifiers, zoom, pointer) = ui.input(|i| (i.modifiers, i.zoom_delta(), i.pointer.hover_pos()));
    if response.hovered() {
        if (zoom - 1.0).abs() > f32::EPSILON {
            let anchor = pointer.map_or([0.0, 0.0], |p| [p.x - rect.center().x, p.y - rect.center().y]);
            state.transform_mut(ViewKind::Volume).zoom_about(zoom, anchor);
        }
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    let delta = response.drag_delta();
    if delta != Vec2::ZERO {
        result.activated = true;
        let primary = response.dragged_by(PointerButton::Primary);
        if response.dragged_by(PointerButton::Middle) || (primary && modifiers.shift) {
            let t = state.transform_mut(ViewKind::Volume);
            t.pan = [t.pan[0] + delta.x, t.pan[1] + delta.y];
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if response.dragged_by(PointerButton::Secondary) {
            if !input.series.is_color() {
                state.drag_window(f64::from(delta.x), f64::from(delta.y), input.series);
            }
        } else if primary {
            state.turn_camera(f64::from(delta.x) * DEGREES_PER_POINT, f64::from(delta.y) * DEGREES_PER_POINT);
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
    }
    if response.double_clicked() {
        state.transform_mut(ViewKind::Volume).fit();
    }
    if response.clicked() || response.clicked_by(PointerButton::Secondary) || response.drag_started() {
        result.activated = true;
    }
}

/// Edge letters (from the camera of the picture shown), the look and the
/// contrast it follows, and the zoom.
fn draw_overlays(
    painter: &egui::Painter,
    rect: Rect,
    input: &ViewportInput<'_>,
    state: &ViewState,
    shown: &PictureRequest,
    refining: bool,
) {
    let labels = shown.camera.edge_labels();
    let r = rect.shrink(8.0);
    let size = 15.0;
    overlay_text(painter, pos2(r.left(), r.center().y), Align2::LEFT_CENTER, &labels.left, size, LETTERS);
    overlay_text(painter, pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, &labels.right, size, LETTERS);
    overlay_text(painter, pos2(r.center().x, r.top()), Align2::CENTER_TOP, &labels.top, size, LETTERS);
    overlay_text(painter, pos2(r.center().x, r.bottom()), Align2::CENTER_BOTTOM, &labels.bottom, size, LETTERS);

    let m = 10.0;
    let style = shown.style;
    let mut lines = vec![tr!("3D view: {look}", look = style.look.label())];
    if refining {
        // Translators: the 3D view is computing a sharper picture.
        lines.push(tr!("Refining…").to_owned());
    }
    overlay_block(painter, rect.left_bottom() + vec2(m, -m), Align2::LEFT_BOTTOM, &lines, OVERLAY_SIZE);

    let lines = contrast_lines(input.series, state, &style);
    overlay_block(painter, rect.right_bottom() + vec2(-m, -m), Align2::RIGHT_BOTTOM, &lines, OVERLAY_SIZE);
}

/// The bottom-right annotation of the 3D view: the window and colours of a
/// look that follows them, and the zoom.
fn contrast_lines(series: &SeriesData, state: &ViewState, style: &Style) -> Vec<String> {
    let mut lines = Vec::new();
    if style.look.follows_window() {
        // As in the other views: "W 400  L 40 HU", uptake values with their decimals.
        lines.push(crate::pet::window_annotation(series.units(), &style.window));
        lines.push(state.preset_label(series).map_or_else(|| tr!("Custom window").to_owned(), str::to_owned));
        if style.look == Look::Mip && style.invert {
            lines.push(tr!("Inverted").to_owned());
        }
        if style.look == Look::Mip && summrise_core::pet::has_colormaps(series.modality()) {
            lines.push(tr!("Colours: {map}", map = state.colormap().label()));
        }
    }
    let zoom = format!("{:.0}", state.transform(ViewKind::Volume).zoom * 100.0);
    lines.push(tr!("Zoom {percent}%", percent = zoom));
    lines
}

/// The caption that marks the 3D view as an illustration. It is part of the
/// view, not an annotation: it stays when annotations are hidden.
fn caption(ui: &Ui, painter: &egui::Painter, rect: Rect, input: &ViewportInput<'_>) {
    let text = tr!("Illustration: colours and surfaces are computed");
    let font = FontId::proportional(11.5);
    let galley = painter.layout(text.to_owned(), font, theme::OVERLAY_TEXT, (rect.width() - 40.0).max(80.0));
    let size = galley.size() + vec2(16.0, 6.0);
    // Under the top edge letter, inside the viewport.
    let top = (rect.top() + 32.0).min(rect.bottom() - size.y - 4.0).max(rect.top());
    let frame = Rect::from_min_size(pos2(rect.center().x - size.x / 2.0, top), size);
    painter.rect(
        frame,
        CornerRadius::same(9),
        Color32::from_black_alpha(170),
        Stroke::new(1.0, Color32::from_gray(90)),
        StrokeKind::Inside,
    );
    painter.galley(frame.min + vec2(8.0, 3.0), galley, theme::OVERLAY_TEXT);
    let response = ui.interact(frame, input.id.with("caption"), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, text));
    response.on_hover_text(tr!(
        "This 3D picture is computed from the scan to show its shape. Its colours are not those of the body, and which parts appear depends on the look and the contrast. It is an illustration, not a basis for diagnosis."
    ));
}

/// The toolbar control of the 3D view, when it is one of `views`: one menu
/// button that names the current look and holds the looks and the six
/// standard views (the toolbar has room for one control at the default
/// window size). It stays open while its entries are used.
pub(crate) fn toolbar(ui: &mut Ui, state: &mut ViewState, series: &SeriesData, views: &[ViewKind]) {
    if !views.contains(&ViewKind::Volume) {
        return;
    }
    let config = MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside);
    let text = format!("{} {}", icon(ICON_3D_ROTATION), state.look.label());
    let (button, _) = MenuButton::new(text).config(config).ui(ui, |ui| {
        let weak = theme::palette(ui).text_weak;
        ui.set_min_width(250.0);
        ui.set_max_width(330.0);
        // Translators: how the 3D view draws the volume (bone, skin, vessels and bone, surface, MIP).
        ui.label(RichText::new(tr!("Look")).small().color(weak));
        for &look in ViewState::looks(series) {
            let entry = Button::selectable(state.look == look, look.label());
            if ui.add(entry).accessible_name(look.label()).on_hover_text(look.description()).clicked() {
                state.set_look(look, series);
            }
        }
        ui.separator();
        ui.label(RichText::new(tr!("Standard views")).small().color(weak));
        for (view, _, letter) in VIEW_KEYS {
            let shortcut = tr!("Shift+{key}", key = letter);
            let entry = Button::selectable(state.standard_view() == Some(view), view.label()).shortcut_text(shortcut);
            if ui.add(entry).accessible_name(view.label()).on_hover_text(tr!("Turn the object to this view")).clicked() {
                state.show_standard_view(view);
            }
        }
        ui.separator();
        let hint = tr!(
            "Drag the picture to turn the object. The arrow keys turn it by 10° (by 45° with Shift), L chooses the next look and R resets the view."
        );
        // Left-aligned: a menu stretches its entries, which would space the words out.
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.add(Label::new(RichText::new(hint).small().color(weak)).wrap());
        });
    });
    button
        .accessible_name(tr!("Look and standard views"))
        .on_hover_text(tr!("How the volume is drawn, and the side from which it is seen (L: next look)"));
}

/// Keys of the 3D view, when it is the `active` view: the arrows turn the
/// object by 10° (45° with Shift); Shift with A, P, L, R, S or I looks from
/// the front, the back, the patient's left or right, above the head or below
/// the feet; L chooses the next look.
///
/// Must run before the viewer's own bindings: those ignore Shift and would
/// take these keys (and the arrows would scroll).
pub(crate) fn keys(ctx: &Context, state: &mut ViewState, series: &SeriesData, active: ViewKind) {
    if active != ViewKind::Volume {
        return;
    }
    let presses = |modifiers: Modifiers, key: Key| ctx.input_mut(|i| i.count_and_consume_key(modifiers, key));
    for (view, key, _) in VIEW_KEYS {
        if presses(Modifiers::SHIFT, key) > 0 {
            state.show_standard_view(view);
        }
    }
    // The combination with Shift first: without it in the pattern, Shift is ignored.
    for (modifiers, step) in [(Modifiers::SHIFT, KEY_STEP_COARSE), (Modifiers::NONE, KEY_STEP)] {
        let right = presses(modifiers, Key::ArrowRight) as f64 - presses(modifiers, Key::ArrowLeft) as f64;
        let down = presses(modifiers, Key::ArrowDown) as f64 - presses(modifiers, Key::ArrowUp) as f64;
        if right != 0.0 || down != 0.0 {
            state.turn_camera(right * step, down * step);
        }
    }
    if presses(Modifiers::NONE, Key::L) > 0 {
        state.next_look(series);
    }
}

/// The status-bar hint of the 3D view.
pub(crate) fn status_hint() -> String {
    tr!(
        "Drag: turn · Arrow keys: turn · Shift+A, P, L, R, S, I: standard views · Shift-drag: pan · Right-drag: contrast · Pinch/{command}-scroll: zoom · Double-click: fit",
        command = command_key()
    )
}

/// The picture in `colormap`: the grey level of every pixel that shows the
/// volume through the map's table, as in the slices; the empty background
/// stays as it is.
fn coloured(rgba: &[u8], colormap: Colormap) -> Cow<'_, [u8]> {
    if colormap == Colormap::Grey {
        return Cow::Borrowed(rgba);
    }
    let table = colormap.table();
    let mut out = rgba.to_vec();
    for pixel in out.as_chunks_mut::<4>().0 {
        if pixel[3] > 0 {
            let [r, g, b] = table[usize::from(pixel[0])];
            pixel[..3].copy_from_slice(&[r, g, b]);
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn a_mip_takes_the_colour_map_and_the_background_stays_empty() {
        // A pixel of the volume (grey level 200) and one of the empty background.
        let rgba = [200, 200, 200, 255, 0, 0, 0, 0];
        assert!(matches!(coloured(&rgba, Colormap::Grey), Cow::Borrowed(_)), "grey: the picture as rendered");
        assert_eq!(&*coloured(&rgba, Colormap::InvertedGrey), &[55, 55, 55, 255, 0, 0, 0, 0]);
        let [r, g, b] = Colormap::HotIron.rgb(200);
        assert_eq!(&*coloured(&rgba, Colormap::HotIron), &[r, g, b, 255, 0, 0, 0, 0]);
    }

    /// The lines say which colours a projection is shown in, for series that have colour maps.
    pub(crate) fn colours_line(series: &SeriesData, state: &ViewState) -> Option<String> {
        let style = state.volume_style();
        contrast_lines(series, state, &style).into_iter().find(|line| line.starts_with("Colours: "))
    }
}
