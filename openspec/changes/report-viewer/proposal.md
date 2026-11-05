## Why

For a patient the radiologist's report is the most valuable item on the disc, and
SumMRIse does not show it: DICOM reports are counted as "non-image objects … not shown",
and report files that hospitals put next to the images are ignored. Professional viewers
display structured reports and PDF documents.

## What Changes

- A "Documents" list per study in the series browser: structured reports, PDF reports
  stored inside DICOM, radiation dose reports, and report files found on the medium
  (PDF, RTF, text, HTML).
- A document view that shows structured reports as headed, readable text with links to
  the images they reference, PDFs page by page, and text, RTF and HTML as plain text.
- Dose reports as a table of the values recorded by the scanner.
- Reports are displayed exactly as written: no summarising, translating or interpreting.

Privacy and safety: documents are read into memory only (no temporary files) and are
never executed; HTML is shown as text without scripts or remote content, so the
no-network rule holds. Reports contain personal details that cannot be masked reliably,
so privacy mode hides a document's content until the user chooses to reveal it.

## Capabilities

### New Capabilities
- `report-viewer`: listing and displaying structured reports, PDF and text documents, and dose reports.

### Modified Capabilities
- `dicom-import`: adds discovery of document objects and of report files on the medium.

## Impact

- `crates/dicom`: document index; readers for structured reports and encapsulated
  documents.
- `crates/core`: selected-document state.
- `crates/ui`: Documents list and document view.
- New dependencies to evaluate: a PDF rasteriser and small RTF and HTML to text
  converters; only permissive licences that pass `cargo deny`.
- Testkit: report fixtures on the synthetic CD.
