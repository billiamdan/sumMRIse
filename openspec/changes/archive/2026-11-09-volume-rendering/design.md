## Context

A complete series that forms a regular volume is available as `summrise_imaging::Volume`
(geometry plus one reference-counted `VoxelBuffer` per slice, shared with the loaded
images). Reformats resample it on a plane (`reslice_plane`) and the viewer draws the
result through `Placement`, with edge letters from the patient directions of
screen-right and screen-down. `ViewState` holds everything the user can set per series;
`ViewTransform` holds zoom, pan and display orientation per view. The inspector's guide
describes the active view with `guide::viewing_direction(right, down)`. Background work
runs on `Job`s that wake the UI when done.

## Goals / Non-Goals

**Goals:**
- A 3D picture of the volume that a patient can turn, zoom and pan, in a few looks.
- Everything shown is true for the picture on screen: edge letters, viewing direction,
  look and contrast; nothing is mirrored; the picture is marked as an illustration.
- The renderer is pure, deterministic and tested with concrete values; the UI thread
  never waits for a full picture.

**Non-Goals:**
- Cut planes and cropping, removing the scanner table, surface extraction (meshes),
  measuring in 3D, saving pictures, GPU rendering, perspective projection.
- Transfer-function editing by the user; segmentation; colour (RGB) volumes.

## Decisions

### D1 — CPU ray casting in `summrise-imaging` (binding)
`crates/imaging/src/render3d.rs` holds the whole renderer: no UI and no GPU code, no
new dependency (glam and rayon are already used by the crate). It runs in parallel over
the rows of the picture with rayon; every pixel is computed on its own with the same
arithmetic, so the picture does not depend on the number of threads (a test renders with
one thread and with all of them and compares the bytes).

The API is small so that another backend can replace it:
`render(&RenderVolume, &Camera, &Style, [width, height], Quality, &cancelled) ->
Option<Rendering>` (premultiplied RGBA; `None` only when cancelled), plus
`RenderVolume::new(&Volume)` (prepares a volume once) and `Camera::fit`.

Why not a GPU ray caster (wgpu, the volume as a 3D texture) now:
- Results must be identical and testable with concrete values on every platform,
  including software-rendered Linux (llvmpipe), virtual machines and old graphics
  hardware. On the CPU the arithmetic is IEEE single precision on every target (glam's
  SIMD paths for the operations used here — component-wise arithmetic, dot products and
  matrix–vector products — add and multiply without fused operations on SSE2 and NEON
  alike); the transcendental functions come from the platform's maths library, as for
  the sigmoid window, and are only used to build the opacity table. GPU results depend on
  the driver (texture filtering precision, fused operations, denormals).
- No dependence on device limits and texture-format features: wgpu's downlevel limits
  allow 3D textures of only 256 texels per side, while a CT is 512 × 512 × several
  hundred; filterable 16-bit normalised formats need an optional feature; and a
  512 × 512 × 600 CT is 300 MB of texture memory, more than integrated graphics in a
  virtual machine often has.
- The imaging layer stays free of UI and GPU code, and the renderer's tests run without
  any GPU.
- Measured below, the CPU is fast enough on this machine for previews well within a
  frame and for full pictures within a tenth of a second.

Alternatives considered:
- wgpu compute or fragment shader — rejected for now for the reasons above; the API above
  is the seam where it can be added later, with these tests as the reference.
- Shear-warp or splatting — rejected: approximations whose artefacts are harder to
  reason about, and no faster than ray casting with empty-space skipping at these sizes.
- Extracting a surface mesh (marching cubes) — out of scope, and it fixes one threshold.

### D2 — Orthographic camera (binding)
`Camera { orientation: DQuat, center: DVec3, field_mm }`. The orientation is applied to
the coronal basis `MprAxis::Coronal.basis()` (`PlaneBasis::rotated`): screen-right is the
patient's left (+X), screen-down the feet (−Z), and the view runs from the front to the
back (+Y). The identity gives that basis bit for bit, so the first view is the coronal
view: the patient's right on the left of the screen, the head up. Rays are parallel to
the viewing direction; pixel (x, y) of a picture of w × h pixels lies at
`center + right·(x + ½ − w/2)·s + down·(y + ½ − h/2)·s` with `s = field_mm / min(w, h)`.
Previews and full pictures of the same request therefore frame the same part of space.

