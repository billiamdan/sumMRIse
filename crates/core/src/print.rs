//! Sheets for printing (`openspec/changes/export-and-share`, design D9 to D11):
//! raster pages on A4 or Letter with 1 to 12 images each, composed in memory as
//! a pure function of the pictures, the options and the fonts, so that all of
//! it is tested without a window or a printer. [`pdf`] turns the pages into a
//! PDF and [`system`] hands that to the print system.
//!
//! This crate knows no fonts: text comes through [`TextRaster`], which the
//! interface implements with its own fonts. Every language therefore prints as
//! it reads on screen, and the PDF holds pictures only, no fonts.

use std::ops::Range;

use summrise_i18n::{tr, tr_ctx};
use summrise_imaging::EdgeLabels;

use crate::export::Picture;

pub mod pdf;
pub mod system;

pub use pdf::PdfWriter;
pub use system::{JOB_TITLE, PrintError, PrintJob, PrintSystem, Printers};

/// Resolution of printed pages.
pub const PRINT_DPI: u32 = 300;
/// Resolution of the preview of a page in the print window.
pub const PREVIEW_DPI: u32 = 110;
/// The sheets on offer.
pub const IMAGES_PER_PAGE: [usize; 6] = [1, 2, 4, 6, 9, 12];

/// Distance of everything from the edge of the sheet, in points (1/72 inch).
const MARGIN: f64 = 24.0;
/// Space between the cells of a sheet, in points.
const CELL_GAP: f64 = 6.0;
/// Scale bars on offer, in millimetres, as in the viewport.
const SCALE_BARS: [u32; 8] = [200, 100, 50, 20, 10, 5, 2, 1];
const INK: [u8; 3] = [0, 0, 0];
/// As on screen: `theme::OVERLAY_TEXT` and the colour of the edge letters.
const CELL_TEXT: [u8; 3] = [0xE8, 0xE8, 0xE8];
const LETTERS: [u8; 3] = [0xFF, 0xD3, 0x6E];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Paper {
    #[default]
    A4,
    Letter,
}

impl Paper {
    pub const ALL: [Self; 2] = [Self::A4, Self::Letter];

    /// Width and height in millimetres, upright.
    fn millimetres(self) -> (f64, f64) {
        match self {
            Self::A4 => (210.0, 297.0),
            Self::Letter => (215.9, 279.4),
        }
    }

    /// What it is called in a list.
    pub fn label(self) -> &'static str {
        match self {
            Self::A4 => "A4",
            // Translators: the paper size of North America (8.5 × 11 inches).
            Self::Letter => tr_ctx!("paper", "Letter"),
        }
    }

    /// What the print system calls it.
    pub(crate) fn media(self) -> &'static str {
        match self {
            Self::A4 => "A4",
            Self::Letter => "Letter",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageOrientation {
    #[default]
    Portrait,
    Landscape,
}

impl PageOrientation {
    pub const ALL: [Self; 2] = [Self::Portrait, Self::Landscape];

    pub fn label(self) -> &'static str {
        match self {
            // Translators: the orientation of a printed page.
            Self::Portrait => tr!("Portrait"),
            Self::Landscape => tr!("Landscape"),
        }
    }
}

/// The shape of a print job: paper, orientation, images per page and resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSetup {
    pub paper: Paper,
    pub orientation: PageOrientation,
    /// One of [`IMAGES_PER_PAGE`].
    pub per_page: usize,
    pub dpi: u32,
}

impl PageSetup {
    /// Width and height of a page in pixels.
    pub fn pixels(&self) -> (usize, usize) {
        let (short, long) = self.paper.millimetres();
        let px = |mm: f64| (mm / 25.4 * f64::from(self.dpi)).round() as usize;
        match self.orientation {
            PageOrientation::Portrait => (px(short), px(long)),
            PageOrientation::Landscape => (px(long), px(short)),
        }
    }

    /// Columns and rows of the images on a page.
    pub fn grid(&self) -> (usize, usize) {
        let landscape = self.orientation == PageOrientation::Landscape;
        match self.per_page {
            0 | 1 => (1, 1),
            2 => {
                if landscape {
                    (2, 1)
                } else {
                    (1, 2)
                }
            }
            3 | 4 => (2, 2),
            5 | 6 => {
                if landscape {
                    (3, 2)
                } else {
                    (2, 3)
                }
            }
            7..=9 => (3, 3),
            _ => {
                if landscape {
                    (4, 3)
                } else {
                    (3, 4)
                }
            }
        }
    }

