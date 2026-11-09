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

Adapted to the code as it is now:

- All reading goes through `Media`, so that report files are also found and read inside
  ZIP archives and disc images. Nothing is unpacked. "Show in folder" exists only for a
  file in a real folder.
- Loose files are kept in `StudyIndex::loose_documents`, once, and listed with every study.
  Alternative rejected: a copy in each `Study`, which says they belong to a study.
- *A folder that contains executable software* is one that holds an executable, directly
  or further down, and no DICOM file of the disc (the DICOMDIR included). So `VIEWER/`
  with `bin/viewer.exe` and `docs/manual.pdf` is left out as a whole, while an autorun
  program in the root of the disc does not hide `REPORT.PDF` beside it: the root holds the
  images. The rule also works in an archive whose content lies below a folder `CD/`.
  Alternatives rejected: only the folder with the executable itself (the manual in
  `VIEWER/docs/` would be listed), and treating the root like any other folder (a report
  next to an autorun program is a common case, and missing it is worse than listing a
  readme). A readme in the root is listed, under "Other files on the disc".
- At most 200 report files are listed, with a warning that says how many are not: a folder
  opened by mistake (the downloads of the user) can hold thousands of PDFs that have
  nothing to do with the study.
- A study of which the disc holds only reports becomes a `Study` without series. It opens on
  its first document, and no series is displayed. A folder with loose files and nothing
  of DICOM still is not a study: "no DICOM images were found" stays true.
- The SOP classes form an allow-list. Key Object Selection, presentation states and the
  like stay counted in `IndexStats::non_images`, and the warning sentence no longer names
  reports. A dose report stored as a Comprehensive SR is recognised by its title code
  (DCM 113701). Encapsulated documents of another MIME type stay counted too.

### D2 — Structured report rendering
The reader walks the Content Sequence: containers become headings (concept name), text
items paragraphs, coded items "name: meaning", numeric items "name: value unit", dates
and person names use the existing formatters, and image items become links by referenced
SOP Instance UID. Unknown value types are shown with their concept name and raw value.
The header shows the title, content date, completion and verification status.

Alternative considered: converting the report to HTML and rendering that — rejected: it
needs an HTML engine.

In detail: the tree is flattened into a list of items in document order, each with its
depth, so that the view does not recurse. A Preliminary Flag of PRELIMINARY counts as not
complete, whatever the Completion Flag says, so that such a report never reads as final;
each of the nine combinations of completion and verification has a sentence of its own,
and only "Final — verified" is not shown in the warning colour. The name of a text item is
shown above it unless it only repeats the heading it stands under ("Finding" under
"Findings"): nothing is lost and the scenario reads as the specification wants it. Numeric
values stay the strings the file holds; units are the UCUM codes, with a dot for a product
shown as "·" (`mGy.cm` is "mGy·cm"), the code `1` and annotations in braces are no units.

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

Evaluation of `hayro` 0.9.0 (November 2026) on PDFs the testkit generates, kept as tests
in `crates/core/src/documents/pdf.rs`, and on 60 PDFs that come with macOS, read only and not
kept:

| Sample | Result |
|---|---|
| Text in Helvetica, not embedded (two pages) | Correct; 1 to 3 ms per page at 200 % |
| Text in an embedded TrueType font | Correct |
| Scanned page, Flate and JPEG | Correct: dark bars where the picture has them |
| Truncated by half or more; bytes flipped; garbage | `Err`, no panic |
| Without its last bytes (the table of objects is gone) | Repaired and shown |
| Cut at every seventh byte | An error or a page, never a panic |
| 60 PDFs of macOS (the first five pages of each, 95 in all) | All rendered, no panic |

Licences: `hayro` is Apache-2.0 OR MIT and its tree is on the allow-list of `deny.toml`
(`cargo deny --all-features check` passes: advisories, bans, licences, sources). The
crate forbids unsafe code itself; its dependencies `pic-scale` and `fearless_simd` use SIMD.
A search of the sources of `hayro`, `hayro-syntax`, `hayro-interpret`, `hayro-font`,
`hayro-jbig2`, `hayro-jpeg2000`, `hayro-ccitt`, `hayro-cmap`, `glifo`, `vello_common` and
`vello_cpu` finds no access to the network or to processes; files are touched only by
debugging helpers and documentation examples that the features used here do not build.
It has limits on nesting and recursion of its own.

Decision: `hayro` is adopted, with its default features (the 14 standard fonts and the
predefined CMaps, which a PDF may rely on without embedding them). `pdfium-render`, the
fallback candidate, is not needed and is rejected: it bundles a C++ binary. Rejected as well:
keeping a parsed `Pdf` and its caches on a long-lived thread per document. Parsing costs
well under a millisecond for a report, so each picture parses anew on its own thread, which
leaves nothing to clean up after a panic.

