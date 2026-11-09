## Context

A PET series is indexed and displayed like any other series. `decode_object` applies the
Modality LUT (rescale slope and intercept, per file), so a PET stored in Bq/ml arrives as
`f32` concentrations; `plan_series` derives the unit from Rescale Type (usually absent
for PET) and only knows "HU" for CT. Window presets, the value read-out, statistics and
reformats all work on whatever values the decoded images hold. The index keeps no Frame
of Reference UID and none of the PET attributes (Units, Decay Correction, the
radiopharmaceutical sequence, patient weight). Every view draws one grey picture: a
texture built by `render_frame` or `render_mpr` and placed on screen by `placement.rs`.
The session loads the displayed series and pauses or evicts the others.

## Goals / Non-Goals

**Goals:**
- SUVbw where the headers allow it, computed once per series by a pure, tested function;
  every value downstream (window, presets, read-out, statistics, reformats) in SUV.
- Every refusal named, nothing guessed, the inputs visible.
- PET colour maps; PET drawn over CT or MR at true positions in every view and
  orientation, with the read-out of both values.
- The second series loads like any series and does not upset the memory budget.

**Non-Goals:**
- SUV by lean body mass or body surface, vendor-private scale factors (Philips CNTS),
  dynamic PET, SPECT quantification, region statistics (SUVmax of a region).
- Registration of series from different frames of reference, or correcting movement.
- Enhanced PET objects' correction attributes (their values are not read; the SUV is then
  refused with the missing input).
- Showing the overlay in a 3D view.

## Decisions

### D1 — SUVbw: the QIBA calculation, happy path, as a pure function (binding)
`summrise_dicom::pet::suv_bw(&PetHeader) -> Result<SuvScale, SuvRefusal>` follows the
vendor-neutral pseudo-code published by QIBA (UPICT/QIBA FDG-PET, "SUV calculation"):

1. Units (0054,1001) must be `BQML`. `GML` (values stored as SUV by the scanner or a
   workstation) is passed through with the factor 1, as in the QIBA code, unless SUV Type
   (0054,1006) names a kind other than body weight; every other unit (`CNTS`, …) is
   refused.
2. Corrected Image (0028,0051) must contain `ATTN` and `DECY`, and Decay Correction
   (0054,1102) must be `START`: the values are decay corrected to the start of the scan.
3. Reference time: Series Date (0008,0021) + Series Time (0008,0031). If an image was
   acquired before that time (earliest Acquisition Date/Time, or Acquisition DateTime,
   over every file of the series), the series time was probably rewritten by later
   processing, and the calculation is refused.
4. Injection time: Radiopharmaceutical Start DateTime (0018,1078) when recorded, else
   Radiopharmaceutical Start Time (0018,1072) on the series date. A UTC offset at the end
   of a DateTime is ignored: all times of one study are recorded in the scanner's local
   time.
5. Decay time = reference − injection; it must not be negative, nor longer than ten
   half-lives (less than 0.1 % of the dose would be left: the recorded times cannot
   belong together, for example an injection date a day off).
6. Decayed dose = Radionuclide Total Dose (0018,1074, Bq) × 2^(−decay time / Radionuclide
   Half Life (0018,1075, s)); factor = Patient Weight (0010,1030, kg) × 1000 / decayed
   dose; SUVbw = concentration [Bq/ml] × factor.

Weight, dose and half-life must be positive and finite (a weight of 0, which some
scanners store when nothing was entered, counts as missing). The dose, half-life and
times are read from the first item of the Radiopharmaceutical Information Sequence
(0054,0016). Each refusal is a variant of `SuvRefusal` with a translated sentence:
`UnitsMissing`, `Units(code)`, `NotAttenuationCorrected`, `NotDecayCorrected`,
`DecayCorrectionMissing`, `DecayCorrection(code)`, `StoredSuvType(code)`,
`WeightMissing`, `DoseMissing`, `HalfLifeMissing`, `InjectionTimeMissing`,
`SeriesTimeMissing`, `InjectionAfterSeries`, `SeriesAfterAcquisition`, `DecayTooLong`.
Dates and times are parsed strictly (DA `YYYYMMDD` or the legacy `YYYY.MM.DD`, TM
`HH[MM[SS[.F…]]]` or `HH:MM:SS`, DT with at least the hour); a malformed value counts as
missing.

