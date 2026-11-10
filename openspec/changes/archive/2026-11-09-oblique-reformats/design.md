## Context

`summrise_imaging::reslice` resamples a volume on one of three fixed planes
(`MprAxis::basis()`: screen-right, screen-down and viewing normal in patient space).
`ViewState` holds the crosshair (a patient position); every reformat view shows the plane
through it, and the viewport draws one horizontal and one vertical line for the two other
planes. `ViewTransform` holds zoom and pan per view; the viewport maps between screen and
image pixels with an axis-aligned rectangle.

## Goals / Non-Goals

**Goals:**
- Tilt the three planes together about the crosshair, by mouse and by keyboard.
- Turn and flip the display of any view.
- Everything shown stays true: edge letters, titles, read-out, scale bar, crosshair.
- With untilted planes and the standard display orientation nothing changes: the same
  pixels, the same slice positions, the same pointer mapping.

**Non-Goals:**
- Curved reformats, planes tilted independently of each other, saving planes, a 3D view.
- Turning the display by angles other than quarter turns.

## Decisions

### D1 — One rotation for three orthogonal planes, about the crosshair (binding)
`ViewState` holds one rotation (`glam::DQuat`, identity by default). The plane of each
view is that rotation applied to the view's standard plane, so the three planes always
stay mutually orthogonal. Every plane passes through the crosshair, which is therefore
the centre of rotation: tilting never moves it.

A view's in-plane axes are the rotation **without its twist about that view's own
normal**, applied to `MprAxis::basis()`: the shortest rotation that takes the standard
normal to the rotated normal (`PlaneBasis::tilted`). A view therefore never spins about
its own normal.

