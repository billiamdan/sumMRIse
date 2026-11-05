## 1. Viewport model (refactor, no behaviour change)

- [ ] 1.1 `Workspace` and `Viewport` types in `summrise-core`; Images, MPR and 2 × 2 expressed as presets of one series
- [ ] 1.2 Render cache and session pinning keyed by viewport; every displayed series stays loaded
- [ ] 1.3 Existing unit tests and the offscreen UI smoke test pass unchanged

## 2. Multi-series layouts

- [ ] 2.1 Layout picker (one, side by side, stacked, 2 × 2) and active-viewport highlight
- [ ] 2.2 Assign series by drag and drop from the browser and by select-then-click; "Drop a series here" placeholder
- [ ] 2.3 Toolbar, keyboard and inspector follow the active viewport
- [ ] 2.4 Series description and study date always visible in multi-series layouts

## 3. Synchronisation and cross-reference lines

- [ ] 3.1 Index the Frame of Reference UID per series
- [ ] 3.2 `summrise-imaging`: nearest slice to a patient position and plane–image intersection segment, with unit tests in millimetres
- [ ] 3.3 Automatic link for series in the same frame of reference within 30°; per-viewport link toggle; "outside range" state
- [ ] 3.4 Manual "link from here" with a constant offset
- [ ] 3.5 Cross-reference lines in every other viewport, hidden with annotations

## 4. Compare studies

- [ ] 4.1 Detect studies of the same patient; match series by modality, plane and description
- [ ] 4.2 "Compare studies" action opening matched series side by side, earlier study on the left

## 5. Verification

- [ ] 5.1 Testkit: a second axial series with different slice spacing and a second study with a later date
- [ ] 5.2 Integration tests: synchronisation at 5 mm against 2.5 mm spacing; cross-reference line of the axial slice at z = 12 mm on the sagittal series at the expected row
- [ ] 5.3 Offscreen UI test of the side-by-side layout with two series; screenshots reviewed
- [ ] 5.4 Notice shown when the displayed series exceed the memory budget