The testkit's numbers: 70 kg, 370 MBq, F-18 half-life 6586.2 s, injection exactly one
half-life before the series time. The decayed dose is 185 MBq, the factor 70 000 g /
185 000 000 Bq = 3.7838 × 10⁻⁴ ml/Bq, and 13 214.3 Bq/ml is SUV 5.0
(13 214.3 × 70 000 / 1.85 × 10⁸ = 5.000 005).

Alternatives considered:
- QIBA's fallback for a series time after the acquisition (use the earliest acquisition
  time) — rejected for now: whether Acquisition Time is the start of a bed or its middle
  differs between vendors and is not recorded, so the fallback can be wrong by minutes;
  the brief asks for the happy path and never to guess.
- Decay Correction `ADMIN` (no decay of the dose needed) — rejected: outside the happy
  path, and rare on discs; refused with its code.
- Philips' private SUV scale factor for `CNTS` — rejected: vendor-private, out of scope.
- A factor per file — rejected: a series whose files disagree would mix units in one
  volume. The factor is computed from the series' header record (D5).

### D2 — One factor per series, applied in exactly one place (binding)
`plan_series` stores the outcome in `SeriesPlan::suv` (`None` for series that are not
PET). `SuvScale::apply(&mut DecodedInstance)` is the only code that multiplies values:
it scales the grey frames and the windows stored in the file (centre and width) by the
factor; a stored window that becomes narrower than the minimum width a window is applied
with (1, `Window::MIN_WIDTH`) is widened to it, so that the window shown is the window
applied. It is called where decoded images enter a series: `SeriesData::fill`, through
which every image the viewer shows passes, and `load_series`, which builds a whole
series at once (and serves as the reference of progressive loading), so both give
identical results. The value-range estimate of the plan is scaled with the same factor.
Everything downstream — initial window, presets, automatic window, drag sensitivity,
value range, read-out, reformats, slabs, the overlay — consumes SUV and does not know
that a factor exists.

Alternatives considered:
- Scaling at display time (window, read-out and statistics each multiply) — rejected:
  every consumer would have to know, and one forgotten place shows Bq/ml labelled SUV.
- Passing the factor to `ImageSource::decode` — rejected: a trait change for every image
  source (tests have several), for a value the series store already has.

### D3 — Units and how SUV values are written
`SeriesData::units()` is `"SUV"` when the factor applies (computed or passed through).
When the calculation is refused, it is the stored unit in words: `BQML` → "Bq/ml", `GML`
→ "g/ml", `CNTS` → "counts", any other code as recorded. SUV values are written with at
most two decimals and the unit first ("SUV 3.1"), as is customary; other units keep
today's form ("40 HU"). The W and L fields allow two decimals for SUV data, and the
annotation reads "W 5  L 2.5 SUV".

### D4 — Uptake presets (contrast-windowing)
For SUV data the preset menu lists the stored windows (scaled, D2), then "SUV 0–5",
"SUV 0–7", "SUV 0–10" and "SUV 0–15" (LINEAR_EXACT windows from 0 to the limit), then
Auto and Full range, so keys 1–9 and A keep their meaning. They have the source
`PresetSource::Clinical` (built in); explanations and hints tell them from the CT
windows by the unit of the series, which is "SUV" only for them. A series in SUV that
stores no window opens with "SUV 0–5" instead of the automatic window (D11); otherwise
the window a series opens with is unchanged (first stored window, else automatic;
progressive-loading D4).

Alternative considered: a new `PresetSource` variant — rejected: it changes a shared
enum that other changes extend, and every match on it, for a distinction the unit
already makes.

