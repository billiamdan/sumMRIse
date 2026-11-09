//! Reports and documents on a disc: structured reports, dose reports and PDF
//! documents stored in DICOM, and report files next to the images.
//!
//! [`DocumentRef`] is what the index knows about a document: its kind, title,
//! date and where it is. [`read_document`] reads one into memory, through
//! [`Media`], so that it works alike for a folder, a ZIP archive and a disc
//! image and never writes a file. Nothing is ever executed or handed to
//! another application: a document is a data structure ([`StructuredReport`],
//! [`DoseReport`]), the bytes of a PDF, or plain text.
//!
//! Documents are untrusted input and are shown exactly as written. Only an
//! allow-list of types is listed, sizes and recursion are bounded, and what
//! cannot be read is an error that the document view shows.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;
use summrise_i18n::{tr, tr_ctx, tr_n};

use crate::attrs;
use crate::discovery::normalise_name;
use crate::error::{DicomError, Result};
use crate::format;
use crate::media::Media;
use crate::progress::Progress;

mod dose;
mod html;
mod rtf;
mod sr;
mod text;

pub use dose::{DoseEvent, DoseReport};
pub use sr::{Completion, ReportContent, ReportItem, ReportStatus, StructuredReport, Verification};
pub use text::PlainText;

/// Largest document read: larger ones are listed as too large to display.
pub const MAX_DOCUMENT_BYTES: u64 = 50 << 20;

/// Most report files listed. A folder that is not a disc (say, the downloads
/// of the user) can hold thousands of PDFs that have nothing to do with the
/// study; the list stays usable and a warning says what is not listed.
pub const MAX_LISTED_FILES: usize = 200;

/// The SOP classes of structured reports that are shown. Key Object Selection
/// is not among them: it says which images matter and has no text to read.
const REPORT_CLASSES: &[&str] = &[
    uids::BASIC_TEXT_SR_STORAGE,
    uids::ENHANCED_SR_STORAGE,
    uids::COMPREHENSIVE_SR_STORAGE,
    uids::COMPREHENSIVE3_DSR_STORAGE,
    uids::EXTENSIBLE_SR_STORAGE,
    uids::PROCEDURE_LOG_STORAGE,
    uids::MAMMOGRAPHY_CADSR_STORAGE,
    uids::CHEST_CADSR_STORAGE,
    uids::COLON_CADSR_STORAGE,
    uids::IMPLANTATION_PLAN_SR_STORAGE,
    uids::ACQUISITION_CONTEXT_SR_STORAGE,
    uids::SIMPLIFIED_ADULT_ECHO_SR_STORAGE,
    uids::RADIOPHARMACEUTICAL_RADIATION_DOSE_SR_STORAGE,
    uids::PATIENT_RADIATION_DOSE_SR_STORAGE,
];

/// The SOP classes of X-ray dose reports.
const DOSE_CLASSES: &[&str] = &[uids::X_RAY_RADIATION_DOSE_SR_STORAGE, uids::ENHANCED_X_RAY_RADIATION_DOSE_SR_STORAGE];

/// The document title (DCM 113701) of a dose report stored in another class.
const DOSE_REPORT_TITLE_CODE: &str = "113701";

/// What kind of document something is, and so how it is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocumentKind {
    /// A structured report in DICOM.
    Report,
    /// An X-ray radiation dose report in DICOM.
    DoseReport,
    /// A PDF stored in DICOM (Encapsulated PDF).
    StoredPdf,
    PdfFile,
    RtfFile,
    TextFile,
    HtmlFile,
}

