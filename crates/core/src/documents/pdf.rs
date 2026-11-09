//! PDF pages, rasterised on demand by `hayro` (pure Rust) on a thread of their
//! own (`design.md`, D3). The bytes stay in memory; nothing is written, nothing
//! is fetched, nothing in the file (scripts, links, attachments) is acted on.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use summrise_dicom::documents::ReportStatus;
use summrise_i18n::tr;

use super::Task;
use crate::jobs::Waker;

/// Most pages offered. Reports have a few; the limit bounds the table of page
/// sizes a hostile file can make the viewer keep.
pub const MAX_PAGES: usize = 2000;

/// Largest picture of a page: memory for 48 million pixels at four bytes each.
const MAX_PAGE_PIXELS: f32 = 48e6;

/// Why a PDF cannot be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfProblem {
    PasswordProtected,
    /// Damaged, or it uses something that the rasteriser does not support.
    Damaged,
}

impl fmt::Display for PdfProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PasswordProtected => tr!("It is protected by a password."),
            Self::Damaged => tr!("It is damaged or uses a feature that is not supported."),
        })
    }
}

/// Size of a page in points (1/72 inch), turned as it is meant to be read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

/// A rasterised page: opaque RGBA, row by row.
#[derive(Clone, PartialEq)]
pub struct PageImage {
    pub page: usize,
    /// Pixels per point of the page: the scale asked for, or less if the
    /// picture would exceed the limits.
    pub scale: f32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Part of the page could not be drawn: a font or a picture that could
    /// not be read. What is missing would not be noticed otherwise.
    pub incomplete: bool,
}

impl fmt::Debug for PageImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageImage")
            .field("page", &self.page)
            .field("size", &(self.width, self.height))
            .field("incomplete", &self.incomplete)
            .finish_non_exhaustive()
    }
}

/// The page sizes of the PDF, of at most [`MAX_PAGES`] pages, and how many
/// pages it has in all.
pub(super) fn inspect(bytes: &Arc<Vec<u8>>) -> Result<(Vec<PageSize>, usize), PdfProblem> {
    let pdf = load(bytes)?;
    let pages = pdf.pages();
    let total = pages.len();
    if total == 0 {
        return Err(PdfProblem::Damaged);
    }
    let sizes = pages
        .iter()
        .take(MAX_PAGES)
        .map(|page| {
            let (width, height) = page.render_dimensions();
            PageSize { width, height }
        })
        .collect();
    Ok((sizes, total))
}

fn load(bytes: &Arc<Vec<u8>>) -> Result<Pdf, PdfProblem> {
    Pdf::new(bytes.clone()).map_err(|e| match e {
        LoadPdfError::Decryption(_) => PdfProblem::PasswordProtected,
        LoadPdfError::Invalid => PdfProblem::Damaged,
    })
}

/// The scale to use for a page of `size` when `wanted` is asked for: pictures
/// stay within [`MAX_PAGE_PIXELS`] and `max_side` pixels per side. `None`
/// when a page of this size cannot be shown.
pub fn bounded_scale(size: PageSize, wanted: f32, max_side: u32) -> Option<f32> {
    let valid = |v: f32| v.is_finite() && v > 0.0;
    if !valid(size.width) || !valid(size.height) || !valid(wanted) {
        return None;
    }
    let side = (max_side.min(u32::from(u16::MAX))) as f32;
    let scale = wanted
        .min(side / size.width)
        .min(side / size.height)
        .min((MAX_PAGE_PIXELS / (size.width * size.height)).sqrt());
    // A page less than a pixel wide has nothing to show.
    (size.width * scale >= 1.0 && size.height * scale >= 1.0).then_some(scale)
}

/// Rasterises page `page` (counted from 0) at `wanted` pixels per point.
pub(crate) fn render(bytes: &Arc<Vec<u8>>, page: usize, wanted: f32, max_side: u32) -> Result<PageImage, PdfProblem> {
    let pdf = load(bytes)?;
    let pages = pdf.pages();
    let the_page = pages.get(page).ok_or(PdfProblem::Damaged)?;
    let (width, height) = the_page.render_dimensions();
    let scale = bounded_scale(PageSize { width, height }, wanted, max_side).ok_or(PdfProblem::Damaged)?;
    let missing = Arc::new(AtomicBool::new(false));
    let warned = missing.clone();
    let settings = InterpreterSettings {
        warning_sink: Arc::new(move |warning| {
            log::debug!("while drawing a PDF page: {warning:?}");
            warned.store(true, Ordering::Relaxed);
        }),
        ..InterpreterSettings::default()
    };
    let pixmap = hayro::render(
        the_page,
        &RenderCache::new(),
        &settings,
        &RenderSettings::default(),
        &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE },
    );
    Ok(PageImage {
        page,
        scale,
        width: u32::from(pixmap.width()),
        height: u32::from(pixmap.height()),
        rgba: pixmap.data_as_u8_slice().to_vec(),
        incomplete: missing.load(Ordering::Relaxed),
    })
}

