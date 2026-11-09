//! A hand-written PDF of the simplest kind: one lossless picture per page. Text
//! is part of the pictures, so no fonts are embedded, and the metadata names the
//! application only (design D9): nothing about the patient can be in it.
//!
//! Hand-written rather than the `pdf-writer` crate: this is one object layout
//! and a cross-reference table, about a hundred lines.

use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::export::Picture;

/// Object numbers fixed from the start: the pages come after them.
const CATALOG: usize = 1;
const PAGES: usize = 2;
const INFO: usize = 3;
const FIRST_PAGE_OBJECT: usize = 4;

/// Writes a PDF in memory, page by page.
#[derive(Debug)]
pub struct PdfWriter {
    out: Vec<u8>,
    /// The byte position of each object, by its number (from 1).
    offsets: Vec<usize>,
    /// The object number of each page.
    pages: Vec<usize>,
}

impl Default for PdfWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfWriter {
    pub fn new() -> Self {
        // The second line marks the file as binary for tools that look.
        let mut out = b"%PDF-1.4\n%".to_vec();
        out.extend_from_slice(&[0xE2, 0xE3, 0xCF, 0xD3, b'\n']);
        Self { out, offsets: vec![0; FIRST_PAGE_OBJECT - 1], pages: Vec::new() }
    }

    fn begin_object(&mut self, number: usize) {
        if self.offsets.len() < number {
            self.offsets.resize(number, 0);
        }
        self.offsets[number - 1] = self.out.len();
        self.out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
    }

    fn end_object(&mut self) {
        self.out.extend_from_slice(b"endobj\n");
    }

    /// Adds a page that shows `image` at `dpi`.
    pub fn add_page(&mut self, image: &Picture, dpi: u32) -> std::io::Result<()> {
        let (image_object, contents, page) = {
            let base = self.offsets.len().max(FIRST_PAGE_OBJECT - 1);
            (base + 1, base + 2, base + 3)
        };
        let (width_pt, height_pt) =
            (image.width as f64 * 72.0 / f64::from(dpi.max(1)), image.height as f64 * 72.0 / f64::from(dpi.max(1)));
        let (stream, colour_space, colours) = compressed_samples(image)?;
        self.begin_object(image_object);
        self.out.extend_from_slice(
            format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /{colour_space} /BitsPerComponent 8 \
                 /Filter /FlateDecode /DecodeParms << /Predictor 12 /Colors {colours} /BitsPerComponent 8 /Columns {} >> \
                 /Length {} >>\nstream\n",
                image.width,
                image.height,
                image.width,
                stream.len(),
            )
            .as_bytes(),
        );
        self.out.extend_from_slice(&stream);
        self.out.extend_from_slice(b"\nendstream\n");
        self.end_object();

        let drawing = format!("q {width_pt:.2} 0 0 {height_pt:.2} 0 0 cm /Picture Do Q\n");
        self.begin_object(contents);
        self.out.extend_from_slice(format!("<< /Length {} >>\nstream\n{drawing}endstream\n", drawing.len()).as_bytes());
        self.end_object();

