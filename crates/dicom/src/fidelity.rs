//! Facts about how faithfully the stored picture shows the acquisition, read
//! with the headers so that they are known before any pixel is decoded:
//! lossy compression, text burned into the picture, an inverted presentation,
//! and the output range of a Modality LUT
//! (`openspec/changes/display-fidelity/design.md`, D7, D8, D10, D12).

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use summrise_i18n::tr;

use crate::attrs;
use crate::format;

/// What the header of one image says about its fidelity.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fidelity {
    /// Set when the image is or was compressed with loss.
    pub lossy: Option<LossyCompression>,
    /// Burned In Annotation (0028,0301) is YES: text such as patient details
    /// may be part of the picture itself.
    pub burned_in_text: bool,
    /// Presentation LUT Shape (2050,0020) is INVERSE: the image is meant to be
    /// shown inverted, like MONOCHROME1.
    pub inverse: bool,
    /// The smallest and largest output of the Modality LUT Sequence, when the
    /// image has one (it replaces rescaling).
    pub modality_lut_range: Option<(f64, f64)>,
}

/// Lossy compression of an image: what the file records, or what its
/// transfer syntax implies.
#[derive(Debug, Clone, PartialEq)]
pub struct LossyCompression {
    /// Lossy Image Compression Ratio values (e.g. 10 for 10:1).
    pub ratios: Vec<f64>,
    /// Names of the methods (e.g. "JPEG"); unknown method codes as recorded.
    pub methods: Vec<String>,
}

impl LossyCompression {
    /// The lossy compression of an image with the attributes of `obj`, stored
    /// in `transfer_syntax`; `None` when it is not known to be lossy.
    pub fn of(obj: &InMemDicomObject, transfer_syntax: &str) -> Option<Self> {
        let flagged = attrs::text(obj, tags::LOSSY_IMAGE_COMPRESSION).as_deref() == Some("01");
        let family = format::lossy_family(transfer_syntax);
        if !flagged && family.is_none() {
            return None;
        }
        let ratios = attrs::floats(obj, tags::LOSSY_IMAGE_COMPRESSION_RATIO)
            .into_iter()
            .filter(|r| r.is_finite() && *r > 0.0)
            .collect();
        let mut methods: Vec<String> = attrs::texts(obj, tags::LOSSY_IMAGE_COMPRESSION_METHOD)
            .into_iter()
            .filter(|code| !code.is_empty())
            .map(|code| method_name(&code).map_or(code, str::to_owned))
            .collect();
        if methods.is_empty() {
            methods.extend(family.map(str::to_owned));
        }
        Some(Self { ratios, methods })
    }

    /// "Lossy 10:1 (JPEG)", "Lossy 10:1", "Lossy (JPEG)" or "Lossy compression".
    pub fn label(&self) -> String {
        let ratio = self.ratios.iter().map(|r| format!("{}:1", format::number(*r, 1))).collect::<Vec<_>>().join(", ");
        let method = self.methods.join(", ");
        match (ratio.is_empty(), method.is_empty()) {
            (false, false) => tr!("Lossy {ratio} ({method})", ratio = ratio, method = method),
            (false, true) => tr!("Lossy {ratio}", ratio = ratio),
            (true, false) => tr!("Lossy ({method})", method = method),
            (true, true) => tr!("Lossy compression").to_owned(),
        }
    }
}

/// The name of a Lossy Image Compression Method code (PS3.3 C.7.6.1.1.5.1).
pub fn method_name(code: &str) -> Option<&'static str> {
    Some(match code.trim() {
        "ISO_10918_1" => "JPEG",
        "ISO_14495_1" => "JPEG-LS",
        "ISO_15444_1" => "JPEG 2000",
        "ISO_15444_15" => "HTJ2K",
        "ISO_13818_2" => "MPEG-2",
        "ISO_14496_10" => "H.264",
        "ISO_23008_2" => "HEVC",
        "ISO_18181_1" => "JPEG XL",
        _ => return None,
    })
}

/// The facts of an image with the attributes of `obj`, stored in
/// `transfer_syntax`; `signed`: Pixel Representation is 1.
pub(crate) fn read(obj: &InMemDicomObject, transfer_syntax: &str, signed: bool) -> Fidelity {
    Fidelity {
        lossy: LossyCompression::of(obj, transfer_syntax),
        burned_in_text: attrs::text(obj, tags::BURNED_IN_ANNOTATION).is_some_and(|v| v.eq_ignore_ascii_case("YES")),
        inverse: is_inverse(obj),
        modality_lut_range: crate::pixels::modality_lut_item(obj)
            .and_then(|item| crate::pixels::table_of_item(item, signed).ok())
            .map(|table| {
                let (lo, hi) = table.output_range();
                (f64::from(lo), f64::from(hi))
            }),
    }
}

