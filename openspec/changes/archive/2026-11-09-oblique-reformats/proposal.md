## Why

Reformats exist only on the three patient axes, but anatomy is rarely aligned with them:
a spine, a knee or a tilted head is cut at an angle by every standard plane. Professional
viewers let the user tilt the planes. Images also cannot be turned or flipped on screen,
which matters for images that were acquired sideways or upside down.

## What Changes

- In the reformat layouts every view shows the lines of the two other planes through the
  crosshair, and each line carries round handles. Dragging a handle turns the two other
  planes about the crosshair; the view in which the user drags stays as it is. Tilting in
  two views gives a double-oblique plane.
- A tilted view says so: its title reads "OBLIQUE", gives the tilt angle and the standard
  plane it is closest to, and the letters at its edges are computed from the directions
  really shown (compound letters such as `LA` or `RPI` appear).
- "Reset planes" (and the existing reset) returns to axial, coronal and sagittal.
- Any view (acquired images and reformats) can be turned left or right in 90° steps and
  flipped horizontally or vertically, from a toolbar menu and from the keyboard. The edge
  letters move with the picture. While a view is mirrored a "Mirrored" badge stays on the
  picture, also when annotations are hidden.
- New keys: `[` and `]` turn, `H` and `V` flip, Alt (Option on macOS) with Left/Right
  tilts the planes by 1° (5° with Shift), Alt with R resets the planes.

Patient safety: the letters at the edges always describe what is on screen, whatever the
tilt, turn or flip; a mirrored picture is never shown without the "Mirrored" badge; a
tilted view is never titled as a standard plane; clicking, the value read-out, the scale
bar and the crosshair stay geometrically exact in every orientation, including images
whose pixels are not square. Tilt, turn and flip belong to one view of one series, are
never saved and are undone by reset. Tilted planes are resampled from the acquired
voxels by interpolation, like the standard reformats.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `image-viewing`: adds tilted (oblique and double-oblique) reformat planes with plane lines and handles, truthful titles and edge letters for tilted views, turning and flipping the display of any view with a "Mirrored" badge, and the keys for all of it.

## Impact

- `crates/imaging`: reslicing and plane extents take an explicit plane orientation; the
  three standard planes give bit-identical results.
- `crates/core`: `ViewState` holds one rotation for the three planes; `ViewTransform`
  holds the display orientation (quarter turns and mirror).
- `crates/ui`: the viewport draws the picture turned or mirrored and maps the pointer
  accordingly; plane lines with handles; orientation menu in the toolbar; new keys.
- No new dependency, no file-format or settings changes; nothing is persisted.
