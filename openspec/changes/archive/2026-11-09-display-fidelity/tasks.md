## 1. Synthetic data (`summrise-testkit`)

- [x] 1.1 `fidelity::write_fidelity_study`: 8-bit and 16-bit palette images with known colours; a radiograph with a two-table VOI LUT Sequence and no window; an image with a Modality LUT Sequence; a signed CT slice with padding −2000 outside a circle; an image with a (6000,3000) rectangle at a known origin; a 12-bit image with an overlay in bit 12 and garbage in bits 13–15; a 3-frame image with a 2-frame overlay from frame 2; an image recorded lossy 10:1 with ISO_10918_1; an ultrasound image with Burned In Annotation YES; a palette image with segmented data only; a MONOCHROME2 radiograph with Presentation LUT Shape INVERSE; test: the files are written and indexed as twelve series

## 2. Pixel maths (`summrise-imaging`)

- [x] 2.1 `lut`: lookup table (first mapped value, bits, clamping, normalisation, input range), palette of three tables (8/16-bit entries to 8 bits); unit tests
- [x] 2.2 `window::lut_to_rgba`: grey through a table, inversion, non-finite values black; unit test
- [x] 2.3 `padding`: `Padding` range, padding-aware min/max; `Volume::with_padding` and resampling that treats padding voxels as outside; unit tests (reslice of a padded volume)
- [x] 2.4 `graphics`: 1-bit `GraphicsPlane` (from bytes, from a function, origin outside the image) and `composite`; unit tests
- [x] 2.5 `test_pattern::DisplayCheck`: layout and pixels; tests: patch values at their pixels for 800 × 600 and 1280 × 720, border, ramp monotonic, line pairs alternate

## 3. Decoding (`summrise-dicom`)

- [x] 3.1 `pixels`: stored values masked to Bits Stored with High Bit and sign extension (1, 8, 16 bits allocated); modality transform (rescale per frame, Modality LUT Sequence); padding detection with the clash check; tests incl. garbage in the high bits and signed 12-bit data
- [x] 3.2 LUT parsing shared by VOI, Modality and palette tables (65536 entries, signed first value, packed 8-bit data, high-byte entries); unit tests
- [x] 3.3 Palette colour decoding; segmented palettes reported; tests: index 3 → (200, 30, 60) for 8- and 16-bit
- [x] 3.4 VOI LUT Sequence read into `DecodedInstance::extras` with names; test: two tables, stored 100 → entry 640
- [x] 3.5 `graphics`: overlay planes from (60xx,3000) through `decode_overlay`, the retired form from the decoded frames, frame origin, malformed planes skipped; tests: rectangle at its origin with unchanged values, bit 12, multi-frame
- [x] 3.6 `fidelity` read with the headers: lossy facts and label, burned-in text, INVERSE, Modality LUT output range; `format::transfer_syntax_is_lossy`; plan marks palette series as colour and INVERSE as inverted; tests: "Lossy 10:1 (JPEG)", "Lossy (JPEG)" for the JPEG localizer, J2K not lossy
- [x] 3.7 Inspector summary: "Lossy compression" row with the same label (also from the transfer syntax), "Burned-in text" row; explanation of the new key in the guide; tests
- [x] 3.8 Padding-aware statistics in `DecodedInstance::initial_window`, `load_series` and thumbnails; test: the automatic window of the padded CT equals that of its pixels inside the circle, value range −1000 … 700 HU

## 4. Series store and view state (`summrise-core`)

- [x] 4.1 `SeriesData` keeps padding and graphics per image and the contrast tables per stack; padding-aware value range and automatic window; volumes carry the padding of their slices; tests
- [x] 4.2 `ViewState::lut` and `show_graphics` (in `is_pristine` and `reset`); `contrast` module: choices in menu order, applying them, default table, leaving a table by drag; per-stack table choice in `StackWindow`; tests

## 5. Viewer (`summrise-ui`)

- [x] 5.1 Render path: grey through the window or the table, padding black, graphics composited from one line of `render_frame`; render key includes table and graphics; test: overlay pixel drawn in the graphics colour, value unchanged; stored 100 drawn as grey 40 through "Normal"
- [x] 5.2 Contrast menu with tables, keys 1–9 over the choices, annotations and inspector row name the table; W/L edits leave the table; tests
- [x] 5.3 Scanner graphics toggle (status bar, shown for series with graphics; the toolbar is full at 1280 points) and key G; tests
- [x] 5.4 Read-out "Outside the scanned area" over padding (acquired and reformat views); test
- [x] 5.5 Lossy badge in row 1 with `orient::badge`; test with the mirrored badge; shown with annotations hidden
- [x] 5.6 Burned-in notice in privacy mode; tests (privacy on/off)
- [x] 5.7 Display check screen, opened from Settings and About, Escape closes; tests (keyboard path, texts)
- [x] 5.8 Offscreen snapshots looked at: palette image, table selected, graphics on and off, lossy and mirrored badges, privacy notice, display check dark and light

## 6. Conformance statement

- [x] 6.1 `docs/dicom-conformance-statement.md` in the PS3.2 layout reduced to what applies
- [x] 6.2 `crates/dicom/tests/conformance.rs`: every registered transfer syntax listed, decode and lossy columns agree with the code and the build options

## 7. Verification

- [x] 7.1 `cargo xtask ci` and `openspec validate --all --strict` pass
- [x] 7.2 New tests that wait for the background loader pass 20 times in a row
- 7.3 Moved to `field-validation` 5.11 when this change was archived: The display check and the scanner-graphics key tried in the real window on a calibrated and an uncalibrated screen (cannot be verified here: the application window cannot be operated from this environment)
