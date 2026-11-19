## 1. Planes on an explicit basis (`summrise-imaging`)

- [x] 1.1 `PlaneBasis` (standard, whole rotation, tilted without twist, line of intersection, tilt angle) and `AnatomicalPlane::nearest`; unit tests: the identity gives the standard basis exactly, tilted planes stay orthonormal and mutually orthogonal, a view does not spin about its own normal
- [x] 1.2 `MprExtent::of_plane` and `reslice_plane`; `MprExtent::of` and `reslice` delegate to them; the existing tests are untouched and pass
- [x] 1.3 Test: for all three axes the output with the identity rotation is bit-identical to a frozen copy of the previous implementation (thin slice and three slab modes; axial, sagittal, gantry-tilted and anisotropic volumes; two output sizes)
- [x] 1.4 Tests with the marker phantom (sphere at L6 A6 S6): where it appears after 45° about the superior axis and after a double-oblique rotation; pixel↔patient round trip on oblique images; slab MIP along a rotated normal; a gantry-tilted (sheared) volume

## 2. View state (`summrise-core`)

- [x] 2.1 `ViewState` holds the plane rotation: `plane_basis`, `tilt_planes`, `reset_planes`, `plane_tilt`, `plane_line`; slice count, index, position and scrolling follow the tilted normal; reset and `is_pristine` include it
- [x] 2.2 Tests: tilting keeps the crosshair fixed and on all three planes; the view tilted in keeps its basis; reset restores the identity; scrolling a tilted view moves the crosshair along the tilted normal by the slice step
- [x] 2.3 `DisplayOrientation` (mirror and quarter turns) in `ViewTransform` with turn, flip and fit; tests over the eight orientations (composition, mapping round trip, screen axes, pan carried along); turning and flipping count in `is_pristine` and are undone by reset

## 3. Viewport and toolbar (`summrise-ui`)

- [x] 3.1 `Placement`: the picture drawn turned and mirrored; pointer mapping, zoom anchor and scale in all eight orientations (test: screen↔pixel round trip with non-square pixels)
- [x] 3.2 Edge letters and scale bar follow the display orientation (test: the letters of an axial image in each orientation)
- [x] 3.3 "Mirrored" badge, also with annotations hidden (test: visible exactly when mirrored)
- [x] 3.4 Plane lines as true intersections, with handles; dragging a handle tilts the two other planes (tests: a drag in the axial view, and in a mirrored and turned view)
- [x] 3.5 Titles, annotations and accessible descriptions of tilted views (test: "Oblique 30.0° · near Coronal")
- [x] 3.6 Toolbar menu "Rotate and flip" with a view chooser and "Reset planes"; keys `[`, `]`, `H`, `V`, Alt+Left/Right (Shift: 5°), Alt+R; the existing reset restores planes and orientation (tests)
- [x] 3.7 A view turned or flipped while its series loads is not taken over by a remembered reformat layout (test with a gated image source)

## 4. Verification

- [x] 4.1 Offscreen snapshots looked at: tilted and double-oblique MPR layout, rotated view, flipped view with badge, annotations hidden, the menu in the dark and the light theme
- [x] 4.2 `cargo xtask ci` and `openspec validate --all --strict` pass
- [x] 4.3 The new UI tests, which wait for the background loader, pass 20 times in a row
- 4.4 Moved to `field-validation` 5.5 when this change was archived: Handles, the feel of dragging and the key bindings tried in the real window on macOS, Windows and Linux (cannot be verified here: the application window cannot be operated from this environment)
