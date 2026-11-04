//! Metadata for the inspector: a complete, labelled attribute tree and a
//! curated, human-readable summary grouped by clinical topic.

use std::path::Path;

use dicom_core::dictionary::{DataDictionary, DataDictionaryEntry};
use dicom_core::value::{PrimitiveValue, Value};
use dicom_core::{Tag, VR};
use dicom_dictionary_std::{StandardDataDictionary, tags};
use dicom_object::{InMemDicomObject, OpenFileOptions};
use summrise_imaging::{AnatomicalPlane, DVec3};

use crate::attrs;
use crate::error::{DicomError, Result};
use crate::format;

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

/// A labelled group of summary rows.
#[derive(Debug, Clone, PartialEq)]
pub struct SummarySection {
    pub title: &'static str,
    pub rows: Vec<(String, String)>,
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

/// Reads all attributes of one file (pixel data is summarised, not loaded).
pub fn read_metadata(path: &Path) -> Result<InstanceMetadata> {
    let file = OpenFileOptions::new()
        .read_until(tags::PIXEL_DATA)
        .open_file(path)
        .map_err(|e| DicomError::Parse { path: path.to_owned(), reason: e.to_string() })?;
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
                (None, false) => "Unknown attribute".to_owned(),
            };
            let (value, items) = match elem.value() {
                Value::Primitive(p) => (primitive_to_string(vr, p), Vec::new()),
                Value::Sequence(seq) => {
                    let items: Vec<Vec<TagNode>> = seq.items().iter().map(nodes).collect();
                    (format!("Sequence of {} item(s)", items.len()), items)
                }
                Value::PixelSequence(seq) => {
                    (format!("Encapsulated pixel data, {} fragment(s)", seq.fragments().len()), Vec::new())
                }
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
    creator.map_or_else(|| "Private attribute".to_owned(), |c| format!("Private ({c})"))
}

fn primitive_to_string(vr: VR, value: &PrimitiveValue) -> String {
    let binary = matches!(vr, VR::OB | VR::OW | VR::OF | VR::OD | VR::OL | VR::OV | VR::UN);
    let text = match value {
        PrimitiveValue::Empty => String::new(),
        PrimitiveValue::U8(bytes) if binary || bytes.len() > 64 => {
            if bytes.len() <= 16 {
                bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
            } else {
                format!("<binary data, {} bytes>", bytes.len())
            }
        }
        PrimitiveValue::Tags(tags) => {
            tags.iter().map(|t| format!("({:04X},{:04X})", t.group(), t.element())).collect::<Vec<_>>().join(", ")
        }
        other if binary => format!("<binary data, {} values>", other.multiplicity()),
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
    rows: Vec<(String, String)>,
}

impl<'a> Rows<'a> {
    fn new(obj: &'a InMemDicomObject) -> Self {
        Self { obj, rows: Vec::new() }
    }

    fn push(&mut self, label: &str, value: Option<String>) {
        if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
            self.rows.push((label.to_owned(), v));
        }
    }

    fn text(&mut self, label: &str, tag: Tag) {
        let v = attrs::text(self.obj, tag);
        self.push(label, v);
    }

    fn with(&mut self, label: &str, tag: Tag, f: impl Fn(&str) -> Option<String>) {
        let v = attrs::text(self.obj, tag).and_then(|v| f(&v).or(Some(v)));
        self.push(label, v);
    }

    fn number(&mut self, label: &str, tag: Tag, unit: &str, decimals: usize) {
        let v =
            attrs::float(self.obj, tag).map(|v| format!("{} {unit}", format::number(v, decimals)).trim().to_owned());
        self.push(label, v);
    }

    fn section(self, title: &'static str) -> Option<SummarySection> {
        (!self.rows.is_empty()).then_some(SummarySection { title, rows: self.rows })
    }
}

/// Clinically grouped, formatted overview of an instance.
pub fn summarize(obj: &InMemDicomObject, transfer_syntax: &str) -> Vec<SummarySection> {
    let modality = attrs::text(obj, tags::MODALITY).unwrap_or_default();
    let mut out = Vec::new();

    let mut r = Rows::new(obj);
    r.with("Name", tags::PATIENT_NAME, |v| Some(format::person_name(v)));
    r.text("Patient ID", tags::PATIENT_ID);
    r.with("Birth date", tags::PATIENT_BIRTH_DATE, format::date);
    r.with("Age", tags::PATIENT_AGE, format::age);
    r.with("Sex", tags::PATIENT_SEX, |v| Some(format::sex(v)));
    r.number("Weight", tags::PATIENT_WEIGHT, "kg", 1);
    r.push("Height", attrs::float(obj, tags::PATIENT_SIZE).map(|m| format!("{} cm", format::number(m * 100.0, 0))));
    out.extend(r.section("Patient"));

    let mut r = Rows::new(obj);
    r.text("Description", tags::STUDY_DESCRIPTION);
    r.push("Date", date_time(obj, tags::STUDY_DATE, tags::STUDY_TIME));
    r.text("Accession number", tags::ACCESSION_NUMBER);
    r.text("Study ID", tags::STUDY_ID);
    r.with("Referring physician", tags::REFERRING_PHYSICIAN_NAME, |v| Some(format::person_name(v)));
    r.text("Institution", tags::INSTITUTION_NAME);
    r.text("Department", tags::INSTITUTIONAL_DEPARTMENT_NAME);
    out.extend(r.section("Study"));

    let mut r = Rows::new(obj);
    r.text("Number", tags::SERIES_NUMBER);
    r.text("Description", tags::SERIES_DESCRIPTION);
    r.push(
        "Modality",
        attrs::text(obj, tags::MODALITY).map(|m| match format::modality_name(&m) {
            Some(name) => format!("{name} ({m})"),
            None => m,
        }),
    );
    r.text("Body part", tags::BODY_PART_EXAMINED);
    r.text("Protocol", tags::PROTOCOL_NAME);
    r.with("Patient position", tags::PATIENT_POSITION, |v| format::patient_position(v).map(str::to_owned));
    r.text("Laterality", tags::LATERALITY);
    r.push("Series date", date_time(obj, tags::SERIES_DATE, tags::SERIES_TIME));
    out.extend(r.section("Series"));

    let mut r = Rows::new(obj);
    r.text("Manufacturer", tags::MANUFACTURER);
    r.text("Model", tags::MANUFACTURER_MODEL_NAME);
    r.text("Station", tags::STATION_NAME);
    r.push("Software", Some(attrs::texts(obj, tags::SOFTWARE_VERSIONS).join(", ")));
    r.number("Field strength", tags::MAGNETIC_FIELD_STRENGTH, "T", 2);
    out.extend(r.section("Equipment"));

    let mut r = Rows::new(obj);
    match modality.as_str() {
        "MR" => {
            r.text("Sequence name", tags::SEQUENCE_NAME);
            r.push("Scanning sequence", {
                let v = attrs::texts(obj, tags::SCANNING_SEQUENCE);
                (!v.is_empty()).then(|| format::scanning_sequence(&v))
            });
            r.push("Sequence variant", Some(attrs::texts(obj, tags::SEQUENCE_VARIANT).join(", ")));
            r.push("Scan options", Some(attrs::texts(obj, tags::SCAN_OPTIONS).join(", ")));
            r.text("Acquisition type", tags::MR_ACQUISITION_TYPE);
            r.number("Repetition time (TR)", tags::REPETITION_TIME, "ms", 2);
            r.number("Echo time (TE)", tags::ECHO_TIME, "ms", 2);
            r.number("Inversion time (TI)", tags::INVERSION_TIME, "ms", 2);
            r.number("Flip angle", tags::FLIP_ANGLE, "°", 1);
            r.text("Echo train length", tags::ECHO_TRAIN_LENGTH);
            r.number("Averages", tags::NUMBER_OF_AVERAGES, "", 1);
            r.number("Imaging frequency", tags::IMAGING_FREQUENCY, "MHz", 3);
            r.number("Pixel bandwidth", tags::PIXEL_BANDWIDTH, "Hz/px", 1);
            r.text("Receive coil", tags::RECEIVE_COIL_NAME);
        }
        "CT" => {
            r.number("Tube voltage", tags::KVP, "kV", 0);
            r.number("Tube current", tags::X_RAY_TUBE_CURRENT, "mA", 0);
            r.number("Exposure", tags::EXPOSURE, "mAs", 0);
            r.number("Exposure time", tags::EXPOSURE_TIME, "ms", 0);
            r.number("CTDIvol", tags::CTD_IVOL, "mGy", 2);
            r.push("Kernel", Some(attrs::texts(obj, tags::CONVOLUTION_KERNEL).join(", ")));
            r.text("Filter", tags::FILTER_TYPE);
            r.number("Pitch factor", tags::SPIRAL_PITCH_FACTOR, "", 3);
            r.number("Rotation time", tags::REVOLUTION_TIME, "s", 2);
            r.number("Collimation", tags::TOTAL_COLLIMATION_WIDTH, "mm", 1);
            r.number("Reconstruction diameter", tags::RECONSTRUCTION_DIAMETER, "mm", 1);
            r.number("Gantry tilt", tags::GANTRY_DETECTOR_TILT, "°", 1);
        }
        _ => {}
    }
    r.text("Contrast agent", tags::CONTRAST_BOLUS_AGENT);
    r.push("Acquired", date_time(obj, tags::ACQUISITION_DATE, tags::ACQUISITION_TIME));
    out.extend(r.section("Acquisition"));

    let mut r = Rows::new(obj);
    r.text("Instance number", tags::INSTANCE_NUMBER);
    r.push("Image type", Some(attrs::texts(obj, tags::IMAGE_TYPE).join(" \\ ")));
    r.push(
        "Matrix",
        attrs::uint(obj, tags::COLUMNS).zip(attrs::uint(obj, tags::ROWS)).map(|(c, rw)| format!("{c} × {rw} px")),
    );
    r.text("Frames", tags::NUMBER_OF_FRAMES);
    let spacing = attrs::float_array::<2>(obj, tags::PIXEL_SPACING);
    r.push("Pixel spacing", spacing.map(|s| format!("{} × {} mm", format::number(s[1], 3), format::number(s[0], 3))));
    r.push(
        "Field of view",
        spacing.zip(attrs::uint(obj, tags::COLUMNS).zip(attrs::uint(obj, tags::ROWS))).map(|(s, (c, rw))| {
            format!("{} × {} mm", format::number(s[1] * f64::from(c), 0), format::number(s[0] * f64::from(rw), 0))
        }),
    );
    r.number("Slice thickness", tags::SLICE_THICKNESS, "mm", 2);
    r.number("Spacing between slices", tags::SPACING_BETWEEN_SLICES, "mm", 2);
    r.number("Slice location", tags::SLICE_LOCATION, "mm", 2);
    r.push(
        "Position",
        attrs::float_array::<3>(obj, tags::IMAGE_POSITION_PATIENT)
            .map(|p| p.iter().map(|v| format::number(*v, 1)).collect::<Vec<_>>().join(", ") + " mm"),
    );
    r.push(
        "Orientation",
        attrs::float_array::<6>(obj, tags::IMAGE_ORIENTATION_PATIENT).map(|o| {
            let row = DVec3::new(o[0], o[1], o[2]);
            let col = DVec3::new(o[3], o[4], o[5]);
            let plane = AnatomicalPlane::from_normal(row.cross(col));
            format!(
                "{plane} — rows → {}, columns → {}",
                summrise_imaging::orientation_label(row),
                summrise_imaging::orientation_label(col)
            )
        }),
    );
    r.text("Photometric interpretation", tags::PHOTOMETRIC_INTERPRETATION);
    r.push(
        "Bit depth",
        attrs::uint(obj, tags::BITS_STORED).zip(attrs::uint(obj, tags::BITS_ALLOCATED)).map(|(s, a)| {
            let signed = attrs::uint(obj, tags::PIXEL_REPRESENTATION) == Some(1);
            format!("{s} of {a} bits, {}", if signed { "signed" } else { "unsigned" })
        }),
    );
    r.push(
        "Rescale",
        attrs::float(obj, tags::RESCALE_SLOPE).zip(attrs::float(obj, tags::RESCALE_INTERCEPT)).map(|(s, i)| {
            let unit = attrs::text(obj, tags::RESCALE_TYPE).map(|u| format!(" ({u})")).unwrap_or_default();
            format!("slope {}, intercept {}{unit}", format::number(s, 4), format::number(i, 2))
        }),
    );
    let windows = crate::decode::dicom_windows(obj);
    r.push(
        "Stored windows",
        (!windows.is_empty()).then(|| {
            windows
                .iter()
                .map(|w| {
                    format!(
                        "{}: C {} / W {}",
                        w.name,
                        format::number(w.window.center, 1),
                        format::number(w.window.width, 1)
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        }),
    );
    r.push(
        "Lossy compression",
        attrs::text(obj, tags::LOSSY_IMAGE_COMPRESSION).map(|v| {
            if v == "01" {
                let ratio = attrs::floats(obj, tags::LOSSY_IMAGE_COMPRESSION_RATIO);
                ratio.first().map_or_else(|| "Yes".to_owned(), |r| format!("Yes (ratio {}:1)", format::number(*r, 1)))
            } else {
                "No".to_owned()
            }
        }),
    );
    r.push("Transfer syntax", Some(format::transfer_syntax_name(transfer_syntax)));
    r.push("SOP class", attrs::text(obj, tags::SOP_CLASS_UID).map(|u| format::sop_class_name(&u)));
    r.text("Character set", tags::SPECIFIC_CHARACTER_SET);
    out.extend(r.section("Image"));

    let mut r = Rows::new(obj);
    r.text("Study UID", tags::STUDY_INSTANCE_UID);
    r.text("Series UID", tags::SERIES_INSTANCE_UID);
    r.text("Instance UID", tags::SOP_INSTANCE_UID);
    r.text("Frame of reference", tags::FRAME_OF_REFERENCE_UID);
    out.extend(r.section("Identifiers"));

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
