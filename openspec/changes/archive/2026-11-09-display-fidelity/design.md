## Context

`decode_object` (`crates/dicom/src/decode.rs`) asks `dicom-pixeldata` 0.10.1 for the
decoded pixel data and turns MONOCHROME frames into modality values with
`to_vec_frame::<f32>`, RGB/YBR frames into 8-bit RGB, and rejects everything else
("PALETTE COLOR images are not supported yet"). Windows come from the VOI LUT module's
Window Center/Width; nothing reads the VOI LUT Sequence, the Modality LUT Sequence,
Presentation LUT Shape, Pixel Padding Value, the overlay groups 60xx, Burned In Annotation
or the lossy-compression attributes. The decoded image of one displayed image lives in a
write-once slot of `SeriesData` (`crates/core/src/series.rs`); `ViewState` holds the
window; `render.rs` turns a slot into a texture; badges on a picture are drawn with
`orient::badge` (row 0 is "Mirrored").

## Goals / Non-Goals

**Goals:**
- Palette colour, contrast tables, the Modality LUT Sequence, Presentation LUT Shape,
  padding and overlay planes handled as PS3.3 defines them, with tests on synthetic files.
- Values shown stay modality values; padding and graphics never become values.
- Facts that limit fidelity are stated on the image (lossy compression, burned-in text in
  privacy mode).
- A display check and a conformance statement that say exactly what they are.

**Non-Goals:**
- Segmented palettes, Supplemental Palette Color (grey + colour ranges), presentation
  states (GSPS), ICC colour management, true monitor calibration (GSDF), curve data (50xx),
  bitmap overlays from other objects, editing overlays.
- Showing Overlay Label/Description text (it may hold free text from the site).

## Decisions

### D1 — Three layers (binding)
- `crates/dicom` decodes: stored values, modality transform, palette lookup, padding
  detection, overlay planes (new modules `pixels`, `graphics`, `fidelity`).
- `crates/imaging` holds the pure pixel maths: lookup tables and their application
  (`lut`), the padding range (`padding`), 1-bit planes and their compositing (`graphics`),
  the display-check picture (`test_pattern`), rendering through a table (`window`).
- `crates/ui` displays: render path, read-out, badges, notice, toggle, display-check screen.
- `crates/core` keeps per-image extras in the series store and the contrast state in
  `ViewState`.

### D2 — What `dicom-pixeldata` 0.10.1 does, and what is added (binding)
Read from its source (`lib.rs`, `lut.rs`, `attribute.rs`, `overlay.rs`):

| Topic | `dicom-pixeldata` 0.10.1 | SumMRIse |
|---|---|---|
| Decompression, planar configuration, YBR in JPEG | decodes every transfer syntax its codecs cover; colour output of codecs is RGB | used as before |
| PALETTE COLOR | `decode_pixel_data` returns the indices with photometric `PaletteColor`; no conversion applies the palette | palette applied here (D11) |
| Rescale Slope/Intercept | read at the top level and from the functional groups (`rescale()`), applied by `to_vec*` | `rescale()` used as before |
| Modality LUT Sequence | not read | parsed and applied here (D12) |
| VOI LUT Sequence | parsed (`voi_lut_sequence()`), applied only when converting to an 8/16-bit image, only one table; the parser rejects 65536-entry tables (first descriptor value 0), 8-bit entries packed two per word, and drops the whole sequence when one item is malformed | parsed here with one parser shared by the three LUT kinds; every table becomes a contrast choice (D4) |
| Masking to Bits Stored | the 16-bit path of `to_vec*` builds a table of 2^BitsStored entries and masks every sample with it (which also sign-extends); the 8-bit path always uses 8 bits (no masking for Bits Stored < 8); High Bit other than BitsStored − 1 is not honoured; 1- and 32-bit allocations are errors | stored values extracted here from the decoded frame bytes (D3) |
| Overlay planes | `decode_overlay(i)` reads a plane with Overlay Data (60xx,3000) correctly, unpacked to one byte per pixel, with origin and frame origin; the retired form in unused pixel bits only from native pixel data; `decode_overlays` fails as a whole when one plane is malformed | `decode_overlay(i)` used per group for (60xx,3000) planes, malformed planes skipped; the retired form extracted here from the decoded frames (every transfer syntax); both kept as 1-bit planes (D6) |
| Pixel padding, Presentation LUT Shape, lossy and burned-in facts | not handled (MONOCHROME1 inversion only in image output) | added (D5, D7, D8, D10) |

