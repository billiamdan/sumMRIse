//! Structured reports: the content tree of a DICOM SR as headed, readable text
//! in document order (PS3.3 C.17.3). The text is shown exactly as written,
//! with nothing summarised, translated or interpreted.

use dicom_core::Tag;
use dicom_core::value::Value;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use summrise_i18n::tr;

use super::text::{MAX_TEXT_CHARS, clean};
use crate::attrs;
use crate::format;

/// Deepest level of the content tree followed. Reports nest a handful of levels.
pub(super) const MAX_TREE_DEPTH: usize = 64;

/// Most items read. A long dose report has a few thousand; the limit bounds
/// what a hostile file can make the view lay out in every frame.
const MAX_ITEMS: usize = 20_000;

/// Longest raw value shown of an item of an unknown type.
const MAX_RAW_CHARS: usize = 512;

/// Whether a report is complete (Completion Flag; a Preliminary Flag of
/// PRELIMINARY counts as not complete).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    Complete,
    Preliminary,
    NotRecorded,
}

/// Whether a report is verified (Verification Flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    Verified,
    Unverified,
    NotRecorded,
}

/// What a report says about its own status, so that a preliminary report is
/// not read as a final one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportStatus {
    pub completion: Completion,
    pub verification: Verification,
}

impl ReportStatus {
    pub(super) fn read(obj: &InMemDicomObject) -> Self {
        let flag = |tag: Tag| attrs::text(obj, tag).map(|v| v.to_ascii_uppercase());
        let completion = match flag(tags::COMPLETION_FLAG).as_deref() {
            _ if flag(tags::PRELIMINARY_FLAG).as_deref() == Some("PRELIMINARY") => Completion::Preliminary,
            Some("COMPLETE") => Completion::Complete,
            Some("PARTIAL") => Completion::Preliminary,
            _ => Completion::NotRecorded,
        };
        let verification = match flag(tags::VERIFICATION_FLAG).as_deref() {
            Some("VERIFIED") => Verification::Verified,
            Some("UNVERIFIED") => Verification::Unverified,
            _ => Verification::NotRecorded,
        };
        Self { completion, verification }
    }

    /// Complete and verified.
    pub fn is_final(self) -> bool {
        self.completion == Completion::Complete && self.verification == Verification::Verified
    }

    /// The status as the header of the document shows it.
    pub fn label(self) -> &'static str {
        use Completion as C;
        use Verification as V;
        match (self.completion, self.verification) {
            // Translators: the status of a report in the header of the document view. "Preliminary" must
            // not read as final, and "not verified" means that no physician has confirmed the report.
            (C::Complete, V::Verified) => tr!("Final — verified"),
            (C::Complete, V::Unverified) => tr!("Complete — not verified"),
            (C::Complete, V::NotRecorded) => tr!("Complete — verification not recorded"),
            (C::Preliminary, V::Verified) => tr!("Preliminary — verified"),
            (C::Preliminary, V::Unverified) => tr!("Preliminary — not verified"),
            (C::Preliminary, V::NotRecorded) => tr!("Preliminary — verification not recorded"),
            (C::NotRecorded, V::Verified) => tr!("Verified — completion not recorded"),
            (C::NotRecorded, V::Unverified) => tr!("Not verified — completion not recorded"),
            (C::NotRecorded, V::NotRecorded) => tr!("Status not recorded"),
        }
    }
}

/// What one item of the content tree shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportContent {
    /// A container: its concept name.
    Heading(String),
    /// A text item. The label is its concept name, unless that only repeats
    /// the heading it stands under.
    Paragraph { label: Option<String>, text: String },
    /// "name: value" with units, for coded, numeric, date, name and other items.
    Value { name: String, value: String },
    /// A reference to an image, by its SOP Instance UID.
    Image { name: Option<String>, sop_instance_uid: Option<String>, frame: Option<u32> },
}

/// One item of the content tree, in document order, with its depth in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportItem {
    pub depth: usize,
    pub content: ReportContent,
}

