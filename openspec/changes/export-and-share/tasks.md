## 1. Rendering and encoding (`summrise-core`)

- [ ] 1.1 Image-only render at native resolution with window and inversion, resampled to square pixels; unit test with the Lung window (−600 HU gives 127 or 128, −1350 HU gives 0, +150 HU gives 255)
- [ ] 1.2 PNG and JPEG encoding without patient metadata; neutral file-name suggestion
- [ ] 1.3 Series-export job with progress, cancel and overwrite confirmation

## 2. UI

- [ ] 2.1 Export commands: Save image…, Copy image, Export series…, with platform shortcuts shown in tooltips
- [ ] 2.2 Export options: image only or with annotations, format, "Include patient details" (off, not remembered)
- [ ] 2.3 Annotated capture pass with the privacy filter and the "Not for diagnostic use" caption
- [ ] 2.4 Burned-in text warning for Burned In Annotation, secondary capture and ultrasound images
- [ ] 2.5 Clipboard copy when no text is selected, with a clear message when the clipboard is unavailable

## 3. Privacy

- [ ] 3.1 Update the README privacy wording to match the amended "No derived data at rest" requirement
- [ ] 3.2 Test that an export writes exactly one file in the chosen location and nothing elsewhere

## 4. Verification

- [ ] 4.1 Tests: exported PNG size and grey values; the 256 × 128 image with Pixel Spacing 1.0 \ 0.5 exported as 256 × 256
- [ ] 4.2 Tests: a default export has no patient name, ID or birth date in the picture, the file name or the metadata
- [ ] 4.3 Series export of the 60-image synthetic CT, and cancelling after 20 files
- [ ] 4.4 Clipboard checked on macOS; Windows and Linux (X11 and Wayland) cannot be verified on this Mac
