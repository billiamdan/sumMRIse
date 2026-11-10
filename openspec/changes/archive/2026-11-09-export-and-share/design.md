## Context

Views are rendered to RGBA textures in `crates/ui/src/render.rs` from modality values and
the contrast window; overlays are painted by egui on top. Native dialogs come from
`rfd`. egui can place an image on the system clipboard (`Context::copy_image`) and
capture a viewport (`ViewportCommand::Screenshot`). The privacy baseline currently
forbids writing pixel data or DICOM attributes to disk.

## Goals / Non-Goals

**Goals:**
- Quick image export that is private by default.
- Exported pictures with correct proportions and predictable grey values.

**Non-Goals:**
- DICOM export and de-identified copies for second opinions: a separate change, because
  it needs the DICOM confidentiality profile (PS3.15) and its own safety review.
- Video or animated export.
- Printing at true size, DICOM printing (Basic Print), and the system's own print dialog.
- 16-bit export (TIFF), sharing by e-mail or cloud (no network).

## Decisions

### D1 — Two render paths
"Image only" re-renders from the data at native resolution with the current window and
inversion, resampled to square pixels when the pixel spacing is not square, without any
text. "With annotations" captures the viewport as displayed, in an export pass that
applies the privacy filter and adds the caption.

Alternative considered: a single screenshot path — rejected: the resolution would depend
on window size and zoom.

### D2 — Formats
PNG (default, lossless, 8-bit) and JPEG (quality 90), with no metadata beyond the
software name.

Alternative considered: 16-bit TIFF — rejected: the target users open exports in
ordinary applications.

### D3 — Private by default
The export dialog has "Include patient details" unticked and never remembers it.
Suggested file names contain only modality, series number and image number, for example
`SumMRIse-CT-S1-031.png`.

### D4 — Burned-in text
A warning precedes the export when Burned In Annotation (0028,0301) is YES or the image
is a secondary capture or ultrasound image, because names inside the pixels cannot be
removed by the application.

### D5 — Clipboard
The copy command places the image-only render on the clipboard, without patient details.
It applies when no text is selected, so copying text from the inspector keeps working.

### D6 — Series export
A background job writes consecutively numbered files into a chosen folder with progress
and cancel. It requires a fully loaded series and asks before overwriting.

### D7 — Caption
Exports with annotations carry "Not for diagnostic use" at the lower edge; image-only
exports are unmarked.

Alternatives considered:
- A caption on every export — rejected: it defaces the plain picture users want to
  share.
- No caption — rejected: annotated screenshots circulate without context.

### D8 — Privacy specification
"No derived data at rest" is modified to allow files the user explicitly exports, in the
chosen location only, and what the system's print system keeps while it prints a job the
user sent.

### D9 — Print pages are composed here, as pictures in a PDF
A print job is composed into pages in memory: raster pages at 300 dpi (A4 or Letter,
portrait or landscape), each placed as one image in a PDF, losslessly compressed. Pictures
come from the "image only" path of D1, with the current window, inversion and colour map.
Text (orientation letters, image number, window values, scale bar label, page header and
footer) is rasterised into the page with the fonts of the interface, so every language
prints the same as it reads on screen. Sheets hold 1, 2, 4, 6, 9 or 12 images per page, in
display order. Page composition is a pure function of the data and the options, so it is
tested without a window or a printer; a series job runs in the background with progress
and cancel, and needs the series fully loaded.

Alternatives considered:
- Vector text in the PDF — rejected: it needs font embedding and subsetting for Cyrillic and
  every other script, for no visible gain at 300 dpi.
- Printing a screenshot of the viewport — rejected: the resolution would depend on the
  window, and a sheet has more pictures than the screen shows.

