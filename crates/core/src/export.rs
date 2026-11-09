//! Getting pictures out of the application (`openspec/changes/export-and-share`):
//! the pure part of saving and exporting images. A [`Picture`] is what a view
//! shows, as plain 8-bit RGB; these functions give it square pixels, turn and
//! mirror it like its view, encode it without any metadata, name it neutrally
//! and write a whole series of them with progress and cancel.
//!
//! The rendering itself lives in `crates/ui/src/export` because the picture of
//! a view is composed there (`render.rs`, `pet.rs`, `fidelity.rs`).
//!
//! Nothing here writes a file unless the user chose the place: this is the one
//! exception to "No derived data at rest".

use std::path::{Path, PathBuf};

use image::ImageEncoder as _;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use summrise_dicom::{InstanceHeader, Progress, Series};
use summrise_i18n::tr;

use crate::orientation::DisplayOrientation;

/// Largest edge a picture gets when it is resampled to square pixels. Spacing
/// is read from the disc: absurd values must not ask for gigabytes.
const MAX_EDGE: usize = 16_384;

/// Quality of exported JPEG files (design D2).
pub const JPEG_QUALITY: u8 = 90;

/// A picture to save, copy or print: 8-bit RGB, row by row from the top left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    /// `width × height × 3` bytes.
    pub rgb: Vec<u8>,
}

impl Picture {
    /// A black picture.
    pub fn black(width: usize, height: usize) -> Self {
        Self { width, height, rgb: vec![0; width * height * 3] }
    }

    /// A picture from premultiplied RGBA on a black ground: the colour bytes
    /// are what shows, the alpha is dropped. `None` if the sizes disagree.
    pub fn from_rgba(width: usize, height: usize, rgba: &[u8]) -> Option<Self> {
        (rgba.len() == width * height * 4).then(|| Self {
            width,
            height,
            rgb: rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect(),
        })
    }

    /// The colour of pixel (`x`, `y`).
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }

    /// Whether every pixel is a shade of grey.
    pub fn is_grey(&self) -> bool {
        self.rgb.as_chunks::<3>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2])
    }

    /// The picture with `width` × `height` pixels: averaged where it shrinks;
    /// where it grows, interpolated (`smooth`) or with its pixels repeated.
    pub fn scaled(&self, width: usize, height: usize, smooth: bool) -> Self {
        if (width, height) == (self.width, self.height) {
            return self.clone();
        }
        if width == 0 || height == 0 || self.width == 0 || self.height == 0 {
            return Self::black(width, height);
        }
        let across = weights(self.width, width, smooth);
        let down = weights(self.height, height, smooth);
        // Along the rows first, into values that keep their fractions.
        let mut wide = vec![0f32; width * self.height * 3];
        for (y, out) in wide.chunks_exact_mut(width * 3).enumerate() {
            let row = &self.rgb[y * self.width * 3..(y + 1) * self.width * 3];
            for (pixel, (start, factors)) in out.as_chunks_mut::<3>().0.iter_mut().zip(&across) {
                for (i, factor) in factors.iter().enumerate() {
                    let source = &row[(start + i) * 3..(start + i) * 3 + 3];
                    pixel[0] += f32::from(source[0]) * factor;
                    pixel[1] += f32::from(source[1]) * factor;
                    pixel[2] += f32::from(source[2]) * factor;
                }
            }
        }
        let mut rgb = vec![0u8; width * height * 3];
        let mut sum = vec![0f32; width * 3];
        for (out, (start, factors)) in rgb.chunks_exact_mut(width * 3).zip(&down) {
            sum.fill(0.0);
            for (i, factor) in factors.iter().enumerate() {
                let row = &wide[(start + i) * width * 3..(start + i + 1) * width * 3];
                for (total, value) in sum.iter_mut().zip(row) {
                    *total += value * factor;
                }
            }
            for (byte, value) in out.iter_mut().zip(&sum) {
                *byte = (value + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
        Self { width, height, rgb }
    }

    /// The picture with square pixels, for a `spacing` of rows and columns in
    /// millimetres (Pixel Spacing order: row spacing, column spacing). It keeps
    /// the finer of the two spacings, so no detail is lost: 256 columns × 128
    /// rows at 1.0 \ 0.5 become 256 × 256. Pixels already square (or an unknown
    /// spacing) leave it as it is. Also returns the millimetres per pixel of
    /// the result, when known.
    pub fn with_square_pixels(self, spacing: Option<[f64; 2]>) -> (Self, Option<f64>) {
        let Some([row, column]) = spacing.filter(|[r, c]| r.is_finite() && c.is_finite() && *r > 0.0 && *c > 0.0)
        else {
            return (self, None);
        };
        let finer = row.min(column);
        if (row - column).abs() <= 1e-3 * finer {
            return (self, Some(column));
        }
        let (width, height) = (self.width as f64 * column / finer, self.height as f64 * row / finer);
        let shrink = (MAX_EDGE as f64 / width.max(height)).min(1.0);
        let (new_width, new_height) =
            (((width * shrink).round() as usize).max(1), ((height * shrink).round() as usize).max(1));
        (self.scaled(new_width, new_height, true), Some(finer / shrink))
    }

    /// The picture turned and mirrored as `orientation` says, as its view
    /// shows it (`placement.rs`), so that it never contradicts the orientation
    /// letters of that view.
    pub fn oriented(&self, orientation: DisplayOrientation) -> Self {
        if orientation.is_standard() {
            return self.clone();
        }
        let (width, height) =
            if orientation.swaps_axes() { (self.height, self.width) } else { (self.width, self.height) };
        let mut rgb = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                let (s, t) = ((x as f64 + 0.5) / width as f64, (y as f64 + 0.5) / height as f64);
                let (u, v) = orientation.to_image(s, t);
                let source_x = ((u * self.width as f64) as usize).min(self.width.saturating_sub(1));
                let source_y = ((v * self.height as f64) as usize).min(self.height.saturating_sub(1));
                rgb.extend_from_slice(&self.pixel(source_x, source_y));
            }
        }
        Self { width, height, rgb }
    }
}