Alternative considered: calling `to_vec_frame_with_options` with an identity rescale to get
masked stored values — rejected: it does not mask 8-bit data with fewer bits stored, ignores
High Bit, fails for 1-bit data, and would still leave the Modality LUT Sequence and padding
(which needs stored values) to us.

### D3 — Stored values first (binding)
For every grey-scale frame the decoder reads each sample from the decoded bytes (1, 8 or 16
bits allocated; 1-bit samples arrive expanded to bytes), shifts native data right by
HighBit + 1 − BitsStored (encapsulated data is decoded right-aligned), masks it to Bits
Stored and sign-extends it when Pixel Representation is 1. Padding is detected on these
stored values, then the modality transform runs. Bits above Bits Stored — overlay bits of
the retired form or garbage — therefore never change a value. 32-bit allocations stay an
error ("32-bit grey-scale images are not supported yet"). For files with High Bit =
Bits Stored − 1 (all synthetic series, and practically every file) the values are the same
as before; the existing tests pass unchanged. A new test writes garbage into bits 13–15 and
an overlay into bit 12 of 12-bit data and checks every value.

### D4 — A contrast table is contrast state next to the window (binding)
`Window` stays `Copy` and unchanged. `ViewState` gains `lut: Option<usize>`: `None` means
the window applies, `Some(i)` means the acquired images are shown through table `i` of the
current stack (`SeriesData::stack_luts`). `window` keeps its meaning and is what reformats
and slabs use: a table applies to acquired images only (reformats resample values between
voxels; a table is defined on the stored grid of one image).
- Default: the first table when the first decoded image of the stack stores no window,
  else the first window, as before.
- Menu and keys: the contrast menu lists the stored windows, then the stored tables (under
  their LUT Explanation, title-cased like window explanations, or "Contrast table n"), then
  the built-in CT windows, Auto and Full range; keys 1–9 count through this list, so key 1
  is always the default. `ViewState::contrast_choices` builds the list (`crates/core/src/
  contrast.rs`), `presets_for` keeps returning windows only.
- Leaving a table: choosing a window, dragging, editing W or L, or the A key switch to an
  ordinary window — the window as it was (the series default or the last one chosen); the
  picture may change at that moment, which is the honest outcome of "adjusting switches to
  a window".
- Per stack: where stacks keep a window each, they keep a table choice each
  (`StackWindow` becomes window, preset, table).
- `is_pristine` and `reset` include it.
- Applying a table: input = modality value rounded to the nearest integer; below the first
  mapped value → first entry, above the last → last entry; output divided by 2^bits − 1 of
  the LUT descriptor (by the largest entry when that is larger, for files whose descriptor
  understates the bits); inverted afterwards for MONOCHROME1 or INVERSE; padding black.
- Labelled truthfully: the acquired view's annotation ("Contrast table" and its name), the
  menu and the inspector show the table; the inspector's "Window" row gives way to a
  "Contrast table" row while a table is shown (two contrasts side by side would read as a
  contradiction); reformat views keep showing their window.

Alternatives considered:
- A table as a kind of `Window` — rejected (binding): `Window` is `Copy` and drives
  reformats and slabs, where a table does not apply.
- Applying the table when decoding — rejected: the read-out must show modality values.
- Approximating a table by a window — rejected: it is not what is stored.

