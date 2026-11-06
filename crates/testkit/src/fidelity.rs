//! Images that are standard but unusual: palette colour, contrast tables
//! (VOI LUT Sequence), a Modality LUT Sequence, pixel padding, scanner
//! graphics (overlay planes), lossy-compression facts, burned-in text and
//! Presentation LUT Shape INVERSE.
//!
//! [`write_fidelity_study`] writes one study (a folder scan, no DICOMDIR) of
//! twelve small series, one case each. In index order (series are sorted by
//! number):
//!
//! | index | series | what it holds |
//! |---|---|---|
//! | [`PALETTE_8`] | 1 image, 8-bit palette colour | index `(x + y) % 8`; index 3 is [`PALETTE_RGB_3`]; tables packed two entries per word |
//! | [`PALETTE_16`] | 1 image, 16-bit palette colour | as above with 16-bit indices and 16-bit entries ([`PALETTE_RGB_3`] × 257); index 5000 lies beyond the table |
//! | [`VOI_LUT`] | 1 radiograph, no window | two contrast tables, NORMAL ([`normal_entry`]) and HARDER ([`harder_entry`]); stored value `y × 64 + x` |
//! | [`MODALITY_LUT`] | 1 image, 10 bits stored | Modality LUT Sequence `round(i² / 16)`, type OD; stored value `y × 32 + x` |
//! | [`PADDED_CT`] | 4 CT slices | signed, intercept −1024, Pixel Padding Value −2000 outside a circle of [`PADDED_CT_RADIUS`] pixels |
//! | [`OVERLAY`] | 1 image, 500 at column 20, row 10 | a 16 × 8 rectangle outline in (6000,3000) at Overlay Origin row 11, column 21 |
//! | [`EMBEDDED_OVERLAY`] | 1 image, 12 of 16 bits stored | a cross in bit 12, garbage in bits 13–15; pixel (5, 5) holds 0xF52C |
//! | [`MULTI_FRAME_OVERLAY`] | 1 file of 3 frames | a 2-frame overlay from Image Frame Origin 2: a row, then a column |
//! | [`LOSSY`] | 1 uncompressed image | Lossy Image Compression 01, ratio 10, method ISO_10918_1 |
//! | [`BURNED_IN`] | 1 ultrasound image | Burned In Annotation YES |
//! | [`SEGMENTED_PALETTE`] | 1 palette image | segmented palette data only |
//! | [`INVERSE`] | 1 MONOCHROME2 radiograph | Presentation LUT Shape INVERSE |

use std::fs;
use std::path::Path;