/// For each of `target` output positions, the first source position that adds
/// to it and the share of each source position from there.
fn weights(source: usize, target: usize, smooth: bool) -> Vec<(usize, Vec<f32>)> {
    let ratio = source as f64 / target as f64;
    (0..target)
        .map(|t| {
            if ratio > 1.0 {
                // Shrinking: the average of what falls into this output pixel.
                let (from, to) = (t as f64 * ratio, (t + 1) as f64 * ratio);
                let first = from.floor() as usize;
                let last = (to.ceil() as usize).min(source);
                let shares: Vec<f32> = (first..last)
                    .map(|s| (((s + 1) as f64).min(to) - (s as f64).max(from)).max(0.0) / ratio)
                    .map(|share| share as f32)
                    .collect();
                (first, shares)
            } else if smooth {
                let position = ((t as f64 + 0.5) * ratio - 0.5).clamp(0.0, (source - 1) as f64);
                let first = position.floor() as usize;
                if first + 1 < source {
                    let fraction = (position - first as f64) as f32;
                    (first, vec![1.0 - fraction, fraction])
                } else {
                    (first, vec![1.0])
                }
            } else {
                (((t as f64 + 0.5) * ratio) as usize, vec![1.0])
            }
        })
        .collect()
}

/// The file formats of exported pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageFormat {
    /// Lossless.
    #[default]
    Png,
    Jpeg,
}

impl ImageFormat {
    pub const ALL: [Self; 2] = [Self::Png, Self::Jpeg];

    /// The extension of its files, without a dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    /// Other spellings of the extension that are fine as they are.
    fn accepts(self, extension: &str) -> bool {
        let extension = extension.to_ascii_lowercase();
        match self {
            Self::Png => extension == "png",
            Self::Jpeg => extension == "jpg" || extension == "jpeg",
        }
    }

    /// What it is called in a list: the name of the format, not translated
    /// apart from what is said about it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => tr!("PNG (no loss of quality)"),
            Self::Jpeg => "JPEG",
        }
    }
}