Fit (`Camera::fit`) centres the camera on the volume and makes `field_mm` the diameter of
the sphere around the corners of the box the voxels fill (half a voxel beyond the outer
voxel centres), so the object never leaves the picture however it is turned and its size
on screen does not change while turning. For the synthetic chest CT (240 × 240 × 200 mm)
that is 394 mm. The view's zoom and pan (`ViewTransform`) act as in the other views:
`field = fit / zoom`, and a pan of (dx, dy) points moves the picture by that much on
screen (the camera centre moves by `−(right·dx + down·dy)` in mm per point), so zooming
about the pointer keeps the point under it (`ViewState::volume_camera`, tested).

Alternatives considered: fitting the projected box of the first view (fills the screen
better from the front, but parts leave the picture when turned and the size changes
while turning) — rejected; perspective projection — out of scope.

### D3 — Looks: transfer functions, compositing, shading, shortcuts (binding)
A look is a transfer function: opacity per millimetre and colour, piecewise linear over
value. It is tabulated in 4096 entries (−1024 to 3071 HU for the fixed looks; the
volume's value range for the looks that follow the window) and interpolated linearly.
The opacity of one sampling step of `d` mm is `1 − (1 − a)^d`, so previews (larger steps)
and full pictures look alike. Samples are trilinear and lie on planes perpendicular to
the view, one step apart (`Quality::Full`: half the smallest voxel spacing,
`Quality::Preview`: one spacing), but never more than 4096 steps across the volume's
bounding sphere: normal data needs fewer (about 1700 for the 512 × 512 × 300 CT), while
a malformed or extremely anisotropic geometry cannot make a ray, and the interface's
preview, take arbitrarily long. Compositing is front to back with premultiplied colour;
the background stays transparent and is drawn black.

Preset numbers (values in HU; opacity per mm; colour in linear RGB 0–1). Below the first
point a look keeps the first point's values, above the last point the last point's.

| Look | Points (value: opacity, colour) |
|---|---|
| Bone | 300: 0, (0.80, 0.70, 0.56) · 500: 0.9, ivory (0.93, 0.89, 0.80) · 1500: 0.95, (1.0, 0.97, 0.92) |
| Skin | −600: 0, (0.80, 0.56, 0.46) · −300: 0.9, (0.93, 0.75, 0.63) |
| Vessels and bone | 150: 0, (0.55, 0.05, 0.05) · 200: 0.25, (0.80, 0.12, 0.10) · 350: 0.3, (0.88, 0.24, 0.18) · 450: 0.9, ivory · 1500: 0.95, (1.0, 0.97, 0.92) |
| Surface | opacity = the contrast window's VOI function of the value (0 below the window, 1 above it), colour (0.90, 0.86, 0.78) |
| MIP | the largest value along the ray through the contrast window as grey; inverted with the inversion setting |

Reasons: Bone starts at 300 HU so that soft tissue (around 40 HU) and contrast-filled
vessels up to about 300 HU stay out; strongly enhanced arteries (above 300 HU) can appear,
which is unavoidable by value alone and is what "Vessels and bone" is for. Skin turns
opaque between air (−1000 HU) and fat (−100 HU), at −600 to −300 HU, so lungs inside are
hidden by the body surface. In "Vessels and bone" vessels are kept fairly transparent per
millimetre (0.25–0.3): the edge of every bone passes through the vessel range by partial
volume and would otherwise look red; bone takes over from 450 HU. CT looks ignore the
window and the inversion; Surface and MIP follow the window (the user's contrast
gestures, presets 1–9 and A change them).

Shading is diffuse with the light at the viewer: `0.25 + 0.75 · |n · v|`, lit from both
sides, where `n` is the normalised gradient turned into patient space (`J⁻ᵀ g` with the
voxel axes `J`, so anisotropic and sheared voxels give correct normals). The gradient is
taken across a cube of ±1.5 voxels from its eight corners (eight samples, as cheap as six
central differences) rather than along three lines: on the binary phantom surfaces the
three-line differences drew the voxel staircase as ridges (measured: dips of 45 grey
levels across a cylinder; with the cube ±10).

Where a ray first meets something the look shows, the step is refined: values between
two samples are taken as linear, only the part of the step beyond the look's onset value
counts, and that part is lit at the surface point. Without this the depth of a surface is
rounded to whole steps, which draws bands across smooth surfaces.

Early ray termination stops a ray once less than 1/512 of what lies behind can show; the
rest changes a channel by less than half a grey level. MIP stops once its value is above
the window (white either way).

Empty-space skipping uses a grid built once per volume: the smallest and largest value of
each block of 8 × 8 × 8 voxels, blocks overlapping by one voxel (block `b` covers voxels
`8b … 8b + 8`, all the voxels a trilinear sample between `8b` and `8b + 8` uses). A block
in which the look's table is zero over its whole range (one table entry spare on each
side) is skipped; for MIP a block whose largest value is not above the ray's maximum so
far. Skipping jumps to the last sample that is at least 0.001 voxel inside the block, on
the same sample planes, so every sample that is evaluated is evaluated exactly as without
skipping and every skipped sample contributes nothing: the picture is bit-identical
(tested on every look and orientation). The grid of the 512 × 512 × 300 CT takes 2.7 ms
and 1.2 MB.

