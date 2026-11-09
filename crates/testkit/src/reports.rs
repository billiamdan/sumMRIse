//! Reports and documents as hospitals put them on a disc: structured reports,
//! a radiation dose report, a PDF stored in DICOM and loose report files next
//! to the images. All of it is synthetic and holds no patient data.
//!
//! [`write_reports_cd`] adds them to a copy of the synthetic CD of
//! [`crate::write_phantom_study`] (which itself stays as it is: other tests
//! count its images):
//!
//! ```text
//! <root>/DICOMDIR                     also lists the reports (SR DOCUMENT, ENCAP DOC)
//! <root>/DICOM/ST000001/SE00000N/…    the images of the phantom
//! <root>/DICOM/ST000001/SE000100/…    "MR Report" and "CT Measurements"
//! <root>/DICOM/ST000001/SE000101/…    the dose report
//! <root>/DICOM/ST000001/SE000102/…    a PDF of two pages stored in DICOM
//! <root>/REPORT.PDF                   a loose PDF
//! <root>/REPORTS/report_001.rtf       Windows-1251 Cyrillic
//! <root>/REPORTS/report_002.htm       with a script and a remote image
//! <root>/VIEWER/…                     viewer software with a manual and a readme
//! ```
//!
//! The PDF writer here makes files with text in a standard font, text in an
//! embedded TrueType font and scanned pages (see [`text_pdf`],
//! [`embedded_font_pdf`] and [`scanned_pdf`]).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use dicom_core::value::{DataSetSequence, PrimitiveValue};
use dicom_core::{DataElement, Length, Tag, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{InMemDicomObject, OpenFileOptions, meta::FileMetaTableBuilder};
use dicom_pixeldata::Transcode;
use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::{FixtureError, PhantomOptions, UID_ROOT, WrittenStudy, dicom_err, put_str, write_phantom_study};

/// Title of the Basic Text SR of the disc.
pub const MR_REPORT_TITLE: &str = "MR Report";
/// Title of the Comprehensive SR of the disc: preliminary and not verified.
pub const MEASUREMENTS_TITLE: &str = "CT Measurements";
/// Title of the dose report of the disc.
pub const DOSE_REPORT_TITLE: &str = "X-Ray Radiation Dose Report";
/// Document Title of the PDF stored in DICOM.
pub const STORED_PDF_TITLE: &str = "Radiology report (PDF)";
/// The date of the reports of the disc, as DICOM writes it.
pub const REPORT_DATE: &str = "20260115";
/// Text of the loose RTF report, written in Windows-1251.
pub const RTF_CYRILLIC_TEXT: &str = "Заключение: патологических изменений не выявлено.";
/// The instance number of the MR image that "MR Report" refers to.
pub const REFERENCED_MR_INSTANCE: u32 = 5;
/// A SOP Instance UID that is not on the disc.
pub const MISSING_IMAGE_UID: &str = "2.25.999999999999999999";

/// Where [`write_reports_cd`] put the documents.
#[derive(Debug, Clone)]
pub struct ReportsCd {
    pub study: WrittenStudy,
    pub mr_report: PathBuf,
    pub measurements: PathBuf,
    pub dose_report: PathBuf,
    pub stored_pdf: PathBuf,
    /// SOP Instance UID of the MR image "MR Report" refers to.
    pub referenced_mr_uid: String,
}

/// Writes the synthetic CD of [`write_phantom_study`] below `root`, with the
/// reports added and its DICOMDIR (if `opts` asks for one) listing them.
pub fn write_reports_cd(root: &Path, opts: &PhantomOptions) -> Result<ReportsCd, FixtureError> {
    // The clutter is what makes `VIEWER/` a folder with software in it.
    let opts = PhantomOptions { include_clutter: true, ..opts.clone() };
    let study = write_phantom_study(root, &opts)?;
    let reports = root.join("DICOM").join("ST000001");
    let mr = study.series("MR").ok_or_else(|| FixtureError::Dicom("no MR series".to_owned()))?;
    let referenced_mr_uid = format!("{}.{REFERENCED_MR_INSTANCE}", mr.uid);
    let ct_uid = study.series("CT").map(|s| format!("{}.1", s.uid)).unwrap_or_default();

    let mr_report = reports.join("SE000100").join("SR000001");
    write_structured_report(
        &mr_report,
        &ReportSpec {
            study_uid: study.study_uid.clone(),
            instance: 1,
            ..ReportSpec::new(uids::BASIC_TEXT_SR_STORAGE, MR_REPORT_TITLE, mr_report_content(&referenced_mr_uid))
        },
    )?;
    let measurements = reports.join("SE000100").join("SR000002");
    write_structured_report(
        &measurements,
        &ReportSpec {
            study_uid: study.study_uid.clone(),
            instance: 2,
            completion: Some("PARTIAL"),
            verification: Some("UNVERIFIED"),
            ..ReportSpec::new(uids::COMPREHENSIVE_SR_STORAGE, MEASUREMENTS_TITLE, measurement_content(&ct_uid))
        },
    )?;
    let dose_report = reports.join("SE000101").join("SR000001");
    write_structured_report(
        &dose_report,
        &ReportSpec {
            study_uid: study.study_uid.clone(),
            series: 101,
            title_code: Some("113701"),
            ..ReportSpec::new(
                uids::X_RAY_RADIATION_DOSE_SR_STORAGE,
                DOSE_REPORT_TITLE,
                dose_content(&DOSE_EVENTS, "570"),
            )
        },
    )?;
    let stored_pdf = reports.join("SE000102").join("DOC00001");
    let pdf = text_pdf(&[
        &["Radiology report", "Page one of a synthetic report."],
        &["Conclusion", "Page two of a synthetic report."],
    ]);
    write_encapsulated_pdf(&stored_pdf, &study.study_uid, STORED_PDF_TITLE, &pdf)?;

    fs::write(root.join("REPORT.PDF"), text_pdf(&[&["Report next to the images", "A loose synthetic PDF."]]))?;
    fs::create_dir_all(root.join("REPORTS"))?;
    fs::write(root.join("REPORTS").join("report_001.rtf"), rtf_cyrillic())?;
    fs::write(root.join("REPORTS").join("report_002.htm"), HTML_REPORT)?;
    fs::write(root.join("VIEWER").join("manual.pdf"), text_pdf(&[&["Manual of the viewer software"]]))?;
    fs::write(root.join("VIEWER").join("readme.txt"), "How to start the viewer software.\r\n")?;

    if let Some(dicomdir) = &study.dicomdir {
        let root_records = [
            (&mr_report, "SR DOCUMENT"),
            (&measurements, "SR DOCUMENT"),
            (&dose_report, "SR DOCUMENT"),
            (&stored_pdf, "ENCAP DOC"),
        ];
        add_dicomdir_records(dicomdir, root, &root_records)?;
    }
    Ok(ReportsCd { study, mr_report, measurements, dose_report, stored_pdf, referenced_mr_uid })
}

/// The irradiation events of the dose report of the disc: protocol, CTDIvol
/// and DLP as recorded.
pub const DOSE_EVENTS: [(&str, &str, &str); 2] = [("Chest", "12.5", "250"), ("Abdomen", "16", "320")];

fn mr_report_content(referenced: &str) -> Vec<ReportNode> {
    vec![
        ReportNode::container(
            "Findings",
            vec![
                ReportNode::text("Finding", "No focal lesion."),
                ReportNode::image("Best illustration of finding", referenced),
                ReportNode::image("Earlier image", MISSING_IMAGE_UID),
            ],
        ),
        ReportNode::container("Impression", vec![ReportNode::text("Impression", "Normal study.")]),
    ]
}

fn measurement_content(ct_image: &str) -> Vec<ReportNode> {
    vec![ReportNode::container(
        "Measurements",
        vec![
            ReportNode::number("Diameter", "12", "mm"),
            ReportNode::code("Finding site", "Lung"),
            ReportNode::Date { name: "Date of measurement", value: REPORT_DATE },
            ReportNode::PersonName { name: "Person observer", value: "EXAMPLE^READER" },
            ReportNode::image("Measured on", ct_image),
            ReportNode::Other { name: "Measured waveform", value_type: "WAVEFORM" },
        ],
    )]
}

/// The content tree of a CT dose report (TID 10011) with these events and
/// this total. No value is computed: a test decides what the scanner recorded.
pub fn dose_content(events: &[(&str, &str, &str)], total_dlp: &str) -> Vec<ReportNode> {
    let mut content = vec![ReportNode::Coded {
        code: "113811",
        name: "CT Accumulated Dose Data",
        children: vec![ReportNode::CodedNumber {
            code: "113813",
            name: "CT Dose Length Product Total",
            value: total_dlp.to_owned(),
            unit: "mGy.cm",
        }],
    }];
    for (protocol, ctdi, dlp) in events {
        content.push(ReportNode::Coded {
            code: "113819",
            name: "CT Acquisition",
            children: vec![
                ReportNode::CodedText { code: "125203", name: "Acquisition Protocol", value: (*protocol).to_owned() },
                ReportNode::Coded {
                    code: "113829",
                    name: "CT Dose",
                    children: vec![
                        ReportNode::CodedNumber {
                            code: "113830",
                            name: "Mean CTDIvol",
                            value: (*ctdi).to_owned(),
                            unit: "mGy",
                        },
                        ReportNode::CodedNumber {
                            code: "113838",
                            name: "DLP",
                            value: (*dlp).to_owned(),
                            unit: "mGy.cm",
                        },
                    ],
                },
            ],
        });
    }
    content
}

// ---------------------------------------------------------------------------
// Structured reports

/// One content item of a structured report.
#[derive(Debug, Clone)]
pub enum ReportNode {
    Container {
        name: &'static str,
        children: Vec<ReportNode>,
    },
    Text {
        name: &'static str,
        value: String,
    },
    Number {
        name: &'static str,
        value: String,
        unit: &'static str,
    },
    Code {
        name: &'static str,
        meaning: &'static str,
    },
    Date {
        name: &'static str,
        value: &'static str,
    },
    PersonName {
        name: &'static str,
        value: &'static str,
    },
    Image {
        name: &'static str,
        sop_instance_uid: String,
        frame: Option<u32>,
    },
    /// An item with a value type the fixture does not fill in.
    Other {
        name: &'static str,
        value_type: &'static str,
    },
    /// A container whose concept name has this DCM code.
    Coded {
        code: &'static str,
        name: &'static str,
        children: Vec<ReportNode>,
    },
    /// A numeric item whose concept name has this DCM code.
    CodedNumber {
        code: &'static str,
        name: &'static str,
        value: String,
        unit: &'static str,
    },
    /// A text item whose concept name has this DCM code.
    CodedText {
        code: &'static str,
        name: &'static str,
        value: String,
    },
}

impl ReportNode {
    pub fn container(name: &'static str, children: Vec<ReportNode>) -> Self {
        Self::Container { name, children }
    }

    pub fn text(name: &'static str, value: &str) -> Self {
        Self::Text { name, value: value.to_owned() }
    }

    pub fn number(name: &'static str, value: &str, unit: &'static str) -> Self {
        Self::Number { name, value: value.to_owned(), unit }
    }

    pub fn code(name: &'static str, meaning: &'static str) -> Self {
        Self::Code { name, meaning }
    }

    pub fn image(name: &'static str, sop_instance_uid: &str) -> Self {
        Self::Image { name, sop_instance_uid: sop_instance_uid.to_owned(), frame: None }
    }

    fn item(&self) -> InMemDicomObject {
        let mut item = InMemDicomObject::new_empty();
        put_str(&mut item, tags::RELATIONSHIP_TYPE, VR::CS, &["CONTAINS"]);
        match self {
            Self::Container { name, children } => {
                container(&mut item, &concept("99SYN", "99SYN", name), children);
            }
            Self::Coded { code, name, children } => {
                container(&mut item, &concept(code, "DCM", name), children);
            }
            Self::Text { name, value } => {
                value_type(&mut item, "TEXT", &concept("99SYN", "99SYN", name));
                put_str(&mut item, tags::TEXT_VALUE, VR::UT, &[value.as_str()]);
            }
            Self::CodedText { code, name, value } => {
                value_type(&mut item, "TEXT", &concept(code, "DCM", name));
                put_str(&mut item, tags::TEXT_VALUE, VR::UT, &[value.as_str()]);
            }
            Self::Number { name, value, unit } => {
                number(&mut item, &concept("99SYN", "99SYN", name), value, unit);
            }
            Self::CodedNumber { code, name, value, unit } => {
                number(&mut item, &concept(code, "DCM", name), value, unit);
            }
            Self::Code { name, meaning } => {
                value_type(&mut item, "CODE", &concept("99SYN", "99SYN", name));
                sequence(&mut item, tags::CONCEPT_CODE_SEQUENCE, vec![code_item("99SYN", "99SYN", meaning)]);
            }
            Self::Date { name, value } => {
                value_type(&mut item, "DATE", &concept("99SYN", "99SYN", name));
                put_str(&mut item, tags::DATE, VR::DA, &[value]);
            }
            Self::PersonName { name, value } => {
                value_type(&mut item, "PNAME", &concept("99SYN", "99SYN", name));
                put_str(&mut item, tags::PERSON_NAME, VR::PN, &[value]);
            }
            Self::Image { name, sop_instance_uid, frame } => {
                value_type(&mut item, "IMAGE", &concept("99SYN", "99SYN", name));
                let mut reference = InMemDicomObject::new_empty();
                put_str(&mut reference, tags::REFERENCED_SOP_CLASS_UID, VR::UI, &[uids::MR_IMAGE_STORAGE]);
                put_str(&mut reference, tags::REFERENCED_SOP_INSTANCE_UID, VR::UI, &[sop_instance_uid.as_str()]);
                if let Some(frame) = frame {
                    put_str(&mut reference, tags::REFERENCED_FRAME_NUMBER, VR::IS, &[&frame.to_string()]);
                }
                sequence(&mut item, tags::REFERENCED_SOP_SEQUENCE, vec![reference]);
            }
            Self::Other { name, value_type: kind } => {
                value_type(&mut item, kind, &concept("99SYN", "99SYN", name));
            }
        }
        item
    }
}

fn code_item(value: &str, scheme: &str, meaning: &str) -> InMemDicomObject {
    let mut code = InMemDicomObject::new_empty();
    put_str(&mut code, tags::CODE_VALUE, VR::SH, &[value]);
    put_str(&mut code, tags::CODING_SCHEME_DESIGNATOR, VR::SH, &[scheme]);
    put_str(&mut code, tags::CODE_MEANING, VR::LO, &[meaning]);
    code
}

fn concept(value: &str, scheme: &str, meaning: &str) -> InMemDicomObject {
    code_item(value, scheme, meaning)
}

fn sequence(obj: &mut InMemDicomObject, tag: Tag, items: Vec<InMemDicomObject>) {
    obj.put(DataElement::new(tag, VR::SQ, DataSetSequence::new(items, Length::UNDEFINED)));
}

fn value_type(item: &mut InMemDicomObject, kind: &str, name: &InMemDicomObject) {
    put_str(item, tags::VALUE_TYPE, VR::CS, &[kind]);
    sequence(item, tags::CONCEPT_NAME_CODE_SEQUENCE, vec![name.clone()]);
}

fn container(item: &mut InMemDicomObject, name: &InMemDicomObject, children: &[ReportNode]) {
    value_type(item, "CONTAINER", name);
    put_str(item, tags::CONTINUITY_OF_CONTENT, VR::CS, &["SEPARATE"]);
    sequence(item, tags::CONTENT_SEQUENCE, children.iter().map(ReportNode::item).collect());
}

fn number(item: &mut InMemDicomObject, name: &InMemDicomObject, value: &str, unit: &str) {
    value_type(item, "NUM", name);
    let mut measured = InMemDicomObject::new_empty();
    put_str(&mut measured, tags::NUMERIC_VALUE, VR::DS, &[value]);
    sequence(&mut measured, tags::MEASUREMENT_UNITS_CODE_SEQUENCE, vec![code_item(unit, "UCUM", unit)]);
    sequence(item, tags::MEASURED_VALUE_SEQUENCE, vec![measured]);
}

/// What a structured report says about itself and what it contains.
#[derive(Debug, Clone)]
pub struct ReportSpec {
    pub sop_class: &'static str,
    pub title: &'static str,
    /// DCM code of the title.
    pub title_code: Option<&'static str>,
    pub study_uid: String,
    pub series: u32,
    pub instance: u32,
    pub completion: Option<&'static str>,
    pub verification: Option<&'static str>,
    pub preliminary: Option<&'static str>,
    pub content_date: Option<&'static str>,
    pub character_set: &'static str,
    pub content: Vec<ReportNode>,
}

impl ReportSpec {
    /// A complete, verified report of the synthetic study, dated [`REPORT_DATE`].
    pub fn new(sop_class: &'static str, title: &'static str, content: Vec<ReportNode>) -> Self {
        Self {
            sop_class,
            title,
            title_code: None,
            study_uid: format!("{UID_ROOT}0"),
            series: 100,
            instance: 1,
            completion: Some("COMPLETE"),
            verification: Some("VERIFIED"),
            preliminary: None,
            content_date: Some(REPORT_DATE),
            character_set: "ISO_IR 192",
            content,
        }
    }

    fn object(&self) -> InMemDicomObject {
        let series_uid = format!("{UID_ROOT}{}", self.series);
        let sop_uid = format!("{series_uid}.{}", self.instance);
        let mut obj = crate::base_object(
            &self.study_uid,
            &series_uid,
            &sop_uid,
            self.sop_class,
            "SR",
            self.series,
            "Reports (synthetic)",
            self.instance,
        );
        put_str(&mut obj, tags::SPECIFIC_CHARACTER_SET, VR::CS, &[self.character_set]);
        if self.character_set != "ISO_IR 192" {
            // The name of the institution has a character that not every character set has.
            obj.remove_element(tags::INSTITUTION_NAME);
        }
        if let Some(date) = self.content_date {
            put_str(&mut obj, tags::CONTENT_DATE, VR::DA, &[date]);
            put_str(&mut obj, tags::CONTENT_TIME, VR::TM, &["101500"]);
        }
        if let Some(flag) = self.completion {
            put_str(&mut obj, tags::COMPLETION_FLAG, VR::CS, &[flag]);
        }
        if let Some(flag) = self.verification {
            put_str(&mut obj, tags::VERIFICATION_FLAG, VR::CS, &[flag]);
        }
        if let Some(flag) = self.preliminary {
            put_str(&mut obj, tags::PRELIMINARY_FLAG, VR::CS, &[flag]);
        }
        let title = match self.title_code {
            Some(code) => concept(code, "DCM", self.title),
            None => concept("18748-4", "LN", self.title),
        };
        container(&mut obj, &title, &self.content);
        obj
    }
}

/// Writes a structured report to `path`.
pub fn write_structured_report(path: &Path, spec: &ReportSpec) -> Result<(), FixtureError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let meta = FileMetaTableBuilder::new().transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN);
    spec.object().with_meta(meta).map_err(dicom_err)?.write_to_file(path).map_err(dicom_err)
}