/// Why a picture could not be exported. Shown to the user as it is.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("{0}")]
    Encode(String),
    #[error("{name}: {source}")]
    Write {
        /// The file name, not its folder, which may hold anything.
        name: String,
        #[source]
        source: std::io::Error,
    },
}

/// `picture` as a file of `format`: 8-bit, greyscale when every pixel is grey,
/// holding nothing but the picture. No metadata of any kind is written, so
/// nothing about the patient can be in it (design D2).
pub fn encode(picture: &Picture, format: ImageFormat) -> Result<Vec<u8>, ExportError> {
    let (width, height) = (
        u32::try_from(picture.width).map_err(|e| ExportError::Encode(e.to_string()))?,
        u32::try_from(picture.height).map_err(|e| ExportError::Encode(e.to_string()))?,
    );
    let grey: Vec<u8>;
    let (samples, color) = if picture.is_grey() {
        grey = picture.rgb.as_chunks::<3>().0.iter().map(|p| p[0]).collect();
        (grey.as_slice(), image::ExtendedColorType::L8)
    } else {
        (picture.rgb.as_slice(), image::ExtendedColorType::Rgb8)
    };
    let mut out = Vec::new();
    let result = match format {
        ImageFormat::Png => PngEncoder::new(&mut out).write_image(samples, width, height, color),
        ImageFormat::Jpeg => {
            JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).write_image(samples, width, height, color)
        }
    };
    result.map_err(|e| ExportError::Encode(e.to_string()))?;
    Ok(out)
}

/// What a file name may say about a series: its modality and number, as
/// `CT-S1`. Only plain letters and digits come from the disc.
fn series_tag(modality: &str, number: Option<i32>) -> String {
    let modality: String = modality.chars().filter(char::is_ascii_alphanumeric).collect();
    let modality = if modality.is_empty() { "IMG" } else { &modality };
    match number.filter(|n| *n >= 0) {
        Some(number) => format!("{modality}-S{number}"),
        None => modality.to_owned(),
    }
}

/// The neutral name suggested for one saved image: `SumMRIse-CT-S1-031.png`.
/// `view` tells a reformat or the 3D view from the acquired images, whose
/// numbers would otherwise coincide: `SumMRIse-CT-S1-MPR-012.png`.
pub fn image_file_name(
    modality: &str,
    series: Option<i32>,
    view: Option<&str>,
    number: Option<usize>,
    format: ImageFormat,
) -> String {
    let mut name = format!("SumMRIse-{}", series_tag(modality, series));
    if let Some(view) = view {
        name.push('-');
        name.push_str(view);
    }
    if let Some(number) = number {
        name.push_str(&format!("-{number:03}"));
    }
    format!("{name}.{}", format.extension())
}

/// The names of `count` consecutively numbered files of a series, 001 upwards
/// (with more digits when there are more images).
pub fn series_file_names(modality: &str, series: Option<i32>, count: usize, format: ImageFormat) -> Vec<String> {
    let width = count.to_string().len().max(3);
    let tag = series_tag(modality, series);
    (1..=count).map(|n| format!("SumMRIse-{tag}-{n:0width$}.{}", format.extension())).collect()
}

/// The neutral name suggested for a PDF of a series: `SumMRIse-CT-S1.pdf`.
pub fn pdf_file_name(modality: &str, series: Option<i32>) -> String {
    format!("SumMRIse-{}.pdf", series_tag(modality, series))
}

/// `path` with the extension of `format`, unless it already has one of its
/// spellings. Save dialogs do not all add it.
pub fn with_extension(path: PathBuf, format: ImageFormat) -> PathBuf {
    if path.extension().and_then(|e| e.to_str()).is_some_and(|e| format.accepts(e)) {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".");
    name.push(format.extension());
    PathBuf::from(name)
}

/// Whether the pixels of this image may themselves show personal details, which
/// the application cannot remove: Burned In Annotation is YES, or it is a
/// secondary capture or an ultrasound image (design D4).
pub fn may_show_personal_details(header: &InstanceHeader, modality: &str) -> bool {
    const ULTRASOUND_SOP_CLASSES: [&str; 2] = ["1.2.840.10008.5.1.4.1.1.6.1", "1.2.840.10008.5.1.4.1.1.3.1"];
    let class = header.sop_class_uid.trim();
    header.fidelity.burned_in_text
        || modality.trim() == "US"
        || ULTRASOUND_SOP_CLASSES.contains(&class)
        || crate::guide::SECONDARY_CAPTURE_SOP_CLASSES.contains(&class)
}