/// A structured report, flattened into its items in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredReport {
    pub status: ReportStatus,
    pub items: Vec<ReportItem>,
    /// Part of the report was not read: it is nested too deeply or is too long.
    pub cut_short: bool,
}

/// A text value as written: only the padding at its end is removed.
pub(super) fn verbatim(obj: &InMemDicomObject, tag: Tag) -> Option<String> {
    let value = obj.get(tag)?.to_str().ok()?;
    let value = clean(value.trim_end_matches([' ', '\0']));
    (!value.trim().is_empty()).then_some(value)
}

/// The concept name of an item.
pub(super) fn concept_name(item: &InMemDicomObject) -> Option<String> {
    let name = attrs::first_item(item, tags::CONCEPT_NAME_CODE_SEQUENCE)?;
    attrs::text(name, tags::CODE_MEANING).or_else(|| attrs::text(name, tags::CODE_VALUE))
}

/// The items directly below an item.
pub(super) fn children(item: &InMemDicomObject) -> &[InMemDicomObject] {
    item.get(tags::CONTENT_SEQUENCE).and_then(|e| e.items()).unwrap_or(&[])
}

/// The value of a numeric item as written, with its unit: "12 mm", "570 mGy·cm".
pub(super) fn measured_value(item: &InMemDicomObject) -> Option<String> {
    let measured = attrs::first_item(item, tags::MEASURED_VALUE_SEQUENCE)?;
    let value = attrs::text(measured, tags::NUMERIC_VALUE)?;
    Some(match unit(measured) {
        Some(unit) => format!("{value} {unit}"),
        None => value,
    })
}

/// The unit of a measured value. UCUM codes are the short forms ("mm"), with
/// a dot for a product; the code "1" means there is no unit, and annotations
/// in braces are not units. Otherwise the code's meaning.
fn unit(measured: &InMemDicomObject) -> Option<String> {
    let code = attrs::first_item(measured, tags::MEASUREMENT_UNITS_CODE_SEQUENCE)?;
    let scheme = attrs::text(code, tags::CODING_SCHEME_DESIGNATOR).unwrap_or_default();
    let value = attrs::text(code, tags::CODE_VALUE);
    if scheme.eq_ignore_ascii_case("UCUM")
        && let Some(value) = &value
    {
        if value == "1" || value.starts_with('{') {
            return None;
        }
        return Some(value.replace('.', "·"));
    }
    attrs::text(code, tags::CODE_MEANING).or(value)
}

/// Elements that say what an item is rather than what it holds.
const STRUCTURE: [Tag; 3] = [tags::RELATIONSHIP_TYPE, tags::VALUE_TYPE, tags::CONTINUITY_OF_CONTENT];

/// The values of an item of a type this reader does not know, as they are.
fn raw_value(item: &InMemDicomObject) -> Option<String> {
    let mut values = Vec::new();
    for element in item.iter() {
        if STRUCTURE.contains(&element.header().tag) {
            continue;
        }
        if let Value::Primitive(value) = element.value() {
            let binary = matches!(value, dicom_core::PrimitiveValue::U8(_));
            if !binary
                && let text = value.to_str()
                && !text.trim().is_empty()
            {
                values.push(text.trim().to_owned());
            }
        }
    }
    let raw = values.join(" ");
    (!raw.is_empty()).then(|| raw.chars().take(MAX_RAW_CHARS).collect())
}

fn referenced_uid(item: &InMemDicomObject) -> Option<String> {
    let reference = attrs::first_item(item, tags::REFERENCED_SOP_SEQUENCE)?;
    attrs::text(reference, tags::REFERENCED_SOP_INSTANCE_UID)
}

struct Walk {
    items: Vec<ReportItem>,
    cut_short: bool,
    /// Characters of text so far.
    characters: usize,
}

impl Walk {
    fn push(&mut self, depth: usize, content: ReportContent) {
        self.items.push(ReportItem { depth, content });
    }