/// The bytes of a report whose content tree is `depth` containers deep, the
/// innermost one holding `innermost_text`. Written by hand, as a sequence of
/// bytes, so that the writer's own recursion does not limit the depth.
pub fn deeply_nested_report(depth: usize, innermost_text: &str) -> Result<Vec<u8>, FixtureError> {
    let mut spec = ReportSpec::new(uids::COMPREHENSIVE_SR_STORAGE, "Deep report", Vec::new());
    spec.content_date = Some(REPORT_DATE);
    // Its own SOP Instance UID: beside the reports of the CD it is not a duplicate of one.
    spec.series = 190;
    let mut obj = spec.object();
    // (0040,A730) goes back on at the end, written by hand. Nothing after it remains in the file.
    obj.remove_element(tags::CONTENT_SEQUENCE);
    let meta = FileMetaTableBuilder::new().transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN);
    let mut bytes = Vec::new();
    obj.with_meta(meta).map_err(dicom_err)?.write_all(&mut bytes).map_err(dicom_err)?;

    let element = |out: &mut Vec<u8>, tag: Tag, vr: &[u8; 2], value: &[u8]| {
        out.extend_from_slice(&tag.group().to_le_bytes());
        out.extend_from_slice(&tag.element().to_le_bytes());
        out.extend_from_slice(vr);
        out.extend_from_slice(&(value.len() as u16).to_le_bytes());
        out.extend_from_slice(value);
    };
    let open_sequence = |out: &mut Vec<u8>| {
        out.extend_from_slice(&tags::CONTENT_SEQUENCE.group().to_le_bytes());
        out.extend_from_slice(&tags::CONTENT_SEQUENCE.element().to_le_bytes());
        out.extend_from_slice(b"SQ\0\0");
        out.extend_from_slice(&u32::MAX.to_le_bytes());
    };
    const ITEM: [u8; 8] = [0xFE, 0xFF, 0x00, 0xE0, 0xFF, 0xFF, 0xFF, 0xFF];
    const ITEM_END: [u8; 8] = [0xFE, 0xFF, 0x0D, 0xE0, 0x00, 0x00, 0x00, 0x00];
    const SEQUENCE_END: [u8; 8] = [0xFE, 0xFF, 0xDD, 0xE0, 0x00, 0x00, 0x00, 0x00];
    for _ in 0..depth {
        open_sequence(&mut bytes);
        bytes.extend_from_slice(&ITEM);
        element(&mut bytes, tags::VALUE_TYPE, b"CS", b"CONTAINER");
    }
    // Under the deepest container: one text item.
    open_sequence(&mut bytes);
    bytes.extend_from_slice(&ITEM);
    element(&mut bytes, tags::VALUE_TYPE, b"CS", b"TEXT");
    let mut text = innermost_text.as_bytes().to_vec();
    if text.len() % 2 == 1 {
        text.push(b' ');
    }
    bytes.extend_from_slice(&tags::TEXT_VALUE.group().to_le_bytes());
    bytes.extend_from_slice(&tags::TEXT_VALUE.element().to_le_bytes());
    bytes.extend_from_slice(b"UT\0\0");
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&text);
    bytes.extend_from_slice(&ITEM_END);
    bytes.extend_from_slice(&SEQUENCE_END);
    for _ in 0..depth {
        bytes.extend_from_slice(&ITEM_END);
        bytes.extend_from_slice(&SEQUENCE_END);
    }
    Ok(bytes)
}