### D4 — Quick and full pictures, latest wins (binding)
`summrise_core::refinement::Progressive` decides frame by frame, without threads or
clocks (so its tests do not depend on time):
- a changed request (camera, style or size) gets a preview at once; any full picture in
  progress is no longer wanted;
- a request that stays the same for a frame gets the full picture, once;
- a delivered picture is shown only if it was asked for the current request and is newer
  than the one shown; a full picture only if it is the one in progress. A stale full
  picture therefore never replaces a newer preview.

`VolumePictures` runs it: previews on the UI thread (small, see below), full pictures on a
`Job` whose cancellation flag the renderer checks per row; the job in progress is always
the one still wanted, any other is cancelled. The UI thread never waits for a full
picture. While a preview is shown and its full picture not yet started, the view asks for
one more frame; the job's waker asks for the frame that shows the full picture.

The full picture has the viewport's size in physical pixels, its longest side capped at
1024 (larger viewports show it magnified, smoothly). Previews start at 256 pixels on the
longest side and adapt to this computer (`PreviewSize`): halved when one took more than
24 ms, four fifths when more than 12 ms, five fourths when less than 6 ms, between 96
and 384. The adaptation only changes the resolution of previews.

Alternatives considered:
- Rendering previews on a background job too — rejected: the picture on screen would lag
  a frame behind the camera and the letters would have to wait for it; previews are cheap.
- Time-based debouncing of the full picture — rejected: frame-based is deterministic and
  testable; a moving pointer cancels the job within one row of work.
- Progressive passes inside one job — rejected: more state for little gain at these
  timings.

Measured on this machine (Apple M5 Pro, 18 rayon threads, release build), testkit CT in
memory, 300 slices, median of 7 (previews) or 3 (full) runs, front view / turned by
(30°, −20°):

| CT in-plane | Look | Preview 256 px | Full 1024 px |
|---|---|---|---|
| 256 × 256 (39 MB) | Bone | 2.1 / 2.8 ms | 25 / 41 ms |
| 256 × 256 | Skin | 1.2 / 1.7 ms | 19 / 27 ms |
| 256 × 256 | Vessels and bone | 1.9 / 2.9 ms | 26 / 43 ms |
| 256 × 256 | MIP | 2.3 / 3.4 ms | 47 / 61 ms |
| 512 × 512 (157 MB) | Bone | 3.4 / 4.7 ms | 50 / 69 ms |
| 512 × 512 | Skin | 2.0 / 2.7 ms | 35 / 42 ms |
| 512 × 512 | Vessels and bone | 3.5 / 4.8 ms | 54 / 73 ms |
| 512 × 512 | MIP | 3.6 / 6.6 ms | 76 / 97 ms |

