## Context

Header indexing (`crates/dicom/src/index.rs`) treats every object without rows and
columns as a non-image and only counts it. Discovery (`discovery.rs`) takes image-type
DICOMDIR records and, in a folder scan, files with the DICM signature. Patient media
carry reports in several forms: structured reports (Basic Text, Enhanced, Comprehensive
SR), Encapsulated PDF objects, X-Ray Radiation Dose SR, pictures of a report page
(secondary capture, already shown as images), and loose PDF, RTF, HTML or text files
written by the disc-burning software.

## Goals / Non-Goals

**Goals:**
- Show every report on the medium readably and safely.
- Keep the privacy rules: nothing written to disk, no network.

**Non-Goals:**
- Editing or creating reports.
- Summaries or explanations of reports (deferred together with any AI feature; see the
  archived `privacy-safeguards` design).
- Key Object Selection, presentation states, RT structures, waveforms.
- Word documents (DOC, DOCX), printing.
- Estimating an effective dose.

## Decisions

### D1 — Document index
`Study` gains `documents: Vec<DocumentRef>` with a kind (structured report, dose report,
encapsulated PDF, file), title, date and source. DICOM documents come from DICOMDIR
records of type SR DOCUMENT and ENCAP DOC and, in folder scans, from the SOP class seen
during header indexing. Loose files are found by extension (pdf, rtf, txt, htm, html)
anywhere on the medium except in folders that contain executable software, up to 50 MB
each; they are not tied to a study and are listed with every study of the medium.

Alternative considered: DICOM documents only — rejected: many discs carry the report
only as a loose file.

### D2 — Structured report rendering
The reader walks the Content Sequence: containers become headings (concept name), text
items paragraphs, coded items "name: meaning", numeric items "name: value unit", dates
and person names use the existing formatters, and image items become links by referenced
SOP Instance UID. Unknown value types are shown with their concept name and raw value.
The header shows the title, content date, completion and verification status.

Alternative considered: converting the report to HTML and rendering that — rejected: it
needs an HTML engine.

### D3 — PDF rendering inside the application
Pages are rasterised on demand at display resolution by a Rust PDF rasteriser
(candidate: `hayro`), behind the same panic isolation as image decoding and with page
and size limits. The candidate is evaluated on sample reports before adoption; the
fallback candidate is `pdfium-render`, which bundles the PDFium binary. If a PDF cannot
be rendered, the view says so and offers "Show in folder" for a file on the medium.

Alternatives considered:
- Handing PDFs to the system viewer through a temporary file — rejected: it writes
  patient data to disk, against "No derived data at rest".
- MuPDF bindings — rejected: AGPL licence.

### D4 — RTF, HTML and text as plain text
Small converters strip RTF control words (honouring the code page) and HTML tags. No
scripts, remote resources or embedded objects are processed.

Alternative considered: full rendering of RTF and HTML — rejected: a large attack
surface for little gain.

### D5 — Dose report
An X-Ray Radiation Dose SR is shown as a table of irradiation events with CTDIvol and
dose-length product and the recorded total. Nothing is calculated beyond what the
scanner recorded.

### D6 — Privacy mode
While privacy mode is on, a document's content is hidden behind "Show anyway", per
document and not remembered.

### D7 — Place in the interface
A selected document replaces the viewport area; "Back to images" returns. Image links
return to the images and select the series and image.

## Risks / Trade-offs

- [PDF rasteriser fidelity, or crashes on damaged files] → evaluation task, panic
  isolation, limits, a clear fallback message.
- [Malicious media] → allow-list of types, nothing executed or opened in other
  applications automatically, no network, size limits.
- [Text in legacy code pages] → code-page handling in the RTF converter; byte-order-mark
  and UTF-8 detection for text files with a Windows-1252 fallback.
- [A preliminary report read as final] → completion and verification status shown in the
  header.
- [Irrelevant loose files listed] → they appear under "Other files on the disc", below
  the reports.

## Open Questions

- Is support for Word documents needed in practice? To be answered by `field-validation`.
- Should a report be shown beside the images once `series-comparison` exists?
