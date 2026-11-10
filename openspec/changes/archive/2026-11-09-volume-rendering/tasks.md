## 1. Test phantom (`summrise-testkit`)

- [x] 1.1 CT phantom with a contrast-filled vessel of 300 HU (radius 8 mm, along z at L 10 mm, P 25 mm) in front of the bone cylinder of 700 HU; air, soft tissue, lungs, bone and marker unchanged; the existing tests pass unchanged
- [x] 1.2 `CtPhantom`: the voxels of the CT in memory, sharing the slice geometry with the DICOM writer (test: they equal the written and decoded series, slice by slice; the vessel reads 300 HU and the bone 700 HU)

## 2. Renderer (`summrise-imaging`, `render3d`)

- [x] 2.1 Orthographic `Camera` (orientation of the coronal basis, fit to the bounding sphere, pixel mapping), `StandardView` and `turned` (tests: the first view is the coronal basis bit for bit with R, L, S, I; the letters of the six standard views; drag directions; a marker 12 mm to the left appears right of the centre and one towards the head above it, and after half a turn the left marker is on the left with L, R, S, I)
- [x] 2.2 Looks as tabulated transfer functions with the numbers of design D3 (tests on the CT phantom: Bone opaque over the bone, transparent over soft tissue, lung and the vessel; Skin opaque over the body and transparent beside it; Vessels and bone red vessel and ivory bone; Surface opacity follows the window; MIP grey follows the window and the inversion)
- [x] 2.3 MIP equals the largest sample along each ray (test against sampling every ray with `Volume::sample`, marker and CT phantoms, two orientations)
- [x] 2.4 Front-to-back compositing with diffuse shading from a gradient across a cube, refined first hit, early termination, min/max block grid and empty-space skipping (test: skipping changes no bit, early termination no channel by more than one grey level, all looks, three orientations, preview and full)
- [x] 2.5 Parallel rows, determinism and cancellation (tests: identical bytes twice and in a pool of one thread; a gantry-tilted volume shows its marker within one pixel of its position in three orientations; a cancelled render gives no picture; at most 4096 steps per ray)
- [x] 2.6 Sampling the shared slices without copying; `Volume::sample` and the renderer's sampler measured (design D5); tests: the prepared volume holds the same slice buffers, the grid holds the range of every block, the renderer's samples agree with `Volume::sample`
- [x] 2.7 Timing measured in release for the testkit CT at 256 × 256 and 512 × 512 with 300 slices (design D4; `cargo test --release -p summrise-imaging --lib timing -- --ignored --nocapture`)

## 3. View state and pictures (`summrise-core`)

- [x] 3.1 `Layout::Volume` ("3D") and `ViewKind::Volume`; `ViewState` holds the camera orientation and the look, both in `reset` and `is_pristine` (tests: opens from the front with Bone for CT and Surface for MR; turning and choosing a look are changes; turning back is pristine again; reset restores them and keeps the position)
- [x] 3.2 `volume3d`: turning, standard views, the looks offered per units, the style, the camera for a viewport with zoom and pan (tests: letters of the standard views, half a turn is the view from the back, panning moves the marker by the pan in points, zooming about a point keeps it in place)
- [x] 3.3 `refinement::Progressive` (tests without clocks: a preview at once and the full picture after a stable frame; a preview each frame while dragging and no full picture; a stale full picture never replaces a newer preview, also when it arrives before it)
- [x] 3.4 `VolumePictures`: previews on the calling thread, full pictures on a cancellable `Job`, the job in progress always the one wanted; preview size adapting to the time previews take; full size capped at 1024 pixels (tests: preview size rule; cap; with real jobs: a preview at once, the full picture later, a new turn cancels the job, another volume starts afresh)

## 4. User interface (`summrise-ui`)

- [x] 4.1 `volume_view`: the picture over the viewport; primary drag turns whatever the tool, Shift-drag and middle-drag pan, right-drag changes the contrast, pinch and Command/Ctrl-scroll zoom, double-click fits; edge letters from the camera of the picture shown; look, window and zoom annotations; "Refining…" while a preview is shown
- [x] 4.2 Caption "Illustration: colours and surfaces are computed", also with annotations hidden, with an explanation on hover (test)
- [x] 4.3 Toolbar menu with the looks and the standard views (stays open while used); keys: arrows (10°, 45° with Shift), Shift+A/P/L/R/S/I, L; the turn-and-flip menu and keys stand aside in the 3D view; status-bar hint (tests)
- [x] 4.4 Dispatch lines in the viewer, the viewport and the render cache; `series.volume()` called in one place for the 3D view; the pictures of a series dropped when another is displayed
- [x] 4.5 Guide: "3D view" with the camera's viewing direction and no image marked in the figure (tests: from the front and from the back)
- [x] 4.6 The toolbar fits at the default window size (1440 points) in every layout: smaller minimum widths for the window and slab boxes (test: no two toolbar widgets overlap)
- [x] 4.7 Offscreen tests: the layout while loading and for colour images; the first view (letters, marker, guide, caption); dragging with previews, then the full picture; panning and fitting; keys; zoom; MR looks following the window; the menu in both themes

## 5. Verification

- [x] 5.1 Snapshots looked at (`3d-*.png`): CT with each look, the view from the back, annotations hidden, the MR surface with two windows and MIP, the look menu for MR, the menu and a side view in the dark and the light theme, the toolbar in every layout, the tooltip while loading; the striped shading of the first version was fixed (design D3)
- [x] 5.2 `cargo xtask ci` and `openspec validate --all --strict` pass
- [x] 5.3 The tests with threads and background jobs (the 3D view's UI tests, the `refinement` tests, the renderer's thread test) pass 20 times in a row
- 5.4 Moved to `field-validation` 5.9 when this change was archived: Turning with a mouse and a trackpad, the feel of previews and refinement, and the keys tried in the real window on macOS, Windows and Linux (cannot be verified here: the application window cannot be operated from this environment)
- 5.5 Moved to `field-validation` 5.10 when this change was archived: Timing on a slower computer and on a software-rendered Linux virtual machine (cannot be verified here: only this Mac is available; estimated with 2 and 4 threads, design D4)
