## 1. Synthetic data (`summrise-testkit`)

- [x] 1.1 `pet::write_pet_study`: the phantom CT and, in its Frame of Reference, a PET of 60 × 60 × 50 voxels of 4 mm (BQML, ATTN/DECY, decay correction START, F-18 half-life 6586.2 s, 370 MBq injected one half-life before the series time, 70 kg, per-slice rescale slopes) with a hot sphere of SUV 5 (13 214.3 Bq/ml) at the marker; a PET without attenuation correction; options for a PET in another Frame of Reference, a missing slice and a missing weight; tests: the numbers give SUV 5, the series are written

## 2. Uptake values (`summrise-dicom`)

- [x] 2.1 `pet` module: `PetHeader::read`, date and time parsing (DA, TM, DT), `suv_bw` with every refusal as a variant and its sentence; unit tests: SUV 5.0 for 13 214.3 Bq/ml, each refusal, GML passed through, negative time difference refused, series time after acquisition refused, more than ten half-lives refused
- [x] 2.2 Index: `Series::frame_of_reference` and `Series::pet` read in `read_header`, merged over the files (one Frame of Reference for all files, earliest acquisition time); tests on the PET study (`summrise-core` `pet` and `session::overlay` tests)
- [x] 2.3 `SeriesPlan::suv`, units "SUV" or the stored unit, value-range estimate scaled; `SuvScale::apply` (frames and stored windows, never narrower than the minimum width) called from `load_series`; test: plan and `load_series` agree, the hot sphere reads SUV 5 in the volume

## 3. Colour maps and blending (`summrise-imaging`)

- [x] 3.1 `colormap`: four 256-entry tables with documented end points, re-colouring of a grey picture that keeps non-finite pixels black; tests for the end points and the control points
- [x] 3.2 `fusion`: `PixelGrid` of a frame plane or a reformat, trilinear `resample`, `blend` with window, colour map, opacity and threshold; tests: a single bright voxel lands on its patient position in any grid orientation, opacity 0 changes nothing, values below the threshold are transparent

## 4. View state, session, guide (`summrise-core`)

- [x] 4.1 `SeriesData::fill` applies the SUV factor; `SeriesData::units` reports SUV; test: progressive loading equals `load_series` for every series of the PET study (pixels, value range, presets, units)
- [x] 4.2 `ViewState`: colour map (inverted grey for PT and NM) and `Fusion` (series, opacity, colour map, colour scale, threshold) with reset, `is_pristine`, settling of the colour scale (SUV 0–5 for a PET in SUV); SUV presets in `presets_for`; tests
- [x] 4.3 Fusion candidates: same study, same non-empty Frame of Reference, PET over CT or MR; test: a PET in another Frame of Reference is not offered
- [x] 4.4 `Session::set_overlay`: loads the overlay like any series, never evicts it while in use, pauses and releases it afterwards, refuses it when it cannot form a volume or does not fit into the budget; `overlay_status`; tests: not evicted while in use and evicted afterwards, unavailable states
- [x] 4.5 Uptake section of the inspector (inputs, result, reason, weight hidden in privacy mode) and explanations of its keys (`SUVbw` in two sentences); SUV presets explained; tests that the texts are plain and short
- [x] 4.6 Test: the PET maximum lies within one PET voxel of the CT marker on the acquired CT image, on each reformat and on a tilted plane (measured: 0.2–0.7 mm)

## 5. Viewer (`summrise-ui`)

- [x] 5.1 `pet::compose_frame` / `compose_reformat` called from one line in `render_frame` and `render_mpr`; render key includes the colour map and the overlay; overlay from the session passed to the render cache
- [x] 5.2 Toolbar menu "PET colours and fusion" (colour maps with colour strips; PET overlay with status, opacity, colour map, colour scale, threshold and the note; "Show over …" from the PET); keys C and F; accessible names and tooltips
- [x] 5.3 Read-out "CT 300 HU · PET SUV 5", annotations (window in SUV, colour map, overlay line, positions note), the overlay's status badge with and without annotations, W/L fields with two decimals for SUV
- [x] 5.4 Inspector: the uptake section with info marks
- [x] 5.5 UI tests: PET opens inverted, C cycles the maps, F draws the PET over the CT with the read-out of both values (also turned and mirrored), MPR with the overlay, the unavailable state with its reason, the inspector's refusal reason

## 6. Verification

- [x] 6.1 Offscreen snapshots looked at: PET alone with each colour map, CT with PET overlay in the acquired view (also turned and mirrored) and the MPR layout, loading and unavailable states, both menus in the dark and the light theme, the inspector's section computed and refused
- [x] 6.2 `cargo xtask ci` and `openspec validate --all --strict` pass
- [x] 6.3 The new tests that wait for background loading pass 20 times in a row
- [ ] 6.4 A real PET/CT disc from a scanner opened and its SUV compared with the scanner's own values (cannot be verified here: no real patient data may be used; tracked with `field-validation`)
- [ ] 6.5 The menu, the keys and the overlay tried in the real window on macOS, Windows and Linux (cannot be verified here: the application window cannot be operated from this environment)