Limited to 4 and to 2 rayon threads (the 512 × 512 CT, front view, Bone and MIP):
previews of 256 pixels 12.5–13.3 ms and 24.5–26.0 ms, full pictures 222–303 ms and
438–582 ms; the preview size adapts downwards there. Without empty-space skipping and
early termination a full picture of 512 pixels of the 512 × 512 CT takes 37.9 ms (Bone)
and 23.7 ms (MIP) instead of 13.2 and 19.5 ms. The measurement is the ignored test
`render3d::tests::timing`
(`cargo test --release -p summrise-imaging --lib timing -- --ignored --nocapture`).

### D5 — Sampling without copying (binding)
`RenderVolume::new` keeps a clone of the `Volume`, which shares the slices
(`Arc<VoxelBuffer>`; a test checks the pointers) — no voxel is copied; it adds only the
min/max grid. The renderer samples with its own trilinear interpolation over the shared
slices: the interpolation of `Volume::sample`, in single precision, without `Option`, and
clamped also beyond the half-voxel border (the gradient reaches a voxel and a half
outside). Measured (full 512 px, front view): through `Volume::sample` 13.1 ms (Bone) and
23.1 ms (MIP) for the 512 × 512 CT, with the renderer's sampler 12.9 and 19.4 ms; for the
256 × 256 CT 6.4 / 14.7 ms against 6.3 / 12.3 ms. A first version also had paths
specialised per sample type (all-`i16`, all-`u16`, all-`f32` slices); measured, they were
no faster than reading through `VoxelBuffer::get` (12.8 against 12.8 ms Bone, 19.5 against
19.4 ms MIP), so they were removed. A test compares the renderer's samples with
`Volume::sample` (within 0.01).

### D6 — View model (binding)
`ViewKind::Volume` and `Layout::Volume` ("3D", one view, `needs_volume`). `ViewState`
gains `camera: DQuat` (identity first) and `look: Look` (Bone for Hounsfield data, else
Surface); `is_pristine` and `reset` include both, so turning or choosing a look is a
user change (a remembered layout does not take over such a view) and R returns to the
first view and look. A turn that comes back to within 1e-7 of the identity (sine of half
the angle) is the identity again, as for tilted planes. The 3D view has no slices
(`slice_count` 0). Its `ViewTransform` holds zoom and pan; its display orientation is
never changed: the turn-and-flip menu is not shown and its keys do nothing in the 3D view.

In the 3D view a primary-button drag always turns the object, whatever the tool; Shift
with the primary button and the middle button pan; the secondary button changes the
contrast (it matters to Surface and MIP); pinch and Command/Ctrl-scroll zoom about the
pointer; double-click fits. Plain scrolling does nothing (there are no slices; turning
by scrolling would turn the object by accident). There is no value read-out, crosshair or
scale bar.

### D7 — UI module and one call of `series.volume()` (binding)
`crates/ui/src/volume_view.rs` holds the view (interaction, the picture's texture,
overlays, caption), its toolbar control, its keys and its status-bar hint, and keeps the
`VolumePictures` of the displayed series. The viewer, viewport and render cache only
dispatch: `viewer.rs` calls `volume_view` for the `Volume` kind and passes
`series.volume()` — the only call for the 3D view — so that a later change can pass "the
volume of the current stack" instead. The view lets go of the pictures (and the shared
volume) when another series is displayed.

### D8 — Turning (decided here)
A drag turns the object about an axis in the screen plane, perpendicular to the drag, so
that its near side follows the pointer (0.5° per point); a drag to the right brings the
side that is on the left of the picture to the front. The camera turns the opposite way
about the centre of the volume (`render3d::turned`). The arrow keys do the same as a drag
in their direction: 10° per press, 45° with Shift.