impl DocumentKind {
    /// The name of the kind, as the list of documents shows it.
    pub fn label(self) -> &'static str {
        match self {
            // Translators: the types of the documents on a disc, as the list of documents shows them.
            Self::Report => tr_ctx!("document type", "Report"),
            Self::DoseReport => tr_ctx!("document type", "Dose report"),
            Self::StoredPdf => tr_ctx!("document type", "PDF document"),
            Self::PdfFile => tr_ctx!("document type", "PDF file"),
            Self::RtfFile => tr_ctx!("document type", "RTF file"),
            Self::TextFile => tr_ctx!("document type", "Text file"),
            Self::HtmlFile => tr_ctx!("document type", "HTML file"),
        }
    }

    /// Whether it is a file next to the images rather than a DICOM object.
    pub fn is_file(self) -> bool {
        matches!(self, Self::PdfFile | Self::RtfFile | Self::TextFile | Self::HtmlFile)
    }

    pub fn is_pdf(self) -> bool {
        matches!(self, Self::StoredPdf | Self::PdfFile)
    }

    /// The kind of a report file, from the name of the file.
    fn of_file(name: &str) -> Option<Self> {
        match Path::new(name).extension()?.to_string_lossy().to_ascii_lowercase().as_str() {
            "pdf" => Some(Self::PdfFile),
            "rtf" => Some(Self::RtfFile),
            "txt" => Some(Self::TextFile),
            "htm" | "html" => Some(Self::HtmlFile),
            _ => None,
        }
    }
}

/// One document of a study or of the medium.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRef {
    pub kind: DocumentKind,
    /// The title as the document has it, or the name of the file. Text from
    /// the disc, never translated.
    pub title: Option<String>,
    /// DICOM date (DA) of the document's content; `None` for a report file.
    pub date: Option<String>,
    /// Where it is in the media of the index.
    pub path: PathBuf,
    /// SOP Instance UID of a DICOM document.
    pub sop_instance_uid: Option<String>,
    /// It is larger than [`MAX_DOCUMENT_BYTES`] and is not loaded.
    pub too_large: bool,
}

impl DocumentRef {
    /// The title to show: the document's own, else the name of its kind.
    pub fn display_title(&self) -> &str {
        self.title.as_deref().unwrap_or(self.kind.label())
    }

    /// The date of the content as `YYYY-MM-DD`.
    pub fn display_date(&self) -> Option<String> {
        self.date.as_deref().and_then(format::date)
    }

    fn sort_key(&self) -> (u8, &Option<String>, &Option<String>, &Path) {
        let rank = match self.kind {
            DocumentKind::Report => 0,
            DocumentKind::StoredPdf => 1,
            DocumentKind::DoseReport => 2,
            _ => 3,
        };
        (rank, &self.date, &self.title, &self.path)
    }
}

/// Puts the documents of a study in the order they are listed: reports first,
/// dose reports after them, each by date and title.
pub(crate) fn sort(documents: &mut [DocumentRef]) {
    documents.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
}

/// The document that a DICOM object without images is, from the allow-list of
/// SOP classes; `None` for anything else (presentation states, key objects…).
pub(crate) fn classify(
    obj: &InMemDicomObject,
    media: &Media,
    path: &Path,
    media_sop_class: &str,
) -> Option<DocumentRef> {
    let sop_class = attrs::text(obj, tags::SOP_CLASS_UID).unwrap_or_else(|| media_sop_class.to_owned());
    let concept = attrs::first_item(obj, tags::CONCEPT_NAME_CODE_SEQUENCE);
    let concept_title = concept.and_then(|name| attrs::text(name, tags::CODE_MEANING));
    let (kind, title) = if sop_class == uids::ENCAPSULATED_PDF_STORAGE {
        let mime = attrs::text(obj, tags::MIME_TYPE_OF_ENCAPSULATED_DOCUMENT);
        if mime.is_some_and(|mime| !mime.eq_ignore_ascii_case("application/pdf")) {
            return None;
        }
        (DocumentKind::StoredPdf, attrs::text(obj, tags::DOCUMENT_TITLE).or(concept_title))
    } else if DOSE_CLASSES.contains(&sop_class.as_str()) {
        (DocumentKind::DoseReport, concept_title)
    } else if REPORT_CLASSES.contains(&sop_class.as_str()) {
        // Some scanners store the dose report as a Comprehensive SR: its title code says what it is.
        let code = concept.and_then(|name| attrs::text(name, tags::CODE_VALUE));
        let dose = code.as_deref() == Some(DOSE_REPORT_TITLE_CODE);
        (if dose { DocumentKind::DoseReport } else { DocumentKind::Report }, concept_title)
    } else {
        return None;
    };
    let title = title.or_else(|| attrs::text(obj, tags::SERIES_DESCRIPTION));
    let date = [tags::CONTENT_DATE, tags::INSTANCE_CREATION_DATE, tags::SERIES_DATE, tags::STUDY_DATE]
        .into_iter()
        .find_map(|tag| attrs::text(obj, tag));
    Some(DocumentRef {
        kind,
        title,
        date,
        path: path.to_path_buf(),
        sop_instance_uid: attrs::text(obj, tags::SOP_INSTANCE_UID),
        too_large: media.size(path).is_some_and(|size| size > MAX_DOCUMENT_BYTES),
    })
}

