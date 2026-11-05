## 1. Decoding

- [x] 1.1 `decode_instance` via `PixelDecoder` with Modality LUT (`to_vec_frame::<f32>`)
- [x] 1.2 Colour path: planar configuration, YBR → RGB; palette colour reported unsupported
- [x] 1.3 DICOM window presets incl. explanations and VOI LUT Function (incl. functional groups)
- [x] 1.4 Optional features `jpeg-ls` (CharLS) and `jpeg-xl` (jxl-oxide)
- [x] 1.5 Unsupported transfer syntax flagged per series in the browser

## 2. Geometry and volumes

- [x] 2.1 `ImagePlane`, `VolumeGeometry::from_sorted_planes` with typed `VolumeError`
- [x] 2.2 Spatial ordering aligned with Instance Number; duplicate-position fallback
- [x] 2.3 Compact `VoxelBuffer` (i16/u16/f32), per-slice storage, trilinear sampling

## 3. Robustness and memory

- [x] 3.1 `catch_unwind` per file; partial-failure warnings
- [x] 3.2 LRU memory budget in `Session`, configurable in Settings
- [x] 3.3 Streamed thumbnails of the representative image

## 4. Verification

- [x] 4.1 Tests: HU values, spatial order, sagittal MR volume, JPEG localizer, RGB capture, eviction

## Follow-ups (moved out of this change)

- 4.2 Build and test with `--features jpeg-ls` in CI (needs CMake on runners) → `jpeg-ls-support`
- 4.3 Real-world sample set covering Enhanced MR/CT multi-frame and JPEG 2000 vendors → `field-validation`
