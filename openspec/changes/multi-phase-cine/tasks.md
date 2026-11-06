## 1. Synthetic data (`summrise-testkit`)

- [x] 1.1 `stacks::write_stacks_study`: (a) 3 time points × 12 slices with Temporal Position Identifier and Trigger Time, marker 600 / 900 / 1200; (b) two echoes (Echo Number, TE 15 / 90 ms) with different value ranges and a stored window each; (c) b = 0 and b = 1000; (d) the data of (a) without any discriminator; (e) (a) with one image missing; (f) 20 images at one position with Frame Time 40 ms; (g) an Enhanced MR object with 2 echoes × 6 slices; test: the files are written and indexed as seven series

## 2. Split from the headers (`summrise-dicom`)

- [x] 2.1 `FrameValues` read per frame in `read_header` (classic attributes; per-frame, then shared functional groups of Enhanced objects); frame-rate attributes in `Acquisition`; unit tests with classic and Enhanced data sets
- [x] 2.2 `stacks` module: positions, discriminators in the documented order, acceptance test, last resort by occurrence, one direction for all stacks, shared-geometry check, labels and summaries through `tr!` / `tr_n!`; unit tests per discriminator and for the refusals
- [x] 2.3 `plan_series` and `load_series` carry the stacks (stack-major slots, one geometry, `SeriesImages::Stacks`), single-position series in time order; tests on the stacks study: number of stacks, labels, slot order, shared geometry, positions in mm, plan equal to `load_series`, uneven series as today
- [x] 2.4 `final_volume_issue` per stack; `series_stacks` for callers that only need the split

## 3. Series store, view state, session, guide (`summrise-core`)

- [x] 3.1 `SeriesData`: `stack_count`, `stack_len`, `stacks`, `stack_label`, `stack_volume` (one volume per stack on completion), `volume()` = stack 0; per-stack first image (window, presets) and value range
- [x] 3.2 `ViewState`: current stack, `slot_index`, slice counts and scrolling inside the stack, reformats and crosshair on the current stack's volume; `is_pristine` and `reset` include the stack and the per-stack windows
- [x] 3.3 Per-stack windows for stacks that differ in contrast, one shared window for time-like stacks; provisional window adopted when the stack's first image arrives; presets per stack; tests: echoes keep their own windows, time points share one, reset
- [x] 3.4 Test: the marker of time point 2 reads 900 at L 40, A 35, S 50 mm in the reformat of stack 2, 1200 in stack 3
- [x] 3.5 `Session::stacks` for every series; the guide says what the stacks are (one statement, with its basis); tests
- [x] 3.6 Progressive loading of a split series: first image after one decode, nothing moves, final state equal to `load_series` for every series of the stacks study (order, per-stack volumes, value range, verdict)

## 4. Viewer (`summrise-ui`)

- [x] 4.1 Render, viewport, inspector and loader use the flat slot; the reformats render the current stack; annotations and accessible descriptions name the stack; "Image set" row
- [x] 4.2 Playback bar with the stack control (previous, next, list) and Left/Right for series with stacks; Up/Down unchanged; tests
- [x] 4.3 `cine` module: play/pause, speed from the data (1–60), loop and back and forth, waiting for unloaded images, stop on series change; unit tests with an injected clock (25 per second for Frame Time 40 ms, orders, waiting)
- [x] 4.4 Space toggles playback (not while a widget has keyboard focus); UI tests: Space toggles, playback through time points at one position, waits at an unloaded image, stops on series change
- [x] 4.5 Browser row says that a series has stacks ("3 time points × 12 images")

## 5. Verification

- [x] 5.1 Offscreen snapshots looked at: stack control, cine controls playing and paused, browser row, dark and light theme
- [x] 5.2 `cargo xtask ci` and `openspec validate --all --strict` pass
- [x] 5.3 The new tests that wait for the background loader pass 20 times in a row
- [ ] 5.4 Playback smoothness and the keys tried in the real window on macOS, Windows and Linux (cannot be verified here: the application window cannot be operated from this environment)