/// [`may_show_personal_details`] for any image of a series.
pub fn series_may_show_personal_details(series: &Series) -> bool {
    series.instances.iter().any(|header| may_show_personal_details(header, &series.modality))
}

/// Which of the files `names` already exist in `folder`.
pub fn existing_files(folder: &Path, names: &[String]) -> Vec<String> {
    names.iter().filter(|name| folder.join(name).exists()).cloned().collect()
}

/// What a series export did.
#[derive(Debug, Default)]
pub struct SeriesExported {
    /// Files written.
    pub written: usize,
    /// Images of the series.
    pub total: usize,
    /// The user cancelled it.
    pub cancelled: bool,
    /// Images that could not be rendered, because they could not be decoded.
    pub missing: usize,
    /// Why it stopped early, if something went wrong.
    pub error: Option<ExportError>,
}

/// Writes one file per entry of `names` into `folder`, with `render` giving the
/// picture of each position. Stops at once when cancelled or when a file cannot
/// be written; what was written stays. An image that cannot be rendered keeps
/// its number free, so that the numbers stay those of the display order.
pub fn export_series(
    folder: &Path,
    names: &[String],
    format: ImageFormat,
    progress: &Progress,
    mut render: impl FnMut(usize) -> Option<Picture>,
) -> SeriesExported {
    let mut done = SeriesExported { total: names.len(), ..SeriesExported::default() };
    progress.begin(tr!("Exporting images"), names.len());
    for (index, name) in names.iter().enumerate() {
        if progress.is_cancelled() {
            done.cancelled = true;
            break;
        }
        let Some(picture) = render(index) else {
            done.missing += 1;
            progress.advance();
            continue;
        };
        let written = encode(&picture, format).and_then(|bytes| write_file(&folder.join(name), name, &bytes));
        match written {
            Ok(()) => done.written += 1,
            Err(e) => {
                done.error = Some(e);
                break;
            }
        }
        progress.advance();
    }
    done
}

/// Writes `bytes` to `path`; `name` is how an error calls the file. A file that
/// could not be written completely is removed again.
pub fn write_file(path: &Path, name: &str, bytes: &[u8]) -> Result<(), ExportError> {
    std::fs::write(path, bytes).map_err(|source| {
        let _ = std::fs::remove_file(path);
        ExportError::Write { name: name.to_owned(), source }
    })
}

#[cfg(test)]
mod tests {
    use summrise_dicom::Fidelity;

    use super::*;

