//! Tile grids: where the tiles of a viewport lie and what the viewport says
//! about them (`openspec/changes/series-comparison/design.md`, D8).
//!
//! The first tile always shows the current image of the series' view state,
//! the following tiles the next images of the same image set. Every tile is
//! drawn by `viewport::picture`, the path of a single image. The annotations
//! appear once per viewport, each on one line in a strip above and below the
//! tiles, so that they cover no picture.

use egui::{Align2, Painter, Rect, pos2, vec2};
use summrise_core::dicom::format;
use summrise_core::{SeriesData, Tiles, ViewKind, ViewState};
use summrise_i18n::tr;

use crate::placement::Placement;
use crate::render::Rendered;
use crate::viewport::{self, CORNER_GAP, Cell, OVERLAY_SIZE, ViewportInput};
use crate::widgets::{elide_lines, overlay_block, share_row, text_width};

/// Height of the strips above and below the tiles, in points: one line of
/// annotation, below the title line in a viewport of several; the scale bar
/// and its label in the lower one.
const TOP_STRIP: f32 = 22.0;
const TOP_STRIP_MULTI_VIEW: f32 = 44.0;
const BOTTOM_STRIP: f32 = 30.0;
const SIDE_MARGIN: f32 = 4.0;

/// The pictures of a tile grid, for the viewport to draw.
pub(crate) struct TileInput<'a> {
    pub tiles: Tiles,
    /// Index of the image of the first tile within its image set, from 0.
    pub first: usize,
    /// Images in the image set.
    pub count: usize,
    /// The picture of each tile; `None` past the last image. The first is the
    /// current image's, as in a single view.
    pub pictures: Vec<Option<&'a Rendered>>,
}

/// The part of a viewport that the tiles share.
pub(crate) fn area(viewport: Rect, multi_view: bool) -> Rect {
    let top = if multi_view { TOP_STRIP_MULTI_VIEW } else { TOP_STRIP };
    Rect::from_min_max(viewport.min + vec2(SIDE_MARGIN, top), viewport.max - vec2(SIDE_MARGIN, BOTTOM_STRIP))
}

/// The tile grid a viewport shows: the one chosen, or the largest smaller one
/// whose tiles stay large enough in a viewport of this size. None but acquired
/// images of more than one image show tiles.
pub(crate) fn shown(chosen: Tiles, viewport: Rect, multi_view: bool, kind: ViewKind, series: &SeriesData) -> Tiles {
    if kind != ViewKind::Stack || series.stack_len() < 2 {
        return Tiles::ONE;
    }
    let size = area(viewport, multi_view).size();
    chosen.largest_fitting([size.x, size.y])
}

/// The rectangles of the tiles of `tiles` in `area`, in reading order.
pub(crate) fn rects(area: Rect, tiles: Tiles) -> Vec<Rect> {
    let side = tiles.side();
    let gap = Tiles::GAP_POINTS;
    let width = (area.width() - gap * (side - 1) as f32) / side as f32;
    let height = (area.height() - gap * (side - 1) as f32) / side as f32;
    (0..tiles.count())
        .map(|tile| {
            let (column, row) = ((tile % side) as f32, (tile / side) as f32);
            Rect::from_min_size(area.min + vec2(column * (width + gap), row * (height + gap)), vec2(width, height))
        })
        .collect()
}

/// The cells of a viewport with a tile grid.
pub(crate) fn cells<'a>(rect: Rect, input: &TileInput<'a>, multi_view: bool) -> Vec<Cell<'a>> {
    let area = area(rect, multi_view);
    rects(area, input.tiles)
        .into_iter()
        .zip(&input.pictures)
        .enumerate()
        .map(|(tile, (rect, picture))| Cell {
            rect,
            rendered: *picture,
            number: picture.map(|_| input.first + tile + 1),
        })
        .collect()
}

/// The images a grid shows, in words: "Images 31 to 39 / 60".
pub(crate) fn describe(input: &TileInput<'_>) -> String {
    let shown = input.pictures.iter().flatten().count();
    let (first, last) = (input.first + 1, input.first + shown.max(1));
    tr!("Images {first} to {last} / {count}", first = first, last = last, count = input.count)
}

