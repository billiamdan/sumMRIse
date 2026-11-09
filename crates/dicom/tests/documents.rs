//! Reports and documents on the synthetic CD, found through a DICOMDIR, through a
//! folder scan and inside archives and disc images, and read through the same way.

use std::path::{Path, PathBuf};

use dicom_core::{DataElement, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::InMemDicomObject;
use dicom_object::meta::FileMetaTableBuilder;
use summrise_dicom::documents::ReportContent;
use summrise_dicom::{
    DicomError, DiscoverySource, DocumentContent, DocumentKind, DocumentRef, Progress, StudyIndex, read_document, scan,
};
use summrise_testkit::PhantomOptions;
use summrise_testkit::archive::{IsoNames, iso_image, zip_folder};
use summrise_testkit::reports::{
    self, MR_REPORT_TITLE, RTF_CYRILLIC_TEXT, ReportSpec, deeply_nested_report, write_reports_cd,
    write_structured_report,
};

fn index_of(path: &Path) -> StudyIndex {
    scan(path, &Progress::new()).expect("scan")
}

fn study_titles(index: &StudyIndex) -> Vec<(DocumentKind, &str)> {
    index.studies[0].documents.iter().map(|d| (d.kind, d.display_title())).collect()
}

fn file_titles(index: &StudyIndex) -> Vec<&str> {
    index.loose_documents.iter().map(DocumentRef::display_title).collect()
}

fn by_title<'a>(index: &'a StudyIndex, title: &str) -> &'a DocumentRef {
    index.studies[0]
        .documents
        .iter()
        .chain(&index.loose_documents)
        .find(|d| d.display_title() == title)
        .unwrap_or_else(|| panic!("no document {title}"))
}

const REPORTS: [(DocumentKind, &str); 4] = [
    (DocumentKind::Report, reports::MEASUREMENTS_TITLE),
    (DocumentKind::Report, MR_REPORT_TITLE),
    (DocumentKind::StoredPdf, reports::STORED_PDF_TITLE),
    (DocumentKind::DoseReport, reports::DOSE_REPORT_TITLE),
];

/// What the CD holds next to its images. The readme of the root is on the
/// list; the manual and the readme of the viewer software are not.
const LOOSE_FILES: [&str; 4] = ["README.TXT", "REPORT.PDF", "report_001.rtf", "report_002.htm"];

fn assert_all_found(index: &StudyIndex) {
    assert_eq!(study_titles(index), REPORTS);
    assert_eq!(file_titles(index), LOOSE_FILES);
    assert_eq!(index.stats.documents, 4);
    assert_eq!(index.stats.non_images, 0, "documents are not counted as objects that are not shown");
    assert_eq!(index.stats.unreadable, 0);
    assert!(index.warnings.is_empty(), "{:?}", index.warnings);
    // The images stay as they are.
    assert_eq!(index.stats.images, 24 + 12 + 3 + 1);
    assert_eq!(index.studies.len(), 1);
    assert_eq!(index.studies[0].series.len(), 4);
}

/// Task 5.2 and the scenario "DICOMDIR with a report record".
#[test]
fn documents_are_indexed_through_a_dicomdir() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions::small()).expect("fixture");
    let index = index_of(dir.path());
    assert!(matches!(index.source, DiscoverySource::Dicomdir(_)), "{:?}", index.source);
    assert_all_found(&index);
}

/// Task 5.2: the same through a folder scan.
#[test]
fn documents_are_indexed_through_a_folder_scan() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() })
        .expect("fixture");
    let index = index_of(dir.path());
    assert_eq!(index.source, DiscoverySource::FolderScan);
    assert_all_found(&index);
}

/// The same from the DICOMDIR itself, when that is what was opened.
#[test]
fn documents_are_found_when_the_dicomdir_itself_is_opened() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions::small()).expect("fixture");
    let index = index_of(&dir.path().join("DICOMDIR"));
    assert_all_found(&index);
}

