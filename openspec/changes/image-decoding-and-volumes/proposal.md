## Why

The old decoder read every pixel as a **big-endian** 16-bit word (DICOM is little
endian), ignored Bits Allocated, Pixel Representation, Rescale Slope/Intercept and
Photometric Interpretation, could not decompress JPEG/JPEG 2000/RLE (common on CDs),
defaulted missing dimensions to 512×512 and sorted slices by Instance Number only.
Images were therefore wrong — sometimes visibly, sometimes subtly (no Hounsfield
units) — which is unacceptable for a medical viewer, even a non-diagnostic one.

## What Changes

- Decode pixel data with dicom-rs `dicom-pixeldata` 0.10 for all transfer syntaxes it
  supports in pure Rust (uncompressed LE/BE, deflate, JPEG baseline/extended/lossless,
  JPEG 2000 via `openjp2`, RLE); JPEG-LS (CharLS) and JPEG XL as opt-in build features.
- Apply the Modality LUT (per-frame or shared functional groups included) so CT values
  are Hounsfield units; expose the unit.
- Handle MONOCHROME1/2, RGB and YBR colour (incl. planar configuration), multi-frame and
  Enhanced multi-frame objects.
- Order frames spatially along the slice normal, aligned with acquisition order; build a
  regular volume (affine incl. gantry tilt) only when the stack is truly regular and
  record why otherwise.
- Store samples compactly (i16/u16 when lossless, else f32); isolate decoder panics;
  keep a memory-budgeted LRU of decoded series; stream thumbnails.

## Capabilities

### New Capabilities
- `image-decoding`: turning DICOM pixel data into correctly scaled, ordered frames and volumes.

### Modified Capabilities
<!-- None. -->

## Impact

`crates/dicom/src/decode.rs`, `crates/imaging/src/volume.rs`, `crates/core/src/session.rs`.
**Patient safety:** displayed values and geometry become standards-conformant; the
previous output must not be relied on.
