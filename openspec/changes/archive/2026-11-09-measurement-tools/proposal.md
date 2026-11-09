## Why

Users cannot measure anything: the only quantitative read-out is the value under the
pointer. Every professional viewer offers length, angle and region statistics, and
patients routinely want to check a size mentioned in their report ("12 mm nodule") or
compare it between two examinations.

## What Changes

- New tools: length (mm), angle (degrees), and ellipse and rectangle regions (area,
  mean, standard deviation, minimum and maximum in the image's units, e.g. HU), usable on
  acquired images and on reformats.
- Measurements can be adjusted by their handles, moved, selected, deleted and cleared;
  they stay attached to the image they were drawn on.
- The unit and the source of the scale are always shown: images without pixel spacing
  are measured in pixels and marked uncalibrated, projection images calibrated at the
  detector say so, and measurements on reformats are marked approximate.
- Measurements exist only while the study is open; they are never written to disk.

Patient safety: measurements are computed in patient millimetres from the DICOM
geometry, are covered by tests against a phantom of known size, and are presented as
approximate and not for diagnosis. SumMRIse remains a non-diagnostic viewer.

## Capabilities

### New Capabilities
- `measurements`: length, angle and region-statistics tools with calibration transparency.

### Modified Capabilities
<!-- None. -->

## Impact

- `crates/imaging`: measurement maths (distance, angle, region rasterisation and
  statistics).
- `crates/dicom`: record where an image's pixel spacing comes from.
- `crates/core`: measurement model per series and tool state.
- `crates/ui`: toolbar tools, viewport interaction and drawing.
- Builds on the viewport model from `series-comparison`; it can be implemented on the
  current single-series viewer if that change is postponed.