use dicom_core::value::{DataSetSequence, PrimitiveValue};
use dicom_core::{DataElement, Length, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;

use crate::{
    FixtureError, Geometry, UID_ROOT, WrittenSeries, WrittenStudy, base_object, ct_hu, put_ds, put_geometry, put_str,
    put_u16, write_file,
};

/// Index of each series in the indexed study.
pub const PALETTE_8: usize = 0;
pub const PALETTE_16: usize = 1;
pub const VOI_LUT: usize = 2;
pub const MODALITY_LUT: usize = 3;
pub const PADDED_CT: usize = 4;
pub const OVERLAY: usize = 5;
pub const EMBEDDED_OVERLAY: usize = 6;
pub const MULTI_FRAME_OVERLAY: usize = 7;
pub const LOSSY: usize = 8;
pub const BURNED_IN: usize = 9;
pub const SEGMENTED_PALETTE: usize = 10;
pub const INVERSE: usize = 11;
/// Number of series.
pub const SERIES: usize = 12;

/// The colour of palette index 3 (8 bits per channel).
pub const PALETTE_RGB_3: [u8; 3] = [200, 30, 60];
/// Columns and rows of the palette images.
pub const PALETTE_SIZE: (u32, u32) = (32, 24);
/// The 16-bit palette image holds this index at column 0, row 0, beyond its 4096 entries.
pub const PALETTE_16_BEYOND: u16 = 5000;

/// Columns and rows of the image with contrast tables; its stored value at (x, y) is y × 64 + x.
pub const VOI_LUT_SIZE: u32 = 64;
/// Columns and rows of the image with a Modality LUT; stored value at (x, y) is y × 32 + x.
pub const MODALITY_LUT_SIZE: u32 = 32;

/// Columns and rows of the padded CT slices, their pixel size and padding.
pub const PADDED_CT_SIZE: u32 = 64;
pub const PADDED_CT_PIXEL_MM: f64 = 3.75;
/// Pixels farther than this from the centre of a padded slice are padding.
pub const PADDED_CT_RADIUS: f64 = 30.0;
/// Stored padding value (Pixel Padding Value) and rescale intercept: padding would read −3024 HU.
pub const PADDING_VALUE: i16 = -2000;
pub const PADDED_CT_INTERCEPT: f64 = -1024.0;
/// Slice heights (S, mm) of the padded CT.
pub const PADDED_CT_HEIGHTS: [f64; 4] = [-6.0, -2.0, 2.0, 6.0];

/// The image with an overlay: its size, its values ([`overlay_image_value`]) and the overlay plane.
pub const OVERLAY_IMAGE_SIZE: (u32, u32) = (64, 48);
/// Columns and rows of the overlay rectangle, and its origin (Overlay Origin: row, column, 1-based).
pub const OVERLAY_SIZE: (u32, u32) = (16, 8);
pub const OVERLAY_ORIGIN: [i16; 2] = [11, 21];

/// The image with an overlay in bit 12: its size and the pixel that holds 0xF52C.
pub const EMBEDDED_SIZE: u32 = 32;
pub const EMBEDDED_SPECIAL_PIXEL: (u32, u32) = (5, 5);
pub const EMBEDDED_SPECIAL_RAW: u16 = 0xF52C;
/// The bit of the overlay in the pixel data.
pub const EMBEDDED_BIT: u16 = 12;
/// Rescale intercept of the image with the overlay in bit 12.
pub const EMBEDDED_INTERCEPT: f64 = -1024.0;

/// The multi-frame image: frames, size, the row of overlay frame 1 and the column of overlay frame 2.
pub const MULTI_FRAMES: u32 = 3;
pub const MULTI_SIZE: u32 = 32;
pub const MULTI_OVERLAY_ROW: u32 = 4;
pub const MULTI_OVERLAY_COLUMN: u32 = 9;

/// Entry `i` of the contrast table NORMAL: a square-root curve over 12 bits.
pub fn normal_entry(i: u32) -> u16 {
    (4095.0 * (f64::from(i.min(4095)) / 4095.0).sqrt()).round() as u16
}

/// Entry `i` of the contrast table HARDER: a square curve over 12 bits.
pub fn harder_entry(i: u32) -> u16 {
    (4095.0 * (f64::from(i.min(4095)) / 4095.0).powi(2)).round() as u16
}

/// Entry `i` of the Modality LUT: round(i² / 16).
pub fn modality_entry(i: u32) -> u16 {
    (f64::from(i).powi(2) / 16.0).round().min(65535.0) as u16
}

/// Stored value of the image with an overlay in bit 12, at (x, y), without overlay or garbage.
pub fn embedded_stored(x: u32, y: u32) -> u16 {
    if (x, y) == EMBEDDED_SPECIAL_PIXEL { 1324 } else { 1024 + 4 * ((x + y) % 64) as u16 }
}

/// Whether the overlay in bit 12 covers (x, y): a cross through (5, 5).
pub fn embedded_overlay(x: u32, y: u32) -> bool {
    x == EMBEDDED_SPECIAL_PIXEL.0 || y == EMBEDDED_SPECIAL_PIXEL.1
}

/// The value of the image with an overlay at (x, y): a gradient, 500 at column 20, row 10.
pub fn overlay_image_value(x: u32, y: u32) -> u16 {
    (470 + x + y) as u16
}

/// Whether pixel (`x`, `y`) of the overlay rectangle is set: its outline.
pub fn overlay_rectangle(x: u32, y: u32) -> bool {
    let (w, h) = OVERLAY_SIZE;
    x < w && y < h && (x == 0 || y == 0 || x == w - 1 || y == h - 1)
}

/// Whether pixel (`x`, `y`) of a padded CT slice is padding.
pub fn padded_ct_is_padding(x: u32, y: u32) -> bool {
    let c = (f64::from(PADDED_CT_SIZE) - 1.0) / 2.0;
    (f64::from(x) - c).hypot(f64::from(y) - c) > PADDED_CT_RADIUS
}

/// Hounsfield units of pixel (`x`, `y`) of the padded CT slice at height `z`
/// (padding aside): the chest phantom of the synthetic CD.
pub fn padded_ct_hu(x: u32, y: u32, z: f64) -> f64 {
    ct_hu(padded_ct_geometry(z).pixel(x, y)).round()
}

fn padded_ct_geometry(z: f64) -> Geometry {
    let half = (f64::from(PADDED_CT_SIZE) - 1.0) * PADDED_CT_PIXEL_MM / 2.0;
    Geometry { position: [-half, -half, z], orientation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], spacing: PADDED_CT_PIXEL_MM }
}

