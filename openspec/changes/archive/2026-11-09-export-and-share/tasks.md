## 1. Rendering and encoding (`summrise-core`, the render path in `summrise-ui`: design D13)

- [x] 1.1 Image-only render at native resolution with window and inversion, resampled to square pixels; unit test with the Lung window (−600 HU gives 127 or 128, −1350 HU gives 0, +150 HU gives 255)
- [x] 1.2 PNG and JPEG encoding without patient metadata; neutral file-name suggestion
- [x] 1.3 Series-export job with progress, cancel and overwrite confirmation

## 2. UI

- [x] 2.1 Export commands: Save image…, Copy image, Export series…, with platform shortcuts shown in tooltips
- [x] 2.2 Export options: image only or with annotations, format, "Include patient details" (off, not remembered)
- [x] 2.3 Annotated capture pass with the privacy filter and the "Not for diagnostic use" caption
- [x] 2.4 Burned-in text warning for Burned In Annotation, secondary capture and ultrasound images
- 2.5 Moved to `field-validation` 5.16 when this change was archived: Clipboard copy when no text is selected (done and tested), with a clear message when the clipboard is unavailable (not possible in general: egui's clipboard route reports no failure; on Linux, where it can refuse images, the message says what to do; design D18)

## 3. Privacy

- [x] 3.1 Update the README privacy wording to match the amended "No derived data at rest" requirement (README and the conformance statement)
- [x] 3.2 Test that an export writes exactly one file in the chosen location and nothing elsewhere

- [x] 3.3 With the other features: a file with annotations shows the tile grid and the measurements of the view (`with_annotations_shows_the_tile_grid_and_image_only_the_current_image`, `a_measurement_is_in_a_file_saved_with_annotations`); the commands rest while a document is shown (`saving_and_printing_rest_while_a_document_is_shown`)

## 4. Printing

- [x] 4.1 Page composition (pure, in `summrise-core`): cells for 1, 2, 4, 6, 9 and 12 images in both orientations on A4 and Letter at 300 dpi; unit tests of the page count (60 images, 9 per page: 7 pages, the last with 55 to 60) and of correct proportions at Pixel Spacing 1.0 \ 0.5
- [x] 4.2 Text on pages with the fonts of the interface in every language; header, footer, image number, orientation letters, scale bar and window values
- [x] 4.3 PDF writer: one lossless picture per page, no patient details in the metadata; test that the file parses (and that a system PDF reader opens it, where there is one)
- [x] 4.4 Print window: what, printer, copies, paper, orientation, images per page, "Include patient details" (off, not remembered), preview with the number of pages; ⌘P / Ctrl+P
- [x] 4.5 `lpstat` and `lp` through absolute paths with a test seam: a fake `lp` receives `%PDF`, the printer, the copies and the title `SumMRIse`, and no file appears; an `lp` error is reported
- [x] 4.6 "Save as PDF…", and "Save PDF and open…" where `lp` is missing; suggested name `SumMRIse-CT-S1.pdf`
- [x] 4.7 Series job in the background with progress and cancel; burned-in warning; default pages hold no patient details (test with the synthetic patient's name, ID and birth date)
- 4.8 Moved to `field-validation` 5.17 when this change was archived: Printed on a real printer from macOS, and "Save PDF and open…" tried on Windows and Linux without CUPS (cannot be verified here: no printer, and only this Mac; "Save PDF and open…" was run on this Mac against a fake opener only)

## 5. Verification

- [x] 5.1 Tests: exported PNG size and grey values; the 256 × 128 image with Pixel Spacing 1.0 \ 0.5 exported as 256 × 256
- [x] 5.2 Tests: a default export has no patient name, ID or birth date in the picture, the file name or the metadata
- [x] 5.3 Series export of the 60-image synthetic CT, and cancelling after 20 files
- 5.4 Moved to `field-validation` 5.18 when this change was archived: Clipboard checked on macOS; Windows and Linux (X11 and Wayland) cannot be verified on this Mac (only the command egui hands to its integration was checked offscreen, not a real paste: the application window cannot be operated here)
