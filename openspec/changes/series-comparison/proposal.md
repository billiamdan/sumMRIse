## Why

Reading tomography is comparative: T1 next to T2, before and after contrast, an axial
series next to a sagittal one, today's examination next to an earlier one. Every
professional viewer shows several series at once with synchronised scrolling and
cross-reference lines. SumMRIse shows one series at a time, so a user cannot see the same
anatomy in two series or tell where a slice sits on another view.

## What Changes

- Layouts with one, two (side by side or stacked) and four viewports. Each viewport
  shows a series the user chooses by dragging it from the series browser, or by selecting
  the viewport and then the series. One viewport is active; the toolbar, the keyboard and
  the inspector follow it.
- Synchronised scrolling: viewports whose series share a frame of reference and a
  similar slice direction move together by anatomical position. The link can be switched
  off per viewport.
- Manual synchronisation for series without a common frame of reference (for example two
  studies): "link from here" keeps the current offset.
- Cross-reference lines: every viewport shows where the active viewport's current slice
  cuts its own image.
- "Compare studies": when the media hold more than one study of the same patient,
  matching series of two studies open side by side, the earlier one on the left.
- Tile layout: a viewport can show consecutive images of a series at once, as a grid of
  2 × 2 to 5 × 5 tiles, each with its image number.
- Linked viewports also follow each other's zoom and pan.
- The existing single-series reformat layouts (MPR, 2 × 2) stay and are expressed in the
  same viewport model.

Patient safety: synchronisation uses patient coordinates, never image numbers, and every
viewport always shows its series description and study date so that two series or two
dates cannot be confused.

## Capabilities

### New Capabilities
- `series-comparison`: multi-series layouts, synchronised scrolling, cross-reference lines and study comparison.

### Modified Capabilities
<!-- None: the existing single-series viewing requirements keep their behaviour. -->

## Impact

- `crates/core`: a workspace/viewport model replaces the single active series of the
  viewer; `Session` keeps every displayed series loaded.
- `crates/imaging`: slice matching by position and plane-intersection maths.
- `crates/dicom`: the Frame of Reference UID per series is already indexed
  (`Series::frame_of_reference`, added by `pet-support`).
- `crates/ui`: viewer, viewport, series browser (drag source), toolbar (layout picker,
  link toggle).
- Memory: two or more decoded series are held at once; the existing budget applies.
- This change introduces the viewport model that `measurement-tools` builds on, so it
  should be implemented first.