/// One series of the study.
struct Spec {
    number: u32,
    modality: &'static str,
    description: &'static str,
    sop_class: &'static str,
}

impl Spec {
    fn uid(&self) -> String {
        format!("{UID_ROOT}.6.{}", self.number)
    }

    fn object(&self, study_uid: &str, instance: u32) -> InMemDicomObject {
        let uid = self.uid();
        base_object(
            study_uid,
            &uid,
            &format!("{uid}.{instance}"),
            self.sop_class,
            self.modality,
            self.number,
            self.description,
            instance,
        )
    }
}

/// Writes the study (see the module documentation) below `root`.
pub fn write_fidelity_study(root: &Path) -> Result<WrittenStudy, FixtureError> {
    let study_uid = format!("{UID_ROOT}.6");
    let dir = root.join("DICOM").join("ST000006");
    let series = vec![
        palette_8(&dir, &study_uid)?,
        palette_16(&dir, &study_uid)?,
        voi_lut(&dir, &study_uid)?,
        modality_lut(&dir, &study_uid)?,
        padded_ct(&dir, &study_uid)?,
        overlay(&dir, &study_uid)?,
        embedded_overlay_series(&dir, &study_uid)?,
        multi_frame_overlay(&dir, &study_uid)?,
        lossy(&dir, &study_uid)?,
        burned_in(&dir, &study_uid)?,
        segmented_palette(&dir, &study_uid)?,
        inverse(&dir, &study_uid)?,
    ];
    Ok(WrittenStudy { root: root.to_path_buf(), study_uid, series, dicomdir: None })
}

fn write_series(dir: &Path, spec: &Spec, images: Vec<InMemDicomObject>) -> Result<WrittenSeries, FixtureError> {
    let dir = dir.join(format!("SE{:06}", spec.number));
    fs::create_dir_all(&dir)?;
    let mut files = Vec::new();
    for (k, obj) in images.into_iter().enumerate() {
        let path = dir.join(format!("IM{:06}", k + 1));
        write_file(obj, &path, uids::EXPLICIT_VR_LITTLE_ENDIAN, None)?;
        files.push(path);
    }
    Ok(WrittenSeries {
        uid: spec.uid(),
        number: spec.number,
        modality: spec.modality,
        description: spec.description,
        files,
    })
}

fn element(tag: Tag, vr: VR, value: PrimitiveValue) -> DataElement<InMemDicomObject> {
    DataElement::new(tag, vr, value)
}

fn words(values: Vec<u16>) -> PrimitiveValue {
    PrimitiveValue::U16(values.into())
}

/// Image Pixel attributes of grey-scale or palette data.
fn put_pixels(obj: &mut InMemDicomObject, (cols, rows): (u32, u32), photometric: &str, bits: (u16, u16), signed: bool) {
    let (allocated, stored) = bits;
    put_u16(obj, tags::SAMPLES_PER_PIXEL, 1);
    put_str(obj, tags::PHOTOMETRIC_INTERPRETATION, VR::CS, &[photometric]);
    put_u16(obj, tags::ROWS, rows as u16);
    put_u16(obj, tags::COLUMNS, cols as u16);
    put_u16(obj, tags::BITS_ALLOCATED, allocated);
    put_u16(obj, tags::BITS_STORED, stored);
    put_u16(obj, tags::HIGH_BIT, stored - 1);
    put_u16(obj, tags::PIXEL_REPRESENTATION, u16::from(signed));
}

fn put_pixel_data_8(obj: &mut InMemDicomObject, pixels: Vec<u8>) {
    obj.put(element(tags::PIXEL_DATA, VR::OB, PrimitiveValue::U8(pixels.into())));
}

fn put_pixel_data_16(obj: &mut InMemDicomObject, pixels: Vec<u16>) {
    obj.put(element(tags::PIXEL_DATA, VR::OW, words(pixels)));
}

