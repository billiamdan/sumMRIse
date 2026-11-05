## Why

"Proper contrast adjustments" were missing: the old renderer recomputed a 2–98 %
percentile window by sorting every pixel on every repaint, ignored the windows stored by
the scanner, had no presets, no interactive adjustment and no inversion — and because
rescale was ignored, CT windows in Hounsfield units could not work at all.

## What Changes

- Implement the DICOM VOI LUT functions LINEAR, LINEAR_EXACT and SIGMOID (PS3.3
  C.11.2.1.2) in `summrise-imaging`.
- Start each series with the first window stored in the files (with its explanation),
  otherwise a robust automatic window (0.5–99.5 percentile, background excluded).
- Presets: stored DICOM windows, clinical CT windows (only for Hounsfield data), Auto and
  Full range; keyboard 1–9 and A.
- Interactive adjustment: drag (left with the contrast tool, or right-drag anytime),
  numeric W/L fields, invert (I), reset (R).

## Capabilities

### New Capabilities
- `contrast-windowing`: mapping modality values to display intensity and letting the user adjust it.

### Modified Capabilities
<!-- None. -->

## Impact

`crates/imaging/src/window.rs`, `crates/core/src/view.rs`, toolbar and viewport in
`crates/ui`. **Patient safety:** standard formulas and preset values are unit tested.
