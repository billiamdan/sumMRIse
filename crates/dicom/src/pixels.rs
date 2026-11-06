//! From decoded pixel data to values: stored values masked to Bits Stored,
//! the modality transform (Rescale Slope/Intercept or the Modality LUT
//! Sequence), pixel padding, palette colour and the contrast tables of the
//! VOI LUT Sequence.
//!
//! `dicom-pixeldata` decompresses; everything from the decoded sample bytes on
//! happens here, so that bits outside Bits Stored (overlay bits of the retired
//! form, garbage) never reach a value and padding is found on stored values
//! (`openspec/changes/display-fidelity/design.md`, D2, D3, D5).

use std::sync::Arc;

use dicom_core::value::{PrimitiveValue, Value};
use dicom_core::{Tag, VR};
use dicom_dictionary_std::tags;
use dicom_object::mem::InMemElement;
use dicom_object::{DefaultDicomObject, InMemDicomObject};
use dicom_pixeldata::{DecodedPixelData, PhotometricInterpretation, PixelRepresentation};
use log::debug;
use summrise_i18n::tr;
use summrise_imaging::{
    Frame, FramePixels, GraphicsPlane, LookupTable, LutError, Padding, Palette, VoiLut, VoxelBuffer, auto_window,
};

use crate::attrs;
use crate::graphics::Overlays;

/// What decoding found in one file besides the values of its frames:
/// contrast tables, padding and scanner graphics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstanceExtras {
    /// The contrast tables of the VOI LUT Sequence, in file order.
    pub voi_luts: Vec<VoiLut>,
    /// One entry per frame, in frame order (empty for files decoded before
    /// these were known, which have neither).
    pub frames: Vec<FrameExtras>,
}

impl InstanceExtras {
    /// What frame `frame` has besides its values.
    pub fn frame(&self, frame: usize) -> Option<&FrameExtras> {
        self.frames.get(frame)
    }

    /// The padding of frame `frame`.
    pub fn padding(&self, frame: usize) -> Option<Padding> {
        self.frame(frame).and_then(|f| f.padding)
    }
}

/// What one frame has besides its values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameExtras {
    /// The values that are padding (see `summrise_imaging::padding`).
    pub padding: Option<Padding>,
    /// The scanner graphics drawn over the frame, shared between frames when
    /// one plane applies to several.
    pub graphics: Vec<Arc<GraphicsPlane>>,
}

/// The decoded frames of one file and what goes with them.
pub(crate) struct Decoded {
    pub(crate) frames: Vec<Frame>,
    pub(crate) extras: InstanceExtras,
    /// Shown inverted: MONOCHROME1, or Presentation LUT Shape INVERSE.
    pub(crate) inverted: bool,
}

