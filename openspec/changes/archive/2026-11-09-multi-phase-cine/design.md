## Context

`plan_series` orders the frames of a series from the indexed headers (`display_order`):
frames with geometry are sorted by position along the slice normal, and when two frames
share a position `VolumeGeometry::from_sorted_planes` answers `DuplicatePositions`; the
series is then shown in instance order as one flat list and reformats are off. The index
keeps per frame only the image plane; per instance the Image Type and the rescale values.
`SeriesData` holds one write-once slot per displayed image and assembles one volume on
completion; `ViewState::slice` is the index into those slots. The viewer advances images
only on input; it has no clock of its own besides `request_repaint_after`.

## Goals / Non-Goals

**Goals:**
- A series with evenly repeated positions is shown as stacks that each behave like an
  ordinary series: scrolling, reformats, slabs, crosshair, contrast.
- What distinguishes a stack is said wherever an image of it is shown.
- Cine playback of images and of time-like stacks, deterministic under test.
- Nothing changes for series that are not split; the guarantees of progressive loading
  hold for split series as well.

**Non-Goals:**
- A 4D viewer showing several dimensions at once (time × echo, b-value × direction).
- Cardiac analysis, exporting video, splitting localizers by orientation.
- Vendor private attributes (b-values or directions held only in private groups).
- Deriving the playback speed from trigger times.

## Decisions

### D1 — Stacks live inside one series (binding)
A split series stays one `Series`, one `SeriesData`, one loader, one entry of the memory
budget and one browser row. The split is decided from the headers in `plan_series`,
before any pixel is read, and stored in `SeriesPlan::stacks` (`None` for a series that is
not split). Slots are stack-major: stack 0 sorted by position, then stack 1, and so on;
every stack has `stack_len` slots and all stacks share one `VolumeGeometry`.

Alternatives considered:
- One `Series` entry per stack — rejected (binding): the stacks are one DICOM series that
  was acquired, loaded, counted against the budget and described as one; separate
  entries would need separate loaders reading the same files, and thumbnails, the guide
  and the recent list would treat parts of a series as series.
- Deciding the split after decoding — rejected: the order would be unknown while loading,
  against the progressive-loading rule that images never change place.

### D2 — The split rule (binding)
In `crates/dicom/src/stacks.rs`, called by `display_order` where the flat verdict is
`DuplicatePositions`, which is only reached for greyscale frames that all have geometry
and share orientation, size and pixel spacing.

1. Positions: frames are grouped by their distance along the common normal; a frame
   farther than 0.01 mm from the first frame of the current group starts a new one.
   P = number of groups, N = number of frames.