    fn gradient(width: usize, height: usize) -> Picture {
        let mut rgb = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgb.extend_from_slice(&[(x * 255 / width.max(2).saturating_sub(1)) as u8, (y * 10) as u8, 7]);
            }
        }
        Picture { width, height, rgb }
    }

    #[test]
    fn scaling_keeps_a_flat_picture_flat_and_a_ramp_a_ramp() {
        let flat = Picture { width: 4, height: 2, rgb: vec![127; 24] };
        for (w, h) in [(8, 4), (2, 1), (3, 5), (7, 1)] {
            for smooth in [true, false] {
                let scaled = flat.scaled(w, h, smooth);
                assert_eq!((scaled.width, scaled.height), (w, h));
                assert!(scaled.rgb.iter().all(|v| *v == 127), "{w} × {h}, smooth {smooth}: {:?}", scaled.rgb);
            }
        }
        // Growing repeats pixels or interpolates between them.
        let two = Picture { width: 2, height: 1, rgb: vec![0, 0, 0, 200, 200, 200] };
        assert_eq!(two.scaled(4, 1, false).rgb, vec![0, 0, 0, 0, 0, 0, 200, 200, 200, 200, 200, 200]);
        let smooth = two.scaled(4, 1, true);
        assert_eq!(smooth.pixel(0, 0), [0; 3]);
        assert_eq!(smooth.pixel(1, 0), [50; 3]);
        assert_eq!(smooth.pixel(2, 0), [150; 3]);
        assert_eq!(smooth.pixel(3, 0), [200; 3]);
        // Shrinking averages.
        let four = Picture { width: 4, height: 1, rgb: [0, 100, 200, 100].iter().flat_map(|v| [*v; 3]).collect() };
        assert_eq!(four.scaled(2, 1, true).rgb, vec![50, 50, 50, 150, 150, 150]);
        // Not an even factor: the shares add up to one.
        let three = four.scaled(3, 1, true);
        assert_eq!(three.pixel(0, 0), [25; 3]);
        assert_eq!(three.pixel(1, 0), [150; 3]);
        assert_eq!(three.pixel(2, 0), [125; 3]);
    }

    /// Design D1: 256 columns × 128 rows at Pixel Spacing 1.0 \ 0.5 is square
    /// in millimetres and keeps its finer spacing.
    #[test]
    fn a_non_square_image_gets_square_pixels_with_its_finer_spacing() {
        let (square, mm) = gradient(256, 128).with_square_pixels(Some([1.0, 0.5]));
        assert_eq!((square.width, square.height, mm), (256, 256, Some(0.5)));
        // The other way around: columns are the coarser ones.
        let (square, mm) = gradient(128, 256).with_square_pixels(Some([0.5, 1.0]));
        assert_eq!((square.width, square.height, mm), (256, 256, Some(0.5)));
        // Square pixels and unknown spacing change nothing.
        let picture = gradient(16, 8);
        let (same, mm) = picture.clone().with_square_pixels(Some([0.7, 0.7]));
        assert_eq!((same == picture, mm), (true, Some(0.7)));
        let (same, mm) = picture.clone().with_square_pixels(None);
        assert_eq!((same == picture, mm), (true, None));
        let (same, mm) = picture.clone().with_square_pixels(Some([0.0, f64::NAN]));
        assert_eq!((same == picture, mm), (true, None));
        // Absurd spacing from a disc does not ask for gigabytes, and the scale stays true.
        let (capped, mm) = gradient(64, 64).with_square_pixels(Some([1000.0, 0.01]));
        assert_eq!(capped.height, MAX_EDGE);
        assert_eq!(capped.width, 1);
        let mm = mm.expect("known");
        assert!((mm * MAX_EDGE as f64 - 64_000.0).abs() < 1.0, "{mm} mm per pixel");
    }

    /// The file follows its view: every corner of the image lands where
    /// `DisplayOrientation::to_screen` puts it, in all eight orientations.
    #[test]
    fn orienting_a_picture_matches_the_view_in_all_eight_orientations() {
        let (width, height) = (5, 3);
        let picture = gradient(width, height);
        for orientation in DisplayOrientation::ALL {
            let turned = picture.oriented(orientation);
            let (w, h) = if orientation.swaps_axes() { (height, width) } else { (width, height) };
            assert_eq!((turned.width, turned.height), (w, h), "{orientation:?}");
            for y in 0..height {
                for x in 0..width {
                    let (s, t) =
                        orientation.to_screen((x as f64 + 0.5) / width as f64, (y as f64 + 0.5) / height as f64);
                    let (tx, ty) = ((s * w as f64) as usize, (t * h as f64) as usize);
                    assert_eq!(turned.pixel(tx, ty), picture.pixel(x, y), "{orientation:?}: pixel ({x}, {y})");
                }
            }
        }
    }

    fn chunks(png: &[u8]) -> Vec<String> {
        let mut names = Vec::new();
        let mut i = 8;
        while i + 8 <= png.len() {
            let length = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
            names.push(String::from_utf8_lossy(&png[i + 4..i + 8]).into_owned());
            i += 12 + length;
        }
        names
    }

    /// Design D2: 8-bit, greyscale for grey pictures, and nothing but the
    /// picture in the file.
    #[test]
    fn encoded_files_hold_the_picture_and_no_metadata() {
        let grey =
            Picture { width: 3, height: 2, rgb: [0, 50, 100, 150, 200, 255].iter().flat_map(|v| [*v; 3]).collect() };
        let png = encode(&grey, ImageFormat::Png).expect("png");
        assert_eq!(chunks(&png).iter().filter(|name| name.as_str() != "IDAT").collect::<Vec<_>>(), ["IHDR", "IEND"]);
        let decoded = image::load_from_memory(&png).expect("decode");
        assert_eq!(decoded.color(), image::ColorType::L8);
        assert_eq!(decoded.to_luma8().into_raw(), vec![0, 50, 100, 150, 200, 255]);

        let colour = gradient(8, 8);
        let decoded = image::load_from_memory(&encode(&colour, ImageFormat::Png).expect("png")).expect("decode");
        assert_eq!(decoded.color(), image::ColorType::Rgb8);
        assert_eq!(decoded.to_rgb8().into_raw(), colour.rgb, "lossless");

        // JPEG: the JFIF header and the picture; no comment, no EXIF.
        let jpeg = encode(&colour, ImageFormat::Jpeg).expect("jpeg");
        assert_eq!(&jpeg[..2], [0xFF, 0xD8]);
        let mut i = 2;
        while jpeg[i] == 0xFF && jpeg[i + 1] != 0xDA {
            let marker = jpeg[i + 1];
            assert_ne!(marker, 0xFE, "no comment");
            assert!(!(0xE1..=0xEF).contains(&marker), "no EXIF or other application data");
            i += 2 + usize::from(jpeg[i + 2]) * 256 + usize::from(jpeg[i + 3]);
        }
        assert_eq!(image::load_from_memory(&jpeg).expect("decode").width(), 8);
    }

    #[test]
    fn suggested_names_are_neutral() {
        assert_eq!(image_file_name("CT", Some(1), None, Some(31), ImageFormat::Png), "SumMRIse-CT-S1-031.png");
        assert_eq!(image_file_name("MR", None, None, Some(1234), ImageFormat::Jpeg), "SumMRIse-MR-1234.jpg");
        assert_eq!(
            image_file_name("CT", Some(1), Some("MPR"), Some(12), ImageFormat::Png),
            "SumMRIse-CT-S1-MPR-012.png"
        );
        assert_eq!(image_file_name("CT", Some(1), Some("3D"), None, ImageFormat::Png), "SumMRIse-CT-S1-3D.png");
        assert_eq!(pdf_file_name("CT", Some(1)), "SumMRIse-CT-S1.pdf");
        // Whatever the disc holds, only letters and digits of the modality come through.
        assert_eq!(pdf_file_name("../ü/\\", Some(-2)), "SumMRIse-IMG.pdf");

        let names = series_file_names("CT", Some(1), 60, ImageFormat::Png);
        assert_eq!((names[0].as_str(), names[59].as_str()), ("SumMRIse-CT-S1-001.png", "SumMRIse-CT-S1-060.png"));
        let names = series_file_names("CT", Some(1), 1000, ImageFormat::Jpeg);
        assert_eq!((names[0].as_str(), names[999].as_str()), ("SumMRIse-CT-S1-0001.jpg", "SumMRIse-CT-S1-1000.jpg"));

        let path = |p: &str| PathBuf::from(p);
        assert_eq!(with_extension(path("/a/b"), ImageFormat::Png), path("/a/b.png"));
        assert_eq!(with_extension(path("/a/b.PNG"), ImageFormat::Png), path("/a/b.PNG"));
        assert_eq!(with_extension(path("/a/b.jpeg"), ImageFormat::Jpeg), path("/a/b.jpeg"));
        assert_eq!(with_extension(path("/a/b.jpg"), ImageFormat::Png), path("/a/b.jpg.png"));
    }

    #[test]
    fn personal_details_may_be_in_burned_in_scanned_and_ultrasound_pictures() {
        let header = |class: &str, burned_in_text: bool| {
            let fidelity = Fidelity { burned_in_text, ..Fidelity::default() };
            InstanceHeader {
                path: PathBuf::new(),
                sop_instance_uid: String::new(),
                sop_class_uid: class.to_owned(),
                transfer_syntax_uid: String::new(),
                instance_number: None,
                frames: 1,
                rows: 1,
                cols: 1,
                samples_per_pixel: 1,
                photometric: String::new(),
                pixel_spacing: None,
                frame_planes: vec![None],
                image_type: Vec::new(),
                bits_stored: 8,
                signed: false,
                rescale: Vec::new(),
                rescale_type: None,
                frame_values: Vec::new(),
                fidelity,
                calibration: Default::default(),
            }
        };
        const CT: &str = "1.2.840.10008.5.1.4.1.1.2";
        assert!(!may_show_personal_details(&header(CT, false), "CT"));
        assert!(may_show_personal_details(&header(CT, true), "CT"));
        assert!(may_show_personal_details(&header("1.2.840.10008.5.1.4.1.1.7", false), "OT"), "secondary capture");
        assert!(may_show_personal_details(&header("1.2.840.10008.5.1.4.1.1.6.1", false), "US"), "ultrasound");
        assert!(may_show_personal_details(&header(CT, false), "US "), "by modality");
    }

    fn names(count: usize) -> Vec<String> {
        series_file_names("CT", Some(1), count, ImageFormat::Png)
    }

    fn listing(folder: &Path) -> Vec<String> {
        let mut files: Vec<String> = std::fs::read_dir(folder)
            .expect("folder")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        files
    }

    /// Spec "Export a series as images": sixty images, numbered in display order.
    #[test]
    fn a_series_export_writes_one_numbered_file_per_image_with_progress() {
        let dir = tempfile::tempdir().expect("tempdir");
        let names = names(60);
        let progress = Progress::new();
        let done = export_series(dir.path(), &names, ImageFormat::Png, &progress, |i| {
            Some(Picture { width: 2, height: 2, rgb: vec![i as u8; 12] })
        });
        assert_eq!((done.written, done.total, done.cancelled, done.missing), (60, 60, false, 0));
        assert!(done.error.is_none());
        assert_eq!((progress.snapshot().done, progress.snapshot().total), (60, 60));
        assert_eq!(listing(dir.path()), names);
        // In display order: the file with number n holds image n.
        for (i, name) in names.iter().enumerate() {
            let decoded = image::open(dir.path().join(name)).expect("decode").to_luma8();
            assert_eq!(decoded.get_pixel(0, 0).0, [i as u8], "{name}");
        }
    }

    /// Spec: "the export stops, the 20 files remain".
    #[test]
    fn cancelling_a_series_export_keeps_what_was_written() {
        let dir = tempfile::tempdir().expect("tempdir");
        let names = names(60);
        let progress = Progress::new();
        let done = export_series(dir.path(), &names, ImageFormat::Png, &progress.clone(), |i| {
            // The user presses Cancel while file 20 is written.
            if i == 19 {
                progress.cancel();
            }
            Some(Picture { width: 2, height: 2, rgb: vec![0; 12] })
        });
        assert_eq!((done.written, done.total, done.cancelled), (20, 60, true));
        assert_eq!(listing(dir.path()), names[..20]);
    }

    #[test]
    fn an_image_that_cannot_be_rendered_keeps_its_number_free_and_a_failure_stops_the_export() {
        let dir = tempfile::tempdir().expect("tempdir");
        let names = names(4);
        let picture = || Picture { width: 1, height: 1, rgb: vec![0; 3] };
        let done = export_series(dir.path(), &names, ImageFormat::Png, &Progress::new(), |i| (i != 1).then(picture));
        assert_eq!((done.written, done.missing), (3, 1));
        assert_eq!(listing(dir.path()), [names[0].clone(), names[2].clone(), names[3].clone()]);

        // A folder that cannot be written to: stops at the first file and says which.
        let gone = dir.path().join("does-not-exist");
        let done = export_series(&gone, &names, ImageFormat::Png, &Progress::new(), |_| Some(picture()));
        assert_eq!(done.written, 0);
        match done.error {
            Some(ExportError::Write { name, .. }) => assert_eq!(name, names[0]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn existing_files_are_found_before_anything_is_written() {
        let dir = tempfile::tempdir().expect("tempdir");
        let names = names(3);
        std::fs::write(dir.path().join(&names[1]), b"mine").expect("write");
        assert_eq!(existing_files(dir.path(), &names), [names[1].clone()]);
    }
}