/// Decodes the frames of `obj` from the pixel data `decoded`: grey-scale
/// frames to modality values (masked stored values through the modality
/// transform, padding found), palette and RGB/YBR frames to 8-bit RGB, and
/// the scanner graphics of every frame. `rgb_frame` converts a colour frame.
pub(crate) fn decode_frames(
    obj: &DefaultDicomObject,
    decoded: &DecodedPixelData<'_>,
    rgb_frame: impl Fn(&DecodedPixelData<'_>, u32) -> Result<Vec<u8>, String>,
) -> Result<Decoded, String> {
    let base: &InMemDicomObject = obj;
    let pi = decoded.photometric_interpretation().clone();
    let spp = decoded.samples_per_pixel();
    let (width, height) = (decoded.columns(), decoded.rows());
    let encapsulated = base.get(tags::PIXEL_DATA).is_some_and(|e| matches!(e.value(), Value::PixelSequence(_)));
    let layout = Layout::of(decoded, encapsulated);
    let overlays = Overlays::read(obj, &layout, spp);
    let count = decoded.number_of_frames();
    let mut frames = Vec::with_capacity(count as usize);
    let mut extras = Vec::with_capacity(count as usize);
    let mut voi_luts = Vec::new();
    let grey = matches!(pi, PhotometricInterpretation::Monochrome1 | PhotometricInterpretation::Monochrome2);
    match (spp, &pi) {
        (1, _) if grey => {
            let transform = ModalityTransform::read(base, decoded, layout.signed)?;
            let padding = padding_values(base, &layout);
            for f in 0..count {
                let raw = layout.raw(decoded, f)?;
                let (values, padding) = grey_frame(&layout, &raw, &transform, f, padding);
                frames.push(Frame { width, height, pixels: FramePixels::Gray(values) });
                extras.push(FrameExtras { padding, graphics: overlays.for_frame(f, &raw, width, height) });
            }
            voi_luts = voi_luts_of(base, layout.signed || transform.can_be_negative(&layout));
        }
        (1, PhotometricInterpretation::PaletteColor) => {
            let palette = read_palette(base)?;
            for f in 0..count {
                let raw = layout.raw(decoded, f)?;
                let rgb = palette.to_rgb(raw.iter().map(|&sample| i64::from(layout.stored(sample))));
                frames.push(Frame { width, height, pixels: FramePixels::Rgb(rgb) });
                extras.push(FrameExtras { padding: None, graphics: overlays.for_frame(f, &raw, width, height) });
            }
        }
        (3, _) => {
            for f in 0..count {
                frames.push(Frame { width, height, pixels: FramePixels::Rgb(rgb_frame(decoded, f)?) });
                extras.push(FrameExtras { padding: None, graphics: overlays.for_frame(f, &[], width, height) });
            }
        }
        // Translators: {kind} is a DICOM photometric interpretation, such as PALETTE COLOR.
        (_, other) => return Err(tr!("{kind} images are not supported yet", kind = other)),
    }
    if overlays.is_empty() && extras.iter().all(|e| e.padding.is_none()) {
        extras.clear();
    }
    let inverted = pi == PhotometricInterpretation::Monochrome1 || (grey && crate::fidelity::is_inverse(base));
    Ok(Decoded { frames, extras: InstanceExtras { voi_luts, frames: extras }, inverted })
}

/// An automatic window from the values of `frame` that are not padding.
pub(crate) fn auto_window_of(frame: &Frame, padding: Option<Padding>) -> Option<summrise_imaging::Window> {
    match &frame.pixels {
        FramePixels::Gray(b) => auto_window(summrise_imaging::window::subsample(b.len(), 65_536, |i| {
            summrise_imaging::padding::visible(b.get(i), padding)
        })),
        FramePixels::Rgb(_) => None,
    }
}

/// Where the stored value sits in a sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) bits_allocated: u16,
    pub(crate) bits_stored: u16,
    /// Right shift that brings the stored bits to the bottom: High Bit + 1 −
    /// Bits Stored for native data; encapsulated data is decoded right-aligned.
    pub(crate) shift: u16,
    pub(crate) signed: bool,
}

impl Layout {
    pub(crate) fn of(decoded: &DecodedPixelData<'_>, encapsulated: bool) -> Self {
        let allocated = decoded.bits_allocated();
        // 1-bit samples arrive expanded to bytes; anything else keeps its size.
        let room = allocated.clamp(1, 16);
        let stored = decoded.bits_stored().clamp(1, room);
        let shift = if encapsulated { 0 } else { (decoded.high_bit() + 1).saturating_sub(stored) };
        // A High Bit beyond the allocated bits is malformed: the stored bits are taken from the bottom.
        let shift = if shift + stored > room { 0 } else { shift };
        Self {
            bits_allocated: allocated,
            bits_stored: stored,
            shift,
            signed: decoded.pixel_representation() == PixelRepresentation::Signed,
        }
    }

    /// The samples of frame `frame` as they are in the decoded data.
    pub(crate) fn raw(&self, decoded: &DecodedPixelData<'_>, frame: u32) -> Result<Vec<u16>, String> {
        let data = decoded.frame_data(frame).map_err(|e| e.to_string())?;
        match self.bits_allocated {
            1 | 8 => Ok(data.iter().map(|&b| u16::from(b)).collect()),
            16 => Ok(data.as_chunks::<2>().0.iter().map(|b| u16::from_ne_bytes(*b)).collect()),
            bits => Err(tr!("{bits}-bit grey-scale images are not supported yet", bits = bits)),
        }
    }