        self.begin_object(page);
        self.out.extend_from_slice(
            format!(
                "<< /Type /Page /Parent {PAGES} 0 R /MediaBox [0 0 {width_pt:.2} {height_pt:.2}] /Contents {contents} 0 R \
                 /Resources << /XObject << /Picture {image_object} 0 R >> >> >>\n"
            )
            .as_bytes(),
        );
        self.end_object();
        self.pages.push(page);
        Ok(())
    }

    /// How many pages it has so far.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Finishes the file: the page tree, the metadata, the cross-reference
    /// table and the trailer.
    pub fn finish(mut self) -> Vec<u8> {
        let kids: Vec<String> = self.pages.iter().map(|page| format!("{page} 0 R")).collect();
        self.begin_object(CATALOG);
        self.out.extend_from_slice(format!("<< /Type /Catalog /Pages {PAGES} 0 R >>\n").as_bytes());
        self.end_object();
        self.begin_object(PAGES);
        self.out.extend_from_slice(
            format!("<< /Type /Pages /Kids [{}] /Count {} >>\n", kids.join(" "), self.pages.len()).as_bytes(),
        );
        self.end_object();
        self.begin_object(INFO);
        self.out.extend_from_slice(b"<< /Title (SumMRIse) /Producer (SumMRIse) >>\n");
        self.end_object();

        let table = self.out.len();
        let size = self.offsets.len() + 1;
        self.out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
        for offset in &self.offsets {
            self.out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        self.out.extend_from_slice(
            format!("trailer\n<< /Size {size} /Root {CATALOG} 0 R /Info {INFO} 0 R >>\nstartxref\n{table}\n%%EOF\n")
                .as_bytes(),
        );
        self.out
    }
}