/// Extensions of executable software. A folder that holds one is the viewer
/// software of the disc, not a place for reports.
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe", "dll", "msi", "bat", "cmd", "com", "scr", "pif", "ocx", "sys", "drv", "cpl", "app", "jar", "class", "dylib",
    "so", "sh", "vbs", "ps1",
];

fn is_executable(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| EXECUTABLE_EXTENSIONS.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
}

/// The folders above `path`, up to and including `root`.
fn folders_of<'a>(path: &'a Path, root: &'a Path) -> impl Iterator<Item = &'a Path> {
    path.ancestors().skip(1).take_while(move |folder| folder.starts_with(root))
}

/// Report files found on the medium, and what is said about those not listed.
#[derive(Debug, Clone, Default)]
pub(crate) struct LooseFiles {
    pub(crate) documents: Vec<DocumentRef>,
    pub(crate) warnings: Vec<String>,
}

/// Finds the report files among `files`, the files below `root`, by the
/// extension of their name: everywhere except in the folders of viewer
/// software, which are the folders that hold executable software, directly or
/// further down, and no DICOM file of the disc (`dicom_files`, the DICOMDIR
/// included). The root of a disc holds the images, so an autorun program
/// beside `REPORT.PDF` does not hide the report.
pub(crate) fn find_loose_files(
    media: &Media,
    files: impl Iterator<Item = PathBuf>,
    root: &Path,
    dicom_files: &[PathBuf],
    progress: &Progress,
) -> Result<LooseFiles> {
    progress.begin(tr!("Looking for report files"), 0);
    let dicom: HashSet<&PathBuf> = dicom_files.iter().collect();
    let mut with_dicom: HashSet<PathBuf> = HashSet::new();
    let mut with_software: HashSet<PathBuf> = HashSet::new();
    let mut candidates: Vec<(PathBuf, DocumentKind, String)> = Vec::new();
    for path in files {
        if progress.is_cancelled() {
            return Err(DicomError::Cancelled);
        }
        if let Some(software) = software_folder(&path, root) {
            with_software.extend(folders_of(&software, root).map(Path::to_path_buf));
            with_software.insert(software);
            continue;
        }
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        if dicom.contains(&path) || normalise_name(&name) == "dicomdir" {
            with_dicom.extend(folders_of(&path, root).map(Path::to_path_buf));
            continue;
        }
        // On a mounted disc the name may still end in the ISO 9660 version.
        let name = name.trim().trim_end_matches(";1").to_owned();
        if let Some(kind) = DocumentKind::of_file(&name) {
            candidates.push((path, kind, name));
        }
    }
    // The images themselves can lie below a folder that is not on the list of files.
    for file in dicom_files {
        with_dicom.extend(folders_of(file, root).map(Path::to_path_buf));
    }
    let is_software = |folder: &Path| with_software.contains(folder) && !with_dicom.contains(folder);
    let mut documents: Vec<DocumentRef> = candidates
        .into_iter()
        .filter(|(path, _, _)| !folders_of(path, root).any(is_software))
        .filter_map(|(path, kind, name)| {
            let size = media.size(&path).unwrap_or(0);
            (size > 0).then_some(DocumentRef {
                kind,
                title: Some(name),
                date: None,
                sop_instance_uid: None,
                too_large: size > MAX_DOCUMENT_BYTES,
                path,
            })
        })
        .collect();
    documents.sort_by(|a, b| a.path.cmp(&b.path));
    let mut warnings = Vec::new();
    if documents.len() > MAX_LISTED_FILES {
        warnings.push(tr_n!(
            "{n} more report file on this medium is not listed",
            "{n} more report files on this medium are not listed",
            documents.len() - MAX_LISTED_FILES
        ));
        documents.truncate(MAX_LISTED_FILES);
    }
    Ok(LooseFiles { documents, warnings })
}