    /// The stored value of a raw sample: its Bits Stored, sign-extended when
    /// Pixel Representation is 1.
    #[inline]
    pub(crate) fn stored(&self, raw: u16) -> i32 {
        let bits = u32::from(self.bits_stored);
        let value = (u32::from(raw) >> self.shift) & ((1u32 << bits) - 1);
        if self.signed && (value >> (bits - 1)) & 1 == 1 { value as i32 - (1i32 << bits) } else { value as i32 }
    }

    /// The smallest and largest stored value possible.
    pub(crate) fn range(&self) -> (i32, i32) {
        let bits = u32::from(self.bits_stored);
        if self.signed { (-(1i32 << (bits - 1)), (1i32 << (bits - 1)) - 1) } else { (0, (1i32 << bits) - 1) }
    }
}

/// How stored values become modality values (PS3.3 C.11.1).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ModalityTransform {
    /// Rescale Slope and Intercept: one pair for all frames, or one per frame.
    Rescale(Vec<(f64, f64)>),
    /// The table of the Modality LUT Sequence.
    Table(LookupTable),
}

impl ModalityTransform {
    /// The Modality LUT Sequence when present (it replaces rescaling), else
    /// the rescale values the pixel decoder found.
    pub(crate) fn read(obj: &InMemDicomObject, decoded: &DecodedPixelData<'_>, signed: bool) -> Result<Self, String> {
        if let Some(item) = modality_lut_item(obj) {
            return table_of_item(item, signed)
                .map(Self::Table)
                .map_err(|reason| tr!("The modality table of this image cannot be used: {reason}", reason = reason));
        }
        let rescale = decoded.rescale().map_err(|e| e.to_string())?;
        Ok(Self::Rescale(rescale.iter().map(|r| (r.slope, r.intercept)).collect()))
    }

    /// The modality value of stored value `stored` in frame `frame`.
    #[inline]
    pub(crate) fn value(&self, frame: u32, stored: i32) -> f32 {
        match self {
            Self::Rescale(pairs) => {
                let (slope, intercept) = pairs.get(frame as usize).or(pairs.first()).copied().unwrap_or((1.0, 0.0));
                (f64::from(stored) * slope + intercept) as f32
            }
            Self::Table(table) => f32::from(table.entry(i64::from(stored))),
        }
    }

    /// Whether modality values can be negative (for reading the first value
    /// mapped of a contrast table stored as unsigned).
    pub(crate) fn can_be_negative(&self, layout: &Layout) -> bool {
        match self {
            Self::Rescale(pairs) => {
                let (lo, hi) = layout.range();
                pairs.iter().any(|&(m, b)| f64::from(lo) * m + b < 0.0 || f64::from(hi) * m + b < 0.0)
            }
            Self::Table(_) => false,
        }
    }
}

/// The first item of the Modality LUT Sequence, if the image has one.
pub(crate) fn modality_lut_item(obj: &InMemDicomObject) -> Option<&InMemDicomObject> {
    attrs::first_item(obj, tags::MODALITY_LUT_SEQUENCE)
}

/// The stored values that are padding (PS3.3 C.7.5.1.1.2): Pixel Padding
/// Value, up to Pixel Padding Range Limit when present. `None` when the
/// image has no padding, or its padding value lies outside the stored range.
pub(crate) fn padding_values(obj: &InMemDicomObject, layout: &Layout) -> Option<(i32, i32)> {
    let value = stored_int(obj, tags::PIXEL_PADDING_VALUE, layout.signed)?;
    let limit = stored_int(obj, tags::PIXEL_PADDING_RANGE_LIMIT, layout.signed).unwrap_or(value);
    let (low, high) = (value.min(limit), value.max(limit));
    let (min, max) = layout.range();
    if high < min || low > max {
        debug!("pixel padding value {value} lies outside the stored values {min}..={max}");
        return None;
    }
    Some((low, high))
}

