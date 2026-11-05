## Why

There is no way to get a picture out of SumMRIse: users cannot save an image to send to
a relative or a doctor, paste it into a message, or keep a series as ordinary image
files. Every professional viewer exports images. For a patient viewer the export must
also be private by default.

## What Changes

- Save the current view as PNG or JPEG, either "image only" (the picture at its native
  resolution with the current contrast) or "with annotations" (as displayed).
- Copy the current view to the clipboard.
- Export a whole series as numbered image files, with progress and cancel.
- The patient's name, ID and birth date are left out of exported pictures, file names
  and file metadata unless the user switches them on for that export.
- A warning when the picture itself may contain personal details (burned-in text).
- Exports with annotations carry a small "Not for diagnostic use" caption.

Privacy: this is the first feature that writes image data to disk. It does so only on an
explicit export and only to the place the user chooses; the "No derived data at rest"
rule is amended accordingly.

## Capabilities

### New Capabilities
- `export`: saving and copying views and exporting series as images, private by default.

### Modified Capabilities
- `privacy`: "No derived data at rest" allows files the user explicitly exports.

## Impact

- `crates/core`: export rendering and the series-export job.
- `crates/ui`: export commands, save dialogs, clipboard, burned-in warning.
- Dependencies: PNG and JPEG encoding through the `image` crate already in the workspace
  (JPEG feature to be enabled).
- Not included: de-identified DICOM export, video export and printing (see design.md).
