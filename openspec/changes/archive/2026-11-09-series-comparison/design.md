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
- Linked contrast windows, a mirrored pointer ("3D cursor"), more than four viewports,
  multi-monitor layouts.
- Tiles of reformats or of the 3D view.

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

As built, zoom, pan and display orientation stay in `ViewState`, per series and view kind,
instead of moving to the viewport: `placement.rs`, `orient.rs`, the crosshair and every
existing test read them there, and the only case this loses is one series shown twice with
the same view kind, which then shows the same picture twice. Moving them to the viewport was
rejected as a large change for that one case. `Viewport { series, view, link, tiles }`,
`Session::active_series` (the series of the active viewport) and `Viewer::active_view` (its
view kind) are what the rest of the application reads. `Viewer::layout` remains the preset,
`Layout::Single` under a multi-series grid; the grid itself is not saved, the preset as
before.

In this change the viewports of a multi-series grid show acquired images only. The Images,
MPR, 2 × 2 and 3D buttons stay as they are, are presets of the active series, and leave the
grid. A reformat or the 3D view next to another series is out of scope; `Viewport::view`
exists so that it can come later.

### D2 — Synchronisation by patient position
For linked viewports in the same frame of reference whose slice normals differ by less
than 30°, the reference point is the centre of the active viewport's current slice; each
follower shows the slice whose plane is nearest to that point along its own normal. Only
header geometry is used.

Alternatives considered: synchronising by image number or by Slice Location — rejected:
wrong when spacing or coverage differ, and Slice Location is optional and unreliable.

As built, the planes are those of the image set shown (`ViewState::stack`), taken from the
frames planned from the headers, so a series follows while it still loads. A follower goes
to the slice when the position of the active viewport's image has changed since the last
frame; merely activating another viewport moves nothing, and neither does changing the image
set of a series. Recomputing it every frame was rejected: with different spacings, clicking
a viewport would shift the others by a slice. A series put into a viewport opens at the
position of the viewport beside it, as soon as its first image is decoded. A series without
a Frame of Reference UID follows nothing by position. "Outside range" means further than half
the average distance between slices beyond the end slice, so the end slice still covers the
half slice that belongs to it. An empty, failed or unlinked viewport drives nothing.

### D3 — Manual link
For series without a common frame of reference the link stores, per follower, the
distance along its normal between its current slice and the active slice at the moment
of linking, and applies equal displacements afterwards. The link indicator reads
"manual".

As built, each viewport of the link stores the position of its slice at that moment and one
common direction (its normal, reversed where it points the other way), so any of them can be
the active one, in either direction of the patient, and nothing creeps: every target follows
from the stored offset, not from the last step. "Link from here" asks for slice directions
within 30° of the active viewport's, and leaves out a viewport whose link the user has
switched off. It is also available within one Frame of Reference, for a patient who moved
between two series (see Risks). Putting another series in a viewport gives it a fresh link.

### D4 — Cross-reference lines
The line is the intersection of the active slice plane with the image shown in another
viewport. It is computed from the signed distances of that image's four corners to the
active plane: the segment joins the two points where the sign changes along the image
edges. Planes within 10° of parallel produce no line.

### D5 — Frame of reference in the index
`Series::frame_of_reference` (added by `pet-support`, `None` when absent) is used. Series
of one examination normally share it; different studies do not, which is why they fall
back to the manual link.

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

### D8 — Tile layout
A viewport with `view` = acquired images has a tile grid (`tiles`: 1 × 1 by default, 2 × 2,
3 × 3, 4 × 4 or 5 × 5; never saved). The first tile (top left) always shows the current
image of the series' view state, the following tiles the next images of the same image
set in reading order, and tiles past the last image stay empty. So the crosshair,
synchronisation, cross-reference lines and cine, which all follow the current image, need
no second notion of position, and there is no selected tile to keep track of. One step
moves the grid by one image, Page Up and Page Down by a whole grid. Every tile goes
through the same per-picture path as a single image (placement, picture, scanner
graphics, padding, measurements, hover read-out), so whatever a picture shows, a tile shows.
Patient and study annotations appear once per viewport; each tile has its image number.
A grid whose tiles would be narrower or lower than 96 points is not offered.