/// Writes `pdf` to `path` as an Encapsulated PDF object.
pub fn write_encapsulated_pdf(path: &Path, study_uid: &str, title: &str, pdf: &[u8]) -> Result<(), FixtureError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let series_uid = format!("{UID_ROOT}102");
    let mut obj = crate::base_object(
        study_uid,
        &series_uid,
        &format!("{series_uid}.1"),
        uids::ENCAPSULATED_PDF_STORAGE,
        "DOC",
        102,
        "Documents (synthetic)",
        1,
    );
    put_str(&mut obj, tags::CONTENT_DATE, VR::DA, &[REPORT_DATE]);
    put_str(&mut obj, tags::CONTENT_TIME, VR::TM, &["101500"]);
    put_str(&mut obj, tags::DOCUMENT_TITLE, VR::ST, &[title]);
    put_str(&mut obj, tags::MIME_TYPE_OF_ENCAPSULATED_DOCUMENT, VR::LO, &["application/pdf"]);
    // A value of an odd length is padded; the length of the document says where it ends.
    let mut bytes = pdf.to_vec();
    obj.put(DataElement::new(tags::ENCAPSULATED_DOCUMENT_LENGTH, VR::UL, PrimitiveValue::from(bytes.len() as u32)));
    if bytes.len() % 2 == 1 {
        bytes.push(0);
    }
    obj.put(DataElement::new(tags::ENCAPSULATED_DOCUMENT, VR::OB, PrimitiveValue::U8(bytes.into())));
    let meta = FileMetaTableBuilder::new().transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN);
    obj.with_meta(meta).map_err(dicom_err)?.write_to_file(path).map_err(dicom_err)
}

