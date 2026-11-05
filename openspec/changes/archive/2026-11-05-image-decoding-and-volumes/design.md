## Context

dicom-rs 0.10 decodes compressed pixel data and applies rescale (including Enhanced
multi-frame functional groups). It does not decide how frames form a volume.

## Decisions

- **Modality values as `f32` per frame, then compacted.** `to_vec_frame::<f32>` applies
  the Modality LUT; `VoxelBuffer::compact` stores integral data as `i16`/`u16`
  (halving memory for CT/MR), otherwise `f32`. Each slice is a separate buffer so no
  single huge allocation is needed and mixed types are possible.
- **Colour** is converted to interleaved RGB8 ourselves (planar configuration 1,
  YBR_FULL/YBR_FULL_422 → RGB with BT.601 full range). Palette colour is reported as
  unsupported.
- **Ordering.** If all frames share a grid and orientation, sort by position along the
  normal and reverse if that contradicts Instance Number order (so image 1 stays the
  first acquired). Duplicate positions (multi-echo, dynamics) fall back to Instance
  Number order.
- **Volume validation** (`VolumeGeometry::from_sorted_planes`): ≥ 2 slices, same size,
  orientation (within 1e-3) and pixel spacing, no duplicate positions, spacing deviation
  ≤ max(10 % of mean, 0.05 mm). The k-axis is the mean step vector, so gantry tilt is an
  exact shear. Failure reasons are typed (`VolumeError`) and shown to the user.
- **Robustness.** Each file is decoded inside `catch_unwind`; failures are collected as
  warnings; the series fails only if nothing decodes.
- **Memory.** Decoded series live in an LRU with a user-configurable budget (default
  4 GB); the active series is never evicted.

## Risks / Trade-offs

- [JPEG-LS needs CharLS (C++/CMake)] → optional `jpeg-ls` feature; series show the
  transfer syntax name and "not supported by this build" instead of failing silently.
- [Non-uniform slice spacing] → no MPR (reason shown) rather than a distorted volume;
  resampling irregular stacks is a possible future change.
