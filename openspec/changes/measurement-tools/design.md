## Context

The viewport already converts between screen points, image pixels and patient
coordinates (`Displayed::pixel_to_patient`, `patient_to_pixel`, `mm_per_pixel_x` in
`crates/ui/src/render.rs`), and the status bar reads the value under the pointer. Tools
are `WindowLevel`, `Pan` and `Zoom`. Acquired frames hold modality values
(`VoxelBuffer`); reformatted images hold resampled values with NaN outside the volume.
The index stores one `pixel_spacing` per image that silently falls back from Pixel
Spacing to Imager Pixel Spacing, so the origin of the scale is not known today.

## Goals / Non-Goals

**Goals:**
- Correct, simple measurements with honest units.
- Identical results at any zoom and under non-square pixels.
- Nothing persisted.

**Non-Goals:**
- Cobb angle, polygon or freehand regions, volumes, measurements across slices.
- Saving measurements as DICOM presentation states or structured reports.
- User calibration, ultrasound region calibration.
- Measurements on 3D or fused views.

## Decisions

### D1 — Points live in patient space
A measurement stores its points in patient coordinates (mm, LPS) together with the plane
it belongs to; images without patient geometry store pixel coordinates and the frame.
Length is the Euclidean distance of the patient points, which equals pixel deltas scaled
by row and column spacing.

Alternative considered: storing screen or pixel coordinates only — rejected: wrong under
non-square pixels and unusable on another view of the same plane.

### D2 — Where a measurement is visible
A measurement drawn on an acquired image is shown on that image only; one drawn on a
reformat is shown while the reformat is within half a step of the position where it was
drawn.

### D3 — Source of region statistics
On acquired images statistics use the stored modality values of the pixels whose centres
lie inside the shape, without interpolation. On reformats they use the displayed
resampled values, and when a slab projection is active the result carries the projection
name (for example "MIP").

Alternative considered: always computing from original voxels — rejected for reformats:
an oblique region has no exact voxel set, and "statistics of what is displayed" is
easier to explain.

### D4 — Calibration ladder
1. Pixel Spacing (0028,0030): millimetres.
2. Imager Pixel Spacing (0018,1164) only: millimetres "at detector — true size may be
   smaller".
3. Neither: pixels, "uncalibrated".

The index records which case applies instead of merging both attributes into one value.

### D5 — Precision and approximation marks
Lengths and angles are shown to one decimal; areas in mm² below 100 mm², otherwise in
cm². Measurements on reformats are prefixed with "≈" and their tooltip states the slice
spacing, because resolution across slices is coarser than within them.

### D6 — Interaction
Click-drag creates a length or a region; an angle takes three clicks. Handles have an
8-point hit radius, Esc cancels, Delete or Backspace removes the selection. A measurement
tool stays active until another tool is chosen; right-drag keeps adjusting contrast.

### D7 — Session-only storage and notice
Measurements are kept in the per-series view state and dropped when the study closes, so
the privacy specification is unchanged. A one-time notice and a status-bar hint while a
measurement tool is active say "Approximate — not for diagnosis"; only the fact that the
notice was shown is remembered.

## Risks / Trade-offs

- [False confidence in sizes] → notice, calibration label, one-decimal precision, tests
  against the phantom.
- [Coarse slice spacing makes reformat measurements imprecise] → "≈" prefix and slice
  spacing in the tooltip.
- [Magnification in projection X-ray images] → "at detector" note.
- [Where users click depends on the contrast window] → documented limitation; results
  themselves do not depend on the window.