    /// How many pages `images` images take.
    pub fn page_count(&self, images: usize) -> usize {
        images.div_ceil(self.per_page.max(1))
    }

    /// Which of `images` images are on page `page` (from 0).
    pub fn page_images(&self, page: usize, images: usize) -> Range<usize> {
        let per_page = self.per_page.max(1);
        (page * per_page).min(images)..((page + 1) * per_page).min(images)
    }

    fn px(&self, points: f64) -> f64 {
        points * f64::from(self.dpi) / 72.0
    }
}

/// Text as coverage, 0 to 255, one byte per pixel from the top left.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextBitmap {
    pub width: usize,
    pub height: usize,
    pub coverage: Vec<u8>,
}

/// Rasterises one line of text at a font size in pixels. The interface does
/// this with its own fonts.
pub trait TextRaster {
    fn line(&mut self, text: &str, size: f64) -> TextBitmap;
}

/// One image on a sheet, with what is said about it.
#[derive(Debug, Clone)]
pub struct PagePicture {
    /// The picture as the view shows it: square pixels, turned and mirrored.
    pub picture: Picture,
    /// Millimetres per pixel of `picture`, when known: gives the scale bar.
    pub mm_per_pixel: Option<f64>,
    /// The orientation letters at the edges of `picture`.
    pub letters: Option<EdgeLabels>,
    /// Its number: "Image 12 / 60".
    pub number: String,
    /// The contrast it is shown with: "W 400  L 40 HU".
    pub window: Option<String>,
    /// Anything else the view would say about it: that it is mirrored, a
    /// reformat or an illustration, its image set, its lossy compression.
    pub notes: Vec<String>,
}

/// What every page of a job says at its top. It holds patient details only
/// when the user asked for them for this job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageHeader {
    /// The study description and date.
    pub study: String,
    /// The series description.
    pub series: String,
    /// The patient's name, ID and birth date; empty by default.
    pub patient: Vec<String>,
}

/// A rectangle of a page, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// A scale bar as drawn on a page, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleBar {
    /// Its first and last column.
    pub columns: (usize, usize),
    /// A row it is drawn on.
    pub row: usize,
    pub millimetres: u32,
}

/// Where the parts of one image are on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct CellLayout {
    pub cell: PageRect,
    pub picture: PageRect,
    pub scale_bar: Option<ScaleBar>,
}

/// A composed page.
#[derive(Debug, Clone)]
pub struct Page {
    pub image: Picture,
    pub cells: Vec<CellLayout>,
}

/// Composes page `page` (from 0) of `pages` with the images `pictures`. `smooth`:
/// a picture is interpolated rather than shown with its pixels repeated, as on
/// screen. The page stays white to save ink; each image is on black in its cell.
pub fn compose_page(
    setup: &PageSetup,
    header: &PageHeader,
    page: usize,
    pages: usize,
    pictures: &[PagePicture],
    smooth: bool,
    fonts: &mut dyn TextRaster,
) -> Page {
    let (width, height) = setup.pixels();
    let mut canvas = Canvas { image: Picture { width, height, rgb: vec![255; width * height * 3] } };
    let margin = setup.px(MARGIN).round() as usize;
    let content_width = width.saturating_sub(2 * margin);

    // Header: the study on the left and the page on the right; then the series; then
    // the patient, if asked for.
    let (title_size, line_size) = (setup.px(10.0), setup.px(9.0));
    let page_label = fonts.line(&tr!("Page {page} of {pages}", page = page + 1, pages = pages), line_size);
    let gap = setup.px(12.0) as usize;
    let study = elided(fonts, &header.study, title_size, content_width.saturating_sub(page_label.width + gap));
    canvas.text(&study, margin, margin, INK);
    canvas.text(&page_label, width.saturating_sub(margin + page_label.width), margin, INK);
    let mut y = margin + study.height.max(page_label.height);
    for line in std::iter::once(&header.series).chain(&header.patient) {
        let line = elided(fonts, line, line_size, content_width);
        canvas.text(&line, margin, y, INK);
        y += line.height;
    }

    let footer = elided(fonts, tr!("Not for diagnostic use"), line_size, content_width);
    let footer_top = height.saturating_sub(margin + footer.height);
    canvas.text(&footer, width.saturating_sub(footer.width) / 2, footer_top, INK);

    // The cells share what is left.
    let gap = setup.px(CELL_GAP).round() as usize;
    let top = y + gap;
    let bottom = footer_top.saturating_sub(gap);
    let (columns, rows) = setup.grid();
    let cell_width = content_width.saturating_sub((columns - 1) * gap) / columns;
    let cell_height = bottom.saturating_sub(top).saturating_sub((rows - 1) * gap) / rows;
    let cells = pictures
        .iter()
        .take(columns * rows)
        .enumerate()
        .map(|(i, picture)| {
            let cell = PageRect {
                x: margin + (i % columns) * (cell_width + gap),
                y: top + (i / columns) * (cell_height + gap),
                width: cell_width,
                height: cell_height,
            };
            draw_cell(&mut canvas, setup, cell, picture, smooth, fonts)
        })
        .collect();
    Page { image: canvas.image, cells }
}