/// The annotations of a viewport with a tile grid, once per viewport.
#[allow(clippy::too_many_arguments)]
pub(crate) fn annotations(
    painter: &Painter,
    rect: Rect,
    place: &Placement,
    input: &ViewportInput<'_>,
    grid: &TileInput<'_>,
    state: &ViewState,
    rendered: &Rendered,
    progress: Option<&str>,
) {
    let m = 10.0;
    let available = rect.width() - 2.0 * m - CORNER_GAP;
    let (study, series) = (input.study, input.series);

    // The row above the tiles: the patient and, in a viewport alone, the series.
    let name = (input.show_details && !input.settings.hide_patient_details).then(|| {
        vec![study.patient.name.as_deref().map(format::person_name).unwrap_or_else(|| tr!("Unknown patient").into())]
    });
    let title = (input.show_details && input.compare.is_none()).then(|| {
        vec![match input.meta.number {
            // The same line as above the images of a single view.
            Some(number) => tr!("S{number}: {title}", number = number, title = input.meta.title()),
            None => input.meta.title(),
        }]
    });
    let width = |lines: &Option<Vec<String>>| lines.as_deref().map_or(0.0, |l| text_width(painter, l, OVERLAY_SIZE));
    let (left_max, right_max) = share_row(width(&name), width(&title), available);
    let top = rect.top() + if input.multi_view { 26.0 } else { 5.0 };
    if let Some(lines) = name {
        let lines = elide_lines(painter, lines, OVERLAY_SIZE, left_max);
        overlay_block(painter, pos2(rect.left() + m, top), Align2::LEFT_TOP, &lines, OVERLAY_SIZE);
    }
    if let Some(lines) = title {
        let lines = elide_lines(painter, lines, OVERLAY_SIZE, right_max);
        overlay_block(painter, pos2(rect.right() - m, top), Align2::RIGHT_TOP, &lines, OVERLAY_SIZE);
    }

    // The row below the tiles: which images, then contrast and zoom.
    let mut position = vec![describe(grid)];
    position.extend(series.stack_label(state.stack()));
    position.extend(progress.map(str::to_owned));
    let bottom_left = vec![position.join(" · ")];
    let mut contrast = Vec::new();
    let table = state.active_lut(series);
    if let Some(table) = table.filter(|_| !series.is_color()) {
        contrast.push(table.name.clone());
    } else if !series.is_color() {
        contrast.push(crate::pet::window_annotation(series.units(), &state.window));
        contrast.push(state.preset_label(series).map_or_else(|| tr!("Custom window").to_owned(), str::to_owned));
    }
    let percent = format!("{:.0}", state.transform(input.kind).zoom * 100.0);
    contrast.push(if state.invert {
        tr!("Zoom {percent}% · Inverted", percent = percent)
    } else {
        tr!("Zoom {percent}%", percent = percent)
    });
    let bottom_right = vec![contrast.join(" · ")];
    let (left_max, right_max) = share_row(
        text_width(painter, &bottom_left, OVERLAY_SIZE),
        text_width(painter, &bottom_right, OVERLAY_SIZE),
        available,
    );
    let bottom_left = elide_lines(painter, bottom_left, OVERLAY_SIZE, left_max);
    let bottom_right = elide_lines(painter, bottom_right, OVERLAY_SIZE, right_max);
    let corner_y = rect.bottom() - 8.0;
    overlay_block(painter, pos2(rect.left() + m, corner_y), Align2::LEFT_BOTTOM, &bottom_left, OVERLAY_SIZE);
    overlay_block(painter, pos2(rect.right() - m, corner_y), Align2::RIGHT_BOTTOM, &bottom_right, OVERLAY_SIZE);
    // What is said about a PET stands above them, over the last tiles.
    let pet = crate::pet::annotation_lines(series, state, input.fusion);
    if !pet.is_empty() {
        let above = pos2(rect.right() - m, corner_y - OVERLAY_SIZE - 6.0);
        let pet = elide_lines(painter, pet, OVERLAY_SIZE, available);
        overlay_block(painter, above, Align2::RIGHT_BOTTOM, &pet, OVERLAY_SIZE);
    }
    // The scale of the first tile: all tiles of a series normally share it.
    let free = (
        rect.left() + m + text_width(painter, &bottom_left, OVERLAY_SIZE) + CORNER_GAP,
        rect.right() - m - text_width(painter, &bottom_right, OVERLAY_SIZE) - CORNER_GAP,
    );
    viewport::draw_scale_bar(painter, rect, place, &rendered.displayed, input, free);
}

#[cfg(test)]
mod tests {
    use egui::pos2;

    use super::*;

    #[test]
    fn tiles_fill_the_area_in_reading_order_without_overlapping() {
        let area = Rect::from_min_size(pos2(100.0, 50.0), vec2(302.0, 152.0));
        let rects = rects(area, Tiles::ALL[1]);
        assert_eq!(rects.len(), 4);
        assert_eq!(rects[0], Rect::from_min_size(pos2(100.0, 50.0), vec2(150.0, 75.0)));
        assert_eq!(rects[1], Rect::from_min_size(pos2(252.0, 50.0), vec2(150.0, 75.0)));
        assert_eq!(rects[2], Rect::from_min_size(pos2(100.0, 127.0), vec2(150.0, 75.0)));
        assert_eq!(rects[3].max, area.max);
    }

    /// The scenario of the specification with the strips of a viewport: what
    /// is offered in a viewport 320 points wide.
    #[test]
    fn a_viewport_320_points_wide_offers_up_to_3_by_3() {
        let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(320.0, 600.0));
        let offered: Vec<usize> = Tiles::ALL
            .into_iter()
            .filter(|tiles| {
                let size = area(viewport, true).size();
                tiles.fits([size.x, size.y])
            })
            .map(Tiles::side)
            .collect();
        assert_eq!(offered, [1, 2, 3]);
    }
}