### D5 — Padding: a range of modality values per image (binding; representation chosen here)
A frame with padding keeps its values as decoded and records `Padding { low, high }`: the
closed interval of modality values that the padding pixels have and no other pixel of the
frame has. No extra memory for the common case.
- Detection: a stored value equal to Pixel Padding Value, or within [Pixel Padding Value,
  Pixel Padding Range Limit] (either order), is padding. The values are compared as stored
  (US or SS by Pixel Representation; the parser already reads them as SS for signed data).
  A padding value outside the stored range matches nothing.
- Exactness: Rescale Slope ≠ 0 is strictly monotonic, so the interval holds exactly the
  padding pixels. For a Modality LUT Sequence (possibly not monotonic) the decoder checks
  every other pixel against the interval; on a clash the padding pixels become NaN in an
  f32 buffer — memory doubles for that frame only, in that rare case only.
- Consumers treat a value inside the frame's interval, or a non-finite value, as "outside
  the scanned area": the automatic window and the value range skip it; rendering draws it
  black (also when inverted, like the outside of a reformat); the read-out says "Outside
  the scanned area"; volumes carry the interval of each slice (`Volume::with_padding`, kept
  behind one pointer so that `Volume` grows by 8 bytes only) and resampling treats a
  padding voxel like the outside of the volume (NaN), so reformats, slabs and their
  read-out neither show nor interpolate padding.
- Test: the automatic window of the padded CT slice equals the automatic window of its
  pixels inside the circle (padding −2000, intercept −1024: −3024 HU never appears).

Alternatives considered:
- NaN in an f32 buffer for every padded frame — rejected (brief): doubles the memory of
  16-bit data.
- A 1-bit mask per frame — exact, 1/16 of the image, but every consumer then needs
  index-aligned look-ups and volumes a mask volume; rejected as more invasive for no gain
  over the checked interval.
- A sentinel value written into the buffer — rejected: i16/u16 data may use every value.
- Replacing padding by the minimum value — rejected: it would be shown and read as a value.

### D6 — Scanner graphics: 1-bit planes composited at render time (binding)
- Planes: `summrise_imaging::graphics::GraphicsPlane` (columns, rows, origin relative to the
  image, bits in `u64` words). Each displayed image keeps the planes that apply to it in its
  slot, `Arc`-shared when one plane applies to several frames. They are never written into
  values.
- Reading (`crates/dicom/src/graphics.rs`): groups 6000–601E. With Overlay Data:
  `decode_overlay(i)`, repacked to bits. Without Overlay Data but with Overlay Bit Position:
  the retired form, extracted from the decoded frames at that bit; a bit position inside
  the stored bits cannot be told apart from image data and is ignored (logged at debug).
  Malformed planes are skipped, never fail the image. Overlay type G and R are both drawn.
- Frames: a plane with Number of Frames in Overlay applies to image frames from Image Frame
  Origin on; a plane without it applies to every frame (as viewers such as dcmtk do).
- Compositing: `summrise_imaging::graphics::composite(rgba, width, height, planes, colour)`
  sets the RGBA pixels under set bits; it is called from one line of `render_frame` (the
  PET fusion change adds its own colour layer in the same function). Colour: cyan
  (0x3D, 0xE0, 0xFF), which stands out on grey and differs from the yellow edge letters
  and the view colours.
- Acquired images only: no graphics on reformats, slabs or the 3D view.
- Toggle: `ViewState::show_graphics` (default on, part of `is_pristine` and `reset`), key G,
  and a toggle "Scanner graphics" at the right of the status bar, shown only while the
  series shown has graphics — so it also tells the user that the images carry some, and
  shows when they are hidden. Called "Scanner graphics" in the interface: "overlays"
  already names the annotations.

Alternatives considered:
- A toolbar toggle — tried and rejected: at 1280 points one more toolbar button made the
  "Open" button cover the "Rotate and flip" menu (seen in a snapshot of the image with
  graphics), and the brief forbids new top-level toolbar widgets.
