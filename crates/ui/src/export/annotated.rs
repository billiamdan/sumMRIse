//! "With annotations": the active view as displayed, in an export pass of its
//! own. The very code that draws the view on screen (`viewport::show`, or the
//! 3D view's overlays) runs once more, in a private egui context with the size
//! of the view, with the privacy filter applied and a caption added under it,
//! and `raster` paints the result. Whatever a picture shows on screen is in the
//! file, nothing of the window around it, nothing flickers, and it is tested
//! offscreen like any other function (design D12).

use egui::epaint::Shape;
use egui::{Align2, Context, FontId, Id, Pos2, Rect, Theme, ThemePreference, Vec2, ViewportId, ViewportInfo, vec2};
use summrise_core::export::Picture;
use summrise_core::{Settings, Tiles, Tool, ViewKind};
use summrise_i18n::tr;

use super::Selected;
use super::raster::Raster;
use crate::render::RenderCache;
use crate::theme;
use crate::tiles::TileInput;
use crate::viewport::{self, ViewportInput};
use crate::volume_view;

/// Height of the band under the view with the caption, in points.
const CAPTION_BAND: f32 = 24.0;
/// The picture has at least this many pixels per point, also from a window on a
/// screen with fewer, so that its text stays sharp.
const MIN_PIXELS_PER_POINT: f32 = 2.0;
/// Larger than any screen's: a picture on screen fits, and egui asserts it.
const MAX_TEXTURE_SIDE: usize = 16_384;

/// The active view in `size` points as a picture, with the patient's details
/// only if `include_details`.
pub(super) fn render(
    parent: &Context,
    selected: &Selected,
    settings: &Settings,
    include_details: bool,
    size: Vec2,
) -> Option<Picture> {
    pass(parent, selected, settings, include_details, size).map(|(picture, _)| picture)
}

/// [`render`], also returning all text drawn into the picture.
pub(crate) fn pass(
    parent: &Context,
    selected: &Selected,
    settings: &Settings,
    include_details: bool,
    size: Vec2,
) -> Option<(Picture, Vec<String>)> {
    let pixels_per_point = parent.pixels_per_point().max(MIN_PIXELS_PER_POINT);
    let ctx = Context::default();
    ctx.set_fonts(super::font_definitions(parent));
    for theme in [Theme::Dark, Theme::Light] {
        ctx.set_style_of(theme, parent.style_of(theme));
    }
    // Viewports are dark whatever the interface is.
    ctx.set_theme(ThemePreference::Dark);

    // Every annotation, and the patient's details only if asked for.
    let settings = Settings { show_overlays: true, hide_patient_details: !include_details, ..settings.clone() };
    let view = Rect::from_min_size(Pos2::ZERO, size);
    let whole = Rect::from_min_max(Pos2::ZERO, view.max + vec2(0.0, CAPTION_BAND));
    let raw_input = egui::RawInput {
        screen_rect: Some(whole),
        max_texture_side: Some(MAX_TEXTURE_SIDE),
        viewports: std::iter::once((
            ViewportId::ROOT,
            ViewportInfo { native_pixels_per_point: Some(pixels_per_point), ..ViewportInfo::default() },
        ))
        .collect(),
        ..egui::RawInput::default()
    };

    let mut state = selected.state.clone();
    let mut renders = RenderCache::default();
    let overlay = selected.overlay.as_ref().map(|(key, data)| crate::pet::Overlay { key: *key, data });
    let mut shown_3d = None;
    let mut output = ctx.run_ui(raw_input, |ui| {
        let kind = selected.view;
        let smooth = settings.smooth_interpolation;
        renders.update(ui.ctx(), selected.key, kind, &selected.series, &state, smooth, overlay);
        // The tile grid, as on screen: the following images of the image set shown.
        let (first, count) = (state.slice_index(kind, &selected.series), selected.series.stack_len());
        let slots: Vec<Option<usize>> =
            selected.tiles.images(first, count).map(|image| image.map(|image| state.stack() * count + image)).collect();
        for &slot in slots.iter().skip(1).flatten() {
            renders.update_tile(ui.ctx(), selected.key, slot, &selected.series, &state, smooth, overlay);
        }
        let pictures = slots.iter().enumerate().map(|(tile, slot)| match tile {
            0 => renders.get(selected.key, kind),
            _ => slot.and_then(|slot| renders.get_tile(selected.key, slot)),
        });
        let grid = (selected.tiles != Tiles::ONE).then(|| TileInput {
            tiles: selected.tiles,
            first,
            count,
            pictures: pictures.collect(),
        });
        let input = ViewportInput {
            id: Id::unique("export view"),
            key: selected.key,
            kind,
            series: &selected.series,
            meta: &selected.meta,
            study: &selected.study,
            rendered: renders.get(selected.key, kind),
            tool: Tool::default(),
            settings: &settings,
            is_active: true,
            multi_view: false,
            show_details: true,
            fusion: selected.fusion.as_ref(),
            tiles: grid.as_ref(),
            compare: None,
        };
        if kind == ViewKind::Volume {
            if let Some(picture) = &selected.picture_3d {
                shown_3d = Some(volume_view::paint_shown(ui, view, &input, &state, picture));
            }
        } else {
            viewport::show(ui, view, &input, &mut state);
        }
        // Under the picture, so that it covers nothing of what the view shows.
        let band = Rect::from_min_max(view.left_bottom(), whole.max);
        ui.painter().rect_filled(band, 0.0, theme::VIEWPORT_BG);
        ui.painter().text(
            band.center(),
            Align2::CENTER_CENTER,
            tr!("Not for diagnostic use"),
            FontId::proportional(12.0),
            theme::OVERLAY_TEXT,
        );
    });
    let shapes = std::mem::take(&mut output.shapes);
    let mut raster = Raster::default();
    raster.apply(&output.textures_delta);
    output.textures_delta.clear();

    let mut texts = Vec::new();
    for shape in &shapes {
        collect_text(&shape.shape, &mut texts);
    }
    let primitives = ctx.tessellate(shapes, output.pixels_per_point);
    let (width, height) =
        ((whole.width() * pixels_per_point).round() as usize, (whole.height() * pixels_per_point).round() as usize);
    let picture = raster.paint(&primitives, width, height, output.pixels_per_point);
    Some((picture, texts))
}

fn collect_text(shape: &Shape, texts: &mut Vec<String>) {
    match shape {
        Shape::Text(text) => texts.push(text.galley.text().to_owned()),
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect_text(shape, texts)),
        _ => {}
    }
}