    /// Reads the items below `item`, which are at `depth`, under the heading `heading`.
    fn children_of(&mut self, item: &InMemDicomObject, depth: usize, heading: Option<&str>) {
        let items = children(item);
        if items.is_empty() {
            return;
        }
        if depth >= MAX_TREE_DEPTH {
            self.cut_short = true;
            return;
        }
        for child in items {
            if self.items.len() >= MAX_ITEMS || self.characters >= MAX_TEXT_CHARS {
                self.cut_short = true;
                return;
            }
            self.item(child, depth, heading);
        }
    }

    fn item(&mut self, item: &InMemDicomObject, depth: usize, heading: Option<&str>) {
        let name = concept_name(item);
        let value_type = attrs::text(item, tags::VALUE_TYPE).unwrap_or_default();
        let content = match value_type.as_str() {
            "CONTAINER" => {
                if let Some(name) = &name {
                    self.push(depth, ReportContent::Heading(name.clone()));
                }
                self.children_of(item, depth + 1, name.as_deref().or(heading));
                return;
            }
            "TEXT" => {
                let text = verbatim(item, tags::TEXT_VALUE).unwrap_or_default();
                self.characters += text.chars().count();
                ReportContent::Paragraph { label: name.filter(|name| !repeats(name, heading)), text }
            }
            "IMAGE" => {
                let frame = attrs::first_item(item, tags::REFERENCED_SOP_SEQUENCE)
                    .and_then(|reference| attrs::uint(reference, tags::REFERENCED_FRAME_NUMBER));
                ReportContent::Image { name, sop_instance_uid: referenced_uid(item), frame }
            }
            kind => ReportContent::Value {
                name: name.unwrap_or_else(|| item_name(item, kind)),
                value: value_of(item, kind),
            },
        };
        self.push(depth, content);
        self.children_of(item, depth + 1, heading);
    }
}

/// The name of an item without a concept name: what kind of item it is.
fn item_name(item: &InMemDicomObject, kind: &str) -> String {
    if kind.is_empty() { attrs::text(item, tags::RELATIONSHIP_TYPE).unwrap_or_default() } else { kind.to_owned() }
}

/// Whether the name of a text item only repeats the heading it stands under,
/// as "Finding" does under "Findings".
fn repeats(name: &str, heading: Option<&str>) -> bool {
    let name = name.to_lowercase();
    heading.is_some_and(|heading| heading.to_lowercase().starts_with(&name))
}

fn value_of(item: &InMemDicomObject, kind: &str) -> String {
    let value = match kind {
        "CODE" => attrs::first_item(item, tags::CONCEPT_CODE_SEQUENCE)
            .and_then(|code| attrs::text(code, tags::CODE_MEANING).or_else(|| attrs::text(code, tags::CODE_VALUE))),
        "NUM" => measured_value(item),
        "DATE" => attrs::text(item, tags::DATE).map(|v| format::date(&v).unwrap_or(v)),
        "TIME" => attrs::text(item, tags::TIME).map(|v| format::time(&v).unwrap_or(v)),
        "DATETIME" => attrs::text(item, tags::DATE_TIME).map(|v| date_time(&v)),
        "PNAME" => attrs::text(item, tags::PERSON_NAME).map(|v| format::person_name(&v)),
        "UIDREF" => attrs::text(item, tags::UID),
        "COMPOSITE" | "WAVEFORM" => referenced_uid(item).or_else(|| raw_value(item)),
        _ => raw_value(item),
    };
    // Translators: an item of a report that records no value.
    value.unwrap_or_else(|| tr!("no value recorded").to_owned())
}

/// A DICOM date and time (DT) as `YYYY-MM-DD HH:MM:SS`; as it is if it is
/// something else.
fn date_time(value: &str) -> String {
    let date = value.get(..8).and_then(format::date);
    let time = value.get(8..).and_then(format::time);
    match (date, time) {
        (Some(date), Some(time)) => format!("{date} {time}"),
        (Some(date), None) => date,
        _ => value.to_owned(),
    }
}