/// A page and a scale to rasterise it at.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PageRequest {
    page: usize,
    scale: f32,
    max_side: u32,
}

/// The pages of an open PDF. One page is rasterised at a time; while that
/// runs, only the last request is kept, so that quick page turns and zooming
/// do not pile up work.
pub struct PdfPages {
    bytes: Arc<Vec<u8>>,
    sizes: Vec<PageSize>,
    total: usize,
    /// The status a PDF stored in DICOM records; `None` for a file.
    pub status: Option<ReportStatus>,
    /// The page on screen, counted from 0.
    page: usize,
    running: Option<(PageRequest, Task<Result<PageImage, PdfProblem>>)>,
    wanted: Option<PageRequest>,
    waker: Option<Waker>,
}

impl fmt::Debug for PdfPages {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PdfPages").field("pages", &self.total).field("page", &self.page).finish_non_exhaustive()
    }
}

impl PdfPages {
    pub(super) fn new(
        bytes: Arc<Vec<u8>>,
        sizes: Vec<PageSize>,
        total: usize,
        status: Option<ReportStatus>,
        waker: Option<Waker>,
    ) -> Self {
        Self { bytes, sizes, total, status, page: 0, running: None, wanted: None, waker }
    }

    /// Number of pages that can be shown.
    pub fn page_count(&self) -> usize {
        self.sizes.len()
    }

    /// Number of pages of the file: more than [`Self::page_count`] when the
    /// file has more than [`MAX_PAGES`].
    pub fn total_pages(&self) -> usize {
        self.total
    }

    /// The page on screen, counted from 0.
    pub fn page(&self) -> usize {
        self.page
    }

    pub fn size(&self, page: usize) -> Option<PageSize> {
        self.sizes.get(page).copied()
    }

    /// Moves `delta` pages on (back when negative), stopping at the first and the last.
    pub fn turn(&mut self, delta: i64) {
        let last = self.page_count().saturating_sub(1) as i64;
        self.page = (self.page as i64).saturating_add(delta).clamp(0, last) as usize;
    }

    /// Asks for a picture of page `page` at `scale` pixels per point, of at
    /// most `max_side` pixels per side.
    pub fn request(&mut self, page: usize, scale: f32, max_side: u32) {
        let request = PageRequest { page, scale, max_side };
        if self.running.as_ref().is_some_and(|(running, _)| *running == request) {
            self.wanted = None;
        } else {
            self.wanted = Some(request);
        }
        self.start_next();
    }

    fn start_next(&mut self) {
        if self.running.is_some() {
            return;
        }
        let Some(request) = self.wanted.take() else { return };
        let bytes = self.bytes.clone();
        let task =
            Task::spawn(self.waker.clone(), move || render(&bytes, request.page, request.scale, request.max_side));
        self.running = Some((request, task));
    }

    /// A picture that has become ready, or why it could not be made. The
    /// page says which page it is of. Call once per UI frame.
    pub fn poll(&mut self) -> Option<(usize, Result<PageImage, PdfProblem>)> {
        let (request, task) = self.running.as_ref()?;
        let (page, result) = (request.page, task.poll()?);
        self.running = None;
        self.start_next();
        Some((page, result.unwrap_or(Err(PdfProblem::Damaged))))
    }