### D10 — Handing pages to the printer
Native print dialogs (AppKit, Win32, GTK) need `unsafe` bindings on three platforms, which
the workspace forbids, and no crate offers a safe, maintained wrapper. So the print window
is the application's own: printer, copies, paper, orientation, images per page and "Include
patient details", with a preview of the first page and the number of pages. "Print" pipes
the PDF to `/usr/bin/lp` on standard input (macOS and Linux with CUPS), with the printers
listed by `/usr/bin/lpstat` and the default preselected. Absolute paths are used so that
nothing on a disc or in the working directory can be run by accident. No temporary file is
written, and the job title is the neutral `SumMRIse`, because print queues are logged. Where
`lp` is missing (Windows, Linux without CUPS), the button reads "Save PDF and open…": it
asks where to save the PDF and opens it in the default application for PDFs, from which the
user prints. "Save as PDF…" is always offered.

Alternatives considered:
- A temporary PDF handed to the system viewer on every platform — rejected: it writes
  patient data to a place the user did not choose.
- Sending images directly to the printer driver through a printing crate — rejected: drivers
  that take only their own format would fail silently, and it cannot be tested here.
- Only "Save as PDF" — rejected: Basic Image Review lists printing, and a patient expects
  a Print button that prints.

### D11 — What a page says
Every page has a header with the study description and date, the series description and
"Page n of m", and a footer with "Not for diagnostic use". Each picture has its image
number, orientation letters, scale bar and window values, on black inside its cell; the page
itself stays white to save ink. The scale bar is true to the printed picture; the print is
not true size and says nothing that suggests it is. Patient name, ID and birth date appear
in the header only when "Include patient details" is ticked for that job. The burned-in text
warning of D4 applies. A reformat prints like an acquired image, marked as a reformat; the
3D view prints with its "Illustration" caption.

### D12 — "With annotations" is an export pass, painted in software
D1 asked for a capture of the viewport. It is done without touching the window: the code
that draws the view (`viewport::show`, or the overlays of the 3D view) runs once more in a
private egui context the size of the active view, with the privacy filter applied and every
annotation on, with a band for the caption under it; what egui tessellates is painted into a
picture by a small software rasteriser (`crates/ui/src/export/raster.rs`). Whatever a picture
shows on screen, also what other features draw per picture, is in the file; nothing around
the view is; nothing changes on screen; and the result is tested offscreen, down to every
text drawn. The picture has at least 2 pixels per point. The caption goes in a band of its
own at the lower edge, because the scale bar and the annotations already use that edge.

Alternatives considered:
- `ViewportCommand::Screenshot`, cropped to the viewport, in a frame with the privacy filter
  on — rejected: the event arrives only from a real window (nothing to test offscreen), that
  frame would show on screen without the patient's details and with the caption, and
  tooltips, windows and the toasts over the view would be in the file.
- Composing the annotations by hand, as on printed pages — rejected: it would duplicate the
  layout of the viewport and miss whatever is added there later, so the file would no
  longer be "as displayed".

### D13 — "Image only" lives where pictures are made
Tasks 1.1 to 1.3 say `summrise-core`, but the picture of a view is composed in `crates/ui`
(`render.rs` with `fidelity.rs` and `pet.rs`). So `crates/ui/src/export/picture.rs` renders
through `render_frame` and `render_mpr`, with scanner graphics switched off in a copy of the
view state, and `crates/core/src/export.rs` holds the pure helpers (square pixels, turning,
encoding, names, the series job). Scanner graphics are left out because they can hold text
and "image only" promises no added text. Contrast, inversion, contrast table and colour map
are as displayed, and a PET overlay shown is part of the picture. Resampling to square
pixels keeps the finer spacing (bilinear in the coarser direction), and it never exceeds
16 384 pixels, whatever spacing a disc claims.

Alternative considered: moving the render paths to core — rejected: they use egui's image
type, and other features are changing those files.

### D14 — A file follows the turn and mirror of its view
An image-only file ignores zoom and pan, but it is turned and mirrored like the view it
came from: it must not contradict the orientation letters the user sees. A file has no
letters of its own, so when the view is mirrored the export window says so, and printed
pages carry the word next to the letters.

Alternative considered: always the standard orientation — rejected: the file would differ
from the screen without saying so.