/// The samples of `image`, grey if every pixel is, each row as its difference
/// to the row above (PNG predictor "Up": pages have large even areas), deflated.
/// Also returns the colour space and the number of samples per pixel.
fn compressed_samples(image: &Picture) -> std::io::Result<(Vec<u8>, &'static str, usize)> {
    let grey = image.is_grey();
    let colours = if grey { 1 } else { 3 };
    let samples: Vec<u8> =
        if grey { image.rgb.as_chunks::<3>().0.iter().map(|pixel| pixel[0]).collect() } else { image.rgb.clone() };
    let row_length = image.width * colours;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
    let mut row = vec![0u8; row_length + 1];
    row[0] = 2;
    let mut above = vec![0u8; row_length];
    for current in samples.chunks_exact(row_length.max(1)) {
        for ((out, value), up) in row[1..].iter_mut().zip(current).zip(&above) {
            *out = value.wrapping_sub(*up);
        }
        encoder.write_all(&row)?;
        above.copy_from_slice(current);
    }
    Ok((encoder.finish()?, if grey { "DeviceGray" } else { "DeviceRGB" }, colours))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Read as _;

    use flate2::read::ZlibDecoder;

    use super::*;

    fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
        haystack[from..].windows(needle.len()).position(|w| w == needle).map(|i| i + from)
    }

    /// What the structure of a PDF says, after checking it: header, trailer,
    /// the cross-reference table entry of every object and the page tree.
    #[derive(Debug)]
    pub(crate) struct Parsed {
        pub(crate) pages: usize,
        pub(crate) media_boxes: Vec<String>,
    }

    pub(crate) fn parse(pdf: &[u8]) -> Parsed {
        assert!(pdf.starts_with(b"%PDF-1."), "the header");
        assert!(pdf.ends_with(b"%%EOF\n"), "the end");
        let tail = String::from_utf8_lossy(&pdf[pdf.len().saturating_sub(200)..]).into_owned();
        let after = tail.split("startxref\n").nth(1).expect("startxref");
        let table: usize = after.lines().next().expect("position").parse().expect("a number");
        assert_eq!(&pdf[table..table + 5], b"xref\n", "startxref points at the table");

        let trailer_at = find(pdf, b"trailer\n", table).expect("trailer");
        let text = String::from_utf8_lossy(&pdf[table..trailer_at]).into_owned();
        let mut lines = text.lines().skip(1);
        let mut head = lines.next().expect("subsection").split(' ');
        assert_eq!(head.next(), Some("0"));
        let size: usize = head.next().expect("size").parse().expect("a number");
        let entries: Vec<&str> = lines.collect();
        assert_eq!(entries.len(), size, "one entry per object");
        assert_eq!(entries[0], "0000000000 65535 f ");
        for (number, entry) in entries.iter().enumerate().skip(1) {
            assert_eq!(entry.len(), 19, "{entry:?}: 20 bytes with the line end");
            assert!(entry.ends_with(" 00000 n "), "{entry:?}");
            let offset: usize = entry[..10].parse().expect("an offset");
            let header = format!("{number} 0 obj\n");
            assert_eq!(&pdf[offset..offset + header.len()], header.as_bytes(), "object {number}");
        }
        let trailer = String::from_utf8_lossy(&pdf[trailer_at..]).into_owned();
        assert!(trailer.contains(&format!("/Size {size}")), "{trailer}");
        assert!(trailer.contains("/Root 1 0 R"), "{trailer}");

        // The page tree: as many /Page objects as /Count says.
        let plain = String::from_utf8_lossy(pdf);
        let count: usize = plain
            .split("/Count ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .and_then(|n| n.parse().ok())
            .expect("/Count");
        let media_boxes: Vec<String> = plain
            .split("/MediaBox [")
            .skip(1)
            .map(|rest| rest.split(']').next().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(plain.matches("/Type /Page ").count(), count, "one /Page per /Count");
        assert_eq!(media_boxes.len(), count);
        Parsed { pages: count, media_boxes }
    }

    /// The samples of the `n`-th picture (from 0), with the predictor undone.
    pub(crate) fn samples(pdf: &[u8], n: usize) -> (Vec<u8>, usize) {
        let mut from = 0;
        let mut stream_at = 0;
        for _ in 0..=n {
            let dict = find(pdf, b"/Subtype /Image", from).expect("a picture");
            stream_at = find(pdf, b"stream\n", dict).expect("its stream") + 7;
            from = stream_at;
        }
        let dict_text = String::from_utf8_lossy(&pdf[stream_at.saturating_sub(300)..stream_at]).into_owned();
        let length: usize =
            dict_text.rsplit("/Length ").next().and_then(|r| r.split(' ').next()).expect("/Length").parse().expect("n");
        let colours: usize =
            dict_text.rsplit("/Colors ").next().and_then(|r| r.split(' ').next()).expect("/Colors").parse().expect("n");
        let columns: usize = dict_text
            .rsplit("/Columns ")
            .next()
            .and_then(|r| r.split(' ').next())
            .expect("/Columns")
            .parse()
            .expect("n");
        let mut raw = Vec::new();
        ZlibDecoder::new(&pdf[stream_at..stream_at + length]).read_to_end(&mut raw).expect("inflate");
        let row_length = columns * colours;
        let mut out = Vec::new();
        let mut above = vec![0u8; row_length];
        for row in raw.chunks_exact(row_length + 1) {
            assert_eq!(row[0], 2, "the predictor Up");
            let current: Vec<u8> = row[1..].iter().zip(&above).map(|(d, u)| d.wrapping_add(*u)).collect();
            out.extend_from_slice(&current);
            above = current;
        }
        (out, colours)
    }

    fn colour_page(width: usize, height: usize) -> Picture {
        let mut rgb = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgb.extend_from_slice(&[(x % 251) as u8, (y % 241) as u8, ((x * y) % 239) as u8]);
            }
        }
        Picture { width, height, rgb }
    }

    fn grey_page(width: usize, height: usize) -> Picture {
        let rgb = (0..width * height).flat_map(|i| [(i % 251) as u8; 3]).collect();
        Picture { width, height, rgb }
    }

    #[test]
    fn the_file_parses_with_one_lossless_picture_per_page() {
        let mut writer = PdfWriter::new();
        let (a4, letter) = (colour_page(62, 88), grey_page(64, 83));
        writer.add_page(&a4, 75).expect("page");
        writer.add_page(&letter, 75).expect("page");
        assert_eq!(writer.page_count(), 2);
        let pdf = writer.finish();
        let parsed = parse(&pdf);
        assert_eq!(parsed.pages, 2);
        assert_eq!(parsed.media_boxes, ["0 0 59.52 84.48", "0 0 61.44 79.68"]);

        let (colour, colours) = samples(&pdf, 0);
        assert_eq!((colour == a4.rgb, colours), (true, 3), "colour, without loss");
        let (grey, colours) = samples(&pdf, 1);
        let expected: Vec<u8> = letter.rgb.as_chunks::<3>().0.iter().map(|p| p[0]).collect();
        assert_eq!((grey == expected, colours), (true, 1), "grey, without loss");
    }

    #[test]
    fn its_metadata_names_the_application_only() {
        let mut writer = PdfWriter::new();
        writer.add_page(&grey_page(8, 8), 72).expect("page");
        let pdf = writer.finish();
        let plain = String::from_utf8_lossy(&pdf);
        assert!(plain.contains("/Title (SumMRIse) /Producer (SumMRIse)"));
        for word in ["/Author", "/Subject", "/Keywords", "/Creator", "/CreationDate", "/Font"] {
            assert!(!plain.contains(word), "{word}");
        }
    }

    #[test]
    fn a_white_page_is_small() {
        let page = Picture { width: 2480, height: 3508, rgb: vec![255; 2480 * 3508 * 3] };
        let mut writer = PdfWriter::new();
        writer.add_page(&page, 300).expect("page");
        let pdf = writer.finish();
        assert!(pdf.len() < 200_000, "{} bytes", pdf.len());
    }

    /// Where the system has a PDF reader, it opens the file and the first page
    /// looks as it should. Without one the test says so and passes.
    #[test]
    fn a_system_pdf_reader_opens_it_where_there_is_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("in.pdf");
        // White, with a black square in the middle of the top half.
        let (width, height) = (620, 880);
        let mut rgb = vec![255u8; width * height * 3];
        for y in 200..400 {
            for x in 200..400 {
                rgb[(y * width + x) * 3..(y * width + x) * 3 + 3].fill(0);
            }
        }
        let mut writer = PdfWriter::new();
        writer.add_page(&Picture { width, height, rgb }, 75).expect("page");
        writer.add_page(&grey_page(620, 880), 75).expect("page");
        std::fs::write(&path, writer.finish()).expect("write");

        let out = dir.path().join("out.png");
        let opened = if std::path::Path::new("/usr/bin/sips").exists() {
            std::process::Command::new("/usr/bin/sips")
                .args(["-s", "format", "png"])
                .arg(&path)
                .arg("--out")
                .arg(&out)
                .output()
                .expect("run sips")
        } else if let Some(pdftoppm) = ["/usr/bin/pdftoppm", "/usr/local/bin/pdftoppm", "/opt/homebrew/bin/pdftoppm"]
            .iter()
            .find(|p| std::path::Path::new(p).exists())
        {
            let prefix = dir.path().join("out");
            let output = std::process::Command::new(pdftoppm)
                .args(["-png", "-singlefile", "-r", "75"])
                .arg(&path)
                .arg(&prefix)
                .output()
                .expect("run pdftoppm");
            assert!(out.exists(), "pdftoppm wrote nothing: {output:?}");
            output
        } else {
            eprintln!("skipped: no PDF reader on this computer (sips or pdftoppm)");
            return;
        };
        assert!(opened.status.success(), "the reader failed: {opened:?}");
        let first = image::open(&out).expect("the reader wrote a picture").to_rgb8();
        assert!(first.width() >= 300, "{} wide", first.width());
        let at = |fx: f64, fy: f64| {
            let (x, y) = ((fx * f64::from(first.width())) as u32, (fy * f64::from(first.height())) as u32);
            first.get_pixel(x, y).0
        };
        assert!(at(0.05, 0.05).iter().all(|v| *v > 240), "white corner: {:?}", at(0.05, 0.05));
        assert!(at(0.48, 0.34).iter().all(|v| *v < 15), "black square: {:?}", at(0.48, 0.34));
        let ratio = f64::from(first.height()) / f64::from(first.width());
        assert!((ratio - 880.0 / 620.0).abs() < 0.02, "proportions of the page: {ratio}");
    }
}