/// Adds directory records for `documents` to the DICOMDIR at `dicomdir`.
fn add_dicomdir_records(dicomdir: &Path, root: &Path, documents: &[(&PathBuf, &str)]) -> Result<(), FixtureError> {
    let mut file = OpenFileOptions::new().open_file(dicomdir).map_err(dicom_err)?;
    let mut records: Vec<InMemDicomObject> =
        file.get(tags::DIRECTORY_RECORD_SEQUENCE).and_then(|e| e.items()).unwrap_or(&[]).to_vec();
    for (path, kind) in documents {
        let relative = path.strip_prefix(root).map_err(dicom_err)?;
        let components: Vec<String> =
            relative.components().map(|c| c.as_os_str().to_string_lossy().to_uppercase()).collect();
        let mut record = InMemDicomObject::new_empty();
        record.put(DataElement::new(tags::OFFSET_OF_THE_NEXT_DIRECTORY_RECORD, VR::UL, PrimitiveValue::from(0u32)));
        record.put(DataElement::new(tags::RECORD_IN_USE_FLAG, VR::US, PrimitiveValue::from(0xFFFFu16)));
        record.put(DataElement::new(
            tags::OFFSET_OF_REFERENCED_LOWER_LEVEL_DIRECTORY_ENTITY,
            VR::UL,
            PrimitiveValue::from(0u32),
        ));
        record.put(DataElement::new(tags::DIRECTORY_RECORD_TYPE, VR::CS, PrimitiveValue::from(*kind)));
        record.put(DataElement::new(
            tags::REFERENCED_FILE_ID,
            VR::CS,
            PrimitiveValue::Strs(components.into_iter().collect()),
        ));
        records.push(record);
    }
    file.put(DataElement::new(
        tags::DIRECTORY_RECORD_SEQUENCE,
        VR::SQ,
        DataSetSequence::new(records, Length::UNDEFINED),
    ));
    file.write_to_file(dicomdir).map_err(dicom_err)
}

