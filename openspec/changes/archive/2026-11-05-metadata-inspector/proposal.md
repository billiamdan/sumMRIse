## Why

The brief asks that "all the parameters with labels" be presented "in a neat and
extensive way". The old app showed only patient ID, study date, modality and series
count; there was no way to see acquisition parameters (TR/TE, kV/mAs, slice thickness…)
or the complete DICOM header.

## What Changes

- A right-hand inspector for the image currently displayed (follows slice changes,
  loads in the background).
- **Overview** tab: a "Display" section (image index, window, value range, volume and
  voxel size, reformatting availability, memory) and curated sections Patient, Study,
  Series, Equipment, Acquisition (MR- and CT-specific parameters with units), Image and
  Identifiers — formatted for humans (names, ISO dates, ages, expanded codes).
- **All attributes** tab: the complete header (file meta + data set) with tag number,
  dictionary name, keyword, VR and value; nested sequences; private tags with their
  creator; full-text search that expands matching sequences; copy via context menu.

## Capabilities

### New Capabilities
- `metadata-inspector`: presentation of DICOM metadata for the displayed image.

### Modified Capabilities
<!-- None. -->

## Impact

`crates/dicom/src/{metadata,format}.rs`, `crates/ui/src/inspector.rs`. **Privacy:** the
inspector honours privacy mode (see `privacy-safeguards`).