    /// Whether a picture is being made.
    pub fn is_rendering(&self) -> bool {
        self.running.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use summrise_testkit::reports::{ScanCompression, embedded_font_pdf, pdf_line_top, scanned_pdf, text_pdf};

    use super::*;

    const SIDE: u32 = 8192;

    fn page_of(pdf: Vec<u8>, page: usize, scale: f32) -> PageImage {
        render(&Arc::new(pdf), page, scale, SIDE).expect("rendered")
    }

    fn darkness(image: &PageImage, rows: std::ops::Range<u32>) -> f64 {
        let width = image.width as usize;
        let mut dark = 0usize;
        for row in rows {
            let start = row as usize * width * 4;
            dark += image.rgba[start..start + width * 4].iter().step_by(4).filter(|red| **red < 128).count();
        }
        dark as f64
    }

    /// The largest jump in brightness between two pixels side by side.
    fn largest_step(image: &PageImage, rows: std::ops::Range<u32>) -> u8 {
        let width = image.width as usize;
        let mut largest = 0;
        for row in rows {
            let start = row as usize * width * 4;
            let line = &image.rgba[start..start + width * 4];
            let reds: Vec<u8> = line.iter().step_by(4).copied().collect();
            for pair in reds.windows(2) {
                largest = largest.max(pair[0].abs_diff(pair[1]));
            }
        }
        largest
    }

    fn text_rows(index: u32, scale: f32) -> std::ops::Range<u32> {
        let top = pdf_line_top(index) as f32 * scale;
        top as u32..(top + 16.0 * scale) as u32
    }

    /// Evaluation (design.md, D3): text in a standard font that is not
    /// embedded. The lines are dark where they should be and the rest is white.
    #[test]
    fn text_in_a_standard_font_is_drawn() {
        let image = page_of(text_pdf(&[&["Radiology report", "No focal lesion."]]), 0, 1.0);
        assert_eq!((image.width, image.height), (595, 842));
        assert!(!image.incomplete);
        assert!(darkness(&image, text_rows(0, 1.0)) > 100.0, "the first line is drawn");
        assert!(darkness(&image, text_rows(1, 1.0)) > 100.0, "the second line is drawn");
        assert_eq!(darkness(&image, text_rows(5, 1.0)), 0.0, "no third line");
        assert!(image.rgba.iter().skip(3).step_by(4).all(|alpha| *alpha == 255), "an opaque page");
    }

    /// Evaluation: an embedded TrueType font.
    #[test]
    fn text_in_an_embedded_font_is_drawn() {
        let image = page_of(embedded_font_pdf(&["Radiology report", "No focal lesion."]), 0, 1.0);
        assert!(!image.incomplete);
        assert!(darkness(&image, text_rows(0, 1.0)) > 100.0);
        assert!(darkness(&image, text_rows(1, 1.0)) > 100.0);
        assert_eq!(darkness(&image, text_rows(5, 1.0)), 0.0);
    }

    /// Evaluation: scanned pages, as Flate and as JPEG pictures: a dark bar where the
    /// picture has a dark bar. The picture has 702 rows on 842 points of page.
    #[test]
    fn scanned_pages_are_drawn() {
        for compression in [ScanCompression::Flate, ScanCompression::Jpeg] {
            let image = page_of(scanned_pdf(compression).expect("scan"), 0, 1.0);
            let page_row = |picture_row: f32| (picture_row * 842.0 / 702.0).round() as u32;
            let pixel = |x: u32, y: u32| image.rgba[((y * image.width + x) * 4) as usize];
            // In a bar (rows 120 to 131 of the picture), and between two bars.
            assert!(pixel(300, page_row(125.0)) < 60, "{compression:?}: the bar");
            assert!(pixel(300, page_row(145.0)) > 200, "{compression:?}: between the bars");
            // Left of the bar, in the margin.
            assert!(pixel(20, page_row(125.0)) > 200, "{compression:?}: the margin");
        }
    }

    /// The scenario of the specification: two pages, both readable, sharp at 200 %.
    #[test]
    fn two_pages_stay_sharp_at_twice_the_size() {
        let pdf = text_pdf(&[&["Radiology report", "Page one."], &["Conclusion"]]);
        let bytes = Arc::new(pdf);
        let (sizes, total) = inspect(&bytes).expect("a PDF");
        assert_eq!(total, 2);
        assert_eq!(sizes, [PageSize { width: 595.0, height: 842.0 }; 2]);
        let first = render(&bytes, 0, 1.0, SIDE).expect("page 1");
        let second = render(&bytes, 1, 1.0, SIDE).expect("page 2");
        assert!(darkness(&first, text_rows(1, 1.0)) > 50.0, "two lines on the first page");
        assert_eq!(darkness(&second, text_rows(1, 1.0)), 0.0, "one line on the second");
        // At 200 % the page is rasterised again, with twice the pixels and as much contrast at its edges.
        let large = render(&bytes, 0, 2.0, SIDE).expect("page 1 at 200 %");
        assert_eq!((large.width, large.height), (1190, 1684));
        let (dark_small, dark_large) = (darkness(&first, text_rows(0, 1.0)), darkness(&large, text_rows(0, 2.0)));
        assert!(dark_large > dark_small * 3.0, "four times the area: {dark_small} and {dark_large}");
        let step = largest_step(&large, text_rows(0, 2.0));
        assert!(step >= 200, "sharp edges at 200 %: a step of {step}");
    }

    #[test]
    fn a_truncated_file_is_a_message_or_as_much_as_there_is() {
        let whole = text_pdf(&[&["Radiology report"], &["Conclusion"]]);
        for keep in [0, 10, whole.len() / 4, whole.len() / 2] {
            let bytes = Arc::new(whole[..keep].to_vec());
            assert_eq!(inspect(&bytes).map(|(_, total)| total), Err(PdfProblem::Damaged), "{keep} of {}", whole.len());
        }
        // With its page tree still there the file is repaired, without its table of objects, and shows what it has.
        let bytes = Arc::new(whole[..whole.len() - 8].to_vec());
        assert_eq!(inspect(&bytes).map(|(_, total)| total), Ok(2));
        let page = render(&bytes, 1, 1.0, SIDE).expect("the last page");
        assert!(darkness(&page, text_rows(0, 1.0)) > 10.0);
        // Cut anywhere: a message or a page, but never a panic.
        for keep in (0..whole.len()).step_by(7) {
            let bytes = Arc::new(whole[..keep].to_vec());
            if let Ok((sizes, _)) = inspect(&bytes) {
                for page in 0..sizes.len() {
                    let _ = render(&bytes, page, 1.0, SIDE);
                }
            }
        }
    }

    #[test]
    fn garbage_is_a_message() {
        for garbage in [b"".as_slice(), b"%PDF-1.4", b"%PDF-1.4\n1 0 obj\n<< >>\nendobj\n", &[0xFF; 4096]] {
            assert_eq!(inspect(&Arc::new(garbage.to_vec())), Err(PdfProblem::Damaged));
        }
        let mut flipped = text_pdf(&[&["Radiology report"]]);
        for (i, byte) in flipped.iter_mut().enumerate() {
            if i % 17 == 0 {
                *byte ^= 0x5A;
            }
        }
        assert_eq!(inspect(&Arc::new(flipped)), Err(PdfProblem::Damaged));
    }

    #[test]
    fn a_page_that_is_not_there_is_a_message() {
        let bytes = Arc::new(text_pdf(&[&["One page"]]));
        assert_eq!(render(&bytes, 1, 1.0, SIDE), Err(PdfProblem::Damaged));
    }

    #[test]
    fn pictures_stay_within_their_limits() {
        let a4 = PageSize { width: 595.0, height: 842.0 };
        assert_eq!(bounded_scale(a4, 2.0, SIDE), Some(2.0));
        // Limited by the side of the texture.
        let scale = bounded_scale(a4, 50.0, 2048).expect("a scale");
        assert!(842.0 * scale <= 2048.0, "{scale}");
        // Limited by the number of pixels.
        let scale = bounded_scale(PageSize { width: 14400.0, height: 14400.0 }, 4.0, 60000).expect("a scale");
        assert!(14400.0 * scale * 14400.0 * scale <= MAX_PAGE_PIXELS * 1.001, "{scale}");
        // Nonsense.
        assert_eq!(bounded_scale(a4, 0.0, SIDE), None);
        assert_eq!(bounded_scale(a4, f32::NAN, SIDE), None);
        assert_eq!(bounded_scale(PageSize { width: 0.0, height: 842.0 }, 1.0, SIDE), None);
        assert_eq!(bounded_scale(PageSize { width: f32::INFINITY, height: 842.0 }, 1.0, SIDE), None);
        // A page of a hair's breadth at a sensible scale.
        assert_eq!(bounded_scale(PageSize { width: 0.1, height: 842.0 }, 1.0, SIDE), None);
    }

    fn pages_of(pdf: Vec<u8>) -> PdfPages {
        let bytes = Arc::new(pdf);
        let (sizes, total) = inspect(&bytes).expect("a PDF");
        PdfPages::new(bytes, sizes, total, None, None)
    }

    fn next_picture(pages: &mut PdfPages) -> (usize, Result<PageImage, PdfProblem>) {
        let start = Instant::now();
        loop {
            if let Some(picture) = pages.poll() {
                return picture;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "timed out rendering");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn pages_turn_within_the_document() {
        let mut pages = pages_of(text_pdf(&[&["One"], &["Two"], &["Three"]]));
        assert_eq!((pages.page(), pages.page_count(), pages.total_pages()), (0, 3, 3));
        pages.turn(-1);
        assert_eq!(pages.page(), 0, "stops at the first");
        pages.turn(1);
        assert_eq!(pages.page(), 1);
        pages.turn(10);
        assert_eq!(pages.page(), 2, "stops at the last");
    }

    #[test]
    fn only_the_last_request_is_kept_while_a_page_is_being_rasterised() {
        let mut pages = pages_of(text_pdf(&[&["One"], &["Two"], &["Three"]]));
        pages.request(0, 1.0, SIDE);
        assert!(pages.is_rendering());
        // Asking again for the same thing changes nothing.
        pages.request(0, 1.0, SIDE);
        // Two more requests while it runs: the first of them is dropped.
        pages.request(1, 1.0, SIDE);
        pages.request(2, 1.0, SIDE);
        let (first, picture) = next_picture(&mut pages);
        assert_eq!(first, 0);
        assert_eq!(picture.expect("rendered").page, 0);
        let (second, picture) = next_picture(&mut pages);
        assert_eq!(second, 2);
        assert_eq!(picture.expect("rendered").page, 2);
        assert!(!pages.is_rendering());
        assert!(pages.poll().is_none());
    }
}
