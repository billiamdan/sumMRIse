//! Metadata for the inspector: a complete, labelled attribute tree and a
//! curated, human-readable summary grouped by clinical topic.

use std::path::Path;

use dicom_core::dictionary::{DataDictionary, DataDictionaryEntry};
use dicom_core::value::{PrimitiveValue, Value};
use dicom_core::{Tag, VR};
use dicom_dictionary_std::{StandardDataDictionary, tags};
use dicom_object::InMemDicomObject;
use summrise_i18n::{tr, tr_ctx, tr_n};
use summrise_imaging::{AnatomicalPlane, DVec3};

use crate::attrs;
use crate::error::{DicomError, Result};
use crate::format;
use crate::media::Media;

/// Maximum characters shown for a single value.
const MAX_VALUE_CHARS: usize = 512;

/// One attribute (data element) of a DICOM object.
#[derive(Debug, Clone, PartialEq)]
pub struct TagNode {
    pub group: u16,
    pub element: u16,
    /// Dictionary keyword, e.g. `RepetitionTime`.
    pub keyword: Option<String>,
    /// Human-readable name, e.g. `Repetition Time`.
    pub name: String,
    pub vr: String,
    pub value: String,
    /// Sequence items (each a list of nested attributes).
    pub items: Vec<Vec<TagNode>>,
}

impl TagNode {
    pub fn tag_label(&self) -> String {
        format!("({:04X},{:04X})", self.group, self.element)
    }

    pub fn is_private(&self) -> bool {
        self.group % 2 == 1
    }

    /// Case-insensitive match against tag number, name, keyword or value
    /// (including nested sequence content).
    pub fn matches(&self, needle_lower: &str) -> bool {
        needle_lower.is_empty()
            || self.tag_label().to_lowercase().contains(needle_lower)
            || format!("{:04x}{:04x}", self.group, self.element).contains(needle_lower)
            || self.name.to_lowercase().contains(needle_lower)
            || self.keyword.as_deref().is_some_and(|k| k.to_lowercase().contains(needle_lower))
            || self.value.to_lowercase().contains(needle_lower)
            || self.items.iter().flatten().any(|n| n.matches(needle_lower))
    }
}

/// One row of a summary section.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryRow {
    pub label: String,
    pub value: String,
    /// Stable key for explanations: the keyword of the DICOM attribute the
    /// row shows (e.g. `RepetitionTime`), independent of the label and of
    /// its translation. Rows computed from several attributes carry the
    /// keyword of their main attribute, or `Matrix` / `FieldOfView`.
    pub key: Option<&'static str>,
}

/// A labelled group of summary rows.
#[derive(Debug, Clone, PartialEq)]
pub struct SummarySection {
    /// Stable name of the section, independent of the title and of its
    /// translation: `Patient`, `Study`, `Series`, `Equipment`,
    /// `Acquisition`, `Image`, `Identifiers` (and `Uptake` for the uptake
    /// values of a PET series). Privacy mode hides the section `Patient`.
    pub key: &'static str,
    /// The title shown, in the language of the interface.
    pub title: &'static str,
    pub rows: Vec<SummaryRow>,
}

#[derive(Debug, Clone)]
pub struct InstanceMetadata {
    /// File Meta Information (group 0002).
    pub file_meta: Vec<TagNode>,
    pub dataset: Vec<TagNode>,
    pub summary: Vec<SummarySection>,
}

impl InstanceMetadata {
    pub fn attribute_count(&self) -> usize {
        fn count(nodes: &[TagNode]) -> usize {
            nodes.iter().map(|n| 1 + n.items.iter().map(|i| count(i)).sum::<usize>()).sum()
        }
        count(&self.file_meta) + count(&self.dataset)
    }
}

