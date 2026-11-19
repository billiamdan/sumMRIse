//! The DICOM conformance statement (`docs/dicom-conformance-statement.md`)
//! is kept in step with the code: its transfer-syntax table lists every
//! transfer syntax of the DICOM library, with whether this build decodes it
//! (`format::transfer_syntax_supported` and the build options; "release builds"
//! means with the features the release packages are built with) and whether it
//! is always lossy (`format::transfer_syntax_is_lossy`).

use std::collections::BTreeSet;

use dicom_transfer_syntax_registry::TransferSyntaxRegistry;
use summrise_dicom::format::{transfer_syntax_is_lossy, transfer_syntax_supported};

const STATEMENT: &str = include_str!("../../../docs/dicom-conformance-statement.md");

/// One row of the transfer-syntax table.
#[derive(Debug)]
struct Row {
    uid: String,
    decoded: String,
    lossy: String,
}

/// The rows of the transfer-syntax table: table rows whose second cell is a
/// transfer syntax UID in backticks.
fn rows() -> Vec<Row> {
    STATEMENT
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
            let uid = cells.get(1)?.strip_prefix('`')?.strip_suffix('`')?;
            if !uid.starts_with("1.2.840.10008.1.2") {
                return None;
            }
            Some(Row { uid: uid.to_owned(), decoded: (*cells.get(2)?).to_owned(), lossy: (*cells.get(3)?).to_owned() })
        })
        .collect()
}

/// Whether this build decodes pixel data in a transfer syntax, as the
/// statement says it.
fn decoded(row: &Row) -> bool {
    match row.decoded.as_str() {
        "Yes" => true,
        "No" => false,
        // The release packages are built with the jpeg-ls feature (xtask, CI).
        "Yes (release builds)" => cfg!(feature = "jpeg-ls"),
        "With the jpeg-xl build option" => cfg!(feature = "jpeg-xl"),
        other => panic!("{}: unknown entry {other:?} in the column \"Decoded\"", row.uid),
    }
}

#[test]
fn every_known_transfer_syntax_is_listed_once() {
    let rows = rows();
    let listed: BTreeSet<&str> = rows.iter().map(|r| r.uid.as_str()).collect();
    assert_eq!(listed.len(), rows.len(), "a transfer syntax is listed twice");
    let known: BTreeSet<String> =
        TransferSyntaxRegistry.iter().map(|ts| ts.uid().trim_end_matches('\0').to_owned()).collect();
    let missing: Vec<&String> = known.iter().filter(|uid| !listed.contains(uid.as_str())).collect();
    let unknown: Vec<&&str> = listed.iter().filter(|uid| !known.contains(**uid)).collect();
    assert!(missing.is_empty(), "transfer syntaxes missing from the statement: {missing:?}");
    assert!(unknown.is_empty(), "transfer syntaxes in the statement that the library does not know: {unknown:?}");
}

#[test]
fn the_decoded_and_lossy_columns_agree_with_the_code() {
    let rows = rows();
    assert!(rows.len() > 40, "the table was found: {} rows", rows.len());
    for row in &rows {
        assert_eq!(
            transfer_syntax_supported(&row.uid),
            decoded(row),
            "{}: column \"Decoded\" says {:?}",
            row.uid,
            row.decoded
        );
        let lossy = match row.lossy.as_str() {
            "Yes" => true,
            "No" => false,
            other => panic!("{}: unknown entry {other:?} in the column \"Always lossy\"", row.uid),
        };
        assert_eq!(transfer_syntax_is_lossy(&row.uid), lossy, "{}: column \"Always lossy\" says {lossy}", row.uid);
    }
    // The scenarios of the specification.
    let row = |uid: &str| rows.iter().find(|r| r.uid == uid).unwrap_or_else(|| panic!("{uid} is listed"));
    let jpeg = row("1.2.840.10008.1.2.4.50");
    assert_eq!((jpeg.decoded.as_str(), jpeg.lossy.as_str()), ("Yes", "Yes"));
    assert_eq!(row("1.2.840.10008.1.2.4.80").decoded, "Yes (release builds)");
    if !cfg!(feature = "jpeg-ls") {
        assert!(!transfer_syntax_supported("1.2.840.10008.1.2.4.80"));
    }
}

#[test]
fn the_statement_has_the_sections_of_ps3_2_that_apply() {
    for heading in [
        "## 1 Conformance Statement Overview",
        "## 4 Networking",
        "## 5 Media Interchange",
        "### 5.5 Supported SOP classes",
        "### 5.6 Transfer syntaxes",
        "### 5.7 Photometric interpretations and pixel data",
        "## 6 Support of Character Sets",
        "## 7 Security",
        "## 8 Annexes",
        "#### 8.1.2 Usage of attributes from received IODs",
        "### 8.4 Grayscale image consistency",
    ] {
        assert!(STATEMENT.lines().any(|line| line == heading), "missing heading {heading:?}");
    }
    // Reader only, no networking, not a medical device.
    assert!(STATEMENT.contains("| General Purpose CD-R Interchange (STD-GEN-CD) | No | Yes |"));
    assert!(STATEMENT.contains("File-set Creator (FSC) and File-set Updater (FSU) are not supported."));
    assert!(STATEMENT.contains("supports no DICOM network service"));
    assert!(STATEMENT.contains("**not a medical device**"));
}
