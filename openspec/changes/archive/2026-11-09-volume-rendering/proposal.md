## Why

Professional viewers show a CT or MR volume as a 3D object: the skeleton, the outline of
the body, contrast-filled vessels. For a patient this is the most intuitive picture of
what was scanned and where. SumMRIse shows slices and reformats only.

## What Changes

- A "3D" layout next to Images / MPR / 2 × 2. It is available when the series forms a
  volume, under the same condition and with the same explanatory tooltip as the reformat
  layouts, and is remembered like them.
- The volume is drawn as a shaded 3D object that turns when the picture is dragged with
  the mouse or trackpad. Zoom and pan work as in the other views (pinch or Command/Ctrl
  with the wheel, Shift-drag or middle-drag, double-click fits); a right-drag changes the
  contrast. The arrow keys turn the object (10° per press, 45° with Shift).
- Six standard views — from the front, the back, the patient's left and right, above the
  head and below the feet — from a toolbar menu and with Shift and the orientation letter
  (A, P, L, R, S, I). The general reset (R) returns to the first view.
- Looks: for CT (Hounsfield units) "Bone", "Skin" and "Vessels and bone" (for
  contrast-enhanced scans) and "MIP" (maximum intensity projection). For data without
  Hounsfield units (MR) "Surface", whose opacity follows the contrast window, and "MIP";
  changing the contrast window changes these two. L chooses the next look.
- While the object is turned or zoomed a quick, coarser picture is shown at once; the
  sharp picture is computed in the background and replaces it as soon as it is ready.
- Letters at the four edges of the 3D view name the patient directions that point left,
  right, up and down for the current turn, computed like the letters of every other view.
  The first view is from the front with the head up, as the coronal view. The "Where is
  this image?" figure of the inspector says from which side the body is seen.
- A caption marks the 3D view as an illustration whose colours and surfaces are computed.

Patient safety: the edge letters and the viewing direction are computed from the camera
of the picture actually on screen, so they are true for every turn; nothing is mirrored
(every turn is a proper rotation, and the display orientation of the other views does not
apply to the 3D view). The 3D view shows no value read-out, crosshair or scale bar, and
its caption — shown also when annotations are hidden — says that it is a computed
illustration: which structures appear depends on the look's value ranges (documented in
the design), so a structure can be hidden or shown by the choice of look.

Privacy: the 3D view shows no patient name, ID or birth date (it has no patient
annotations at all), nothing derived from the images is written to disk, and nothing about
the 3D view is saved except the layout choice that is saved for every layout.

## Capabilities

### New Capabilities
- `volume-rendering`: the 3D layout; the ray-cast picture of a volume with looks, turning,
  standard views, zoom and pan; quick and full pictures; truthful edge letters, viewing
  direction and caption; the renderer's API in `summrise-imaging`.

### Modified Capabilities
<!-- None: the 3D layout follows the availability rule of the reformat layouts without
changing it; its own rules are in the new capability. -->

## Impact

- `crates/imaging`: new module `render3d` (camera, looks, prepared volume with a min/max
  grid, the ray caster, standard views, turning). No new dependency.
- `crates/core`: `Layout::Volume` and `ViewKind::Volume`; `ViewState` holds the camera
  orientation and the look (reset and `is_pristine` include them); new modules `volume3d`
  (camera, looks and standard views on `ViewState`) and `refinement` (quick and full
  pictures with a background job, latest request wins).
- `crates/ui`: new module `volume_view` (the view, its toolbar menu, keys, overlays and
  caption); dispatch lines in the viewer, viewport and render cache; the guide describes
  the 3D view's viewing direction; the turn-and-flip menu and keys stand aside in 3D; the
  minimum widths of the window and slab boxes are smaller so that the extra layout button
  fits at the default window size.
- `crates/testkit`: the CT phantom gains a contrast-filled vessel (300 HU); its voxels are
  available in memory (`CtPhantom`) for renderer tests and measurements.
- Settings: `default_layout` can now be `Volume`; nothing else is persisted.
