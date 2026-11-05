## 1. Imaging maths

- [x] 1.1 Plane classification, orientation letters, edge labels
- [x] 1.2 `MprAxis` bases, `MprExtent`, `reslice` with trilinear sampling and NaN outside
- [x] 1.3 Slab projections (MIP/MinIP/mean)
- [x] 1.4 Unit tests with an asymmetric marker phantom for axial, sagittal and coronal acquisitions

## 2. View state

- [x] 2.1 `ViewState`: slice, cursor, slab, per-view zoom/pan; stack ↔ MPR sync
- [x] 2.2 Layouts Single / MPR / 2×2 with automatic fallback when no volume

## 3. UI

- [x] 3.1 Viewport widget: aspect-correct display, wheel/trackpad accumulation, pinch, tools, double-click fit
- [x] 3.2 Crosshair lines in partner colours; coloured view titles; active-view border
- [x] 3.3 Overlays: corners, orientation letters, scale bar (no collisions), privacy mode
- [x] 3.4 Status-bar read-out of pixel, patient position and value
- [x] 3.5 Render cache keyed by inputs; interpolation toggle

## 4. Verification

- [x] 4.1 Integration test: marker quadrant on MPR of decoded CT
- [x] 4.2 Offscreen screenshots of Images, MPR and 2×2 layouts reviewed

## Follow-ups (moved out of this change)

- 4.3 Cine playback for dynamic/multi-phase series (future change) → roadmap (README, "Later ideas")
- 4.4 3D volume rendering (future change) → roadmap (README, "Later ideas")