/// Reads all attributes of one file in `media`, the media of the index the
/// file is from (pixel data is summarised, not loaded).
pub fn read_metadata(media: &Media, path: &Path) -> Result<InstanceMetadata> {
    let file = media
        .open_object(path, Some(tags::PIXEL_DATA))
        .map_err(|reason| DicomError::Parse { path: media.display_path(path), reason })?;
    let meta = file.meta();
    let mut file_meta = vec![
        simple_node(
            tags::MEDIA_STORAGE_SOP_CLASS_UID,
            "UI",
            &format::sop_class_name(meta.media_storage_sop_class_uid()),
        ),
        simple_node(tags::MEDIA_STORAGE_SOP_INSTANCE_UID, "UI", meta.media_storage_sop_instance_uid()),
        simple_node(
            tags::TRANSFER_SYNTAX_UID,
            "UI",
            &format!(
                "{} ({})",
                format::transfer_syntax_name(meta.transfer_syntax()),
                meta.transfer_syntax().trim_end_matches('\0')
            ),
        ),
        simple_node(tags::IMPLEMENTATION_CLASS_UID, "UI", &meta.implementation_class_uid),
    ];
    if let Some(name) = &meta.implementation_version_name {
        file_meta.push(simple_node(tags::IMPLEMENTATION_VERSION_NAME, "SH", name));
    }
    let obj: &InMemDicomObject = &file;
    let dataset = nodes(obj);
    let summary = summarize(obj, meta.transfer_syntax());
    Ok(InstanceMetadata { file_meta, dataset, summary })
}

fn simple_node(tag: Tag, vr: &str, value: &str) -> TagNode {
    let keyword = StandardDataDictionary.by_tag(tag).map(|e| e.alias().to_owned());
    TagNode {
        group: tag.group(),
        element: tag.element(),
        name: keyword.as_deref().map(format::keyword_to_words).unwrap_or_default(),
        keyword,
        vr: vr.to_owned(),
        value: value.trim_end_matches('\0').to_owned(),
        items: Vec::new(),
    }
}

fn nodes(obj: &InMemDicomObject) -> Vec<TagNode> {
    obj.iter()
        .map(|elem| {
            let tag = elem.header().tag;
            let vr = elem.vr();
            let keyword = StandardDataDictionary.by_tag(tag).map(|e| e.alias().to_owned());
            let name = match (&keyword, tag.group() % 2 == 1) {
                (Some(k), _) => format::keyword_to_words(k),
                (None, true) if (0x0010..=0x00FF).contains(&tag.element()) => "Private Creator".to_owned(),
                (None, true) => private_name(obj, tag),
                // Translators: the name shown for an attribute that the DICOM dictionary does not know.
                (None, false) => tr!("Unknown attribute").to_owned(),
            };
            let (value, items) = match elem.value() {
                Value::Primitive(p) => (primitive_to_string(vr, p), Vec::new()),
                Value::Sequence(seq) => {
                    let items: Vec<Vec<TagNode>> = seq.items().iter().map(nodes).collect();
                    (tr_n!("Sequence of {n} item", "Sequence of {n} items", items.len()), items)
                }
                Value::PixelSequence(seq) => (
                    tr_n!(
                        "Encapsulated pixel data, {n} fragment",
                        "Encapsulated pixel data, {n} fragments",
                        seq.fragments().len()
                    ),
                    Vec::new(),
                ),
            };
            TagNode {
                group: tag.group(),
                element: tag.element(),
                keyword,
                name,
                vr: vr.to_string().to_owned(),
                value,
                items,
            }
        })
        .collect()
}

fn private_name(obj: &InMemDicomObject, tag: Tag) -> String {
    let block = tag.element() >> 8;
    let creator = attrs::text(obj, Tag(tag.group(), block));
    // Translators: the name of an attribute that a manufacturer added; {creator} names the manufacturer's block.
    creator.map_or_else(|| tr!("Private attribute").to_owned(), |c| tr!("Private ({creator})", creator = c))
}

fn primitive_to_string(vr: VR, value: &PrimitiveValue) -> String {
    let binary = matches!(vr, VR::OB | VR::OW | VR::OF | VR::OD | VR::OL | VR::OV | VR::UN);
    let text = match value {
        PrimitiveValue::Empty => String::new(),
        PrimitiveValue::U8(bytes) if binary || bytes.len() > 64 => {
            if bytes.len() <= 16 {
                bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
            } else {
                tr_n!("<binary data, {n} byte>", "<binary data, {n} bytes>", bytes.len())
            }
        }
        PrimitiveValue::Tags(tags) => {
            tags.iter().map(|t| format!("({:04X},{:04X})", t.group(), t.element())).collect::<Vec<_>>().join(", ")
        }
        other if binary => tr_n!("<binary data, {n} value>", "<binary data, {n} values>", other.multiplicity()),
        other => other.to_str().into_owned(),
    };
    let text = text.trim_end_matches(['\0', ' ']).to_owned();
    if text.chars().count() > MAX_VALUE_CHARS {
        let mut cut: String = text.chars().take(MAX_VALUE_CHARS).collect();
        cut.push('…');
        cut
    } else {
        text
    }
}