2. A split is tried only when P ≥ 2, N = k × P with k ≥ 2, and every position holds
   exactly k frames. Otherwise nothing changes (today's flat list and explanation).
3. Discriminators, in this fixed order; the first one that is *accepted* wins. A
   discriminator is accepted when every frame has a value for it, the distinct values
   are exactly k, and each value's frames hold every one of the P positions exactly once.

   | # | Stack kind | Classic attribute | Enhanced (per-frame, else shared functional group) | Stacks ordered by |
   |---|---|---|---|---|
   | 1 | Time point | Temporal Position Identifier (0020,0100) | Frame Content › Temporal Position Index (0020,9128) | value |
   | 2 | Heart phase | Trigger Time (0018,1060) | Cardiac Synchronization › Nominal Cardiac Trigger Delay Time (0020,9153) | value |
   | 3 | Echo | Echo Number(s) (0018,0086) and Echo Time (0018,0081) | MR Echo › Effective Echo Time (0018,9082) | echo number, then echo time |
   | 4 | Diffusion weighting | Diffusion b-value (0018,9087) | MR Diffusion › Diffusion b-value | value |
   | 5 | Image type | Image Type (0008,0008) | MR or CT Image Frame Type › Frame Type (0008,9007) | first occurrence |
   | 6 | Acquisition | Acquisition Number (0020,0012) | Frame Content › Frame Acquisition Number (0020,9156) | value |
   | 7 | Sequence | Sequence Name (0018,0024) | Pulse Sequence Name (0018,9005) | first occurrence |
   | — | Part | the order in which the frames occur at each position (instance number, then file, then frame) | | occurrence |

   Numbers are compared to a thousandth of their unit (ms, s/mm²). The last resort always
   succeeds once step 2 holds.
4. Order inside the stacks: positions run along the normal in the direction in which
   stack 0 has its lowest instance number first (the rule of the flat list); the same
   direction is used for every stack, so image i of every stack is the same slice.
5. Shared geometry: the frame of stack s at position i must lie within 0.01 mm of the
   frame of stack 0 at position i (whole Image Position (Patient), not only the distance
   along the normal); otherwise the series is not split. The verdict is that of stack 0's
   planes: a volume, or for example "not evenly spaced" when a slice position is missing
   in every stack. Such stacks are still split (scrolling inside a stack is still right),
   only without reformats.

Time points and heart phases are *time-like*; all other kinds differ in contrast (D8).

Alternatives considered:
- Dimension Index Values of Enhanced objects — rejected for now: interpreting the
  Dimension Index Sequence is general but has no classic counterpart; the functional
  groups listed give the same result for the cases asked for and use one rule for both
  kinds of object.
- Combined discriminators (b-value with gradient direction) — rejected for now: a DTI
  series with 30 directions falls back to "Part n / 31", which is true; naming directions
  is a possible follow-up.
- Splitting an uneven series as far as it goes — rejected: an image missing in one stack
  would shift the others or leave holes that look like slices; the series keeps today's
  explanation instead.
- A tolerance relative to the slice spacing — rejected: repeated positions are written
  identically by the scanner; 0.01 mm is far below any slice spacing and far above the
  rounding of decimal strings.

### D3 — Per-frame values in the index
`InstanceHeader::frame_values: Vec<FrameValues>` (one entry per frame, like
`frame_planes`) holds the values of the table above, read in `read_header` from the data
set that is parsed anyway (up to the pixel data, also from archives). For each value the
per-frame functional group is read first, then the shared group, then the top-level
attribute. An empty vector means "nothing recorded" (struct literals in tests).

Alternatives considered:
- Reading the values when the series is planned — rejected: planning must not open
  files; from an archive or a CD every header would be read twice.
- A table beside the instances in `StudyIndex` — rejected: `plan_series` takes a
  `Series`, and the values belong to the frames as the planes do.

### D4 — Series whose images all lie at one position (binding)
P = 1 (a cine of one slice): not split. The flat list is ordered by Temporal Position
Identifier, else Trigger Time, when every frame records it (stable sort of the instance
order), else in instance order as today; cine plays it. Series without geometry
(ultrasound, angiography) are not touched: their frames are in file order, which is time
order.

### D5 — `SeriesData` (binding)
`stack_count()` (1 when not split), `stack_len()` (`len()` when not split), `stacks()`,
`stack_label(stack)`, `stack_volume(stack)`; `volume()` stays and is stack 0's volume, for
callers that do not know about stacks (the 3D view being built in parallel). On
completion one volume per stack is assembled over the shared slots, with the shared
geometry. A hole in any stack (an image that failed) gives the series the verdict "not
evenly spaced", computed from the first stack with a hole, as for a flat series: reformats
are offered for all stacks or for none, so that the layouts never switch on and off with
the stack.

Alternative considered: a verdict per stack — rejected for the reason just given.

### D6 — `ViewState`: stack, image inside the stack and flat slot (binding)
`ViewState` gets the current stack (`stack()`, `set_stack`, `step_stack`). `slice` stays
the index of the acquired image **inside the current stack**; `slot_index(series)`
(`stack × stack_len + slice`) is the index into `SeriesData::frames`/`image`/`frame`, and
is used wherever frames, file paths (inspector), the loader's displayed index or the
render cache are looked up. Slice counts, Home/End, scrolling and the image numbers shown
count inside the stack ("Image 7 / 12"). Reformats, slabs and the crosshair use the
current stack's volume; the crosshair position is shared, so changing the stack keeps the
image number, the crosshair and every reformat plane. `is_pristine` requires stack 0 and
every per-stack window at its default; choosing another stack is a user change, so a
remembered reformat layout does not take over a view in use. `reset` restores the default
window of every stack and keeps the position, which is the image and the stack.

### D7 — Contrast per stack
Stacks that differ in contrast (echo, diffusion weighting, image type, acquisition,
sequence, part) keep a window each; time points and heart phases share one, so that a
loop through them does not flicker.
- A stack's default is the first stored window of the first decoded image *of that
  stack*, else an automatic window from that image — the rule of the series default
  (progressive-loading D4) applied per stack. `SeriesData` keeps one such record per
  stack, filled when the first image of the stack is decoded and never recomputed.
- When the user chooses a stack none of whose images is decoded yet, the series' window
  is used provisionally; when the stack's first image arrives (the loader reads the
  displayed image next) its own default replaces it, unless the user has changed the
  window meanwhile. Until then the stack shows only loading placeholders, so no picture
  is ever shown with a window that then jumps.
