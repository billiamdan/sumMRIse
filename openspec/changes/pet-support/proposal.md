## Why

Oncology discs hold a PET series next to a CT. Today the PET values are shown in the
units they are stored in (thousands of Bq/ml, or no unit at all), on the grey scale of a
CT, and never together with the anatomy: the PET series is a grey blur that tells a
patient nothing. Professional viewers show standardised uptake values, the colour scales
customary for PET, and the PET drawn over the CT.

## What Changes

- **Uptake values.** For a PET series whose headers allow it, values are shown as
  standardised uptake values by body weight (SUVbw): in the value read-out ("SUV 3.1"),
  the contrast fields and annotations, the presets (upper limits SUV 5, 7, 10 and 15,
  plus automatic) and the inspector. When an input is missing or unsupported (weight,
  injected dose, half-life, injection or series time, units other than Bq/ml, a decay
  correction other than to the start of the scan, images not corrected for attenuation
  or decay, an injection after the series time), the values stay in the stored unit and
  the inspector says which input stops the calculation. An "Uptake values" section of
  the inspector lists every input with its value, and its explanation says in two
  sentences what SUV is and that it rests on values entered by staff.
- **PET display.** A PET or nuclear-medicine series opens with the inverted grey scale
  customary for PET (high uptake dark). The colour maps "Grey", "Inverted grey", "Hot
  iron" and "Rainbow" can be chosen for these series from a toolbar menu or with C.
- **Fusion.** When the study holds a PET series and a CT or MR series in the same Frame
  of Reference, the viewer offers to draw the PET over the anatomical series, and from
  the PET series to put the CT or MR underneath (menu or F). The overlay has an opacity,
  a colour map, a colour scale and a lower threshold below which the PET is transparent.
  It is drawn on acquired images and on reformats, follows scrolling, zoom, tilted planes
  and the display orientation, and the read-out shows both values ("CT 40 HU · PET SUV
  3.1"). A note says that positions come from the scanners and that movement between the
  two scans can shift them.
- **Overlay loading.** The overlay series loads in the background like any series, with
  visible progress, and is kept in memory while it is drawn. If it cannot form a volume
  or does not fit into the memory budget, the overlay is unavailable and says why.

Patient safety: SUV is computed only when every input is recorded and supported; nothing
is guessed, every refusal is named, and the inputs are shown so the number can be
checked. SUV and the fused picture are labelled as approximate and not diagnostic: SUV
depends on weight, dose and times typed in by staff and on the scanner's calibration,
and fusion uses the positions recorded by the scanners only (no registration), so
movement between the scans shifts the PET against the anatomy. Patient weight, read for
the calculation, is a patient detail: it is hidden in privacy mode and never logged.
Nothing new is written to disk or to the settings; the colour map and the overlay are
view state and are never saved.

## Capabilities

### New Capabilities
- `pet-support`: standardised uptake values with their refusals and inputs, PET display
  defaults and colour maps, positional PET fusion over CT or MR (drawing, read-out,
  loading, availability, note), and its limits (approximate, not diagnostic, not shown in
  a 3D view).

### Modified Capabilities
- `contrast-windowing`: adds the uptake presets for SUV data and the colour maps of PET
  and nuclear-medicine series.

## Impact

- `crates/imaging`: new `colormap` (four 256-entry lookup tables) and `fusion`
  (resampling a volume on the pixel grid of a displayed picture, colour mapping and
  alpha blending).
- `crates/dicom`: new `pet` module (header values, the SUVbw calculation as a pure
  function with every refusal as an enum variant, the one place that scales decoded
  values); the index keeps the Frame of Reference UID and the PET header values of each
  series; `SeriesPlan` carries the SUV outcome and `plan_series`/`load_series` report
  SUV as the unit.
- `crates/core`: the colour map and the overlay settings in `ViewState` (reset, pristine,
  never saved); SUV presets; the inspector's uptake section; fusion candidates; the
  session keeps one overlay series loading and protected from eviction while in use;
  explanations of the new inspector rows.
- `crates/ui`: a toolbar menu for colours and fusion, keys C and F, blending in the frame
  and reformat render paths, read-out and annotations, the inspector section.
- `crates/testkit`: a PET/CT study (PET in the CT's Frame of Reference with a hot sphere
  of SUV 5 at the marker, a non-attenuation-corrected PET, optional variants).
- API changes for other code: new fields `Series::frame_of_reference`, `Series::pet`
  and `SeriesPlan::suv`; `RenderCache::update` and `Inspector::show` take one more
  argument. No new dependency, no file-format or settings change.