/// Draws one image with its letters, number, window values and scale bar. No
/// text covers the picture: the letters have a band around it and the rest a
/// strip under it.
fn draw_cell(
    canvas: &mut Canvas,
    setup: &PageSetup,
    cell: PageRect,
    picture: &PagePicture,
    smooth: bool,
    fonts: &mut dyn TextRaster,
) -> CellLayout {
    canvas.fill(cell, [0, 0, 0]);
    let (letter_size, text_size) = match setup.per_page {
        0..=2 => (setup.px(14.0), setup.px(10.0)),
        3..=6 => (setup.px(11.0), setup.px(8.5)),
        _ => (setup.px(9.0), setup.px(7.0)),
    };
    let pad = (text_size * 0.4).ceil() as usize;
    let letter_band = if picture.letters.is_some() { (letter_size * 1.3).ceil() as usize } else { pad };
    let row_height = (text_size * 1.35).ceil() as usize;
    let strip = 2 * row_height + pad;

    // The picture, fitted with its proportions into what is left of the cell.
    let room_width = cell.width.saturating_sub(2 * letter_band).max(1);
    let room_height = cell.height.saturating_sub(2 * letter_band + strip).max(1);
    let (source_width, source_height) = (picture.picture.width.max(1), picture.picture.height.max(1));
    let scale = (room_width as f64 / source_width as f64).min(room_height as f64 / source_height as f64);
    let shown_width = ((source_width as f64 * scale).round() as usize).clamp(1, room_width);
    let shown_height = ((source_height as f64 * scale).round() as usize).clamp(1, room_height);
    let shown = PageRect {
        x: cell.x + letter_band + (room_width - shown_width) / 2,
        y: cell.y + letter_band + (room_height - shown_height) / 2,
        width: shown_width,
        height: shown_height,
    };
    canvas.picture(&picture.picture.scaled(shown_width, shown_height, smooth), shown);

    if let Some(letters) = &picture.letters {
        let left = fonts.line(&letters.left, letter_size);
        let right = fonts.line(&letters.right, letter_size);
        let top = fonts.line(&letters.top, letter_size);
        let bottom = fonts.line(&letters.bottom, letter_size);
        let middle_x = shown.x + shown.width / 2;
        let middle_y = shown.y + shown.height / 2;
        let beside = |height: usize| middle_y.saturating_sub(height / 2);
        let across = |width: usize| middle_x.saturating_sub(width / 2);
        canvas.text(&left, shown.x.saturating_sub(left.width + 1).max(cell.x), beside(left.height), LETTERS);
        canvas.text(&right, shown.x + shown.width + 1, beside(right.height), LETTERS);
        canvas.text(&top, across(top.width), shown.y.saturating_sub(top.height).max(cell.y), LETTERS);
        canvas.text(&bottom, across(bottom.width), shown.y + shown.height, LETTERS);
    }

    // Under the picture: number and window, then the notes and the scale bar.
    let inner_left = cell.x + pad;
    let inner_right = (cell.x + cell.width).saturating_sub(pad).max(inner_left);
    let inner_width = inner_right - inner_left;
    let row_a = (cell.y + cell.height).saturating_sub(strip) + pad / 2;
    let row_b = row_a + row_height;
    let window = picture.window.as_ref().map(|w| elided(fonts, w, text_size, inner_width * 3 / 5));
    let window_width = window.as_ref().map_or(0, |w| w.width + pad);
    let number = elided(fonts, &picture.number, text_size, inner_width.saturating_sub(window_width));
    canvas.text(&number, inner_left, row_a, CELL_TEXT);
    if let Some(window) = &window {
        canvas.text(window, inner_right.saturating_sub(window.width), row_a, CELL_TEXT);
    }

    let mut used_on_right = 0;
    let mut scale_bar = None;
    if let Some(mm_per_pixel) = picture.mm_per_pixel.filter(|mm| mm.is_finite() && *mm > 0.0) {
        let px_per_mm = shown_width as f64 / (source_width as f64 * mm_per_pixel);
        let longest = cell.width as f64 * 0.25;
        let shortest = text_size;
        if let Some(&millimetres) =
            SCALE_BARS.iter().find(|&&mm| (shortest..=longest).contains(&(f64::from(mm) * px_per_mm)))
        {
            // True to the printed picture; the print itself is not true size.
            let length = (f64::from(millimetres) * px_per_mm).round() as usize;
            let label = fonts.line(&format!("{millimetres} mm"), text_size);
            let line = setup.px(0.75).round().max(1.0) as usize;
            let tick = (text_size * 0.35) as usize;
            let row = row_b + row_height / 2;
            let last = inner_right.saturating_sub(1);
            let first = (last + 1).saturating_sub(length);
            canvas.fill(PageRect { x: first, y: row, width: length, height: line }, CELL_TEXT);
            for x in [first, (last + 1).saturating_sub(line)] {
                canvas
                    .fill(PageRect { x, y: row.saturating_sub(tick), width: line, height: 2 * tick + line }, CELL_TEXT);
            }
            let label_x = first.saturating_sub(pad + label.width);
            canvas.text(&label, label_x, row_b, CELL_TEXT);
            used_on_right = length + pad + label.width + pad;
            scale_bar = Some(ScaleBar { columns: (first, last), row, millimetres });
        }
    }
    if !picture.notes.is_empty() {
        let notes = elided(fonts, &picture.notes.join(" · "), text_size, inner_width.saturating_sub(used_on_right));
        canvas.text(&notes, inner_left, row_b, CELL_TEXT);
    }
    CellLayout { cell, picture: shown, scale_bar }
}