/// The folder of software that `path` shows there is, if it is executable
/// software: its parent folder, or the application bundle (`Viewer.app`) that
/// it lies in.
fn software_folder(path: &Path, root: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut folder = root.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else { continue };
        if components.peek().is_none() {
            break;
        }
        folder.push(name);
        if Path::new(name).extension().is_some_and(|e| e.eq_ignore_ascii_case("app")) {
            return Some(folder);
        }
    }
    let name = path.file_name()?.to_string_lossy();
    is_executable(name.trim().trim_end_matches(";1")).then(|| path.parent().unwrap_or(root).to_path_buf())
}

/// What a document holds, read into memory.
#[derive(Debug, Clone)]
pub enum DocumentContent {
    Report(StructuredReport),
    Dose(DoseReport),
    /// The bytes of a PDF, and the status a PDF stored in DICOM records.
    Pdf {
        bytes: Vec<u8>,
        status: Option<ReportStatus>,
    },
    Text(PlainText),
}

/// The error for a DICOM document that cannot be parsed.
fn unreadable(media: &Media, document: &DocumentRef, reason: String) -> DicomError {
    DicomError::Parse { path: media.display_path(&document.path), reason }
}

/// Reads a document into memory. Blocking; run it off the UI thread.
pub fn read_document(media: &Media, document: &DocumentRef) -> Result<DocumentContent> {
    if document.too_large || media.size(&document.path).is_some_and(|size| size > MAX_DOCUMENT_BYTES) {
        return Err(DicomError::Document(too_large_message()));
    }
    match document.kind {
        DocumentKind::Report | DocumentKind::DoseReport | DocumentKind::StoredPdf => {
            let file = media
                .open_object(&document.path, Some(tags::PIXEL_DATA))
                .map_err(|reason| unreadable(media, document, reason))?;
            let obj: &InMemDicomObject = &file;
            match document.kind {
                DocumentKind::Report => Ok(DocumentContent::Report(sr::read(obj))),
                DocumentKind::DoseReport => Ok(DocumentContent::Dose(dose::read(obj))),
                _ => stored_pdf(obj),
            }
        }
        DocumentKind::PdfFile => Ok(DocumentContent::Pdf { bytes: media.read(&document.path)?, status: None }),
        DocumentKind::RtfFile => {
            rtf::to_text(&media.read(&document.path)?).map(DocumentContent::Text).map_err(DicomError::Document)
        }
        DocumentKind::HtmlFile => Ok(DocumentContent::Text(html::to_text(&media.read(&document.path)?))),
        DocumentKind::TextFile => {
            text::decode_file(&media.read(&document.path)?).map(DocumentContent::Text).map_err(DicomError::Document)
        }
    }
}

/// The message for a document larger than [`MAX_DOCUMENT_BYTES`].
pub fn too_large_message() -> String {
    tr!("This document is larger than {limit}, the most that is displayed.", limit = size_limit())
}

fn size_limit() -> String {
    format!("{} MB", MAX_DOCUMENT_BYTES >> 20)
}

/// The bytes of the PDF of an Encapsulated PDF object.
fn stored_pdf(obj: &InMemDicomObject) -> Result<DocumentContent> {
    let missing = || DicomError::Document(tr!("this object holds no document").to_owned());
    let element = obj.get(tags::ENCAPSULATED_DOCUMENT).ok_or_else(missing)?;
    let mut bytes = element.to_bytes().map_err(|_| missing())?.into_owned();
    // A value of an odd length is padded; the length of the document says where it ends.
    if let Some(length) = attrs::uint(obj, tags::ENCAPSULATED_DOCUMENT_LENGTH)
        && (length as usize) < bytes.len()
    {
        bytes.truncate(length as usize);
    }
    let status = attrs::text(obj, tags::VERIFICATION_FLAG).map(|_| ReportStatus::read(obj));
    Ok(DocumentContent::Pdf { bytes, status })
}

#[cfg(test)]
mod tests {
    use summrise_testkit::reports::{self, DOSE_REPORT_TITLE, MR_REPORT_TITLE, ReportSpec};

    use super::*;

    fn folder_media() -> Media {
        Media::folder()
    }