/// The descriptor and data of one palette channel.
fn put_palette_channel(obj: &mut InMemDicomObject, channel: usize, descriptor: [u16; 3], data: Vec<u16>) {
    let descriptors = [
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
    ];
    let tables = [
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    ];
    obj.put(element(descriptors[channel], VR::US, words(descriptor.to_vec())));
    obj.put(element(tables[channel], VR::OW, words(data)));
}

/// The colour of palette index `i` (index 3 is [`PALETTE_RGB_3`]).
pub fn palette_colour(i: u32) -> [u8; 3] {
    if i == 3 { PALETTE_RGB_3 } else { [(i * 37 % 256) as u8, (255 - i * 11 % 256) as u8, (i * 73 % 256) as u8] }
}

fn palette_8(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 31,
        modality: "US",
        description: "PALETTE 8-BIT (synthetic)",
        sop_class: uids::ULTRASOUND_IMAGE_STORAGE,
    };
    let mut obj = spec.object(study_uid, 1);
    let (w, h) = PALETTE_SIZE;
    put_pixels(&mut obj, PALETTE_SIZE, "PALETTE COLOR", (8, 8), false);
    for channel in 0..3 {
        let entries: Vec<u8> = (0..256).map(|i| palette_colour(i)[channel]).collect();
        // 8-bit entries are stored as 8 bits allocated: two entries per 16-bit word, the first in the low byte.
        let packed = entries.chunks(2).map(|p| u16::from(p[0]) | u16::from(p[1]) << 8).collect();
        put_palette_channel(&mut obj, channel, [256, 0, 8], packed);
    }
    put_pixel_data_8(&mut obj, (0..w * h).map(|i| ((i % w + i / w) % 8) as u8).collect());
    write_series(dir, &spec, vec![obj])
}

fn palette_16(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 32,
        modality: "US",
        description: "PALETTE 16-BIT (synthetic)",
        sop_class: uids::ULTRASOUND_IMAGE_STORAGE,
    };
    let mut obj = spec.object(study_uid, 1);
    let (w, h) = PALETTE_SIZE;
    put_pixels(&mut obj, PALETTE_SIZE, "PALETTE COLOR", (16, 16), false);
    for channel in 0..3 {
        let entries = (0..4096).map(|i| u16::from(palette_colour(i)[channel]) * 257).collect();
        put_palette_channel(&mut obj, channel, [4096, 0, 16], entries);
    }
    let mut pixels: Vec<u16> = (0..w * h).map(|i| ((i % w + i / w) % 8) as u16).collect();
    pixels[0] = PALETTE_16_BEYOND;
    put_pixel_data_16(&mut obj, pixels);
    write_series(dir, &spec, vec![obj])
}

/// An item of a LUT sequence: descriptor, data and an optional explanation.
fn lut_item(descriptor: [u16; 3], data: Vec<u16>, explanation: Option<&str>, kind: Option<&str>) -> InMemDicomObject {
    let mut item = InMemDicomObject::new_empty();
    item.put(element(tags::LUT_DESCRIPTOR, VR::US, words(descriptor.to_vec())));
    if let Some(text) = explanation {
        put_str(&mut item, tags::LUT_EXPLANATION, VR::LO, &[text]);
    }
    if let Some(kind) = kind {
        put_str(&mut item, tags::MODALITY_LUT_TYPE, VR::LO, &[kind]);
    }
    item.put(element(tags::LUT_DATA, VR::OW, words(data)));
    item
}

fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> DataElement<InMemDicomObject> {
    DataElement::new(tag, VR::SQ, DataSetSequence::new(items, Length::UNDEFINED))
}

fn voi_lut(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 33,
        modality: "DX",
        description: "RADIOGRAPH WITH CONTRAST TABLES (synthetic)",
        sop_class: uids::DIGITAL_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
    };
    let mut obj = spec.object(study_uid, 1);
    let n = VOI_LUT_SIZE;
    put_pixels(&mut obj, (n, n), "MONOCHROME2", (16, 12), false);
    obj.put(sequence(
        tags::VOILUT_SEQUENCE,
        vec![
            lut_item([4096, 0, 12], (0..4096).map(normal_entry).collect(), Some("NORMAL"), None),
            lut_item([4096, 0, 12], (0..4096).map(harder_entry).collect(), Some("HARDER"), None),
        ],
    ));
    put_pixel_data_16(&mut obj, (0..n * n).map(|i| i as u16).collect());
    write_series(dir, &spec, vec![obj])
}