/// An integer attribute in the representation of the stored values: values
/// read as US for signed data are taken as 16-bit two's complement.
fn stored_int(obj: &InMemDicomObject, tag: Tag, signed: bool) -> Option<i32> {
    let element = obj.get(tag)?;
    let value = element.to_int::<i32>().ok()?;
    Some(if signed && element.vr() == VR::US && value > 32767 { value - 65536 } else { value })
}

/// The modality values of one grey-scale frame from its raw samples, and
/// its padding: the interval of the padding pixels' values, which no other
/// pixel of the frame has. When another pixel shares a padding value (a
/// Modality LUT that is not monotonic), the padding pixels become `NaN`
/// instead, in a 32-bit float buffer.
pub(crate) fn grey_frame(
    layout: &Layout,
    raw: &[u16],
    transform: &ModalityTransform,
    frame: u32,
    padding: Option<(i32, i32)>,
) -> (VoxelBuffer, Option<Padding>) {
    let is_padding = |stored: i32| padding.is_some_and(|(low, high)| (low..=high).contains(&stored));
    let mut values = Vec::with_capacity(raw.len());
    let mut range: Option<(f32, f32)> = None;
    for &sample in raw {
        let stored = layout.stored(sample);
        let value = transform.value(frame, stored);
        if is_padding(stored) {
            range = Some(range.map_or((value, value), |(lo, hi)| (lo.min(value), hi.max(value))));
        }
        values.push(value);
    }
    let Some((low, high)) = range else {
        return (VoxelBuffer::compact(values), None);
    };
    let padding_range = Padding::new(low, high);
    let clash = raw.iter().zip(&values).any(|(&r, &v)| !is_padding(layout.stored(r)) && padding_range.contains(v));
    if clash {
        debug!("padding values are also image values; padding is kept as NaN");
        for (&r, v) in raw.iter().zip(values.iter_mut()) {
            if is_padding(layout.stored(r)) {
                *v = f32::NAN;
            }
        }
        return (VoxelBuffer::compact(values), None);
    }
    (VoxelBuffer::compact(values), Some(padding_range))
}

// ---------------------------------------------------------------------------
// Lookup tables

/// A lookup table from its LUT Descriptor and LUT Data elements (D13):
/// entries (0 means 65536), first value mapped (read as signed when the
/// element is SS, or when `signed` and the value exceeds 32767), bits per
/// entry; data as 16-bit words or bytes, 8-bit entries also packed two per word.
pub(crate) fn table(descriptor: &InMemElement, data: &InMemElement, signed: bool) -> Result<LookupTable, String> {
    let values = descriptor.to_multi_int::<i64>().map_err(|e| e.to_string())?;
    let [entries, first, bits] = values
        .get(..3)
        .and_then(|v| <[i64; 3]>::try_from(v).ok())
        .ok_or_else(|| tr!("the table descriptor has {count} values instead of 3", count = values.len()))?;
    let entries = match entries.rem_euclid(65536) {
        0 => 65536,
        n => n as usize,
    };
    let first = if descriptor.vr() != VR::SS && signed && first > 32767 { first - 65536 } else { first };
    let bits = bits.rem_euclid(65536) as u16;
    let words = entries_of(data, entries, bits)?;
    LookupTable::new(first, bits, words).map_err(|e| match e {
        LutError::Empty => tr!("the table has no entries").to_owned(),
        LutError::Bits(bits) => tr!("the table has {bits} bits per entry; 1 to 16 are possible", bits = bits),
    })
}