/// The brief: loose files are found inside archives and disc images, through the media.
#[test]
fn documents_are_found_and_read_inside_archives_and_disc_images() {
    let dir = tempfile::tempdir().expect("tempdir");
    let folder = dir.path().join("CD");
    write_reports_cd(&folder, &PhantomOptions::small()).expect("fixture");
    let zip = zip_folder(&folder, "PATIENT CD").expect("zip");
    // Every name of a disc image also has to fit ISO 9660 level 1.
    let reports = folder.join("REPORTS");
    std::fs::rename(reports.join("report_001.rtf"), reports.join("RPT1.RTF")).expect("rename");
    std::fs::rename(reports.join("report_002.htm"), reports.join("RPT2.HTM")).expect("rename");
    let joliet = iso_image(&folder, IsoNames::Joliet).expect("iso");
    let level1 = iso_image(&folder, IsoNames::Level1).expect("iso");
    let archives = [
        ("study.zip", zip, "report_001.rtf", "report_002.htm"),
        ("joliet.iso", joliet, "RPT1.RTF", "RPT2.HTM"),
        ("level1.iso", level1, "RPT1.RTF", "RPT2.HTM"),
    ];
    for (name, bytes, rtf, html) in archives {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).expect("write");
        let index = index_of(&path);
        assert_eq!(study_titles(&index), REPORTS, "{name}");
        assert_eq!(file_titles(&index), ["README.TXT", "REPORT.PDF", rtf, html], "{name}");
        assert_eq!(index.stats.non_images, 0, "{name}");
        assert_eq!(index.stats.images, 24 + 12 + 3 + 1, "{name}");
        // Nothing but paths inside the archive.
        let pdf = by_title(&index, "REPORT.PDF");
        assert!(pdf.path.is_relative(), "{name}: {}", pdf.path.display());
        match read_document(&index.media, pdf).expect(name) {
            DocumentContent::Pdf { bytes, .. } => assert!(bytes.starts_with(b"%PDF"), "{name}"),
            other => panic!("{name}: {other:?}"),
        }
        match read_document(&index.media, by_title(&index, rtf)).expect(name) {
            DocumentContent::Text(text) => assert!(text.text.contains(RTF_CYRILLIC_TEXT), "{name}: {}", text.text),
            other => panic!("{name}: {other:?}"),
        }
        match read_document(&index.media, by_title(&index, reports::STORED_PDF_TITLE)).expect(name) {
            DocumentContent::Pdf { bytes, .. } => assert!(bytes.starts_with(b"%PDF"), "{name}"),
            other => panic!("{name}: {other:?}"),
        }
        assert!(matches!(
            read_document(&index.media, by_title(&index, MR_REPORT_TITLE)).expect(name),
            DocumentContent::Report(_)
        ));
    }
}

#[test]
fn every_study_of_the_medium_shares_the_files_and_keeps_its_own_reports() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() })
        .expect("fixture");
    // A report of another study on the same disc: a study of its own without images.
    let other = ReportSpec {
        study_uid: "2.25.42".to_owned(),
        ..ReportSpec::new(uids::BASIC_TEXT_SR_STORAGE, "Report of another study", Vec::new())
    };
    write_structured_report(&dir.path().join("DICOM").join("OTHER").join("SR1"), &other).expect("report");
    let index = index_of(dir.path());
    assert_eq!(index.studies.len(), 2);
    let lonely = index.studies.iter().find(|s| s.uid == "2.25.42").expect("the other study");
    assert_eq!(lonely.series.len(), 0);
    assert_eq!(lonely.documents.len(), 1);
    assert_eq!(lonely.documents[0].display_title(), "Report of another study");
    assert_eq!(lonely.image_count(), 0);
    assert_eq!(index.loose_documents.len(), 4, "not tied to a study");
}

#[test]
fn a_disc_with_only_a_report_opens() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = ReportSpec::new(uids::BASIC_TEXT_SR_STORAGE, MR_REPORT_TITLE, Vec::new());
    write_structured_report(&dir.path().join("SR1"), &spec).expect("report");
    let index = index_of(dir.path());
    assert_eq!(index.stats.images, 0);
    assert_eq!(index.studies.len(), 1);
    assert_eq!(study_titles(&index), [(DocumentKind::Report, MR_REPORT_TITLE)]);
    assert_eq!(index.studies[0].title(), "Synthetic phantom study", "the description of the study it belongs to");
    assert_eq!(index.studies[0].modalities(), [] as [&str; 0]);
}

/// A folder with loose files but nothing of DICOM is still not a study.
#[test]
fn a_folder_with_only_loose_files_is_not_a_study() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("REPORT.PDF"), reports::text_pdf(&[&["A report"]])).expect("write");
    assert!(matches!(scan(dir.path(), &Progress::new()), Err(DicomError::NothingFound(_))));
}