/// Presentation LUT Shape is INVERSE (at the top level, or stored flat in the
/// shared functional group of an Enhanced object).
pub(crate) fn is_inverse(obj: &InMemDicomObject) -> bool {
    attrs::text(obj, tags::PRESENTATION_LUT_SHAPE)
        .or_else(|| attrs::shared_group(obj).and_then(|group| attrs::text(group, tags::PRESENTATION_LUT_SHAPE)))
        .is_some_and(|shape| shape.eq_ignore_ascii_case("INVERSE"))
}

/// Modality LUT Type of the Modality LUT Sequence, the unit of its output.
pub(crate) fn modality_lut_type(obj: &InMemDicomObject) -> Option<String> {
    crate::pixels::modality_lut_item(obj).and_then(|item| attrs::text(item, tags::MODALITY_LUT_TYPE))
}

#[cfg(test)]
mod tests {
    use dicom_core::{DataElement, PrimitiveValue, VR};
    use dicom_dictionary_std::uids;

    use super::*;

    fn text(tag: dicom_core::Tag, value: &str) -> DataElement<InMemDicomObject> {
        DataElement::new(tag, VR::CS, PrimitiveValue::from(value))
    }

    #[test]
    fn lossy_facts_come_from_the_attributes_or_the_transfer_syntax() {
        let recorded = InMemDicomObject::from_element_iter([
            text(tags::LOSSY_IMAGE_COMPRESSION, "01"),
            DataElement::new(tags::LOSSY_IMAGE_COMPRESSION_RATIO, VR::DS, PrimitiveValue::from("10")),
            text(tags::LOSSY_IMAGE_COMPRESSION_METHOD, "ISO_10918_1"),
        ]);
        let lossy = LossyCompression::of(&recorded, uids::EXPLICIT_VR_LITTLE_ENDIAN).expect("lossy");
        assert_eq!(lossy.label(), "Lossy 10:1 (JPEG)");
        // A lossy transfer syntax without attributes: its family.
        let none = InMemDicomObject::new_empty();
        let jpeg = LossyCompression::of(&none, uids::JPEG_BASELINE8_BIT).expect("lossy");
        assert_eq!(jpeg.label(), "Lossy (JPEG)");
        // JPEG 2000 may be lossless: lossy only when the file says so.
        assert_eq!(LossyCompression::of(&none, uids::JPEG2000), None);
        assert_eq!(LossyCompression::of(&none, uids::EXPLICIT_VR_LITTLE_ENDIAN), None);
        let said_no = InMemDicomObject::from_element_iter([text(tags::LOSSY_IMAGE_COMPRESSION, "00")]);
        assert_eq!(LossyCompression::of(&said_no, uids::JPEG2000), None);
        // Recompressed twice, an unknown method code, no ratio.
        let twice = InMemDicomObject::from_element_iter([
            text(tags::LOSSY_IMAGE_COMPRESSION, "01"),
            DataElement::new(
                tags::LOSSY_IMAGE_COMPRESSION_RATIO,
                VR::DS,
                PrimitiveValue::Strs(vec!["10".to_owned(), "2.5".to_owned()].into()),
            ),
            DataElement::new(
                tags::LOSSY_IMAGE_COMPRESSION_METHOD,
                VR::CS,
                PrimitiveValue::Strs(vec!["ISO_15444_1".to_owned(), "VENDOR_X".to_owned()].into()),
            ),
        ]);
        let label = LossyCompression::of(&twice, uids::JPEG2000).expect("lossy").label();
        assert_eq!(label, "Lossy 10:1, 2.5:1 (JPEG 2000, VENDOR_X)");
        let bare = LossyCompression { ratios: Vec::new(), methods: Vec::new() };
        assert_eq!(bare.label(), "Lossy compression");
        assert_eq!(LossyCompression { ratios: vec![12.0], methods: Vec::new() }.label(), "Lossy 12:1");
    }

    #[test]
    fn burned_in_text_and_inverse_presentation() {
        let obj = InMemDicomObject::from_element_iter([
            text(tags::BURNED_IN_ANNOTATION, "YES"),
            text(tags::PRESENTATION_LUT_SHAPE, "INVERSE"),
        ]);
        let facts = read(&obj, uids::EXPLICIT_VR_LITTLE_ENDIAN, false);
        assert!(facts.burned_in_text && facts.inverse && facts.lossy.is_none() && facts.modality_lut_range.is_none());
        let plain = InMemDicomObject::from_element_iter([
            text(tags::BURNED_IN_ANNOTATION, "NO"),
            text(tags::PRESENTATION_LUT_SHAPE, "IDENTITY"),
        ]);
        assert_eq!(read(&plain, uids::EXPLICIT_VR_LITTLE_ENDIAN, false), Fidelity::default());
        assert_eq!(method_name("ISO_14495_1"), Some("JPEG-LS"));
        assert_eq!(method_name("SOMETHING"), None);
    }
}