- The preset menu, the number keys and "A" offer the current stack's stored windows, an
  automatic window from its loaded images and its full range; drag sensitivity and the
  limits of the W and L fields follow the stack's value range.

Alternatives considered:
- One window for all stacks — rejected: with echoes of 15 and 90 ms or b = 0 and 1000
  the second set appears almost black in the window of the first.
- A window per stack for time points too — rejected (binding): playback would flicker,
  and the time points of one acquisition share their contrast.
- Defaults from stored windows read with the headers — rejected: most series store no
  window; the default must be found the same way as for a series that is not split.

### D8 — Cine (binding: viewer state, UI clock, injected clock in tests)
`crates/ui/src/cine.rs` holds the playback state of the viewer; it is never saved.
- `tick(now, …)` is called once per frame with the UI clock (`InputState::time`); it
  steps at most once per frame when a step is due, and asks for the next repaint with
  `request_repaint_after`. No thread. Under egui_kittest the clock advances by the
  harness step, so tests set the time they need; the unit tests pass the time directly.
- What is played is fixed when Play is pressed: for a series with time-like stacks the
  stacks at the current position, in every view; otherwise the images of the active view
  (the acquired stack or a reformat). If that view leaves the layout, or another series
  is selected, playback stops.
- Loop (… n−1, 0, 1 …) or back and forth (0 … n−1, n−2 … 0, 1 …; the end images are not
  shown twice).
- Before stepping onto an acquired image that is still loading, playback waits; the
  loader is told that image as the displayed one, so it is read next. Images that failed
  count as present: they are shown with their reason, as when scrolling.
- Speed: Recommended Display Frame Rate (0008,2144), else Cine Rate (0018,0040), else
  1000 / Frame Time (0018,1063), as read with the series' acquisition parameters; 10 per
  second when none is recorded; always 1 to 60 per second. A speed the user sets is kept
  until another series is selected.

Alternatives considered:
- A timer thread — rejected (binding): the repaint request is enough, and the UI clock
  makes tests deterministic.
- Catching up several images when frames are late — rejected: it would skip images; the
  loop runs slower instead.
- Skipping images that are still loading — rejected (binding): the loop would show a
  different sequence each time round.

### D9 — The playback bar
A bar at the bottom of the image area (between the series browser and the inspector),
shown for every series of more than one image, holds, for a series with stacks, "Image
set" with previous/next buttons and a list of the stacks, then Play/Pause, the speed
(1–60 per second), Loop and Back and forth and, while playback waits, a spinner with
"Waiting for images to load". Loop and Back and forth are icon buttons, as on media
players, with their names for assistive technology and in their tooltips: with text they
did not fit beside the browser and the inspector at 1440 points. The waiting text is cut
short with an ellipsis when the bar is narrower (at 1280 points with both side panels
open); the viewport still shows "Loading n / N". Every control has an accessible name and
a tooltip with its key.
- Keys: Space plays and pauses when no widget has keyboard focus (with a button focused,
  Space activates the button, as everywhere in egui). For a series with several stacks
  Left and Right change the stack; Up and Down change the image as before; Alt+Left/Right
  in a reformat view still tilt the planes, because those bindings are checked first.

Alternatives considered:
- The toolbar — rejected: it is full at 1280 points with the inspector open
  (archive-import D11), and the controls belong to the images.
- Controls drawn on the picture — rejected: they would compete with dragging and
  scrolling on the image, the scale bar and the annotations.

### D10 — Naming the stacks
Labels go through `tr!`, with numbers formatted before they are inserted and values from
the disc inserted untranslated: "Time point 3 / 20", "Heart phase 2 / 25 · 40 ms",
"Echo 2 / 2 · TE 90 ms", "b = 1000 s/mm²", "Image type 2 / 2 · P" (only the values of
Image Type that differ between the stacks), "Acquisition 2 / 3", "Sequence 2 / 2 ·
*ep_b1000t", "Part 2 / 3". The browser row and the inspector say "3 time points × 12
images" (`tr_n!`). The bottom-left annotations of every view of a split series and the
viewport's accessible label name the stack; the inspector has an "Image set" row, and its
"Image" row counts inside the stack.