#[test]
fn what_is_not_on_the_list_of_types_stays_counted() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() })
        .expect("fixture");
    let write = |name: &str, class: &str| {
        let mut obj = InMemDicomObject::new_empty();
        obj.put(DataElement::new(tags::SOP_CLASS_UID, VR::UI, class));
        obj.put(DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, format!("2.25.77.{name}").as_str()));
        obj.put(DataElement::new(tags::STUDY_INSTANCE_UID, VR::UI, "2.25.78"));
        let meta = FileMetaTableBuilder::new().transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN);
        obj.with_meta(meta).expect("meta").write_to_file(dir.path().join(name)).expect("write");
    };
    write("1", uids::GRAYSCALE_SOFTCOPY_PRESENTATION_STATE_STORAGE);
    write("2", uids::KEY_OBJECT_SELECTION_DOCUMENT_STORAGE);
    let index = index_of(dir.path());
    assert_eq!(index.stats.non_images, 2);
    assert_eq!(index.stats.documents, 4);
    assert_eq!(index.warnings, ["2 objects that are not images (such as annotations) are not shown"]);
}

#[test]
fn a_report_stored_twice_is_listed_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = ReportSpec::new(uids::BASIC_TEXT_SR_STORAGE, MR_REPORT_TITLE, Vec::new());
    write_structured_report(&dir.path().join("SR1"), &spec).expect("report");
    write_structured_report(&dir.path().join("SR2"), &spec).expect("report");
    let index = index_of(dir.path());
    assert_eq!(index.studies[0].documents.len(), 1);
    assert_eq!(index.stats.duplicates, 1);
}

/// Requirement "Safe handling": a file with nesting that would overflow the
/// stack of the parser is counted as unreadable and does not end the application.
#[test]
fn a_report_that_nests_too_deeply_for_the_parser_does_not_end_the_application() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() })
        .expect("fixture");
    // Beyond what overflows the stack of a worker thread, by far.
    std::fs::write(dir.path().join("DEEP"), deeply_nested_report(100_000, "Too deep").expect("bytes")).expect("write");
    let index = index_of(dir.path());
    assert_eq!(index.stats.unreadable, 1);
    assert_eq!(index.stats.documents, 4);
    assert_eq!(index.warnings, ["1 file could not be read and was skipped"]);
}

/// A report nested beyond what is followed, but within what can be parsed, is listed and says what is missing.
#[test]
fn a_report_that_nests_deeply_is_read_as_far_as_it_is_followed() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() })
        .expect("fixture");
    std::fs::write(dir.path().join("DEEP"), deeply_nested_report(100, "Deep down").expect("bytes")).expect("write");
    let index = index_of(dir.path());
    assert_eq!(index.stats.unreadable, 0);
    let deep = by_title(&index, "Deep report");
    match read_document(&index.media, deep).expect("read") {
        DocumentContent::Report(report) => {
            assert!(report.cut_short);
            let text = |item: &summrise_dicom::documents::ReportItem| matches!(&item.content, ReportContent::Paragraph { text, .. } if text == "Deep down");
            assert!(!report.items.iter().any(text));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_document_that_went_missing_is_an_error_that_can_be_shown() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_reports_cd(dir.path(), &PhantomOptions::small()).expect("fixture");
    let index = index_of(dir.path());
    let report: PathBuf = by_title(&index, MR_REPORT_TITLE).path.clone();
    std::fs::remove_file(&report).expect("remove");
    let error = read_document(&index.media, by_title(&index, MR_REPORT_TITLE)).expect_err("gone");
    assert!(!error.to_string().is_empty());
    // A file that is not what it is called.
    let rtf = by_title(&index, "report_001.rtf");
    std::fs::write(&rtf.path, b"not an RTF document").expect("write");
    let error = read_document(&index.media, rtf).expect_err("not RTF");
    assert_eq!(error.to_string(), "this file is not an RTF document");
    let readme = by_title(&index, "README.TXT");
    std::fs::write(&readme.path, b"MZ\x90\x00\x03\x00").expect("write");
    assert_eq!(read_document(&index.media, readme).expect_err("binary").to_string(), "this file does not hold text");
}

/// The character set of the object decodes the text of a report, as for every
/// other attribute.
#[test]
fn the_character_set_of_the_object_decodes_the_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let written = "Röntgen: Größe 5 mm";
    let spec = ReportSpec {
        character_set: "ISO_IR 100",
        ..ReportSpec::new(
            uids::BASIC_TEXT_SR_STORAGE,
            MR_REPORT_TITLE,
            vec![summrise_testkit::reports::ReportNode::text("Note", written)],
        )
    };
    write_structured_report(&dir.path().join("SR1"), &spec).expect("report");
    let index = index_of(dir.path());
    match read_document(&index.media, by_title(&index, MR_REPORT_TITLE)).expect("read") {
        DocumentContent::Report(report) => {
            let ReportContent::Paragraph { text, .. } = &report.items[0].content else { panic!("a paragraph") };
            assert_eq!(text, written);
        }
        other => panic!("{other:?}"),
    }
}