// ---------------------------------------------------------------------------
// Curated summary

struct Rows<'a> {
    obj: &'a InMemDicomObject,
    rows: Vec<SummaryRow>,
}

/// Dictionary keyword of `tag`, the key of the rows that show it.
fn keyword(tag: Tag) -> Option<&'static str> {
    StandardDataDictionary.by_tag(tag).map(|entry| entry.alias)
}

impl<'a> Rows<'a> {
    fn new(obj: &'a InMemDicomObject) -> Self {
        Self { obj, rows: Vec::new() }
    }

    fn push(&mut self, label: &str, value: Option<String>) {
        self.keyed(label, None, value);
    }

    fn keyed(&mut self, label: &str, key: Option<&'static str>, value: Option<String>) {
        if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
            self.rows.push(SummaryRow { label: label.to_owned(), value: v, key });
        }
    }

    /// A computed row that shows (mainly) the attribute `tag`.
    fn of(&mut self, label: &str, tag: Tag, value: Option<String>) {
        self.keyed(label, keyword(tag), value);
    }

    fn text(&mut self, label: &str, tag: Tag) {
        let v = attrs::text(self.obj, tag);
        self.of(label, tag, v);
    }

    fn with(&mut self, label: &str, tag: Tag, f: impl Fn(&str) -> Option<String>) {
        let v = attrs::text(self.obj, tag).and_then(|v| f(&v).or(Some(v)));
        self.of(label, tag, v);
    }

    fn number(&mut self, label: &str, tag: Tag, unit: &str, decimals: usize) {
        let v =
            attrs::float(self.obj, tag).map(|v| format!("{} {unit}", format::number(v, decimals)).trim().to_owned());
        self.of(label, tag, v);
    }

    fn section(self, key: &'static str, title: &'static str) -> Option<SummarySection> {
        (!self.rows.is_empty()).then_some(SummarySection { key, title, rows: self.rows })
    }
}