### D11 — The session knows the stacks of every series
`Session` computes `summrise_dicom::series_stacks` once per series when it is created
(headers only; the same function `plan_series` uses), so that the browser can say which
series have stacks before any of them is loaded: `Session::stacks(key)`.

Alternative considered: computing it in the browser on every frame — rejected: a sort of
every series' positions on every repaint.

### D12 — The guide (plain-language-guide)
`describe_series` adds, after the plane of the slices, one statement for a split series,
for example "This series holds 3 time points of the same slices." with the basis
"Recorded in the images: temporal position." (`tr_n!`); "Part" says that nothing recorded
tells the sets apart. The rest of the guide is unchanged.

### D13 — Progressive loading (binding)
The loader is unchanged: it works on flat slots, so the images nearest the displayed one
in the same stack come first and runs cross into the next stack only at its ends. The
first image is on screen after one decode, nothing moves while loading, and the final
state equals `load_series`, which now plans the same stacks and assembles the same
volumes (`SeriesImages::Stacks`). The reference comparison of progressive loading
(`session::tests::final_state_equals_non_progressive_loading_for_every_synthetic_series`)
is kept as it is, and the same comparison, with the volume of every stack, runs over all
seven series of the stacks study in `crates/core/tests/stacks.rs` (time points, echoes,
b-values, parts, an Enhanced object, and the two series that are not split).

### D14 — Synthetic data
`summrise_testkit::stacks::write_stacks_study` writes seven MR series on a 48 × 48 axial
grid of 5 mm pixels (origin −120, −120 mm): (a) 3 time points × 12 slices (S 20 to 75 mm,
5 mm apart) with Temporal Position Identifier and Trigger Time, a marker sphere at L 40,
A 35, S 50 mm (`MARKER_POSITION`) of 600, 900 and 1200; (b) two echoes (Echo Number 1/2,
TE 15/90 ms) with values around 2000 and 300 and a stored window each; (c) b = 0 and
b = 1000 s/mm²; (d) the data of (a) without any discriminator; (e) (a) with one image
missing; (f) 20 images at one position with Frame Time 40 ms; (g) one Enhanced MR object
with 2 echoes (TE 30/100 ms) × 6 slices, frames interleaved.

## Risks / Trade-offs

- [A discriminator splits real data wrongly] → it is accepted only if every stack holds
  every position exactly once and the stacks share their geometry; the label says what the
  split rests on; "Part n / k" when nothing recorded tells the sets apart.
- [Series of many sets (DTI)] → "Part n / k" with one stack per set, true but unnamed.
- [The first image of a newly chosen stack changes the window numbers once] → only while
  that stack shows placeholders, and never after the user changed the window.
- [A slow machine cannot keep the speed] → one step per frame at most: the loop runs
  slower than set, never skips.
- [Memory] → the per-frame values take about 150 bytes per frame (some 90 KB for 600
  frames) in the index.
- [Stacks of two slices] → like a two-slice series today: no reformats (3 are needed).

## Migration Plan

API changes for code outside this change (no file-format or settings change):
- `InstanceHeader` has a new field `frame_values: Vec<FrameValues>`; struct literals must
  fill it (`Vec::new()`: nothing recorded).
- `SeriesPlan` has a new field `stacks: Option<Stacks>`; `LoadedSeries` too, and
  `SeriesImages` a new variant `Stacks(Vec<Volume>)`.
- `Acquisition` has three new fields (`recommended_frame_rate`, `cine_rate`,
  `frame_time_ms`); literals with `..Acquisition::default()` are unaffected.
- `SeriesData::volume()` is the volume of stack 0; stack-aware code uses
  `stack_volume(state.stack())`.
- `ViewState::slice` is the image inside the current stack; `ViewState::slot_index` is
  the index into `SeriesData::frames`. `ViewState::presets(series)` gives the presets of
  stack 0, `ViewState::presets_for(series, stack)` those of any stack.
- New: `summrise_dicom::series_stacks`, `Session::stacks`.
