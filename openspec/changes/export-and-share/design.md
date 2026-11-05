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
- Video or animated export (after cine exists), printing.
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
chosen location only.

## Risks / Trade-offs

- [Identifiable images shared unintentionally] → details off by default, burned-in
  warning, neutral file names.
- [8-bit pictures mistaken for diagnostic images] → caption on annotated exports.
- [Clipboard managers keep copies] → the clipboard receives the image-only picture
  without patient details.
- [Clipboard differences between platforms, especially Wayland] → tested per platform,
  with a clear message when the clipboard is unavailable.