    fn loose(root: &Path, files: &[&str], dicom: &[&str]) -> Vec<String> {
        for file in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            std::fs::write(path, "content").expect("write");
        }
        let all: Vec<PathBuf> = files.iter().map(|f| root.join(f)).collect();
        let dicom: Vec<PathBuf> = dicom.iter().map(|f| root.join(f)).collect();
        let found = find_loose_files(&folder_media(), all.into_iter(), root, &dicom, &Progress::new()).expect("found");
        found
            .documents
            .iter()
            .map(|d| d.path.strip_prefix(root).expect("below").to_string_lossy().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn viewer_folders_are_left_out_and_reports_are_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let found = loose(
            dir.path(),
            &[
                "AUTORUN.INF",
                "DICOMDIR",
                "DICOM/ST1/IM1",
                "REPORT.PDF",
                "REPORTS/report_001.rtf",
                "VIEWER/viewer.exe",
                "VIEWER/manual.pdf",
                "VIEWER/readme.txt",
                "VIEWER/help/index.html",
            ],
            &["DICOM/ST1/IM1"],
        );
        assert_eq!(found, ["REPORT.PDF", "REPORTS/report_001.rtf"]);
    }

    #[test]
    fn software_further_down_makes_the_whole_folder_software() {
        let dir = tempfile::tempdir().expect("tempdir");
        let found = loose(
            dir.path(),
            &[
                "DICOM/IM1",
                "TOOLS/bin/viewer.dll",
                "TOOLS/docs/manual.pdf",
                "Viewer.app/Contents/Resources/License.rtf",
            ],
            &["DICOM/IM1"],
        );
        assert_eq!(found, [] as [&str; 0]);
    }

