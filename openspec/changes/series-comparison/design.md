## Context

The viewer (`crates/ui/src/viewer.rs`) is built around one active series:
`Session::active_series`, a `ViewState` per series, and a `Layout` (Images, MPR, 2 × 2)
that maps to view kinds of that one series. The render cache is keyed by
`(SeriesKey, ViewKind)` and pruned to the active series, and the memory budget protects
only the active series from eviction. The index stores per-frame image planes but not
the Frame of Reference UID.

## Goals / Non-Goals

**Goals:**
- Show any two to four series next to each other.
- Scroll them together by anatomical position and show where a slice lies on other views.
- Compare two studies of the same patient from one disc.
- No regression of the single-series reformat layouts.

**Non-Goals:**
- Automatic image registration between studies.
- Hanging protocols (rules that choose layouts automatically).
- Fused overlays such as PET on CT.
- Linked contrast windows, a mirrored pointer ("3D cursor"), more than four viewports,
  multi-monitor layouts.

## Decisions

### D1 — Workspace and viewport model
`Workspace { grid, viewports, active }` in `summrise-core`, with
`Viewport { series: Option<SeriesKey>, view: ViewKind, link }`. State that belongs to a
series (contrast window, slice and crosshair position, slab) stays in the per-series
`ViewState` and is shared by all viewports of that series; zoom and pan are per viewport.
The current Images, MPR and 2 × 2 layouts become presets that fill the grid with
viewports of one series.

Alternatives considered:
- A separate "compare mode" beside the existing viewer — rejected: two code paths for
  viewports, overlays and (later) measurements.
- Independent state per viewport even for the same series — rejected: the reformat
  crosshair relies on shared state.

### D2 — Synchronisation by patient position
For linked viewports in the same frame of reference whose slice normals differ by less
than 30°, the reference point is the centre of the active viewport's current slice; each
follower shows the slice whose plane is nearest to that point along its own normal. Only
header geometry is used.

Alternatives considered: synchronising by image number or by Slice Location — rejected:
wrong when spacing or coverage differ, and Slice Location is optional and unreliable.

### D3 — Manual link
For series without a common frame of reference the link stores, per follower, the
distance along its normal between its current slice and the active slice at the moment
of linking, and applies equal displacements afterwards. The link indicator reads
"manual".

### D4 — Cross-reference lines
The line is the intersection of the active slice plane with the image shown in another
viewport. It is computed from the signed distances of that image's four corners to the
active plane: the segment joins the two points where the sign changes along the image
edges. Planes within 10° of parallel produce no line.

### D5 — Frame of reference in the index
`Series` gains `frame_of_reference_uid: Option<String>` (None when instances disagree).
Series of one examination normally share it; different studies do not, which is why they
fall back to the manual link.

### D6 — Matching series between studies
Studies belong to the same patient when the Patient ID matches (or name and birth date
when the ID is missing). Pairs are chosen by modality, anatomical plane and series
description (exact match first, then word overlap); the user can replace either side.

Alternative considered: automatic rigid registration — rejected: heavy, error-prone and
out of scope for a non-diagnostic viewer.

### D7 — Memory
Every displayed series is pinned and never evicted. When the pinned series exceed the
budget, a notice names the memory setting. Each displayed series loads independently;
with `progressive-loading`, the active viewport's series gets priority.

## Risks / Trade-offs

- [Two series or two dates confused] → series description and study date are always
  visible in multi-series layouts, even with annotations hidden.
- [Synchronisation misleads when the patient moved between series] → it is by scanner
  coordinates, can be switched off, and the manual link is available.
- [Crowded interface in small windows] → at most four viewports and a minimum viewport
  size.
- [Regressions from the refactor] → phase 1 is a pure refactor guarded by the existing
  view-state tests and the offscreen UI test.

## Migration Plan

1. Viewport model with unchanged behaviour (presets for Images, MPR, 2 × 2).
2. Multi-series layouts and the active viewport.
3. Synchronisation and cross-reference lines.
4. Compare studies.

Each phase is shippable on its own.

## Open Questions

- Should contrast windows be linkable between viewports of the same modality?
- Is a mirrored pointer worth adding once measurements exist?
