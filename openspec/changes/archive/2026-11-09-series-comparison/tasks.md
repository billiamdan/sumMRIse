## 1. Viewport model (refactor, no behaviour change)

- [x] 1.1 `Workspace` and `Viewport` types in `summrise-core`; Images, MPR, 2 × 2 and 3D expressed as presets of one series
- [x] 1.2 Render cache and session pinning for every series on screen (keyed by series, view kind and tile image; see design D1, D8); every displayed series stays loaded
- [x] 1.3 Existing unit tests and the offscreen UI smoke test pass unchanged

## 2. Multi-series layouts

- [x] 2.1 Layout picker (one, side by side, stacked, 2 × 2: the Compare menu) and active-viewport highlight
- [x] 2.2 Assign series by drag and drop from the browser and by select-then-click; "Drop a series here" placeholder
- [x] 2.3 Toolbar, keyboard and inspector follow the active viewport
- [x] 2.4 Series description and study date always visible in multi-series layouts

## 3. Synchronisation and cross-reference lines

- [x] 3.1 Index the Frame of Reference UID per series (it is: `Series::frame_of_reference`; the test of task 4.1 relies on it)
- [x] 3.2 `summrise-imaging`: nearest slice to a patient position and plane–image intersection segment, with unit tests in millimetres
- [x] 3.3 Automatic link for series in the same frame of reference within 30°; per-viewport link toggle; "outside range" state
- [x] 3.4 Manual "link from here" with a constant offset
- [x] 3.5 Cross-reference lines in every other viewport, hidden with annotations
- [x] 3.6 Linked viewports follow the active viewport's zoom and pan (relative), with a unit test of the factor and the distance

## 4. Compare studies

- [x] 4.1 Detect studies of the same patient; match series by modality, plane and description
- [x] 4.2 "Compare studies" action opening matched series side by side, earlier study on the left

## 5. Tile layout

- [x] 5.1 `tiles` on the viewport (1 × 1 to 5 × 5, never saved); the images of a grid from the current image, within its image set; unit tests with 60 images (31 to 39, 58 to 60 and empty tiles)
- [x] 5.2 Every tile through the single-image path, with its own hover read-out; pictures kept until image, window or colour map change
- [x] 5.3 Scroll by one image, Page Up and Page Down by a grid; double-click opens one tile; `T` steps through the grids; grids with tiles under 96 points not offered
- [x] 5.4 Offscreen UI test of a 3 × 3 grid with its numbers; screenshot reviewed at 1440 and 960 points

## 6. Verification

- [x] 6.1 Testkit: a second axial series with different slice spacing and a second study with a later date
- [x] 6.2 Integration tests: synchronisation at 5 mm against 2.5 mm spacing; cross-reference line of the axial slice at z = 12 mm on the sagittal series at the expected row
- [x] 6.3 Offscreen UI test of the side-by-side layout with two series; screenshots reviewed
- [x] 6.4 Notice shown when the displayed series exceed the memory budget
- [x] 6.5 PET overlay in a multi-series layout: drawn in the active viewport, said in the others, unchanged in the layouts of one series