Limits and isolation: a thread with a stack of 32 MiB and `catch_unwind` per document and
per picture (`documents::Task`); at most 50 MB per document, 2000 pages, 48 million pixels per
picture and the largest texture of the GPU. One picture is made at a time and only the last
request is kept, so that quick page turns do not pile up work. A picture is requested
again when the scale on screen changes by more than 2 %; until it is there, the last one
stands in, scaled. `hayro` reports fonts and pictures it could not read: the view then says
that something may be missing, because that would not be noticed otherwise.

Known limits: password-protected PDFs other than those with an empty password are refused
with a message. A hostile PDF can keep its thread busy or take memory until it is done
(a stream that inflates enormously is not bounded by the limits above); the window stays
responsive, but the thread cannot be cancelled. The text of a PDF page is a picture and
cannot be selected.

### D4 — RTF, HTML and text as plain text
Small converters strip RTF control words (honouring the code page) and HTML tags. No
scripts, remote resources or embedded objects are processed.

Alternative considered: full rendering of RTF and HTML — rejected: a large attack
surface for little gain.

Code pages come from `encoding_rs` (pure Rust, maintained, permissive), which also gives
Shift-JIS, GBK and the labels of HTML. Alternatives rejected: `encoding`, which is
unmaintained and fails `cargo deny` as a direct dependency, and tables by hand, in which an
error would silently show the wrong letters in a report. An RTF takes the code page from
`\ansicpg` and, where a font says more, from `\fcharset` of its font. OEM code pages 437
and 850 are not in `encoding_rs` and read as Windows-1252. A text or HTML file that does not
say what it is and is not UTF-8 is read as Windows-1252, and the view says that this is an
assumption, because a Cyrillic file in Windows-1251 would come out wrong. A file named `.txt`
with bytes no text has is refused rather than shown as garbage. `<head>`, `<title>`, scripts,
styles, frames and objects are skipped with their content; attributes, and so links and
image URLs, are never looked at.

### D5 — Dose report
An X-Ray Radiation Dose SR is shown as a table of irradiation events with CTDIvol and
dose-length product and the recorded total. Nothing is calculated beyond what the
scanner recorded.

### D6 — Privacy mode
While privacy mode is on, a document's content is hidden behind "Show anyway", per
document and not remembered.

The titles go as well, in the list and in the header: a Document Title or the name of a file
can name the patient, and cannot be told apart from one that does not. Until the document
is revealed only its type and date show. Turning privacy mode off and on again hides a
revealed document again. Alternative rejected: keeping titles in the list, which would
leave `Report_Doe_Jane.pdf` on the screen in privacy mode.

### D7 — Place in the interface
A selected document replaces the viewport area; "Back to images" returns. Image links
return to the images and select the series and image.

The list goes above the series, with a height of its own: for a patient the report is the most
valuable item on the disc, and it would be out of sight below a long list of series. It goes
through one call in the browser and one small branch in the central panel of `Viewer`, so
`Viewer::viewports` is not touched. While a document is open, the image controls of the
toolbar are disabled (they would act on images that are covered) and the keys for the
images are not read, playback stops and its bar is hidden. Alternative rejected: a further
toolbar button or tab; the toolbar is full, and the list is where documents are looked for.
Esc returns; a series in the browser returns too, also the one that was on screen.

A link also switches to the single view, without changing the saved layout: a reformat
could not show the image that was asked for. The image is loaded first, and comes on screen
as soon as the series can be shown (`settle_image`), through the series' `ViewState`, also in
a series split into stacks. A reference to a frame the file does not have counts as an image
that is not on the disc.

Page Up and Page Down turn the pages of a PDF and move through other documents; the arrow keys
scroll. The zoom buttons are reached with Tab: there is no shortcut for them, because egui's
own zoom of the interface has the usual keys.

### D8 — Files that nest too deeply for the parser
The object parser of dicom-rs recurses per nesting level and has no limit: about 1500 levels
overflow the 2 MiB stack of a worker thread (measured: 1000 levels pass, 2000 do not), which
fit in 30 kB of a file. A stack overflow is not a panic: it ends the application, on any disc,
for any kind of object. So `Media::open_object` first runs the file through the parser's token
stream, which is not recursive, and refuses more than 128 nested sequences. The file then
counts as unreadable, with the existing warning. Below that, the reader of the content tree
follows 64 levels, and the view says that part of the report is not shown.

Alternatives rejected: a larger stack, because the depth a file can hold is limited only by its
size; a look at the bytes for item tags, which cannot tell nesting of defined lengths from a
long flat sequence; a nesting walker of our own, which could disagree with the parser about
what a file means. The price is reading each header twice: indexing the 148 files of the
synthetic CD from the file cache takes 4.5 ms instead of 3.4 ms, and from a disc the second
read comes from that cache as well.

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