fn modality_lut(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 34,
        modality: "CR",
        description: "MODALITY TABLE (synthetic)",
        sop_class: uids::COMPUTED_RADIOGRAPHY_IMAGE_STORAGE,
    };
    let mut obj = spec.object(study_uid, 1);
    let n = MODALITY_LUT_SIZE;
    put_pixels(&mut obj, (n, n), "MONOCHROME2", (16, 10), false);
    obj.put(sequence(
        tags::MODALITY_LUT_SEQUENCE,
        vec![lut_item([1024, 0, 16], (0..1024).map(modality_entry).collect(), None, Some("OD"))],
    ));
    put_pixel_data_16(&mut obj, (0..n * n).map(|i| i as u16).collect());
    write_series(dir, &spec, vec![obj])
}

fn padded_ct(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec =
        Spec { number: 35, modality: "CT", description: "CT PADDED (synthetic)", sop_class: uids::CT_IMAGE_STORAGE };
    let n = PADDED_CT_SIZE;
    let mut images = Vec::new();
    for (k, &z) in PADDED_CT_HEIGHTS.iter().enumerate() {
        let mut obj = spec.object(study_uid, k as u32 + 1);
        put_geometry(&mut obj, &padded_ct_geometry(z), n, n);
        put_str(&mut obj, tags::IMAGE_TYPE, VR::CS, &["ORIGINAL", "PRIMARY", "AXIAL"]);
        put_ds(&mut obj, tags::SLICE_THICKNESS, &[4.0]);
        put_pixels(&mut obj, (n, n), "MONOCHROME2", (16, 16), true);
        put_ds(&mut obj, tags::RESCALE_INTERCEPT, &[PADDED_CT_INTERCEPT]);
        put_ds(&mut obj, tags::RESCALE_SLOPE, &[1.0]);
        put_str(&mut obj, tags::RESCALE_TYPE, VR::LO, &["HU"]);
        obj.put(element(tags::PIXEL_PADDING_VALUE, VR::SS, PrimitiveValue::I16(vec![PADDING_VALUE].into())));
        let pixels: Vec<u16> = (0..n * n)
            .map(|i| {
                let (x, y) = (i % n, i / n);
                let stored =
                    if padded_ct_is_padding(x, y) { PADDING_VALUE } else { (padded_ct_hu(x, y, z) + 1024.0) as i16 };
                stored as u16
            })
            .collect();
        put_pixel_data_16(&mut obj, pixels);
        images.push(obj);
    }
    write_series(dir, &spec, images)
}

/// The attributes of overlay plane group `group`.
fn put_overlay(
    obj: &mut InMemDicomObject,
    group: u16,
    (cols, rows): (u32, u32),
    origin: [i16; 2],
    bits: (u16, u16),
    data: Option<Vec<u8>>,
) {
    obj.put(element(Tag(group, 0x0010), VR::US, words(vec![rows as u16])));
    obj.put(element(Tag(group, 0x0011), VR::US, words(vec![cols as u16])));
    put_str(obj, Tag(group, 0x0040), VR::CS, &["G"]);
    obj.put(element(Tag(group, 0x0050), VR::SS, PrimitiveValue::I16(origin.to_vec().into())));
    obj.put(element(Tag(group, 0x0100), VR::US, words(vec![bits.0])));
    obj.put(element(Tag(group, 0x0102), VR::US, words(vec![bits.1])));
    put_str(obj, Tag(group, 0x1500), VR::LO, &["SYNTHETIC MARK"]);
    if let Some(mut data) = data {
        if data.len() % 2 == 1 {
            data.push(0);
        }
        obj.put(element(Tag(group, 0x3000), VR::OB, PrimitiveValue::U8(data.into())));
    }
}

/// Packs one bit per pixel, least significant bit first, as Overlay Data is stored.
fn pack_bits(bits: impl IntoIterator<Item = bool>) -> Vec<u8> {
    let bits: Vec<bool> = bits.into_iter().collect();
    bits.chunks(8).map(|byte| byte.iter().enumerate().fold(0u8, |acc, (i, &b)| acc | (u8::from(b) << i))).collect()
}

