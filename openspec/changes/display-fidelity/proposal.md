## Why

Discs from real hospitals carry images that are standard but unusual: ultrasound and
stored screenshots in palette colour, X-ray and mammography images whose contrast is a
stored table rather than a window, CT slices padded with an "outside the scan" value,
graphics drawn by the scanner in separate bit planes, images that were compressed with
loss, and pictures with patient details burned into the pixels. Today some of these fail
("PALETTE COLOR images are not supported yet"), some show misleading values (−3024 HU
outside a CT circle, overlay bits leaking into grey values) and some hide a fact the user
should know (that detail was discarded, or that privacy mode cannot hide text in the
picture). Each gap is small; together they decide whether odd files display correctly and
honestly.

## What Changes

- Palette-colour images (Photometric Interpretation PALETTE COLOR) are shown in their
  colours. Segmented palettes are reported as not supported.
- Contrast tables: every table of the VOI LUT Sequence is offered in the contrast menu
  under its LUT Explanation; the first table is the default when the file stores no window.
  Adjusting contrast by hand switches to an ordinary window. Reformats and slabs keep using
  windows: a table applies to the acquired images only.
- The Modality LUT Sequence is applied when present instead of Rescale Slope/Intercept.
- Presentation LUT Shape INVERSE is honoured like MONOCHROME1.
- Stored values are masked to Bits Stored (sign-extended when signed) before any modality
  transform, so overlay bits or garbage in unused bits never change a value.
- Pixel padding (Pixel Padding Value and Range Limit) is left out of the automatic window
  and the value range, drawn as background, left out of reformats, and the read-out says
  "Outside the scanned area" instead of a value such as −3024 HU.
- Scanner graphics (DICOM overlay planes, groups 60xx, both the Overlay Data form and the
  retired form in unused bits of the pixel data) are drawn over the acquired image in a
  contrasting colour, on by default, with a toggle in the status bar (shown while the
  series has scanner graphics) and the key G. Multi-frame overlays follow their frame
  origin.
- A lossy-compression mark ("Lossy 10:1 (JPEG)") is drawn on every image that is or was
  compressed with loss, whether annotations are shown or not; the inspector states the same.
- In privacy mode, an image recorded with burned-in annotation says that patient details
  may be part of the picture itself and cannot be hidden.
- A built-in display check (from Settings and from the About dialog): grey steps from 0 to
  100 %, a 5 % patch inside black and a 95 % patch inside white, a continuous ramp, fine line
  pairs and a border, with plain instructions. It is a quick check in the style of the SMPTE
  pattern, not a calibration and not a compliance claim.
- A DICOM conformance statement, `docs/dicom-conformance-statement.md`, in the layout of
  PS3.2 reduced to what applies (media reader only, no networking); a test keeps its
  transfer-syntax table in step with the code.

Patient safety: values shown stay modality values (tables change only grey levels, never
the read-out); padding is never shown as a measured value; overlay bits never enter pixel
values and graphics are never baked into them; facts that limit fidelity (lossy
compression, burned-in text that privacy mode cannot hide) are stated on the image itself.
The display check involves no patient data and claims nothing beyond a visual check.

Privacy: nothing new is written to disk or to the settings; the scanner-graphics toggle is
view state, the display check is generated in memory.

## Capabilities

### New Capabilities
- `dicom-conformance`: the DICOM conformance statement and the test that keeps it in step with the code.

### Modified Capabilities
- `image-decoding`: adds palette colour, the Modality LUT Sequence, masking to Bits Stored, Presentation LUT Shape INVERSE, pixel padding, scanner graphics planes and the lossy-compression facts.
- `image-viewing`: adds drawing of scanner graphics with toggle and key, padding as background with its read-out, and the lossy mark.
- `contrast-windowing`: adds contrast tables (VOI LUT Sequence) as contrast choices and their rules.
- `privacy`: adds the burned-in text notice in privacy mode.
- `application-shell`: adds the display check.

## Impact

- `crates/imaging`: new modules `lut` (lookup tables, palette), `padding`, `graphics`
  (1-bit planes and compositing) and `test_pattern` (the display check); `window` renders
  through a table; `Volume` knows the padding of its slices.
- `crates/dicom`: new modules `pixels` (stored values, modality transform, padding,
  palette), `graphics` (overlay planes) and `fidelity` (lossy, burned-in, inverse facts read
  with the headers); `DecodedInstance` gains `extras`, `InstanceHeader` gains `fidelity`;
  `format` knows which transfer syntaxes are lossy.
- `crates/core`: the series store keeps padding and graphics per image and the contrast
  tables per stack; `ViewState` gains the contrast table and the graphics toggle; a
  `contrast` module lists the contrast choices.
- `crates/ui`: render path, read-out, badges and notices, status-bar toggle, the display
  check screen.
- `crates/testkit`: a study of twelve small series covering each case.
- `docs/dicom-conformance-statement.md` (new).
- No new dependency, no file-format or settings change.