### D5 — Frame of Reference and PET values in the index (binding)
`Series` gets `frame_of_reference: Option<String>` (Frame of Reference UID) and `pet:
Option<PetHeader>` (only for modality PT). Both are read in `read_header` from the data
set that is parsed anyway, like the other series attributes from the first file indexed,
and merged while the files are grouped: the Frame of Reference UID is kept only if every
file of the series records the same one, and the earliest acquisition time over all
files is kept for D1 step 3. `PetHeader` holds the patient's weight, the one patient
attribute a series record now carries: it does not identify the patient, is never
logged, and is hidden in privacy mode where it is shown (D12).

Alternatives considered:
- Fields on `InstanceHeader` — rejected: the Frame of Reference and the PET values are
  series-level in the PET and CT IODs, and every `InstanceHeader` literal would change.
- Fields on `Acquisition` — rejected: that record promises to hold nothing about the
  patient, and the weight is patient data.

### D6 — Colour maps (binding)
`summrise_imaging::colormap::Colormap` has four lookup tables of 256 RGB entries, indexed
by the display level (0 = the low end of the window, 255 = the high end):

| Map | Entry 0 | Entry 255 | Between |
|---|---|---|---|
| Grey | black | white | grey ramp |
| Inverted grey | white | black | grey ramp, reversed: high uptake dark |
| Hot iron | black | (255, 254, 252) | red rises over entries 0–127 (2 per entry, then 255), green over 128–255, blue over 192–255 (4 per entry): black, red, orange, yellow, white |
| Rainbow | black | red | linear through blue (51), cyan (102), green (153) and yellow (204) |

A map re-colours the grey picture that the window produces: each pixel's grey level picks
the entry; pixels outside the volume (not finite) stay black, as they do for the grey
scale. Inversion (I) still applies before the map. PET and nuclear-medicine series open
with Inverted grey (Grey for MONOCHROME1 images, which are inverted already), which is
customary and shows high uptake dark; Rainbow is offered because it is customary but is
never the default. Other modalities use Grey and are not offered maps.

Alternatives considered:
- Mapping inside `gray_to_rgba` — rejected: it changes a shared function that thumbnails
  and another change's lookup-table work use; the re-colouring pass works after any grey
  pipeline.
- "Inverted grey" as the invert flag — rejected: the brief names four colour maps, and a
  MONOCHROME1 series uses the flag already.

### D7 — Fusion is positional only (binding)
A PET series (modality PT, grey, with geometry) can be drawn over a CT or MR series of the
same study when both record the same, non-empty Frame of Reference UID. No registration
is attempted: positions come from the scanners. A PET in another Frame of Reference is
not offered at all. Positions are compared in patient coordinates (LPS) only.

### D8 — Resampling and blending, one helper per render path (binding)
`summrise_imaging::fusion`:
- `PixelGrid`: the patient position of pixel (0, 0) of the displayed picture and the
  steps for one column and one row — from the acquired image's plane (Image Position,
  Orientation and Pixel Spacing) or from the reformat (`MprImage`).
- `resample(volume, grid)`: the overlay's volume sampled trilinearly (`Volume::sample`)
  at the centre of every pixel; NaN outside the volume.
- `blend(rgba, grid, volume, look)`: for every pixel with an overlay value at or above the
  threshold, the colour of the value (window → level → colour map) is mixed in:
  `out = (1 − opacity) × base + opacity × colour`. Below the threshold and outside the
  overlay volume the picture is unchanged; with opacity 0 nothing changes.

`crates/ui/src/pet.rs` applies the series' colour map and then the overlay in one
function (`compose`), reached from one line in `render_frame` (`compose_frame`) and one in
`render_mpr` (`compose_reformat`); the line for the scanner graphics of another change
belongs after it, so that they are drawn over the PET as well. The composite is the
view's texture, so zoom, pan, quarter turns and mirroring (`placement.rs`) apply to both
series alike, and the read-out samples the overlay at the patient position under the
pointer with the same function. In a slab view the overlay is sampled on the central
plane of the slab.