    /// An autorun program in the root does not hide what lies next to it: the root holds the images.
    #[test]
    fn an_executable_beside_the_images_hides_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let found = loose(dir.path(), &["AUTORUN.EXE", "REPORT.PDF", "IM1", "READ ME.txt", "Report.HTM"], &["IM1"]);
        assert_eq!(found, ["READ ME.txt", "REPORT.PDF", "Report.HTM"]);
        // A folder with images and software in it is a folder of images.
        let dir = tempfile::tempdir().expect("tempdir");
        let found = loose(dir.path(), &["CD/viewer.exe", "CD/images/IM1", "CD/report.pdf"], &["CD/images/IM1"]);
        assert_eq!(found, ["CD/report.pdf"]);
    }

    #[test]
    fn only_the_types_on_the_list_are_listed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let found = loose(
            dir.path(),
            &["a.doc", "b.docx", "c.js", "d.png", "e.pdf", "f.RTF", "g.txt", "h.htm", "i.html", "IM1", "j.exe.txt"],
            &["IM1"],
        );
        assert_eq!(found, ["e.pdf", "f.RTF", "g.txt", "h.htm", "i.html", "j.exe.txt"]);
        // Names on a mounted disc can end in the version of ISO 9660, which is not shown.
        assert_eq!(DocumentKind::of_file("REPORT.PDF"), Some(DocumentKind::PdfFile));
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("REPORT.PDF;1"), "%PDF").expect("write");
        let files = vec![dir.path().join("REPORT.PDF;1")];
        let found =
            find_loose_files(&folder_media(), files.into_iter(), dir.path(), &[], &Progress::new()).expect("ok");
        assert_eq!(found.documents[0].title.as_deref(), Some("REPORT.PDF"));
        assert_eq!(found.documents[0].kind, DocumentKind::PdfFile);
    }

    #[test]
    fn a_folder_full_of_pdfs_is_cut_off_with_a_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let names: Vec<String> = (0..MAX_LISTED_FILES + 3).map(|i| format!("{i:04}.pdf")).collect();
        for name in &names {
            std::fs::write(dir.path().join(name), "%PDF").expect("write");
        }
        let files = names.iter().map(|n| dir.path().join(n));
        let found = find_loose_files(&folder_media(), files, dir.path(), &[], &Progress::new()).expect("ok");
        assert_eq!(found.documents.len(), MAX_LISTED_FILES);
        assert_eq!(found.warnings, ["3 more report files on this medium are not listed"]);
    }

    #[test]
    fn an_oversized_file_is_listed_as_too_large_and_is_not_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("HUGE.PDF");
        let file = std::fs::File::create(&path).expect("create");
        // Sparse: nothing is actually written.
        file.set_len(MAX_DOCUMENT_BYTES + 1).expect("set length");
        let found =
            find_loose_files(&folder_media(), [path].into_iter(), dir.path(), &[], &Progress::new()).expect("ok");
        let huge = &found.documents[0];
        assert!(huge.too_large);
        let error = read_document(&folder_media(), huge).expect_err("not loaded");
        assert_eq!(error.to_string(), "This document is larger than 50 MB, the most that is displayed.");
    }

    fn classified(class: &'static str, title: &'static str, code: Option<&'static str>) -> Option<DocumentRef> {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("SR");
        let spec = ReportSpec { title_code: code, ..ReportSpec::new(class, title, Vec::new()) };
        reports::write_structured_report(&path, &spec).expect("report");
        let file = folder_media().open_object(&path, None).expect("parse");
        classify(&file, &folder_media(), &path, class)
    }

    #[test]
    fn reports_are_recognised_by_their_sop_class() {
        let report = classified(uids::BASIC_TEXT_SR_STORAGE, MR_REPORT_TITLE, None).expect("a report");
        assert_eq!(report.kind, DocumentKind::Report);
        assert_eq!(report.display_title(), MR_REPORT_TITLE);
        assert_eq!(report.display_date().as_deref(), Some("2026-01-15"));
        let dose = classified(uids::X_RAY_RADIATION_DOSE_SR_STORAGE, DOSE_REPORT_TITLE, None).expect("a dose report");
        assert_eq!(dose.kind, DocumentKind::DoseReport);
        // A dose report stored as a Comprehensive SR.
        let dose =
            classified(uids::COMPREHENSIVE_SR_STORAGE, DOSE_REPORT_TITLE, Some("113701")).expect("a dose report");
        assert_eq!(dose.kind, DocumentKind::DoseReport);
        // Key objects stay out: they have no text to read.
        assert_eq!(classified(uids::KEY_OBJECT_SELECTION_DOCUMENT_STORAGE, "Rejected", None), None);
        assert_eq!(classified(uids::GRAYSCALE_SOFTCOPY_PRESENTATION_STATE_STORAGE, "State", None), None);
    }

    #[test]
    fn documents_are_listed_with_reports_first() {
        let doc = |kind, title: &str, date: &str| DocumentRef {
            kind,
            title: Some(title.to_owned()),
            date: Some(date.to_owned()),
            path: PathBuf::from(title),
            sop_instance_uid: None,
            too_large: false,
        };
        let mut docs = vec![
            doc(DocumentKind::DoseReport, "Dose", "20260101"),
            doc(DocumentKind::StoredPdf, "PDF", "20260101"),
            doc(DocumentKind::Report, "B", "20260102"),
            doc(DocumentKind::Report, "A", "20260103"),
        ];
        sort(&mut docs);
        let titles: Vec<&str> = docs.iter().map(DocumentRef::display_title).collect();
        assert_eq!(titles, ["B", "A", "PDF", "Dose"]);
    }

    #[test]
    fn a_document_without_a_title_is_called_by_its_kind() {
        let untitled = DocumentRef {
            kind: DocumentKind::Report,
            title: None,
            date: None,
            path: PathBuf::new(),
            sop_instance_uid: None,
            too_large: false,
        };
        assert_eq!(untitled.display_title(), "Report");
        assert_eq!(untitled.display_date(), None);
    }

    #[test]
    fn a_stored_pdf_loses_its_padding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("DOC");
        // An odd length: the writer pads the value with a zero byte, and records the true length.
        let pdf = reports::text_pdf(&[&["Odd"]]);
        let pdf = if pdf.len().is_multiple_of(2) { [pdf.as_slice(), b"\n"].concat() } else { pdf };
        reports::write_encapsulated_pdf(&path, "2.25.1", "Odd", &pdf).expect("object");
        let file = folder_media().open_object(&path, None).expect("parse");
        let document = classify(&file, &folder_media(), &path, "").expect("a document");
        assert_eq!(document.kind, DocumentKind::StoredPdf);
        match read_document(&folder_media(), &document).expect("read") {
            DocumentContent::Pdf { bytes, status } => {
                assert_eq!(bytes, pdf);
                assert_eq!(status, None);
            }
            other => panic!("not a PDF: {other:?}"),
        }
    }
}