/// Clinically grouped, formatted overview of an instance.
pub fn summarize(obj: &InMemDicomObject, transfer_syntax: &str) -> Vec<SummarySection> {
    let modality = attrs::text(obj, tags::MODALITY).unwrap_or_default();
    let mut out = Vec::new();

    // Translators: the labels of the image information (inspector), grouped in sections.
    let mut r = Rows::new(obj);
    r.with(tr_ctx!("patient", "Name"), tags::PATIENT_NAME, |v| Some(format::person_name(v)));
    r.text(tr!("Patient ID"), tags::PATIENT_ID);
    r.with(tr!("Birth date"), tags::PATIENT_BIRTH_DATE, format::date);
    r.with(tr!("Age"), tags::PATIENT_AGE, format::age);
    r.with(tr!("Sex"), tags::PATIENT_SEX, |v| Some(format::sex(v)));
    r.number(tr!("Weight"), tags::PATIENT_WEIGHT, "kg", 1);
    r.push(
        tr!("Height"),
        attrs::float(obj, tags::PATIENT_SIZE).map(|m| format!("{} cm", format::number(m * 100.0, 0))),
    );
    out.extend(r.section("Patient", tr_ctx!("inspector section", "Patient")));

    let mut r = Rows::new(obj);
    r.text(tr!("Description"), tags::STUDY_DESCRIPTION);
    r.push(tr!("Date"), date_time(obj, tags::STUDY_DATE, tags::STUDY_TIME));
    r.text(tr!("Accession number"), tags::ACCESSION_NUMBER);
    r.text(tr!("Study ID"), tags::STUDY_ID);
    r.with(tr!("Referring physician"), tags::REFERRING_PHYSICIAN_NAME, |v| Some(format::person_name(v)));
    r.text(tr!("Institution"), tags::INSTITUTION_NAME);
    r.text(tr!("Department"), tags::INSTITUTIONAL_DEPARTMENT_NAME);
    out.extend(r.section("Study", tr_ctx!("inspector section", "Study")));

    let mut r = Rows::new(obj);
    // Translators: the number of the series on the disc.
    r.text(tr_ctx!("series", "Number"), tags::SERIES_NUMBER);
    r.text(tr!("Description"), tags::SERIES_DESCRIPTION);
    r.of(
        tr!("Modality"),
        tags::MODALITY,
        attrs::text(obj, tags::MODALITY).map(|m| match format::modality_name(&m) {
            Some(name) => format!("{name} ({m})"),
            None => m,
        }),
    );
    r.text(tr!("Body part"), tags::BODY_PART_EXAMINED);
    r.text(tr!("Protocol"), tags::PROTOCOL_NAME);
    r.with(tr!("Patient position"), tags::PATIENT_POSITION, |v| format::patient_position(v).map(str::to_owned));
    // Translators: which side of the body was examined (DICOM Laterality: R or L).
    r.text(tr!("Laterality"), tags::LATERALITY);
    r.push(tr!("Series date"), date_time(obj, tags::SERIES_DATE, tags::SERIES_TIME));
    out.extend(r.section("Series", tr_ctx!("inspector section", "Series")));

    let mut r = Rows::new(obj);
    r.text(tr!("Manufacturer"), tags::MANUFACTURER);
    // Translators: the scanner's model name.
    r.text(tr!("Model"), tags::MANUFACTURER_MODEL_NAME);
    // Translators: the name of the scanner (DICOM Station Name).
    r.text(tr!("Station"), tags::STATION_NAME);
    r.push(tr!("Software"), Some(attrs::texts(obj, tags::SOFTWARE_VERSIONS).join(", ")));
    r.number(tr!("Field strength"), tags::MAGNETIC_FIELD_STRENGTH, "T", 2);
    out.extend(r.section("Equipment", tr_ctx!("inspector section", "Equipment")));

    let mut r = Rows::new(obj);
    match modality.as_str() {
        "MR" => {
            r.text(tr!("Sequence name"), tags::SEQUENCE_NAME);
            r.of(tr!("Scanning sequence"), tags::SCANNING_SEQUENCE, {
                let v = attrs::texts(obj, tags::SCANNING_SEQUENCE);
                (!v.is_empty()).then(|| format::scanning_sequence(&v))
            });
            r.of(
                tr!("Sequence variant"),
                tags::SEQUENCE_VARIANT,
                Some(attrs::texts(obj, tags::SEQUENCE_VARIANT).join(", ")),
            );
            r.of(tr!("Scan options"), tags::SCAN_OPTIONS, Some(attrs::texts(obj, tags::SCAN_OPTIONS).join(", ")));
            r.text(tr!("Acquisition type"), tags::MR_ACQUISITION_TYPE);
            // Translators: the abbreviations TR, TE and TI are the usual ones in MR; keep them.
            r.number(tr!("Repetition time (TR)"), tags::REPETITION_TIME, "ms", 2);
            r.number(tr!("Echo time (TE)"), tags::ECHO_TIME, "ms", 2);
            r.number(tr!("Inversion time (TI)"), tags::INVERSION_TIME, "ms", 2);
            r.number(tr!("Flip angle"), tags::FLIP_ANGLE, "°", 1);
            r.text(tr!("Echo train length"), tags::ECHO_TRAIN_LENGTH);
            // Translators: how many times each signal was measured and averaged (DICOM Number of Averages).
            r.number(tr!("Averages"), tags::NUMBER_OF_AVERAGES, "", 1);
            r.number(tr!("Imaging frequency"), tags::IMAGING_FREQUENCY, "MHz", 3);
            r.number(tr!("Pixel bandwidth"), tags::PIXEL_BANDWIDTH, "Hz/px", 1);
            r.text(tr!("Receive coil"), tags::RECEIVE_COIL_NAME);
        }
        "CT" => {
            r.number(tr!("Tube voltage"), tags::KVP, "kV", 0);
            r.number(tr!("Tube current"), tags::X_RAY_TUBE_CURRENT, "mA", 0);
            // Translators: the X-ray exposure of a CT, in mAs.
            r.number(tr!("Exposure"), tags::EXPOSURE, "mAs", 0);
            r.number(tr!("Exposure time"), tags::EXPOSURE_TIME, "ms", 0);
            // Translators: CTDIvol, the CT dose index; the abbreviation is international.
            r.number(tr!("CTDIvol"), tags::CTD_IVOL, "mGy", 2);
            // Translators: the reconstruction kernel (filter) of a CT.
            r.of(tr!("Kernel"), tags::CONVOLUTION_KERNEL, Some(attrs::texts(obj, tags::CONVOLUTION_KERNEL).join(", ")));
            // Translators: the filter of the X-ray beam of a CT (DICOM Filter Type).
            r.text(tr!("Filter"), tags::FILTER_TYPE);
            r.number(tr!("Pitch factor"), tags::SPIRAL_PITCH_FACTOR, "", 3);
            r.number(tr!("Rotation time"), tags::REVOLUTION_TIME, "s", 2);
            r.number(tr!("Collimation"), tags::TOTAL_COLLIMATION_WIDTH, "mm", 1);
            r.number(tr!("Reconstruction diameter"), tags::RECONSTRUCTION_DIAMETER, "mm", 1);
            r.number(tr!("Gantry tilt"), tags::GANTRY_DETECTOR_TILT, "°", 1);
        }
        _ => {}
    }
    r.text(tr!("Contrast agent"), tags::CONTRAST_BOLUS_AGENT);
    // Translators: the date and time the images were acquired.
    r.push(tr_ctx!("date and time", "Acquired"), date_time(obj, tags::ACQUISITION_DATE, tags::ACQUISITION_TIME));
    out.extend(r.section("Acquisition", tr_ctx!("inspector section", "Acquisition")));

    let mut r = Rows::new(obj);
    r.text(tr!("Instance number"), tags::INSTANCE_NUMBER);
    r.of(tr!("Image type"), tags::IMAGE_TYPE, Some(attrs::texts(obj, tags::IMAGE_TYPE).join(" \\ ")));
    r.keyed(
        // Translators: the number of pixels of the image (columns × rows).
        tr!("Matrix"),
        Some("Matrix"),
        attrs::uint(obj, tags::COLUMNS).zip(attrs::uint(obj, tags::ROWS)).map(|(c, rw)| format!("{c} × {rw} px")),
    );
    r.text(tr!("Frames"), tags::NUMBER_OF_FRAMES);
    let spacing = attrs::float_array::<2>(obj, tags::PIXEL_SPACING);
    r.of(
        tr!("Pixel spacing"),
        tags::PIXEL_SPACING,
        spacing.map(|s| format!("{} × {} mm", format::number(s[1], 3), format::number(s[0], 3))),
    );
    r.keyed(
        tr!("Field of view"),
        Some("FieldOfView"),
        spacing.zip(attrs::uint(obj, tags::COLUMNS).zip(attrs::uint(obj, tags::ROWS))).map(|(s, (c, rw))| {
            format!("{} × {} mm", format::number(s[1] * f64::from(c), 0), format::number(s[0] * f64::from(rw), 0))
        }),
    );
    r.number(tr!("Slice thickness"), tags::SLICE_THICKNESS, "mm", 2);
    r.number(tr!("Spacing between slices"), tags::SPACING_BETWEEN_SLICES, "mm", 2);
    r.number(tr!("Slice location"), tags::SLICE_LOCATION, "mm", 2);
    r.of(
        // Translators: the position of the image in the patient (DICOM Image Position), in mm.
        tr_ctx!("image position", "Position"),
        tags::IMAGE_POSITION_PATIENT,
        attrs::float_array::<3>(obj, tags::IMAGE_POSITION_PATIENT)
            .map(|p| p.iter().map(|v| format::number(*v, 1)).collect::<Vec<_>>().join(", ") + " mm"),
    );
    r.of(
        tr!("Orientation"),
        tags::IMAGE_ORIENTATION_PATIENT,
        attrs::float_array::<6>(obj, tags::IMAGE_ORIENTATION_PATIENT).map(|o| {
            let row = DVec3::new(o[0], o[1], o[2]);
            let col = DVec3::new(o[3], o[4], o[5]);
            let plane = AnatomicalPlane::from_normal(row.cross(col));
            // Translators: {row} and {column} are orientation letters of the patient (L, R, A, P, S, I),
            // which are not translated.
            tr!(
                "{plane} (rows toward {row}, columns toward {column})",
                plane = plane,
                row = summrise_imaging::orientation_label(row),
                column = summrise_imaging::orientation_label(col)
            )
        }),
    );
    r.text(tr!("Photometric interpretation"), tags::PHOTOMETRIC_INTERPRETATION);
    r.of(
        tr!("Bit depth"),
        tags::BITS_STORED,
        attrs::uint(obj, tags::BITS_STORED).zip(attrs::uint(obj, tags::BITS_ALLOCATED)).map(|(s, a)| {
            if attrs::uint(obj, tags::PIXEL_REPRESENTATION) == Some(1) {
                tr!("{stored} of {allocated} bits, signed", stored = s, allocated = a)
            } else {
                tr!("{stored} of {allocated} bits, unsigned", stored = s, allocated = a)
            }
        }),
    );
    r.of(
        // Translators: the conversion of stored values to measured values (DICOM Rescale Slope and Intercept).
        tr!("Rescale"),
        tags::RESCALE_SLOPE,
        attrs::float(obj, tags::RESCALE_SLOPE).zip(attrs::float(obj, tags::RESCALE_INTERCEPT)).map(|(s, i)| {
            let (slope, intercept) = (format::number(s, 4), format::number(i, 2));
            match attrs::text(obj, tags::RESCALE_TYPE) {
                Some(unit) => tr!(
                    "slope {slope}, intercept {intercept} ({unit})",
                    slope = slope,
                    intercept = intercept,
                    unit = unit
                ),
                None => tr!("slope {slope}, intercept {intercept}", slope = slope, intercept = intercept),
            }
        }),
    );
    let windows = crate::decode::dicom_windows(obj);
    r.of(
        tr!("Stored windows"),
        tags::WINDOW_CENTER,
        (!windows.is_empty()).then(|| {
            windows
                .iter()
                .map(|w| {
                    // Translators: a stored contrast window: its name, C for its centre (level), W for its width.
                    tr!(
                        "{name}: C {center} / W {width}",
                        name = w.name,
                        center = format::number(w.window.center, 1),
                        width = format::number(w.window.width, 1)
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        }),
    );
    // The label of the lossy mark on the image ("Lossy 10:1 (JPEG)"), also when only the
    // transfer syntax tells; "No" when the file says it was not lossy-compressed.
    r.of(
        tr!("Lossy compression"),
        tags::LOSSY_IMAGE_COMPRESSION,
        crate::fidelity::LossyCompression::of(obj, transfer_syntax)
            .map(|lossy| lossy.label())
            .or_else(|| attrs::text(obj, tags::LOSSY_IMAGE_COMPRESSION).map(|_| tr!("No").to_owned())),
    );
    r.of(
        summrise_i18n::tr!("Burned-in text"),
        tags::BURNED_IN_ANNOTATION,
        attrs::text(obj, tags::BURNED_IN_ANNOTATION).map(|v| {
            if v.eq_ignore_ascii_case("YES") { summrise_i18n::tr!("Yes") } else { summrise_i18n::tr!("No") }.to_owned()
        }),
    );
    // Translators: DICOM terms (transfer syntax: how the pixels are encoded; SOP class: the kind of object).
    r.of(tr!("Transfer syntax"), tags::TRANSFER_SYNTAX_UID, Some(format::transfer_syntax_name(transfer_syntax)));
    r.of(
        tr!("SOP class"),
        tags::SOP_CLASS_UID,
        attrs::text(obj, tags::SOP_CLASS_UID).map(|u| format::sop_class_name(&u)),
    );
    r.text(tr!("Character set"), tags::SPECIFIC_CHARACTER_SET);
    out.extend(r.section("Image", tr_ctx!("inspector section", "Image")));

    let mut r = Rows::new(obj);
    // Translators: unique identifiers (UIDs) of the study, the series and the image.
    r.text(tr!("Study UID"), tags::STUDY_INSTANCE_UID);
    r.text(tr!("Series UID"), tags::SERIES_INSTANCE_UID);
    r.text(tr!("Instance UID"), tags::SOP_INSTANCE_UID);
    r.text(tr!("Frame of reference"), tags::FRAME_OF_REFERENCE_UID);
    out.extend(r.section("Identifiers", tr_ctx!("inspector section", "Identifiers")));

    out
}

fn date_time(obj: &InMemDicomObject, date_tag: Tag, time_tag: Tag) -> Option<String> {
    let d = attrs::text(obj, date_tag).and_then(|d| format::date(&d));
    let t = attrs::text(obj, time_tag).and_then(|t| format::time(&t));
    match (d, t) {
        (Some(d), Some(t)) => Some(format!("{d} {t}")),
        (Some(d), None) => Some(d),
        (None, Some(t)) => Some(t),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The explanations of `summrise_core::guide` are looked up by these
    /// keys: the spelling of the dictionary keywords is part of the contract.
    #[test]
    fn row_keys_are_dictionary_keywords() {
        let expected = [
            (tags::REPETITION_TIME, "RepetitionTime"),
            (tags::ECHO_TIME, "EchoTime"),
            (tags::INVERSION_TIME, "InversionTime"),
            (tags::FLIP_ANGLE, "FlipAngle"),
            (tags::ECHO_TRAIN_LENGTH, "EchoTrainLength"),
            (tags::NUMBER_OF_AVERAGES, "NumberOfAverages"),
            (tags::IMAGING_FREQUENCY, "ImagingFrequency"),
            (tags::PIXEL_BANDWIDTH, "PixelBandwidth"),
            (tags::RECEIVE_COIL_NAME, "ReceiveCoilName"),
            (tags::SEQUENCE_NAME, "SequenceName"),
            (tags::SCANNING_SEQUENCE, "ScanningSequence"),
            (tags::SEQUENCE_VARIANT, "SequenceVariant"),
            (tags::SCAN_OPTIONS, "ScanOptions"),
            (tags::MR_ACQUISITION_TYPE, "MRAcquisitionType"),
            (tags::MAGNETIC_FIELD_STRENGTH, "MagneticFieldStrength"),
            (tags::KVP, "KVP"),
            (tags::X_RAY_TUBE_CURRENT, "XRayTubeCurrent"),
            (tags::EXPOSURE, "Exposure"),
            (tags::EXPOSURE_TIME, "ExposureTime"),
            (tags::CTD_IVOL, "CTDIvol"),
            (tags::CONVOLUTION_KERNEL, "ConvolutionKernel"),
            (tags::FILTER_TYPE, "FilterType"),
            (tags::SPIRAL_PITCH_FACTOR, "SpiralPitchFactor"),
            (tags::REVOLUTION_TIME, "RevolutionTime"),
            (tags::TOTAL_COLLIMATION_WIDTH, "TotalCollimationWidth"),
            (tags::RECONSTRUCTION_DIAMETER, "ReconstructionDiameter"),
            (tags::GANTRY_DETECTOR_TILT, "GantryDetectorTilt"),
            (tags::CONTRAST_BOLUS_AGENT, "ContrastBolusAgent"),
            (tags::MODALITY, "Modality"),
            (tags::BODY_PART_EXAMINED, "BodyPartExamined"),
            (tags::PROTOCOL_NAME, "ProtocolName"),
            (tags::SERIES_DESCRIPTION, "SeriesDescription"),
            (tags::PATIENT_POSITION, "PatientPosition"),
            (tags::LATERALITY, "Laterality"),
            (tags::ACCESSION_NUMBER, "AccessionNumber"),
            (tags::INSTANCE_NUMBER, "InstanceNumber"),
            (tags::IMAGE_TYPE, "ImageType"),
            (tags::NUMBER_OF_FRAMES, "NumberOfFrames"),
            (tags::PIXEL_SPACING, "PixelSpacing"),
            (tags::SLICE_THICKNESS, "SliceThickness"),
            (tags::SPACING_BETWEEN_SLICES, "SpacingBetweenSlices"),
            (tags::SLICE_LOCATION, "SliceLocation"),
            (tags::IMAGE_POSITION_PATIENT, "ImagePositionPatient"),
            (tags::IMAGE_ORIENTATION_PATIENT, "ImageOrientationPatient"),
            (tags::PHOTOMETRIC_INTERPRETATION, "PhotometricInterpretation"),
            (tags::BITS_STORED, "BitsStored"),
            (tags::RESCALE_SLOPE, "RescaleSlope"),
            (tags::WINDOW_CENTER, "WindowCenter"),
            (tags::LOSSY_IMAGE_COMPRESSION, "LossyImageCompression"),
            (tags::BURNED_IN_ANNOTATION, "BurnedInAnnotation"),
            (tags::TRANSFER_SYNTAX_UID, "TransferSyntaxUID"),
            (tags::SOP_CLASS_UID, "SOPClassUID"),
            (tags::STUDY_INSTANCE_UID, "StudyInstanceUID"),
            (tags::SERIES_INSTANCE_UID, "SeriesInstanceUID"),
            (tags::SOP_INSTANCE_UID, "SOPInstanceUID"),
            (tags::FRAME_OF_REFERENCE_UID, "FrameOfReferenceUID"),
        ];
        for (tag, key) in expected {
            assert_eq!(keyword(tag), Some(key), "{tag}");
        }
        // Private attributes have no keyword, so rows that showed one would have no key.
        assert_eq!(keyword(Tag(0x0029, 0x1010)), None);
    }
}