fn overlay(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 36,
        modality: "OT",
        description: "SCANNER GRAPHICS (synthetic)",
        sop_class: uids::SECONDARY_CAPTURE_IMAGE_STORAGE,
    };
    let mut obj = spec.object(study_uid, 1);
    let (w, h) = OVERLAY_IMAGE_SIZE;
    put_pixels(&mut obj, OVERLAY_IMAGE_SIZE, "MONOCHROME2", (16, 12), false);
    let (ow, oh) = OVERLAY_SIZE;
    let data = pack_bits((0..ow * oh).map(|i| overlay_rectangle(i % ow, i / ow)));
    put_overlay(&mut obj, 0x6000, OVERLAY_SIZE, OVERLAY_ORIGIN, (1, 0), Some(data));
    put_pixel_data_16(&mut obj, (0..w * h).map(|i| overlay_image_value(i % w, i / w)).collect());
    write_series(dir, &spec, vec![obj])
}

fn embedded_overlay_series(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 37,
        modality: "CT",
        description: "OVERLAY IN BIT 12 (synthetic)",
        sop_class: uids::CT_IMAGE_STORAGE,
    };
    let mut obj = spec.object(study_uid, 1);
    let n = EMBEDDED_SIZE;
    put_pixels(&mut obj, (n, n), "MONOCHROME2", (16, 12), false);
    put_ds(&mut obj, tags::RESCALE_INTERCEPT, &[EMBEDDED_INTERCEPT]);
    put_ds(&mut obj, tags::RESCALE_SLOPE, &[1.0]);
    put_str(&mut obj, tags::RESCALE_TYPE, VR::LO, &["HU"]);
    // The retired form: no Overlay Data, the plane lives in bit 12 of the pixel data.
    put_overlay(&mut obj, 0x6000, (n, n), [1, 1], (16, EMBEDDED_BIT), None);
    let pixels = (0..n * n)
        .map(|i| {
            let (x, y) = (i % n, i / n);
            if (x, y) == EMBEDDED_SPECIAL_PIXEL {
                return EMBEDDED_SPECIAL_RAW;
            }
            let overlay = if embedded_overlay(x, y) { 1 << EMBEDDED_BIT } else { 0 };
            let garbage = (((x ^ y) & 7) as u16) << 13;
            embedded_stored(x, y) | overlay | garbage
        })
        .collect();
    put_pixel_data_16(&mut obj, pixels);
    write_series(dir, &spec, vec![obj])
}

fn multi_frame_overlay(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 38,
        modality: "XA",
        description: "MULTI-FRAME GRAPHICS (synthetic)",
        sop_class: uids::X_RAY_ANGIOGRAPHIC_IMAGE_STORAGE,
    };
    let mut obj = spec.object(study_uid, 1);
    let n = MULTI_SIZE;
    put_pixels(&mut obj, (n, n), "MONOCHROME2", (8, 8), false);
    put_str(&mut obj, tags::NUMBER_OF_FRAMES, VR::IS, &[&MULTI_FRAMES.to_string()]);
    // Overlay frame 1 is a row, overlay frame 2 a column; they apply to image frames 2 and 3.
    let frame_1 = (0..n * n).map(|i| i / n == MULTI_OVERLAY_ROW);
    let frame_2 = (0..n * n).map(|i| i % n == MULTI_OVERLAY_COLUMN);
    put_overlay(&mut obj, 0x6000, (n, n), [1, 1], (1, 0), Some(pack_bits(frame_1.chain(frame_2))));
    put_str(&mut obj, Tag(0x6000, 0x0015), VR::IS, &["2"]);
    obj.put(element(Tag(0x6000, 0x0051), VR::US, words(vec![2])));
    let pixels = (0..MULTI_FRAMES).flat_map(|f| (0..n * n).map(move |_| (50 + 50 * f) as u8)).collect();
    put_pixel_data_8(&mut obj, pixels);
    write_series(dir, &spec, vec![obj])
}

/// A small grey-scale picture: a soft gradient with a brighter disc.
fn gradient(n: u32) -> Vec<u8> {
    let c = f64::from(n) / 2.0;
    (0..n * n)
        .map(|i| {
            let (x, y) = (f64::from(i % n), f64::from(i / n));
            let disc = if (x - c).hypot(y - c) < c / 2.0 { 90.0 } else { 0.0 };
            (40.0 + x * 2.0 + disc).min(255.0) as u8
        })
        .collect()
}