Alternative considered: a turntable (horizontal drags about the patient's long axis) —
rejected: it needs a special case at the poles (looking from the head or the feet) and
does not match the arrow keys as simply; roll that accumulates with a free trackball is
undone by the standard views and by R.

### D9 — Standard views and their keys (decided here)
Rotations of the coronal basis, with the letters (left, right, top, bottom) they give:

| View | Shortcut | Screen-right, screen-down | Letters |
|---|---|---|---|
| From the front | Shift+A | +X, −Z (as coronal) | R, L, S, I |
| From the back | Shift+P | −X, −Z | L, R, S, I |
| From the patient's left | Shift+L | +Y, −Z (as sagittal) | A, P, S, I |
| From the patient's right | Shift+R | −Y, −Z | P, A, S, I |
| From above the head | Shift+S | −X, +Y (front up) | L, R, A, P |
| From below the feet | Shift+I | +X, +Y (as axial) | R, L, A, P |

Shift with the orientation letter of the side the view looks from: the same letters the
edges show. A, R and I alone keep their meaning (Auto window, reset, invert): the viewer's
bindings ignore Shift, so the 3D view's keys are checked first and consume these
combinations, as the tilt keys do. L alone chooses the next look (free in every view).

Alternatives considered: Ctrl/⌘ with digits (taken by the operating system on some
platforms, not mnemonic); plain letters (A, R, I and the digits are taken).

### D10 — Truthful labelling (binding outcome; details decided here)
- Edge letters are `EdgeLabels::new(right, down)` of the camera of the picture on screen
  (stored with the picture), exactly as for the other views; they are annotations
  (hidden with O). Every turn is a proper rotation: the 3D view is never mirrored.
- The inspector's "Where is this image?" uses `viewing_direction(right, down)` of that
  camera for its sentences, titles the section "3D view" (a 3D picture is not an image
  in a plane, so "Coronal image" would be wrong), and marks no image in the figure.
- A caption under the top letter — "Illustration: colours and surfaces are computed" —
  stays when annotations are hidden; its tooltip explains that colours are not those of
  the body, that what appears depends on the look and the contrast, and that it is no
  basis for diagnosis.
- The bottom-left annotation names the look of the picture on screen ("3D view: Bone")
  and says "Refining…" while a preview is shown; the bottom-right one gives the window
  (for the looks that follow it) and the zoom. The 3D view shows no patient details.

### D11 — Toolbar (decided here)
One menu button names the current look and holds the looks and the six standard views
(with their shortcuts) and a hint; it stays open while it is used. At the default window
size (1440 points) the "3D" layout button (45 points with its spacing) pushed the slab box
of the reformat layouts 10 points under the "Open" button; lowering the minimum widths of
the window box (130 → 112 points) and the slab box (96 → 80) leaves 24 points of room.
Both boxes still grow to fit longer names ("Mediastinum" fits in 112). A test checks that
no two toolbar widgets overlap at 1440 points in any layout. (With a slab mode chosen,
the slab slider did not fit at 1440 points before this change either.)

Alternative considered: a "Look" combo box and a separate views button — rejected: about
60 points wider; they did not fit.

### D12 — Test phantom (decided here)
The testkit CT gains a contrast-filled vessel (300 HU, radius 8 mm, along z at L 10 mm,
P 25 mm, in front of the spine-like bone cylinder of 700 HU). Air (−1000 HU), soft tissue
(40 HU), the lungs (about −820 HU), the bone and the marker stay as they were (the bone's
test is the same expression); the existing tests that read phantom values pass unchanged. `CtPhantom` gives the same voxels in memory (sharing the slice
geometry with the DICOM writer), for renderer tests and measurements at 512 × 512 × 300
without writing files.

## Risks / Trade-offs

- [A look hides or invents structures] → looks are documented value ranges, the caption
  says the picture is computed, and MIP shows the values through the usual window.
- [Partial volume colours edges] → vessel opacity kept low; the first hit is refined.
- [Staircases on coarse or binary data] → gradients across a cube; coarse data still
  looks blocky (the MR phantom has 10.9 mm between its sagittal slices), which is true to
  the data.
- [Slow computers] → previews adapt their size; full pictures are cancelled when no
  longer wanted; measured with 2 threads: 0.4–0.6 s for a full 1024-pixel picture of a
  512 × 512 × 300 CT.
- [Memory] → nothing is copied; the grid is 1.2 MB for 157 MB of voxels; the pictures of
  a series are dropped when another series is displayed.
- [A preview of the previous request stays on screen if a preview cannot be rendered] →
  its letters are those of its own camera, so they stay true; the next frame asks for the
  full picture of the current request.
- [Keys of later changes] → Shift+A/P/L/R/S/I, L and the arrows are taken in the 3D view.