/// Reads the report in `obj`.
pub(super) fn read(obj: &InMemDicomObject) -> StructuredReport {
    let mut walk = Walk { items: Vec::new(), cut_short: false, characters: 0 };
    walk.children_of(obj, 0, None);
    StructuredReport { status: ReportStatus::read(obj), items: walk.items, cut_short: walk.cut_short }
}

#[cfg(test)]
mod tests {
    use dicom_dictionary_std::uids;
    use dicom_object::OpenFileOptions;
    use summrise_testkit::reports::{ReportNode, ReportSpec, write_structured_report};

    use super::*;

    fn report_of(spec: &ReportSpec) -> StructuredReport {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("SR");
        write_structured_report(&path, spec).expect("report");
        let file = OpenFileOptions::new().open_file(&path).expect("parse");
        read(&file)
    }

    fn comprehensive(content: Vec<ReportNode>) -> ReportSpec {
        ReportSpec::new(uids::COMPREHENSIVE_SR_STORAGE, "Report", content)
    }

    fn contents(report: &StructuredReport) -> Vec<ReportContent> {
        report.items.iter().map(|item| item.content.clone()).collect()
    }

    fn heading(text: &str) -> ReportContent {
        ReportContent::Heading(text.to_owned())
    }

    fn value(name: &str, value: &str) -> ReportContent {
        ReportContent::Value { name: name.to_owned(), value: value.to_owned() }
    }

    fn paragraph(text: &str) -> ReportContent {
        ReportContent::Paragraph { label: None, text: text.to_owned() }
    }

    /// The scenario of the specification.
    #[test]
    fn findings_and_impression_are_headings_with_their_text_in_order() {
        let report = report_of(&comprehensive(vec![
            ReportNode::container("Findings", vec![ReportNode::text("Finding", "No focal lesion.")]),
            ReportNode::container("Impression", vec![ReportNode::text("Impression", "Normal study.")]),
        ]));
        assert_eq!(
            contents(&report),
            [heading("Findings"), paragraph("No focal lesion."), heading("Impression"), paragraph("Normal study.")]
        );
        assert_eq!(report.items[0].depth, 0);
        assert_eq!(report.items[1].depth, 1);
        assert!(!report.cut_short);
    }

    #[test]
    fn a_numeric_item_shows_its_value_and_unit() {
        let report = report_of(&comprehensive(vec![
            ReportNode::number("Diameter", "12", "mm"),
            ReportNode::number("Dose", "570", "mGy.cm"),
            ReportNode::number("Count", "3", "1"),
            ReportNode::number("Ratio", "0.5", "{ratio}"),
        ]));
        assert_eq!(
            contents(&report),
            [value("Diameter", "12 mm"), value("Dose", "570 mGy·cm"), value("Count", "3"), value("Ratio", "0.5"),]
        );
    }

    #[test]
    fn items_of_every_kind_are_shown_as_name_and_value() {
        let report = report_of(&comprehensive(vec![
            ReportNode::code("Finding site", "Lung"),
            ReportNode::Date { name: "Date", value: "20260115" },
            ReportNode::PersonName { name: "Observer", value: "EXAMPLE^READER" },
            ReportNode::Other { name: "No value", value_type: "WAVEFORM" },
            ReportNode::Other { name: "A new type", value_type: "SOMETHING" },
        ]));
        assert_eq!(
            contents(&report),
            [
                value("Finding site", "Lung"),
                value("Date", "2026-01-15"),
                value("Observer", "READER EXAMPLE"),
                value("No value", "no value recorded"),
                value("A new type", "no value recorded"),
            ]
        );
    }

    #[test]
    fn a_text_item_keeps_a_name_that_says_something() {
        let report = report_of(&comprehensive(vec![ReportNode::container(
            "Findings",
            vec![ReportNode::text("Comparison", "Prior study of 2025.")],
        )]));
        assert_eq!(
            contents(&report)[1],
            ReportContent::Paragraph { label: Some("Comparison".to_owned()), text: "Prior study of 2025.".to_owned() }
        );
    }