// ---------------------------------------------------------------------------
// RTF and HTML

/// An RTF report in Windows-1251: [`RTF_CYRILLIC_TEXT`] as `\'xx` escapes
/// after the header a word processor writes, with what must not be shown (a
/// font table, a colour table, a stylesheet, document information, an embedded
/// picture) around it.
pub fn rtf_cyrillic() -> Vec<u8> {
    let mut rtf: Vec<u8> = Vec::new();
    rtf.extend_from_slice(
        br"{\rtf1\ansi\ansicpg1251\deff0{\fonttbl{\f0\fnil\fcharset204 Times New Roman Cyr;}}{\colortbl;\red0\green0\blue0;}",
    );
    rtf.extend_from_slice(br"{\stylesheet{\s0 Normal;}}{\info{\author Hidden author}}{\*\generator Synthetic 1.0;}");
    rtf.extend_from_slice(br"\viewkind4\uc1\pard\f0\fs24 ");
    for ch in RTF_CYRILLIC_TEXT.chars() {
        if ch.is_ascii() {
            rtf.push(ch as u8);
        } else {
            // In Windows-1251 the letters А to я run in order from 0xC0.
            let byte = 0xC0 + (ch as u32 - 'А' as u32) as u8;
            rtf.extend_from_slice(format!("\\'{byte:02x}").as_bytes());
        }
    }
    rtf.extend_from_slice(br"\par{\pict\wmetafile8 0123456789abcdef}\par}");
    rtf
}

