## 1. Discovery and index (`summrise-dicom`)

- [x] 1.1 Accept SR DOCUMENT and ENCAP DOC records from DICOMDIR; classify document SOP classes during header indexing
- [x] 1.2 Find loose report files by extension, skipping folders with executables and files over 50 MB
- [x] 1.3 `Study.documents` with kind, title, date and source; documents no longer counted as "not shown"

## 2. Readers

- [x] 2.1 Structured-report reader: content tree with value types, header status and image references
- [x] 2.2 Dose-report extraction: irradiation events with CTDIvol and DLP, and the recorded total
- [x] 2.3 Encapsulated-document reader returning the PDF bytes in memory
- [x] 2.4 RTF to text (with code pages), HTML to text, and plain-text decoding

## 3. PDF rendering

- [x] 3.1 Evaluate the Rust PDF rasteriser on sample reports (text, scanned pages, embedded fonts) and record the result and the fallback decision in design.md
- [x] 3.2 Page rasterisation on demand with zoom, page and size limits, and panic isolation

## 4. UI

- [x] 4.1 Documents section in the series browser with type, title and date; hidden when empty
- [x] 4.2 Document view for structured text, PDF pages and plain text, with "Back to images"
- [x] 4.3 Image links select the series and image; missing references shown as text
- [x] 4.4 Privacy-mode reveal, "Show in folder" for files on the medium (the button and the command it builds), accessible names
- 4.5 Moved to `field-validation` 5.15 when this change was archived: "Show in folder" really opens the file manager on macOS, Windows and Linux. Not verified: the tests check the command and do not start another application, and Windows and Linux are not available on this machine

## 5. Verification

- [x] 5.1 Testkit: Basic Text SR, dose SR, Encapsulated PDF, loose PDF, RTF and HTML files, and a viewer folder with a readme on the synthetic CD
- [x] 5.2 Integration tests: documents indexed through DICOMDIR and through a folder scan; viewer files not listed
- [x] 5.3 Rendering tests: "Findings" and "Impression" headings, "Diameter: 12 mm", dose total 570 mGy·cm, HTML script not executed, Windows-1251 RTF
- [x] 5.4 Offscreen UI test of the document view; new dependencies pass `cargo deny`