- Turning the annotations button into a menu for both — rejected: it would change how an
  existing control works (two clicks instead of one) for a feature most series lack.
- Baking graphics into the texture at decode time — rejected (binding): they must be
  switchable and must never alter values.
- A persisted setting for the toggle — rejected: switched off once, graphics would be
  missing on the next disc without notice; view state starts on for every series and R
  restores it.
- Unpacked planes (one byte per pixel, as `dicom-pixeldata` returns them) — rejected: 8×
  the memory.

### D7 — Lossy mark (binding)
- Facts: Lossy Image Compression (0028,2110) = "01", or a transfer syntax that is always
  lossy (JPEG Baseline and Extended and the retired lossy JPEG processes, JPEG-LS
  near-lossless, MPEG-2, MPEG-4 AVC/H.264, HEVC/H.265, JPEG XL JPEG recompression). JPEG
  2000, HTJ2K and JPEG XL can be either: lossy only when (0028,2110) says so. Ratio from
  (0028,2112), method from (0028,2114) mapped to names (ISO_10918_1 → JPEG, ISO_14495_1 →
  JPEG-LS, ISO_15444_1 → JPEG 2000, ISO_15444_15 → HTJ2K, ISO_13818_2 → MPEG-2,
  ISO_14496_10 → H.264, ISO_23008_2 → HEVC, ISO_18181_1 → JPEG XL); without a recorded
  method, the family of a lossy transfer syntax.
- Read with the headers into `InstanceHeader::fidelity` (`crates/dicom/src/fidelity.rs`), so
  the mark is there before the pixels.
- Shown with `orient::badge` in row 1 (row 0 stays "Mirrored"), so the two never cover each
  other and the mark does not move when the view is mirrored; shown whether annotations are
  on or off. Text: "Lossy 10:1 (JPEG)", "Lossy 10:1", "Lossy (JPEG)" or "Lossy
  compression"; the tooltip explains it. Acquired view: the image shown; reformat views:
  any image of the series.
- Inspector: the existing row "Lossy compression" shows the same label, also when only the
  transfer syntax tells.

### D8 — Burned-in text in privacy mode (binding)
When privacy mode is on and the image shown records Burned In Annotation (0028,0301) =
"YES", the acquired view shows "Patient details may be part of the picture itself and
cannot be hidden." at its top-left corner — the place of the patient details it hides —
whether annotations are on or off, in the warning colour on a dark backing, readable by
assistive technology. Only "YES" triggers it; an absent attribute is not guessed at.

### D9 — Display check (binding)
- Picture (`summrise_imaging::test_pattern::DisplayCheck::new(width, height)`), generated
  for the physical pixel size of the area it fills, in memory, without patient data:
  50 % grey background; a two-pixel border (white outside, black inside); margin
  m = max(8, round(4 % of the shorter side)); 11 grey steps 0, 10 … 100 % across the top
  third, framed by two black pixels so that the 50 % step stands out from the background of
  the same level; a 5 % square inside the 0 % step and a 95 % square inside the 100 % step
  (side 45 % of the smaller step dimension); a continuous ramp from black to white; four
  line-pair blocks (1-pixel vertical, 1-pixel horizontal, 2-pixel vertical, 1-pixel
  checkerboard). Levels are 8-bit code values round(p × 255). The layout (rectangles and
  their levels) is returned with the pixels, for labels and tests.
- Screen: a full-window layer above the viewer, the picture painted 1:1 (nearest-neighbour
  texture at a pixel-aligned rectangle), step labels and plain instructions below it, a
  Close button; Escape closes it. In dark and light theme only the instructions panel
  follows the theme.
