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
- Print the current image, or a whole series as sheets of 1 to 12 images per page, or save
  the same pages as a PDF. Pages leave out the patient's name, ID and birth date unless
  the user switches them on for that print job, and always carry "Not for diagnostic use".

Privacy: this is the first feature that writes image data to disk. It does so only on an
explicit export and only to the place the user chooses, and printing hands the pages to
the system's print system without a temporary file of the application's own; the "No
derived data at rest" rule is amended accordingly.

## Capabilities

### New Capabilities
- `export`: saving, copying and printing views and exporting series as images or PDF, private by default.

### Modified Capabilities
- `privacy`: "No derived data at rest" allows files the user explicitly exports and pages the user sends to the printer.

## Impact

- `crates/core`: export rendering, the series-export job, page composition, the PDF writer.
- `crates/ui`: export commands, save dialogs, clipboard, burned-in warning, print window.
- Dependencies: PNG and JPEG encoding through the `image` crate already in the workspace
  (JPEG feature to be enabled); no native print bindings.
- External programs: `lp` and `lpstat` of the system's print system, where it has one.
- Not included: de-identified DICOM export and video export (see design.md).