    #[test]
    fn images_are_references_by_uid_and_frame() {
        let report = report_of(&comprehensive(vec![ReportNode::Image {
            name: "Key image",
            sop_instance_uid: "1.2.3".to_owned(),
            frame: Some(7),
        }]));
        assert_eq!(
            contents(&report),
            [ReportContent::Image {
                name: Some("Key image".to_owned()),
                sop_instance_uid: Some("1.2.3".to_owned()),
                frame: Some(7),
            }]
        );
    }

    /// Requirement "Reports are shown verbatim": every text item appears unchanged.
    #[test]
    fn text_is_shown_exactly_as_written() {
        let written = "  Zwei  Leerzeichen,\nÄÖÜ ß, Заключение, 山田, 5 < 7 & \"x\" {y} \\z.";
        let report = report_of(&comprehensive(vec![ReportNode::text("Note", written)]));
        assert_eq!(
            contents(&report),
            [ReportContent::Paragraph { label: Some("Note".to_owned()), text: written.to_owned() }]
        );
    }

    #[test]
    fn status_comes_from_the_flags() {
        let status = |completion, verification, preliminary| {
            report_of(&ReportSpec { completion, verification, preliminary, ..comprehensive(Vec::new()) }).status
        };
        // The scenario of the specification.
        let partial = status(Some("PARTIAL"), Some("UNVERIFIED"), None);
        assert_eq!(partial.label(), "Preliminary — not verified");
        assert!(!partial.is_final());
        let done = status(Some("COMPLETE"), Some("VERIFIED"), None);
        assert_eq!(done.label(), "Final — verified");
        assert!(done.is_final());
        assert_eq!(status(Some("COMPLETE"), Some("UNVERIFIED"), None).label(), "Complete — not verified");
        assert_eq!(status(None, None, None).label(), "Status not recorded");
        assert_eq!(status(Some("PARTIAL"), None, None).label(), "Preliminary — verification not recorded");
        // A report that calls itself preliminary is not read as final, whatever else it says.
        let preliminary = status(Some("COMPLETE"), Some("VERIFIED"), Some("PRELIMINARY"));
        assert_eq!(preliminary.label(), "Preliminary — verified");
        assert!(!preliminary.is_final());
        // A report that says FINAL still needs to be complete and verified.
        assert_eq!(status(Some("COMPLETE"), Some("VERIFIED"), Some("FINAL")).label(), "Final — verified");
    }

    /// A report nested beyond the limit, which the parser can still read, says that part of it is
    /// not shown and shows the rest.
    #[test]
    fn a_tree_deeper_than_the_limit_is_cut_off_and_says_so() {
        let bytes = summrise_testkit::reports::deeply_nested_report(MAX_TREE_DEPTH + 10, "Too deep").expect("bytes");
        let file = OpenFileOptions::new().from_reader(bytes.as_slice()).expect("parse");
        let report = read(&file);
        assert!(report.cut_short);
        assert!(
            !report
                .items
                .iter()
                .any(|item| matches!(&item.content, ReportContent::Paragraph { text, .. } if text == "Too deep"))
        );
        // Within the limit: all of it.
        let bytes = summrise_testkit::reports::deeply_nested_report(10, "Deep enough").expect("bytes");
        let file = OpenFileOptions::new().from_reader(bytes.as_slice()).expect("parse");
        let report = read(&file);
        assert!(!report.cut_short);
        assert!(report.items.iter().any(|item| item.content == paragraph("Deep enough")));
    }

    #[test]
    fn a_report_without_structure_is_empty_not_an_error() {
        let report = read(&InMemDicomObject::new_empty());
        assert_eq!(report.items, []);
        assert_eq!(report.status.label(), "Status not recorded");
    }
}