- Opened from the settings window ("Check the display…") and from the About dialog (F1,
  the app's help). Keyboard path: Ctrl/Cmd+comma or F1, Tab to the button, Space. No new
  shortcut: free keys are scarce and other features add some in parallel.
- Wording: "A quick check in the style of the SMPTE test pattern. It is not a calibration
  and does not show that the screen meets any standard."

Alternatives considered: a fixed bitmap (rejected: scaled by the window, it would blur the
line pairs); a window inside the viewer (rejected: the surroundings disturb the check).

### D10 — Presentation LUT Shape
INVERSE (2050,0020, top level or in the shared functional group) is read with the headers
and when decoding; a series opens inverted when its images are MONOCHROME1 or INVERSE —
not inverted twice for MONOCHROME1 + INVERSE, which PS3.3 requires to go together for DX.

### D11 — Palette colour
Indices are the masked stored values; each channel has its own descriptor (entries, first
index mapped, bits 8 or 16) and table; indices below the first or beyond the last entry use
the first or last entry. 16-bit entries become 8-bit by rounding v × 255 / 65535. 8-bit
tables are accepted packed two entries per 16-bit word (as PS3.3 specifies), one entry per
word, or one per byte; entries in the high byte of each word are recognised. A file with
Segmented Palette Color Lookup Table Data and no plain table fails with "Segmented palette
colour images are not supported yet". The header marks the series as colour (no windows, no
reformats) before decoding.

### D12 — Modality LUT Sequence
The first item replaces Rescale Slope/Intercept (PS3.3 says they are never both present;
the table wins if a file has both). Values outside the table's input range use its first or
last entry. Modality LUT Type gives the unit unless it is "US" (unspecified). The table's
output range is read with the headers for the value-range estimate while loading.

### D13 — LUT data parsing (shared)
One parser for the three LUT kinds: descriptor = entries (0 means 65536), first value
mapped (read as SS when the element is SS or the data are signed and the value exceeds
32767), bits per entry (1–16); data as 16-bit words or bytes; length checked against the
entries (8-bit tables also packed). Malformed tables are skipped (VOI), or make the image
fail with a reason (palette, Modality LUT).

### D14 — Conformance statement
`docs/dicom-conformance-statement.md` follows PS3.2 (Annex A layout) reduced to what
applies: overview, no networking, media storage as File-set Reader only, SOP classes,
transfer syntaxes, photometric interpretations, attributes read and ignored, character
sets, security, and the annexes that apply. Its transfer-syntax table lists every
transfer syntax of the dicom-rs registry with "Yes", "No" or "With the jpeg-ls build
option"; `crates/dicom/tests/conformance.rs` compares each row with
`format::transfer_syntax_supported` (and the feature flags) and checks that every
registered transfer syntax is listed, and that the lossy column matches
`format::transfer_syntax_is_lossy`.

## Risks / Trade-offs

- [Non-conformant LUT data (wrong bits, entries in the high byte)] → the parser accepts the
  common variants (D13); anything else is reported, never guessed.
- [A file with both a window and a table] → the window stays the default, as before; the
  table is one entry further down the menu and key list.
- [Keys 1–9 shift when tables exist] → key 1 is still the default; tables exist mostly in
  X-ray and mammography images, which have no CT presets.
- [Padding read-out in reformats] → a resampled point touching a padding voxel reads
  "Outside the scanned area"; the boundary of the scan circle is therefore up to one voxel
  narrower in reformats than in the acquired images.
- [G may be wanted by another feature] → listed in the hand-over; the binding is in one place.
- [The status-bar toggle appears only for series with graphics] → no permanent width; key G
  works always.
- [Graphics are magnified with the picture] → they are drawn into the picture at image
  resolution, so with smooth magnification on they look soft when zoomed in, like the image.
- [Display check under OS scaling] → if the window is scaled by the system, line pairs blur;
  the instructions say to use the screen's native resolution.

## Migration Plan

No stored data changes. `DecodedInstance` gains `extras` and `InstanceHeader` gains
`fidelity` (struct literals elsewhere add `..Default::default()`-style fields);
`ViewState` gains `lut` and `show_graphics`.