fn lossy(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec =
        Spec { number: 39, modality: "MR", description: "LOSSY 10:1 (synthetic)", sop_class: uids::MR_IMAGE_STORAGE };
    let mut obj = spec.object(study_uid, 1);
    put_pixels(&mut obj, (48, 48), "MONOCHROME2", (8, 8), false);
    put_str(&mut obj, tags::LOSSY_IMAGE_COMPRESSION, VR::CS, &["01"]);
    put_ds(&mut obj, tags::LOSSY_IMAGE_COMPRESSION_RATIO, &[10.0]);
    put_str(&mut obj, tags::LOSSY_IMAGE_COMPRESSION_METHOD, VR::CS, &["ISO_10918_1"]);
    put_pixel_data_8(&mut obj, gradient(48));
    write_series(dir, &spec, vec![obj])
}

fn burned_in(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 40,
        modality: "US",
        description: "BURNED-IN TEXT (synthetic)",
        sop_class: uids::ULTRASOUND_IMAGE_STORAGE,
    };
    let mut obj = spec.object(study_uid, 1);
    let n = 64u32;
    put_pixels(&mut obj, (n, n), "MONOCHROME2", (8, 8), false);
    put_str(&mut obj, tags::BURNED_IN_ANNOTATION, VR::CS, &["YES"]);
    // Bright blocks in the top left corner stand for burned-in text (no real text).
    let mut pixels = gradient(n);
    for (i, p) in pixels.iter_mut().enumerate() {
        let (x, y) = (i as u32 % n, i as u32 / n);
        if (2..4).contains(&y) && x < 30 && x % 4 != 3 || (6..8).contains(&y) && x < 18 && x % 3 != 2 {
            *p = 250;
        }
    }
    put_pixel_data_8(&mut obj, pixels);
    write_series(dir, &spec, vec![obj])
}

fn segmented_palette(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 41,
        modality: "US",
        description: "SEGMENTED PALETTE (synthetic)",
        sop_class: uids::ULTRASOUND_IMAGE_STORAGE,
    };
    let mut obj = spec.object(study_uid, 1);
    put_pixels(&mut obj, (16, 16), "PALETTE COLOR", (8, 8), false);
    let descriptors = [
        tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
        tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
    ];
    let segmented = [
        tags::SEGMENTED_RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        tags::SEGMENTED_BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
    ];
    for (descriptor, data) in descriptors.into_iter().zip(segmented) {
        obj.put(element(descriptor, VR::US, words(vec![256, 0, 16])));
        // One linear segment from 0 to 65535 over 256 entries (opcode 1, length, end value), after a discrete start.
        obj.put(element(data, VR::OW, words(vec![0, 1, 0, 1, 255, 65535])));
    }
    put_pixel_data_8(&mut obj, (0..256).map(|i| i as u8).collect());
    write_series(dir, &spec, vec![obj])
}

fn inverse(dir: &Path, study_uid: &str) -> Result<WrittenSeries, FixtureError> {
    let spec = Spec {
        number: 42,
        modality: "DX",
        description: "INVERSE PRESENTATION (synthetic)",
        sop_class: uids::DIGITAL_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
    };
    let mut obj = spec.object(study_uid, 1);
    put_pixels(&mut obj, (48, 48), "MONOCHROME2", (8, 8), false);
    put_str(&mut obj, tags::PRESENTATION_LUT_SHAPE, VR::CS, &["INVERSE"]);
    put_pixel_data_8(&mut obj, gradient(48));
    write_series(dir, &spec, vec![obj])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_twelve_series_of_one_case_each() {
        let dir = std::env::temp_dir().join(format!("summrise-testkit-fidelity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let study = write_fidelity_study(&dir).expect("fixture");
        assert_eq!(study.series.len(), SERIES);
        assert!(study.series.iter().all(|s| s.files.len() == if s.number == 35 { 4 } else { 1 }));
        assert!(study.series.windows(2).all(|w| w[0].number < w[1].number), "index order is number order");
        // The tables and the pixels the tests rely on.
        assert_eq!((normal_entry(100), harder_entry(4095), modality_entry(100)), (640, 4095, 625));
        assert_eq!(embedded_stored(5, 5), 1324);
        assert!(embedded_overlay(5, 30) && !embedded_overlay(6, 6));
        assert!(padded_ct_is_padding(0, 0) && !padded_ct_is_padding(32, 32));
        assert_eq!(pack_bits([true, false, false, false, false, false, false, true, true]), [0x81, 0x01]);
        let bytes = fs::read(&study.series[OVERLAY].files[0]).expect("read");
        assert_eq!(&bytes[128..132], b"DICM");
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}