/// `text` shortened with "…" to at most `max_width` pixels.
fn elided(fonts: &mut dyn TextRaster, text: &str, size: f64, max_width: usize) -> TextBitmap {
    let full = fonts.line(text, size);
    if full.width <= max_width {
        return full;
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().chain(std::iter::once(&'…')).collect();
        if fonts.line(&candidate, size).width <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let kept: String = chars[..lo].iter().collect();
    fonts.line(&format!("{}…", kept.trim_end()), size)
}

impl Picture {
    /// The picture with `caption` centred in a black band under it, which
    /// covers nothing of the picture: the 3D view's "Illustration", which stays
    /// with its picture wherever it goes.
    pub fn with_caption(&self, caption: &str, fonts: &mut dyn TextRaster) -> Self {
        let size = (self.width as f64 / 48.0).max(11.0);
        let line = elided(fonts, caption, size, self.width);
        let pad = (size * 0.4).ceil() as usize;
        let band = line.height + 2 * pad;
        let mut canvas = Canvas { image: Self::black(self.width, self.height + band) };
        canvas.image.rgb[..self.rgb.len()].copy_from_slice(&self.rgb);
        canvas.text(&line, self.width.saturating_sub(line.width) / 2, self.height + pad, CELL_TEXT);
        canvas.image
    }
}

/// A page being drawn.
struct Canvas {
    image: Picture,
}

impl Canvas {
    fn fill(&mut self, rect: PageRect, color: [u8; 3]) {
        let width = self.image.width;
        let right = (rect.x + rect.width).min(width);
        for y in rect.y..(rect.y + rect.height).min(self.image.height) {
            for pixel in
                self.image.rgb[(y * width + rect.x.min(right)) * 3..(y * width + right) * 3].as_chunks_mut::<3>().0
            {
                pixel.copy_from_slice(&color);
            }
        }
    }

    fn picture(&mut self, picture: &Picture, at: PageRect) {
        let width = self.image.width;
        for (y, row) in picture.rgb.chunks_exact((picture.width * 3).max(1)).enumerate() {
            if at.y + y >= self.image.height || at.x + picture.width > width {
                break;
            }
            let start = ((at.y + y) * width + at.x) * 3;
            self.image.rgb[start..start + row.len()].copy_from_slice(row);
        }
    }

    /// Draws text with its top left corner at (`x`, `y`), cut off at the page's edges.
    fn text(&mut self, text: &TextBitmap, x: usize, y: usize, color: [u8; 3]) {
        let width = self.image.width;
        for row in 0..text.height.min(self.image.height.saturating_sub(y)) {
            for column in 0..text.width.min(width.saturating_sub(x)) {
                let coverage = u32::from(text.coverage[row * text.width + column]);
                if coverage == 0 {
                    continue;
                }
                let at = ((y + row) * width + x + column) * 3;
                for (channel, &ink) in self.image.rgb[at..at + 3].iter_mut().zip(&color) {
                    let mixed = (u32::from(*channel) * (255 - coverage) + u32::from(ink) * coverage + 127) / 255;
                    *channel = mixed as u8;
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use summrise_imaging::DVec3;
    use summrise_testkit::{PATIENT_ID, PATIENT_NAME};

    use super::*;

    /// A font of solid blocks that remembers everything it was asked to write.
    #[derive(Default)]
    pub(crate) struct Blocks {
        pub(crate) written: Vec<String>,
    }

    impl TextRaster for Blocks {
        fn line(&mut self, text: &str, size: f64) -> TextBitmap {
            self.written.push(text.to_owned());
            let (width, height) = ((text.chars().count() as f64 * size * 0.5).ceil() as usize, size.ceil() as usize);
            TextBitmap { width, height, coverage: vec![255; width * height] }
        }
    }

    fn setup(per_page: usize, orientation: PageOrientation, paper: Paper) -> PageSetup {
        PageSetup { paper, orientation, per_page, dpi: PRINT_DPI }
    }

    pub(crate) fn picture(width: usize, height: usize, grey: u8, mm_per_pixel: Option<f64>) -> PagePicture {
        PagePicture {
            picture: Picture { width, height, rgb: vec![grey; width * height * 3] },
            mm_per_pixel,
            letters: Some(EdgeLabels::new(DVec3::X, DVec3::Y)),
            number: "Image 1 / 60".to_owned(),
            window: Some("W 400  L 40 HU".to_owned()),
            notes: Vec::new(),
        }
    }

    fn header() -> PageHeader {
        PageHeader { study: "Synthetic study".to_owned(), series: "CT CHEST".to_owned(), patient: Vec::new() }
    }

    fn compose(setup: &PageSetup, pictures: &[PagePicture]) -> Page {
        compose_page(setup, &header(), 0, 1, pictures, true, &mut Blocks::default())
    }

    #[test]
    fn page_sizes_at_300_dpi() {
        let a4 = setup(1, PageOrientation::Portrait, Paper::A4);
        assert_eq!(a4.pixels(), (2480, 3508));
        assert_eq!(setup(1, PageOrientation::Landscape, Paper::A4).pixels(), (3508, 2480));
        assert_eq!(setup(1, PageOrientation::Portrait, Paper::Letter).pixels(), (2550, 3300));
        assert_eq!(setup(1, PageOrientation::Landscape, Paper::Letter).pixels(), (3300, 2550));
    }

    /// Spec: a 60-image series with 9 images per page: 7 pages, the first
    /// with images 1 to 9, the last with images 55 to 60.
    #[test]
    fn a_series_on_sheets_has_the_right_pages() {
        let sheet = setup(9, PageOrientation::Portrait, Paper::A4);
        assert_eq!(sheet.page_count(60), 7);
        assert_eq!(sheet.page_images(0, 60), 0..9);
        assert_eq!(sheet.page_images(6, 60), 54..60);
        assert_eq!(sheet.page_images(7, 60), 60..60);
        assert_eq!(sheet.page_count(0), 0);
        assert_eq!(setup(12, PageOrientation::Portrait, Paper::A4).page_count(12), 1);
        assert_eq!(setup(12, PageOrientation::Portrait, Paper::A4).page_count(13), 2);
    }

    /// Task 4.1: every sheet in both orientations on both papers: the right
    /// number of cells, inside the page, side by side without touching.
    #[test]
    fn every_sheet_has_its_cells_inside_the_page() {
        for paper in Paper::ALL {
            for orientation in PageOrientation::ALL {
                for per_page in IMAGES_PER_PAGE {
                    let what = format!("{per_page} per page, {paper:?}, {orientation:?}");
                    let sheet = setup(per_page, orientation, paper);
                    let (columns, rows) = sheet.grid();
                    assert_eq!(columns * rows, per_page, "{what}");
                    let (width, height) = sheet.pixels();
                    if width > height {
                        assert!(columns >= rows, "{what}: wide pages have the columns");
                    } else {
                        assert!(rows >= columns, "{what}: tall pages have the rows");
                    }
                    let pictures: Vec<PagePicture> = (0..per_page).map(|_| picture(64, 64, 200, Some(1.0))).collect();
                    let page = compose(&sheet, &pictures);
                    assert_eq!((page.image.width, page.image.height), (width, height), "{what}");
                    assert_eq!(page.cells.len(), per_page, "{what}");
                    for (i, layout) in page.cells.iter().enumerate() {
                        let cell = layout.cell;
                        assert!(cell.width > 0 && cell.height > 0, "{what}");
                        assert!(cell.x + cell.width <= width && cell.y + cell.height <= height, "{what}: {cell:?}");
                        let shown = layout.picture;
                        assert!(shown.width > 100 && shown.height > 100, "{what}: {shown:?}");
                        assert!(
                            shown.x >= cell.x
                                && shown.y >= cell.y
                                && shown.x + shown.width <= cell.x + cell.width
                                && shown.y + shown.height <= cell.y + cell.height,
                            "{what}: the picture stays in its cell"
                        );
                        for other in &page.cells[i + 1..] {
                            let apart = cell.x + cell.width <= other.cell.x
                                || other.cell.x + other.cell.width <= cell.x
                                || cell.y + cell.height <= other.cell.y
                                || other.cell.y + other.cell.height <= cell.y;
                            assert!(apart, "{what}: cells overlap");
                        }
                    }
                }
            }
        }
    }

    /// Design D11: the page stays white, each image is on black in its cell,
    /// and a sheet that is not full leaves the rest of the page white.
    #[test]
    fn the_page_stays_white_and_the_cells_are_black() {
        let sheet = setup(9, PageOrientation::Portrait, Paper::A4);
        let pictures = [picture(64, 64, 200, Some(1.0)), picture(64, 64, 200, Some(1.0))];
        let page = compose(&sheet, &pictures);
        let image = &page.image;
        assert_eq!(image.pixel(0, 0), [255; 3]);
        assert_eq!(image.pixel(image.width - 1, image.height - 1), [255; 3]);
        let first = &page.cells[0];
        assert_eq!(image.pixel(first.cell.x + 2, first.cell.y + 2), [0; 3], "on black");
        assert_eq!(image.pixel(first.picture.x + 5, first.picture.y + 5), [200; 3], "the picture");
        // Where the third image would go.
        let below = first.cell.y + first.cell.height + 200;
        assert_eq!(image.pixel(first.cell.x + 200, below), [255; 3], "no ink for an empty cell");
    }

    /// Spec "Non-square pixels" (task 4.1): the picture arrives with square
    /// pixels and keeps its proportions in the cell, on any sheet.
    #[test]
    fn a_picture_keeps_its_proportions_on_every_sheet() {
        // 256 columns × 128 rows at 1.0 \ 0.5 becomes 256 × 256 (see `export`).
        let (square, mm) =
            Picture { width: 256, height: 128, rgb: vec![90; 256 * 128 * 3] }.with_square_pixels(Some([1.0, 0.5]));
        let square = PagePicture { picture: square, mm_per_pixel: mm, ..picture(1, 1, 0, None) };
        // And a wide one: 240 × 120, twice as wide as it is high.
        let wide = picture(240, 120, 90, None);
        for orientation in PageOrientation::ALL {
            for per_page in IMAGES_PER_PAGE {
                let sheet = setup(per_page, orientation, Paper::A4);
                let page = compose(&sheet, std::slice::from_ref(&square));
                let shown = page.cells[0].picture;
                assert!(shown.width.abs_diff(shown.height) <= 1, "{per_page}, {orientation:?}: {shown:?}");
                let page = compose(&sheet, std::slice::from_ref(&wide));
                let shown = page.cells[0].picture;
                let ratio = shown.width as f64 / shown.height as f64;
                assert!((ratio - 2.0).abs() < 0.02, "{per_page}, {orientation:?}: {shown:?}");
            }
        }
    }

    /// Spec "The scale bar on paper": measured on the page, it keeps its
    /// proportion to the picture and is labelled "50 mm".
    #[test]
    fn the_scale_bar_is_true_to_the_printed_picture() {
        // 240 mm wide: 512 pixels of 0.46875 mm.
        let mm_per_pixel = 240.0 / 512.0;
        let slice = picture(512, 512, 40, Some(mm_per_pixel));
        let sheet = setup(4, PageOrientation::Portrait, Paper::A4);
        let mut fonts = Blocks::default();
        let page = compose_page(&sheet, &header(), 0, 1, &[slice], true, &mut fonts);
        let layout = &page.cells[0];
        let bar = layout.scale_bar.expect("a scale bar");
        assert_eq!(bar.millimetres, 50);
        assert!(fonts.written.iter().any(|text| text == "50 mm"), "{:?}", fonts.written);

        // Measured on the page: the run of its colour on its row.
        let on_bar = |x: usize| page.image.pixel(x, bar.row) == CELL_TEXT;
        let mut first = bar.columns.0;
        while on_bar(first - 1) {
            first -= 1;
        }
        let mut last = bar.columns.1;
        while on_bar(last + 1) {
            last += 1;
        }
        let measured = (last - first + 1) as f64;
        let expected = layout.picture.width as f64 * 50.0 / 240.0;
        assert!((measured - expected).abs() <= 1.0, "{measured} pixels, expected {expected}");
        // It lies under the picture, inside the cell: nothing covers the image.
        assert!(bar.row > layout.picture.y + layout.picture.height);
        assert!(bar.row < layout.cell.y + layout.cell.height);

        // Without a known spacing there is none.
        let page = compose(&sheet, &[picture(512, 512, 40, None)]);
        assert_eq!(page.cells[0].scale_bar, None);
    }

    #[test]
    fn a_scale_bar_needs_room() {
        // A tiny object: 0.001 mm per pixel; even 1 mm is wider than a quarter of the cell.
        let sheet = setup(12, PageOrientation::Portrait, Paper::A4);
        let page = compose(&sheet, &[picture(64, 64, 40, Some(0.001))]);
        assert_eq!(page.cells[0].scale_bar, None);
        // Enormous: even 200 mm is shorter than the height of the text.
        let page = compose(&sheet, &[picture(64, 64, 40, Some(100.0))]);
        assert_eq!(page.cells[0].scale_bar, None);
    }

    /// Design D11: what a page says. Letters, number, window values, the
    /// header and "Page n of m"; "Not for diagnostic use" on every page.
    #[test]
    fn every_page_says_what_it_shows() {
        let sheet = setup(2, PageOrientation::Portrait, Paper::A4);
        let mut slice = picture(64, 64, 40, Some(1.0));
        slice.notes = vec!["Mirrored".to_owned(), "Lossy 10:1 (JPEG)".to_owned()];
        let mut fonts = Blocks::default();
        compose_page(&sheet, &header(), 2, 7, &[slice], true, &mut fonts);
        for expected in [
            "Synthetic study",
            "CT CHEST",
            "Page 3 of 7",
            "Not for diagnostic use",
            "Image 1 / 60",
            "W 400  L 40 HU",
            "R",
            "L",
            "A",
            "P",
            "Mirrored · Lossy 10:1 (JPEG)",
        ] {
            assert!(fonts.written.iter().any(|text| text == expected), "{expected:?} in {:?}", fonts.written);
        }
    }

    /// Spec "Printed pages are private by default": the patient appears only
    /// in the header given, and a default header has none.
    #[test]
    fn a_default_page_says_nothing_about_the_patient() {
        let sheet = setup(1, PageOrientation::Portrait, Paper::A4);
        let mut fonts = Blocks::default();
        compose_page(&sheet, &header(), 0, 1, &[picture(64, 64, 40, Some(1.0))], true, &mut fonts);
        let text = fonts.written.join("\n");
        assert!(!text.contains(PATIENT_NAME) && !text.contains(PATIENT_ID) && !text.contains("1970"), "{text}");

        let with = PageHeader {
            patient: vec![PATIENT_NAME.to_owned(), PATIENT_ID.to_owned(), "Born 1970-01-01".to_owned()],
            ..header()
        };
        let mut fonts = Blocks::default();
        let page = compose_page(&sheet, &with, 0, 1, &[picture(64, 64, 40, Some(1.0))], true, &mut fonts);
        for expected in [PATIENT_NAME, PATIENT_ID, "Born 1970-01-01"] {
            assert!(fonts.written.iter().any(|text| text == expected), "{expected:?}");
        }
        // The lines take room from the cells rather than running over them.
        let plain = compose(&sheet, &[picture(64, 64, 40, Some(1.0))]);
        assert!(page.cells[0].cell.y > plain.cells[0].cell.y);
    }

    #[test]
    fn text_too_long_is_shortened_and_never_leaves_the_page() {
        let sheet = setup(12, PageOrientation::Landscape, Paper::Letter);
        let long = "An extremely long description that a hospital might put on a disc ".repeat(8);
        let header = PageHeader { study: long.clone(), series: long.clone(), patient: vec![long.clone()] };
        let mut slice = picture(64, 64, 40, Some(1.0));
        slice.number = long.clone();
        slice.window = Some(long.clone());
        slice.notes = vec![long.clone(), long];
        let mut fonts = Blocks::default();
        let pictures: Vec<PagePicture> = (0..12).map(|_| slice.clone()).collect();
        let page = compose_page(&sheet, &header, 0, 1, &pictures, true, &mut fonts);
        assert!(fonts.written.iter().any(|text| text.ends_with('…')));
        assert_eq!(page.cells.len(), 12);
    }

    /// Design D1: the caption of the 3D view lies under the picture and covers
    /// none of it.
    #[test]
    fn a_caption_goes_under_the_picture() {
        let plain = picture(200, 100, 90, None).picture;
        let mut fonts = Blocks::default();
        let captioned = plain.with_caption("Illustration", &mut fonts);
        assert_eq!(fonts.written, ["Illustration"]);
        assert_eq!(captioned.width, 200);
        assert!(captioned.height > 100);
        assert_eq!(captioned.rgb[..plain.rgb.len()], plain.rgb, "the picture is untouched");
        let band = &captioned.rgb[plain.rgb.len()..];
        assert!(band.contains(&CELL_TEXT[0]), "the text is in the band");
        assert_eq!(captioned.pixel(0, captioned.height - 1), [0; 3], "on black");
        // Wider than the picture: shortened.
        let mut fonts = Blocks::default();
        let narrow = picture(20, 20, 90, None).picture.with_caption("Illustration: colours", &mut fonts);
        assert_eq!(narrow.width, 20);
        assert!(fonts.written.iter().any(|text| text.ends_with('…')), "{:?}", fonts.written);
    }

    /// A picture can be any shape: no panic for extreme ones.
    #[test]
    fn extreme_pictures_are_drawn_without_trouble() {
        for per_page in IMAGES_PER_PAGE {
            let sheet = PageSetup { dpi: PREVIEW_DPI, ..setup(per_page, PageOrientation::Portrait, Paper::A4) };
            for (w, h) in [(1, 1), (1, 400), (400, 1), (2, 2000)] {
                let page = compose(&sheet, &[picture(w, h, 77, Some(0.5))]);
                assert_eq!(page.cells.len(), 1);
            }
        }
    }
}