Alternatives considered:
- A second texture drawn on top — rejected: a second mapping that must agree with the
  first in eight orientations, and a re-render of both anyway.
- Nearest-neighbour sampling — rejected: blocky 4 mm voxels and positions off by up to
  half a voxel.

### D9 — The session keeps one overlay series (binding)
`Session::set_overlay(Option<SeriesKey>)` names the series drawn over the displayed one.
It loads like any series (its own progressive loader, nearest the middle image first)
and is not paused while it is the overlay; the active series is paused and resumed as
before. `enforce_budget` never evicts the active series or the overlay. When the overlay
changes or is switched off, the previous one is paused if it is still loading (unless it
is the displayed series) and can be evicted again. Before loading, two checks from the
headers: whether the overlay's images form a volume (`plan_series`), and whether the
displayed series and the overlay fit into the budget together (an estimate: rows ×
columns × frames × 2 bytes for 16-bit integer data, 4 bytes for PET and rescaled data,
3 for colour; the size of what is loaded where it is known). `Session::overlay_status`
gives Loading (n of N), Ready, or Unavailable with the reason ("The PET series does not
form a volume: slices are not evenly spaced …", "Together with the displayed series it
needs about 312 MB; the memory budget is 256 MB.", "It could not be loaded: …"). Nothing
of a refused overlay is loaded.

Alternatives considered:
- Treating the overlay as a second active series in the pause logic — rejected: the brief
  keeps the pausing rules of the active series; the overlay is a separate role.
- Evicting other series to make room before refusing — rejected: eviction already happens
  for everything that is not displayed; the check concerns the two that must stay.

### D10 — Overlay state is view state (binding; defaults chosen here)
`ViewState` holds the colour map of the series and an optional `Fusion`: the overlay
series, opacity (default 50 %), colour map (default Hot iron; Hot iron or Rainbow of
the PET's view when the CT is put underneath from the PET, since the grey scales would
hide the anatomy), colour scale (a window in the overlay's units) and threshold (default
10 % of the colour scale above its lower end: SUV 0.5 for SUV 0–5). The colour scale is
"SUV 0–5" for a PET in SUV, the window of the PET's view when the CT is put underneath
from the PET, and otherwise the overlay's own opening window; it is fixed when the
overlay's first image is decoded. `reset` switches the overlay off and restores the
colour map; a view with an overlay or another colour map is not pristine; none of it is
saved.

Alternative considered: the overlay's own opening window for every PET — rejected after
looking at the result: without a stored window that is the automatic window of one image,
which for a slice without strong uptake spans little more than the body's own values, so
the body maps to the top of the colour scale and the fused picture is washed out. SUV
0–5 is the range PET/CT fusion is customarily shown in. The PET shown alone opens the
same way when it stores no window (D4).

### D11 — Controls (open choices decided here)
- One toolbar menu button ("PET colours and fusion"), shown only for PET or nuclear
  medicine series and for series that have a fusion partner. For a PET or NM series it
  lists the colour maps and, if there are partners, "Show over …" for each CT or MR; for a
  CT or MR it lists "No PET overlay" and each PET, and while one is chosen its status,
  opacity, colour map, colour scale (the overlay's presets), threshold and the note on
  positions.
- C cycles the colour map of the PET (of the series, or of the overlay on a CT or MR); F
  switches the overlay on (first PET) and off, and on a PET series opens the first CT or
  MR with this PET over it.
- The overlay's status is a status badge on the picture (`orient::badge`, row 2, below
  "Mirrored" in row 0 and the lossy-compression mark of another change in row 1), shown
  whether annotations are on or not: "PET loading 12 / 50", or "PET overlay unavailable:
  no regular volume" (or "not enough memory", "could not be loaded") with the full reason
  on hover and in the menu. A first version drew a text box above the scale bar; the
  snapshots showed it covering the bottom edge letter, which must stay visible.
- Annotation lines name the overlay ("PET overlay 50 % · Hot iron · SUV 0–5") and say
  "Aligned by scanner positions only", and a PET or NM series names its colour map
  ("Colours: Inverted grey"). They come first in the bottom-right block, above window and
  zoom, where they stay clear of the scale bar in small views.
- The read-out: "CT 40 HU · PET SUV 3.1" while an overlay is drawn; "SUV 3.1" for a PET
  alone; other units as before ("40 HU").
- The W and L fields allow two decimals for SUV, and a preset of an SUV series is hinted
  as "Standard PET window".

### D12 — The inspector's uptake section
For a PET series the inspector shows, after "Display", a section "Uptake values" built
from the series' header record and the SUV outcome (not from the displayed file, so it
always describes what is shown): what the values are shown as (with the explanation of
SUV), the reason when SUV is not computed, the inputs (weight, injected dose, injection
time, reference time, half-life, decay correction, corrections, stored unit, tracer)
and, when computed, the dose at the reference time. Rows carry DICOM keywords as keys for
explanations (`SUVbw` for the computed row); the weight row has none, because rows about
the patient are never explained (plain-language-guide), and its value is hidden in
privacy mode.