### D15 — Text on pages comes through an injected rasteriser
`crates/core` knows no fonts. `compose_page` takes a `TextRaster`; `crates/ui/src/print.rs`
implements it with a `Fonts` instance of its own (the definitions of the interface, plus the
fonts the system provided for other scripts), laying out a line at the size in pixels and
copying the coverage from the glyph atlas. This works on any thread and needs no window.
Text never lies on a picture: the letters have a band around it and the rest a strip under it.

Alternatives considered: rendering text through the GPU — rejected: jobs run in the
background, without a window. Prepared bitmaps handed to core — rejected: the strings depend
on the page and on what fits.

### D16 — The PDF is hand-written
One object layout and a cross-reference table, about a hundred lines, one image per page
with deflate and the PNG "Up" predictor, grey samples when a page is all grey. Its metadata
is the title and producer `SumMRIse`, without a date.

Alternative considered: the `pdf-writer` crate — rejected as not needed: no new dependency.

### D17 — No metadata at all in image files
D2 allows the software name. The encoders write none, so PNG files hold IHDR, IDAT and IEND
only and JPEG files the JFIF header: the simplest way to be sure nothing about the patient is
in them. "Include patient details" adds lines to the picture, never metadata.

### D18 — Clipboard: `Context::copy_image`, which does not report back
The copy command uses egui's own route, which runs after the frame and only logs a failure.
So a "clear message when the clipboard is unavailable" is not possible in general. On Linux,
where the clipboard can refuse images (Wayland without the data-control protocol), the
message says what to do if the image does not paste.

Alternative considered: `arboard` directly, which returns an error — rejected: a second
clipboard owner beside egui's for a message, and `Context::copy_image` is what the brief
names.

### D19 — Privacy, a little stricter than asked
- "Include patient details" is disabled while privacy mode is on, because that mode hides
  these details everywhere. It also applies to pictures with text only.
- The warning about burned-in text also precedes the clipboard ("Copy anyway"), because a
  picture on the clipboard leaves as well. For a reformat or the 3D view it applies if any
  image of the series may show details, because they are made from all of them. The buttons
  start on "Cancel".
- Toasts name the file or folder, never the whole path: a screenshot could show it.

### D20 — What a series export does in unusual cases
It takes every image in display order across image sets, each with the window of its set,
and the transform of the acquired view. An image that could not be decoded keeps its number
free, so that the numbers stay those of the display order, and the message says so. A file
that cannot be written stops the export. Suggested names of reformats and the 3D view carry
`MPR` or `3D`, because their numbers would otherwise coincide with those of acquired images.

### D21 — Details of the print window
- `lp` also gets `-o media=A4` (or `Letter`): the paper the user chose. Nothing else.
- If `lpstat` fails, the window offers the system's default printer anyway: `lp` may still
  know where to print, and what it says is reported. `lpstat -e` empty: no printer is set up,
  "Print" is disabled and "Save as PDF…" stays.
- Printing the image on screen always uses one page; "images per page" applies to a series.
- Paper, orientation and images per page are kept for the session, not saved. Printing
  starts on "Print" once the printers are known.
- The preview is composed in the background at 110 dpi. It is the first page, in the same
  layout as the print.
- Windows close with Esc and "Cancel", not with a click beside them: options set and jobs
  running should not be lost to a stray click.
- `PrintSystem::at` and the `Chooser` trait are the test seams: fake scripts in a temporary
  folder stand in for `lp`, `lpstat` and the opener, a fake for the file dialogs.

## Risks / Trade-offs

- [Identifiable images shared unintentionally] → details off by default, burned-in
  warning, neutral file names.
- [8-bit pictures mistaken for diagnostic images] → caption on annotated exports.
- [Clipboard managers keep copies] → the clipboard receives the image-only picture
  without patient details.
- [Clipboard differences between platforms, especially Wayland] → tested per platform,
  with a clear message when the clipboard is unavailable.
- [A job kept by the print system] → only the job the user sent, with a neutral title; the
  print window says that the system may keep a copy until it has printed.
- [No print system on Windows] → "Save PDF and open…", which cannot be verified here; the
  page composition and the PDF are the same on every platform.
- [Large PDFs] → raster pages of a few megabytes each; a 60-image series on 9-image sheets
  is 7 pages.