/// The table of an item that holds LUT Descriptor and LUT Data (Modality
/// and VOI LUT Sequences).
pub(crate) fn table_of_item(item: &InMemDicomObject, signed: bool) -> Result<LookupTable, String> {
    let descriptor = item.get(tags::LUT_DESCRIPTOR).ok_or_else(|| tr!("the table descriptor is missing").to_owned())?;
    let data = item.get(tags::LUT_DATA).ok_or_else(|| tr!("the table data is missing").to_owned())?;
    table(descriptor, data, signed)
}

/// The entries of LUT Data for a table of `entries` entries of `bits` bits.
fn entries_of(data: &InMemElement, entries: usize, bits: u16) -> Result<Vec<u16>, String> {
    let mismatch = |found: usize| {
        Err(tr!("the table holds {found} values instead of {expected}", found = found, expected = entries))
    };
    match data.value() {
        Value::Primitive(PrimitiveValue::U16(words)) => {
            words_to_entries(words, entries, bits).map_or_else(|| mismatch(words.len()), Ok)
        }
        Value::Primitive(PrimitiveValue::I16(words)) => {
            let words: Vec<u16> = words.iter().map(|&w| w as u16).collect();
            words_to_entries(&words, entries, bits).map_or_else(|| mismatch(words.len()), Ok)
        }
        Value::Primitive(PrimitiveValue::U8(bytes)) => {
            if bits <= 8 && (bytes.len() == entries || bytes.len() == entries + 1) {
                Ok(bytes[..entries].iter().map(|&b| u16::from(b)).collect())
            } else if bytes.len() == 2 * entries {
                Ok(bytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect())
            } else {
                mismatch(bytes.len())
            }
        }
        _ => Err(tr!("the table data is not a list of numbers").to_owned()),
    }
}

/// Table entries from 16-bit words: one entry per word (8-bit entries in the
/// high byte recognised), or 8-bit entries packed two per word, low byte first.
fn words_to_entries(words: &[u16], entries: usize, bits: u16) -> Option<Vec<u16>> {
    if words.len() == entries {
        let high_bytes = bits <= 8 && words.iter().any(|&w| w > 255) && words.iter().all(|&w| w & 0xFF == 0);
        return Some(if high_bytes { words.iter().map(|&w| w >> 8).collect() } else { words.to_vec() });
    }
    if bits <= 8 && words.len() == entries.div_ceil(2) {
        return Some(words.iter().flat_map(|w| w.to_le_bytes()).take(entries).map(u16::from).collect());
    }
    None
}

// ---------------------------------------------------------------------------
// Palette colour and contrast tables

/// The red, green and blue tables of a palette colour image.
pub(crate) fn read_palette(obj: &InMemDicomObject) -> Result<Palette, String> {
    let channel = |descriptor: Tag, data: Tag, segmented: Tag| -> Result<LookupTable, String> {
        match (obj.get(descriptor), obj.get(data)) {
            (Some(descriptor), Some(data)) => table(descriptor, data, false),
            (_, None) if obj.get(segmented).is_some() => {
                Err(tr!("Segmented palette colour images are not supported yet").to_owned())
            }
            _ => Err(tr!("The colour table of this palette colour image is missing").to_owned()),
        }
    };
    Ok(Palette {
        red: channel(
            tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
            tags::RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            tags::SEGMENTED_RED_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        )?,
        green: channel(
            tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
            tags::GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            tags::SEGMENTED_GREEN_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        )?,
        blue: channel(
            tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DESCRIPTOR,
            tags::BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
            tags::SEGMENTED_BLUE_PALETTE_COLOR_LOOKUP_TABLE_DATA,
        )?,
    })
}