This deviates from the letter of the brief ("each view's basis is that rotation applied
to `MprAxis::basis()`") in this one point, because the letter contradicts the behaviour
the brief asks for. If the whole rotation is applied to all three bases, they turn
rigidly together, and the direction in which plane B cuts view A, expressed in A's own
axes, is `(n_A × n_B) · right_A`, which a common rotation leaves unchanged. The lines
could then never tilt on screen. Dragging a handle would instead spin the picture of the
view the user is working in under lines that stay horizontal and vertical, and, because
the extent is the bounding box of the projected corners (D2), that picture would also
change size and place under the pointer. The brief says that a drag "turns the two other
planes", that each line has a handle, and that the lines "stay perpendicular on screen":
the view being dragged stays still and its lines turn. Removing the twist gives exactly
that, and keeps everything else of the decision (one rotation, orthogonal planes,
crosshair as the centre, identity equal to today).
`PlaneBasis::rotated` (the whole rotation) exists next to `PlaneBasis::tilted`;
`ViewState::plane_basis` is the only place that chooses between them.

Alternatives considered:
- The whole rotation applied to every basis — rejected for the reasons above.
- Three independent planes — rejected: out of scope, and the lines in a view would no
  longer be perpendicular.
- Euler angles per axis — rejected: the result depends on the order and locks up at 90°.
- One stored basis per view — rejected: three bases can drift apart numerically and
  nothing guarantees that they stay orthogonal.

### D2 — Reslicing on an explicit basis (binding)
`reslice_plane(volume, &PlaneBasis, position, slab, max_dim)` and
`MprExtent::of_plane(geometry, &PlaneBasis)` take the plane orientation explicitly. The
extent is the bounding box of the eight volume corners projected on the basis; slice
positions run along the basis normal, with a step equal to the largest projection of a
voxel axis on that normal (the native slice spacing for the standard planes); slabs
project along the basis normal. `reslice` and `MprExtent::of` keep their signatures and
delegate with `PlaneBasis::standard(axis)`, so there is one implementation. With the
identity rotation `PlaneBasis::tilted` returns exactly the standard basis, and a test
compares the output bit for bit with a frozen copy of the previous implementation.

Alternatives considered:
- A fixed output size centred on the crosshair — rejected: it would change the standard
  views and cut off parts of the volume.
- A second resampler for tilted planes — rejected: two implementations that must agree.

### D3 — Lines are true intersections, with handles (binding)
The line of plane B in view A runs through the crosshair in the direction `n_A × n_B`,
drawn through the same mapping as the picture (D4). The two lines of a view are
perpendicular on screen because the planes are orthogonal. Each line has a round handle
on either side of the crosshair. Dragging a handle turns the two other planes about
view A's normal by the angle the pointer travels around the crosshair, measured in image
coordinates, so the line follows the pointer in every display orientation. Clicking
still moves the crosshair; scrolling moves it along the view's own (tilted) normal by
the slice step of D2. As for the standard planes, slice positions form a grid
(`MprExtent::position`) and scrolling goes to the next position on it: after a click
has put the crosshair between two positions, the first step lands on the grid.
Handles sit on the drawn part of their line, inside the picture, and belong to the
annotations (hidden with them, like the lines). Assistive technology can turn the planes
by one degree per step through a handle (slider increment and decrement).

Alternative considered: drawing horizontal and vertical lines and turning the picture
instead — see D1.

### D4 — Display orientation in the viewport transform (binding)
`ViewTransform` gains a `DisplayOrientation`: a mirror flag (left-right, applied first)
and quarter turns clockwise (0–3). These are the eight possible orientations; turning
and flipping are compositions in that group. The texture is drawn with permuted texture
coordinates; the image is not rendered again. One type (`Placement`) holds the mapping
between screen and image pixels and is used for painting, the value under the pointer,
clicking, the crosshair, the handles, the edge letters and the scale bar, so they cannot
disagree. Turning carries the pan vector along, so the picture turns about the centre of
the viewport; zoom stays relative to "fit", which is recomputed for the turned picture.
Double-click still fits (zoom and pan) and keeps the orientation.

Alternatives considered:
- Rendering turned pixels into the texture — rejected: more work per change and a second
  place where orientation is decided.
- A free rotation angle — rejected: not asked for, and it would need its own truthful
  labelling.
- Applying the orientation in the series data — rejected: it is a property of one view.

### D5 — Safety (binding)
- Edge letters are computed from the patient directions of screen-right and screen-down
  after tilt, turn and flip, never from the view's name.
- "Mirrored" is shown whenever the mirror flag is set, also with annotations hidden.
- A tilted view is titled "OBLIQUE" with its tilt angle and the nearest standard plane.
  The title of a reformat view is always visible. The angle is the angle between the
  view's plane and its own standard plane (0–180°); the nearest plane is the patient
  axis with the largest share of the normal.
- Nothing about tilt or display orientation is written to settings; a new series opens
  untilted and in the standard orientation; reset restores both.
- Tilting, turning and flipping count as user changes: such a view is not "as opened"
  (`ViewState::is_pristine`), so a remembered layout does not take it over.

### D6 — Keys (binding; choices recorded here)
`]` turns the active view right, `[` left; `H` flips horizontally, `V` vertically.
Alt (Option on macOS) with Right or Left tilts the planes of the active reformat view by
1° clockwise or counter-clockwise on screen, 5° with Shift; Alt with R resets the planes.
`R` (existing reset) also restores the planes and the display orientation.

Alt was chosen because Ctrl/⌘ with the arrows is taken by the operating system on macOS
(switching desktops) and because the existing arrow bindings ignore Alt and Shift: the
new bindings are checked first and consume the key.

Alternative considered: Ctrl/⌘ with the arrows — rejected for the reason above.

### D7 — One toolbar menu
Turn, flip and "Reset planes" sit in one toolbar menu ("Rotate and flip"), which stays
open while its entries are used. At the default window size the toolbar has room for one
more button in the reformat layouts, not for five. Turning and flipping act on the active
view, which is the view the pointer was last over; on its way to the toolbar the pointer
may cross another view, so with several views on screen the menu shows which view it acts
on and lets the user pick another. "Reset planes" is enabled whenever the planes are
tilted, as Alt+R is. While a menu covers a view, the pointer reads nothing through it and
does not make that view the active one.

Alternatives considered:
- Five buttons — rejected: they push the right-hand buttons out of the window at 1440
  points.
- Acting on the view under the menu or on all views — rejected: ambiguous, and turning
  every view at once is rarely wanted.

### D8 — The "Mirrored" badge
The badge is drawn by a small helper (`orient::badge`) that places status badges centred
under the top edge letter of the picture, one row each, inside the viewport. A later
change adds a "lossy" mark with the same placement rules, in the next row.

## Risks / Trade-offs

- [A view is seen from an unusual side after large tilts] → the letters are computed
  from the real directions and the title says "OBLIQUE"; reset is one key away.
- [A plane turned by exactly 180° has no shortest rotation] → an arbitrary but valid
  in-plane orientation is used at that single position; labels stay true.
- [Tilted planes are larger than the standard ones] → the extent is the bounding box of
  the projected corners, so the picture is smaller on screen and has empty corners.
- [The crosshair can leave the volume when scrolling a tilted view] → as today for
  volumes that are not aligned with the patient axes; the views then show empty space.
- [Resampling cost while dragging] → the two other views are resampled on every frame
  of a drag, at most 768 pixels per side as before; the view being dragged keeps its
  plane (it is resampled only when rounding changes its basis in the last bits).
- [Alt with the arrow keys in a view that is not a reformat] → keeps its old meaning
  (scrolling): the bindings for the arrows ignore modifiers, as before.