/// An HTML report with a script, a style sheet, a remote image and a link.
pub const HTML_REPORT: &str = r#"<!DOCTYPE html>
<html><head><title>Hidden title</title>
<style>p { color: red; }</style>
<script>document.write("SCRIPT RAN"); fetch("http://example.invalid/leak");</script>
</head><body>
<h1>Radiology report</h1>
<p>The lungs are clear &amp; the heart is normal.</p>
<img src="http://example.invalid/tracker.png" alt="Remote picture">
<p>See <a href="http://example.invalid/more">the archive</a> for more.</p>
<!-- A comment that is not shown. -->
</body></html>
"#;

// ---------------------------------------------------------------------------
// PDF

/// A4 in points.
const PAGE_SIZE: (u32, u32) = (595, 842);

/// The objects of a PDF file; object `n` is `objects[n - 1]`.
#[derive(Default)]
struct PdfFile {
    objects: Vec<Vec<u8>>,
}

impl PdfFile {
    fn reserve(&mut self) -> usize {
        self.objects.push(Vec::new());
        self.objects.len()
    }

    fn add(&mut self, body: impl Into<Vec<u8>>) -> usize {
        self.objects.push(body.into());
        self.objects.len()
    }

    fn set(&mut self, id: usize, body: impl Into<Vec<u8>>) {
        self.objects[id - 1] = body.into();
    }

