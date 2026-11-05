## 1. Maths (`summrise-imaging`)

- [ ] 1.1 Distance and angle in patient space; unit tests with Pixel Spacing 1.0 \ 0.5 (50.0 mm, 100.0 mm, 90.0°)
- [ ] 1.2 Ellipse and rectangle rasterisation by pixel centres; area; mean, standard deviation, minimum and maximum over modality values, ignoring values outside a reformat
- [ ] 1.3 Calibration ladder (Pixel Spacing, Imager Pixel Spacing, none) with the source recorded in the index

## 2. Model (`summrise-core`)

- [ ] 2.1 Measurement type (kind, points in patient or pixel coordinates, owning image or plane), kept per series for the session only
- [ ] 2.2 Tool state: create, select, move handle, move whole, delete, clear all; Esc cancels

## 3. UI

- [ ] 3.1 Toolbar buttons with accessible names and single-key shortcuts shown in tooltips
- [ ] 3.2 Viewport interaction and drawing (lines, handles, haloed labels) on acquired and reformat views
- [ ] 3.3 Labels: one decimal, unit, "≈" on reformats, calibration notes, projection name for region statistics on slabs
- [ ] 3.4 One-time "Approximate — not for diagnosis" notice and status-bar hint while a measurement tool is active

## 4. Verification

- [ ] 4.1 Integration test on the 256-pixel synthetic CT: sphere diameter 19.0–21.0 mm; soft-tissue mean 37–43 HU; air mean −1000 HU with standard deviation 0
- [ ] 4.2 Test that the same points give the same length at different zoom levels
- [ ] 4.3 Offscreen UI test drawing each tool; accessibility check passes
- [ ] 4.4 Test that measurements are gone after closing and re-opening a study