/// The contrast tables of the VOI LUT Sequence (top level, else in the
/// functional groups of an Enhanced object), named by their LUT Explanation.
/// A malformed table is left out. `signed`: modality values can be negative.
pub(crate) fn voi_luts_of(obj: &InMemDicomObject, signed: bool) -> Vec<VoiLut> {
    let Some(items) = attrs::classic_or_functional(obj, tags::FRAME_VOILUT_SEQUENCE)
        .find_map(|holder| holder.get(tags::VOILUT_SEQUENCE).and_then(|e| e.items()))
    else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let table =
                table_of_item(item, signed).map_err(|e| debug!("contrast table {} left out: {e}", i + 1)).ok()?;
            let name = attrs::text(item, tags::LUT_EXPLANATION)
                .map(|text| crate::decode::title_case(&text))
                .unwrap_or_else(|| tr!("Contrast table {number}", number = i + 1));
            Some(VoiLut { name, table })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use dicom_core::DataElement;

    use super::*;

    fn layout(bits_allocated: u16, bits_stored: u16, shift: u16, signed: bool) -> Layout {
        Layout { bits_allocated, bits_stored, shift, signed }
    }

    #[test]
    fn stored_values_are_masked_and_sign_extended() {
        // 12 of 16 bits: an overlay bit (12) and garbage (13–15) never change the value.
        let ct = layout(16, 12, 0, false);
        assert_eq!(ct.stored(0xF52C), 1324);
        assert_eq!(ct.stored(0x052C), 1324);
        assert_eq!(ct.stored(0x1FFF), 4095);
        // Signed 12 bits: 0x0FFF is −1, 0x0800 the smallest value.
        let signed = layout(16, 12, 0, true);
        assert_eq!((signed.stored(0x0FFF), signed.stored(0xF800), signed.stored(0x07FF)), (-1, -2048, 2047));
        assert_eq!(signed.range(), (-2048, 2047));
        // Signed 16 bits, and the stored bits at the top of the sample (High Bit 15, 12 bits stored).
        assert_eq!(layout(16, 16, 0, true).stored(0xF830), -2000);
        assert_eq!(layout(16, 12, 4, false).stored(0x52C0 | 0x000F), 1324);
        // 1-bit samples arrive expanded to 0 or 255.
        assert_eq!((layout(1, 1, 0, false).stored(255), layout(1, 1, 0, false).stored(0)), (1, 0));
        // 8 bits with 6 stored: the two top bits are masked away.
        assert_eq!(layout(8, 6, 0, false).stored(0xC5), 5);
    }

    #[test]
    fn padding_is_an_interval_of_values_no_other_pixel_has() {
        let ct = layout(16, 16, 0, true);
        let rescale = ModalityTransform::Rescale(vec![(1.0, -1024.0)]);
        let raw: Vec<u16> = [-2000i16, 24, 1064, 1724, -2000].iter().map(|&v| v as u16).collect();
        let (values, padding) = grey_frame(&ct, &raw, &rescale, 0, Some((-2000, -2000)));
        assert_eq!(padding, Some(Padding::new(-3024.0, -3024.0)));
        assert!(matches!(values, VoxelBuffer::I16(_)), "no extra memory");
        assert_eq!((values.get(0), values.get(1), values.get(3)), (-3024.0, -1000.0, 700.0));
        // No padding pixel in this frame: no padding.
        let (_, none) = grey_frame(&ct, &raw[1..4], &rescale, 0, Some((-2000, -2000)));
        assert_eq!(none, None);
        // A table that maps padding and an image value to the same output: padding becomes NaN.
        let table = LookupTable::new(0, 16, vec![7, 7, 9, 11]).expect("table");
        let flat = layout(16, 12, 0, false);
        let (values, padding) = grey_frame(&flat, &[0, 1, 2, 3], &ModalityTransform::Table(table), 0, Some((0, 0)));
        assert_eq!(padding, None);
        assert!(values.get(0).is_nan() && values.get(1) == 7.0 && values.get(3) == 11.0);
    }

    #[test]
    fn padding_values_are_read_in_the_representation_of_the_stored_values() {
        let signed = layout(16, 16, 0, true);
        let obj = |elements: Vec<DataElement<InMemDicomObject>>| InMemDicomObject::from_element_iter(elements);
        let ss = DataElement::new(tags::PIXEL_PADDING_VALUE, VR::SS, PrimitiveValue::I16(vec![-2000].into()));
        assert_eq!(padding_values(&obj(vec![ss]), &signed), Some((-2000, -2000)));
        // Read as US (implicit VR before Pixel Representation was known): two's complement.
        let us = DataElement::new(tags::PIXEL_PADDING_VALUE, VR::US, PrimitiveValue::U16(vec![63536].into()));
        assert_eq!(padding_values(&obj(vec![us.clone()]), &signed), Some((-2000, -2000)));
        // A range in either order.
        let limit = DataElement::new(tags::PIXEL_PADDING_RANGE_LIMIT, VR::US, PrimitiveValue::U16(vec![0].into()));
        let ten = DataElement::new(tags::PIXEL_PADDING_VALUE, VR::US, PrimitiveValue::U16(vec![10].into()));
        assert_eq!(padding_values(&obj(vec![ten, limit]), &layout(16, 12, 0, false)), Some((0, 10)));
        // Outside the stored values: nothing can match.
        assert_eq!(padding_values(&obj(vec![us]), &layout(16, 12, 0, false)), None);
        assert_eq!(padding_values(&obj(Vec::new()), &signed), None);
    }

    fn element(tag: Tag, vr: VR, value: PrimitiveValue) -> InMemElement {
        DataElement::new(tag, vr, value)
    }

    fn descriptor(values: [u16; 3]) -> InMemElement {
        element(tags::LUT_DESCRIPTOR, VR::US, PrimitiveValue::U16(values.to_vec().into()))
    }

    #[test]
    fn tables_are_read_in_every_form_the_standard_and_real_files_use() {
        let data = |words: Vec<u16>| element(tags::LUT_DATA, VR::OW, PrimitiveValue::U16(words.into()));
        // One entry per word.
        let t = table(&descriptor([4, 0, 12]), &data(vec![1, 2, 3, 4]), false).expect("table");
        assert_eq!((t.entries(), t.first(), t.bits()), (&[1, 2, 3, 4][..], 0, 12));
        // 8-bit entries packed two per word, low byte first (PS3.3 C.7.6.3.1.5).
        let t = table(&descriptor([4, 0, 8]), &data(vec![0x0201, 0x0403]), false).expect("table");
        assert_eq!(t.entries(), [1, 2, 3, 4]);
        // 8-bit entries in the high byte of each word.
        let t = table(&descriptor([2, 0, 8]), &data(vec![0x0100, 0xFF00]), false).expect("table");
        assert_eq!(t.entries(), [1, 255]);
        // 65536 entries: the descriptor says 0.
        let t = table(&descriptor([0, 0, 16]), &data(vec![9; 65536]), false).expect("table");
        assert_eq!(t.len(), 65536);
        // A signed first value stored as US, and the same stored as SS.
        let t = table(&descriptor([2, 64512, 16]), &data(vec![0, 1]), true).expect("table");
        assert_eq!(t.first(), -1024);
        let ss = element(tags::LUT_DESCRIPTOR, VR::SS, PrimitiveValue::I16(vec![2, -1024, 16].into()));
        assert_eq!(table(&ss, &data(vec![0, 1]), false).expect("table").first(), -1024);
        // Bytes: one per entry, or little-endian words.
        let bytes = |b: Vec<u8>| element(tags::LUT_DATA, VR::OB, PrimitiveValue::U8(b.into()));
        assert_eq!(table(&descriptor([3, 0, 8]), &bytes(vec![5, 6, 7, 0]), false).expect("t").entries(), [5, 6, 7]);
        assert_eq!(table(&descriptor([2, 0, 16]), &bytes(vec![1, 1, 0, 2]), false).expect("t").entries(), [257, 512]);
        // Malformed: wrong length, wrong descriptor.
        assert!(table(&descriptor([5, 0, 16]), &data(vec![1, 2]), false).is_err());
        let short = element(tags::LUT_DESCRIPTOR, VR::US, PrimitiveValue::U16(vec![2, 0].into()));
        assert!(table(&short, &data(vec![1, 2]), false).is_err());
        assert!(table(&descriptor([2, 0, 0]), &data(vec![1, 2]), false).is_err());
    }
}