### D13 — Not in the 3D view (binding)
The overlay is composited in the acquired-image and reformat render paths only. A 3D view
renders `series.volume()` of the displayed series alone.

### D14 — Split base series and dynamic PET (binding)
A base series split into stacks uses the positions of the image shown (acquired view) and
the volume of its current stack (reformats); the overlay series uses `volume()`, which is
stack 0 for a split series; dynamic PET is out of scope.

## Risks / Trade-offs

- [SUV shown although an input is wrong (a mistyped weight or injection time)] →
  approximate and not for diagnosis in the spec, the inputs listed in the inspector, the
  explanation says the value rests on entries made by staff.
- [Patients move between the CT and the PET; breathing differs] → the note in the menu and
  the annotation; nothing claims registration.
- [Refusing on a rewritten series time hides SUV for some post-processed series] → the
  reason is shown; the values stay true in Bq/ml.
- [Memory: two volumes at once] → checked against the budget before loading; the overlay
  is evicted like any series once it is switched off.
- [Rendering cost] → the overlay is resampled only when the picture is rendered again
  (the render key includes its settings), at most 768 pixels per side for reformats.
- [Thumbnails of PET series stay grey] → the browser's previews are not re-coloured;
  the view opens with the inverted grey scale.
- [Windows narrower than one SUV] → `Window::MIN_WIDTH` (1) applies to every unit; for
  SUV it is the narrowest window, which is narrower than any customary PET window.
- [The PET's automatic window on a flat image] → `auto_window` widens a range narrower
  than one unit to the image's whole range, which for SUV data can be the whole image, so
  a PET without stored windows opened with little contrast (seen in the 3D projection,
  where the whole body saturated). A series in SUV that stores no window therefore opens
  with SUV 0–5 (D4); Auto stays in the menu, and data kept in Bq/ml keeps the automatic
  window.
- [The CT's body edge next to the coarse PET] → where the PET (4 mm voxels) falls below
  the threshold at the body's edge, single CT pixels show untinted; this is the threshold
  doing what it says.

## Migration Plan

API changes for code outside this change (no file-format or settings change):
- `Series` has new fields `frame_of_reference: Option<String>` and `pet:
  Option<PetHeader>`; struct literals must fill them (`None`).
- `SeriesPlan` has a new field `suv: Option<Result<SuvScale, SuvRefusal>>` (`None`: not
  PET).
- `ViewState` has the colour map and the overlay (`colormap()`, `fusion()`, …); reset and
  `is_pristine` include them.
- `RenderCache::update` takes the overlay series (`Option<&SeriesData>`), and
  `Inspector::show` the uptake section, as one more argument each.
- New: `summrise_dicom::pet`, `summrise_imaging::{colormap, fusion}`,
  `summrise_core::pet`, `Session::{set_overlay, overlay, overlay_status,
  overlay_candidates, underlay_candidates}`, `summrise_testkit::pet`.