    fn stream(&mut self, dictionary: &str, data: &[u8]) -> usize {
        let mut body = format!("<< {dictionary} /Length {} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        self.add(body)
    }

    /// A page tree with one page per content stream, the catalogue and the file.
    fn finish(mut self, resources: &str, pages: &[Vec<u8>]) -> Vec<u8> {
        let tree = self.reserve();
        let mut kids = String::new();
        for content in pages {
            let content = self.stream("", content);
            let page = self.add(format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 {} {}] /Resources << {resources} >> /Contents {content} 0 R >>",
                PAGE_SIZE.0, PAGE_SIZE.1
            ));
            kids.push_str(&format!("{page} 0 R "));
        }
        self.set(tree, format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()));
        let catalog = self.add(format!("<< /Type /Catalog /Pages {tree} 0 R >>"));

        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(self.objects.len());
        for (i, object) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(object);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", self.objects.len() + 1).as_bytes());
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {} /Root {catalog} 0 R >>\nstartxref\n{xref}\n%%EOF\n", self.objects.len() + 1)
                .as_bytes(),
        );
        out
    }
}

fn pdf_string(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '(' | ')' | '\\' => format!("\\{c}"),
            c => c.to_string(),
        })
        .collect()
}

/// The distance of line `index` of a page from the top of the page, in points.
pub const fn pdf_line_top(index: u32) -> u32 {
    60 + index * 28
}

/// Lines of text of 16 points in the font `/F1`.
fn text_content(lines: &[&str]) -> Vec<u8> {
    let mut content = String::new();
    for (i, line) in lines.iter().enumerate() {
        // PDF counts from the bottom of the page; the baseline lies 12 points below the top of the line.
        let y = PAGE_SIZE.1 - pdf_line_top(i as u32) - 12;
        content.push_str(&format!("BT /F1 16 Tf 72 {y} Td ({}) Tj ET\n", pdf_string(line)));
    }
    content.into_bytes()
}

/// A PDF with one page per entry of `pages`, each with its lines of text in
/// the standard font Helvetica, which is not embedded.
pub fn text_pdf(pages: &[&[&str]]) -> Vec<u8> {
    let mut pdf = PdfFile::default();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
    let contents: Vec<Vec<u8>> = pages.iter().map(|lines| text_content(lines)).collect();
    pdf.finish(&format!("/Font << /F1 {font} 0 R >>"), &contents)
}

/// A PDF of one page with lines of text in an embedded TrueType font: the
/// monospaced Hack that comes with egui, of which every glyph is 602
/// thousandths of an em wide.
pub fn embedded_font_pdf(lines: &[&str]) -> Vec<u8> {
    let mut pdf = PdfFile::default();
    let font = epaint_default_fonts::HACK_REGULAR;
    let program = pdf.stream(&format!("/Length1 {}", font.len()), font);
    let descriptor = pdf.add(format!(
        "<< /Type /FontDescriptor /FontName /Embedded /Flags 33 /FontBBox [-500 -300 1100 1000] /ItalicAngle 0 \
         /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 /FontFile2 {program} 0 R >>"
    ));
    let widths = vec!["602"; 224].join(" ");
    let font = pdf.add(format!(
        "<< /Type /Font /Subtype /TrueType /BaseFont /Embedded /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 255 \
         /Widths [{widths}] /FontDescriptor {descriptor} 0 R >>"
    ));
    pdf.finish(&format!("/Font << /F1 {font} 0 R >>"), &[text_content(lines)])
}

/// How the picture of a scanned page is compressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanCompression {
    Flate,
    Jpeg,
}

/// Size of the picture of a scanned page, in pixels.
const SCAN_SIZE: (u32, u32) = (496, 702);

/// A picture of a page of text: a light background with dark bars for lines.
fn scan_pixels() -> Vec<u8> {
    let (w, h) = SCAN_SIZE;
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let line = y > 80 && y % 40 < 12 && x > 60 && x < w - 60;
            let grain = ((x * 7 + y * 13) % 5) as u8;
            pixels.push(if line { 30 + grain } else { 235 + grain });
        }
    }
    pixels
}