As built, annotations lie on one line each in a strip above and a strip below the tiles (22
or 44 points above, 30 below), so they cover no picture, and the 96 points are measured
in that area. Edge letters (smaller), the lossy mark and hover read-out are per tile; a
click in a tile moves nothing, as there is no selected tile. The scale bar is the first
tile's: the images of a series normally have one scale. A window that gets too small
shows the largest grid that still fits, and the choice comes back when there is room again.
A tile's picture is kept by the index of its image, so one scroll step renders one new
picture and the current image, not all of them. All this also holds for the acquired view
of the 2 × 2 layout.

Alternatives considered:
- A selected tile that moves inside a fixed page, as on some workstations — rejected: a
  second position beside the current image, with no benefit for a patient's overview.
- Stepping by whole grids only — rejected: the user loses the place between grids.

### D10 — The active viewport and what follows it
Clicking, scrolling, zooming or dragging in a viewport makes it the active one; `N` goes to
the next. In the presets the view under the pointer also becomes active, as it always did.
In a multi-series grid it does not: the toolbar and the inspector would change as the
pointer passes, and with them the reference of the links. The status bar still reads what
lies under the pointer, of the series there. With an empty viewport active,
`Session::active_series` is `None`; a series clicked in the browser goes there. So does any
other way of selecting a series (`Session::select_series`), which keeps one path.

Alternative considered: activating on hover everywhere — rejected for the reason above.

### D11 — Assigning series
Series are dropped with egui's payload API, or the keyboard and accessibility path: `N` (or
a click) to the viewport, then Tab and Space or a click on the series in the browser. Dragging
is only on while a multi-series grid is shown, so that in the presets the list scrolls as
before. In a multi-series grid the study picker only lists another study: replacing what the
active viewport shows by that study's default series, as in a preset, would make dragging
from another study impossible.

### D12 — PET in a multi-series grid
`Session` keeps one overlay. Its series is that of the active viewport, so a PET is drawn
there and over no other viewport. A viewport beside it whose series has an overlay set shows
its images alone with the badge "PET overlay only in the active viewport", instead of the old
overlay or nothing said; the setting stays with the series and comes back when the viewport
is activated. The presets are unchanged.

Alternatives considered: keeping an overlay per viewport — rejected: more memory for a rare
case, and a second model beside `Session::set_overlay`. Switching the overlay off in other
viewports silently — rejected: the user would not know why it disappeared.

### D13 — Loading several series
`Session::set_shown` names the series on screen; they are not evicted or paused. While the
active series loads, the others read only the image on screen (`SeriesLoader::set_background`),
so that a viewport shows something and follows, and the medium is not read in two places at
once; when the active series is complete, the others go on. Pausing them altogether was
rejected: a linked viewport would show a spinner while the user scrolls.

### D9 — Linked zoom and pan
Basic Image Review asks for synchronised scroll, pan and zoom. A zoom step of the active
viewport multiplies the zoom of every viewport linked to it by the same factor, and a pan
moves each by the same distance on screen. The link is relative, as the manual link of
D3 is: viewports keep whatever difference they had. Turn, mirror and contrast are not
linked, and a viewport whose link is off is left alone.

Alternative considered: matching millimetres per point and the patient position at the
centre — rejected: it makes viewports jump at the moment of linking, and the manual link
has no common coordinates to match.

As built, `viewport::show` reports what the user did (zoom by a factor about a point, pan by a
distance) and the workspace applies it to the views of the linked viewports, each view once.
A double-click that fits is a zoom and a pan like any other. Toolbar Reset (R) stays with the
active viewport's series. Viewports with an automatic link that follows nothing, such as a
sagittal beside an axial, follow no zoom either.

## Risks / Trade-offs

- [Two series or two dates confused] → series description and study date are always
  visible in multi-series layouts, even with annotations hidden.
- [Synchronisation misleads when the patient moved between series] → it is by scanner
  coordinates, can be switched off, and the manual link is available.
- [Crowded interface in small windows] → at most four viewports, a minimum viewport size,
  and tile grids offered only where their tiles stay at least 96 points wide and high.
- [Many textures at 5 × 5] → a tile's picture is rendered once and kept until its image,
  window or colour map changes; memory is bounded by 25 pictures per viewport.
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