/// The picture compressed as JPEG, by the same encoder that writes the JPEG
/// images of the phantom.
fn jpeg_of(pixels: Vec<u8>) -> Result<Vec<u8>, FixtureError> {
    let (w, h) = SCAN_SIZE;
    let mut obj = InMemDicomObject::new_empty();
    put_str(&mut obj, tags::SOP_CLASS_UID, VR::UI, &[uids::SECONDARY_CAPTURE_IMAGE_STORAGE]);
    put_str(&mut obj, tags::SOP_INSTANCE_UID, VR::UI, &["2.25.1"]);
    crate::put_u16(&mut obj, tags::SAMPLES_PER_PIXEL, 1);
    put_str(&mut obj, tags::PHOTOMETRIC_INTERPRETATION, VR::CS, &["MONOCHROME2"]);
    crate::put_u16(&mut obj, tags::ROWS, h as u16);
    crate::put_u16(&mut obj, tags::COLUMNS, w as u16);
    crate::put_u16(&mut obj, tags::BITS_ALLOCATED, 8);
    crate::put_u16(&mut obj, tags::BITS_STORED, 8);
    crate::put_u16(&mut obj, tags::HIGH_BIT, 7);
    crate::put_u16(&mut obj, tags::PIXEL_REPRESENTATION, 0);
    obj.put(DataElement::new(tags::PIXEL_DATA, VR::OB, PrimitiveValue::U8(pixels.into())));
    let meta = FileMetaTableBuilder::new().transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN);
    let mut file = obj.with_meta(meta).map_err(dicom_err)?;
    file.transcode(&dicom_transfer_syntax_registry::entries::JPEG_BASELINE.erased()).map_err(dicom_err)?;
    let fragments = file.get(tags::PIXEL_DATA).and_then(|e| e.fragments());
    fragments.and_then(|f| f.first().cloned()).ok_or_else(|| FixtureError::Dicom("no JPEG fragment".to_owned()))
}

/// A PDF of one page that is a scanned picture of a page, without any text:
/// light with dark bars. A bar covers the rows of the picture `y` with
/// `y > 80 && y % 40 < 12`, from column 60 to 60 columns before the right
/// edge, of a picture of 496 × 702 pixels that fills the page.
pub fn scanned_pdf(compression: ScanCompression) -> Result<Vec<u8>, FixtureError> {
    let (w, h) = SCAN_SIZE;
    let pixels = scan_pixels();
    let (filter, data) = match compression {
        ScanCompression::Flate => {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&pixels)?;
            ("/FlateDecode", encoder.finish()?)
        }
        ScanCompression::Jpeg => ("/DCTDecode", jpeg_of(pixels)?),
    };
    let mut pdf = PdfFile::default();
    let image = pdf.stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter {filter}"
        ),
        &data,
    );
    let content = format!("q {} 0 0 {} 0 0 cm /Im1 Do Q\n", PAGE_SIZE.0, PAGE_SIZE.1).into_bytes();
    Ok(pdf.finish(&format!("/XObject << /Im1 {image} 0 R >>"), &[content]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cyrillic_rtf_holds_only_escapes_in_windows_1251() {
        let rtf = rtf_cyrillic();
        assert!(rtf.is_ascii());
        let text = String::from_utf8_lossy(&rtf);
        // "З" is 0xC7 and "а" is 0xE0 in Windows-1251.
        assert!(text.contains(r"\'c7\'e0\'ea"), "{text}");
    }

    #[test]
    fn pdf_files_have_their_table_of_objects_right() {
        let pdf = text_pdf(&[&["One"], &["Two"]]);
        assert!(pdf.starts_with(b"%PDF-1.4"));
        let text = String::from_utf8_lossy(&pdf).into_owned();
        let xref = text.rfind("startxref\n").map(|at| &text[at + 10..]).expect("startxref");
        let offset: usize = xref.lines().next().and_then(|l| l.parse().ok()).expect("offset");
        assert!(pdf[offset..].starts_with(b"xref\n"));
        assert_eq!(text.matches("/Type /Page ").count(), 2);
    }

    #[test]
    fn deep_report_is_a_readable_dicom_file_at_a_small_depth() {
        let bytes = deeply_nested_report(3, "Deep text").expect("bytes");
        assert_eq!(&bytes[128..132], b"DICM");
        let file = OpenFileOptions::new().from_reader(bytes.as_slice()).expect("parse");
        let mut item: &InMemDicomObject = &file;
        for _ in 0..3 {
            item = &item.get(tags::CONTENT_SEQUENCE).and_then(|e| e.items()).expect("nested")[0];
        }
        let inner = &item.get(tags::CONTENT_SEQUENCE).and_then(|e| e.items()).expect("text")[0];
        assert_eq!(
            inner.get(tags::TEXT_VALUE).and_then(|e| e.to_str().ok()).as_deref().map(str::trim),
            Some("Deep text")
        );
    }
}
