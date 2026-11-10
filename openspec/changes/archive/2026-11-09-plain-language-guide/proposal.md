## Why

SumMRIse is used by patients and relatives. A series called `t2_tse_sag_384`, a row
"Repetition time (TR) 4000 ms" or a preset called "Mediastinum" means nothing to them,
and the letters at the edges of an image do not tell them from where they are looking at
the body. Professional viewers do not explain any of this; a viewer for patients can.

## What Changes

- Every series is described in plain language: one short line under its title in the
  series browser ("MR scan, T2-weighted") and an "About this series" card in the
  inspector with a headline and a few statements — what kind of scan it is, the plane of
  the slices in everyday words, for MR the weighting and what tends to appear bright or
  dark with it, for CT the slice thickness, the reconstruction kernel as recorded and
  whether a contrast agent is recorded. Localizers, diffusion images, ADC maps, dose
  report images, secondary captures, PET, nuclear medicine, X-ray projections and
  ultrasound are recognised as such.
- Uncertainty is stated. A weighting inferred from timing parameters reads "Likely …"
  and shows the values it rests on; a weighting named in the series description or the
  protocol name is reported as named by the scanner operator; when the rules do not apply
  the guide says that it cannot tell. Every statement shows its basis.
- Rows of the inspector overview that have an explanation show an info mark; hovering it
  or reaching it with the keyboard shows one or two plain sentences, including what the
  unit means. Each contrast preset says in its tooltip what the window is for and which
  values it shows.
- "Where is this image?": a small schematic figure in the inspector. With a recognisable
  Body Part Examined it is a body outline with the examined region highlighted and the
  image on screen drawn as a line or band; otherwise it is the scanned block with the
  image plane inside it and orientation letters. One or two sentences say from where the
  body is seen and which side is on the left of the screen. The figure is captioned as
  schematic and not to scale.
- A setting "Show explanations" (on by default) hides all of it.

Patient safety: the guide describes how the images were made and never what they show.
It makes no statement about what is normal or abnormal, about diagnosis or about
treatment, and a fixed line in the card says so. Classification prefers "cannot tell" to
a label that may be wrong, and what is inferred is worded as likely. The viewing
direction and the plane in the figure are computed from the direction cosines of the
image that is on screen, so they are true for mirrored, rotated and oblique images too;
what is approximate (an oblique plane, the figure) is labelled as such.

Privacy: the guide reads acquisition, series and geometry attributes only — never the
patient's name, ID, birth date, sex or age — so it shows the same text with privacy mode
on or off. Nothing is written to disk and nothing leaves the computer.

## Capabilities

### New Capabilities
- `plain-language-guide`: plain-language description of each series with its basis, the
  viewing direction and position figure, preset explanations, the "Show explanations"
  setting and the rule that the guide never interprets images.

### Modified Capabilities
- `metadata-inspector`: adds explanations of the overview rows, keyed by DICOM keyword.

## Impact

- `crates/dicom`: new `acquisition` module and `Series::acquisition` (series-level
  acquisition parameters read from classic attributes or Enhanced multi-frame functional
  groups during header indexing); summary rows become `SummaryRow` with a stable `key`.
- `crates/core`: new `guide` module (classification, texts, viewing direction, body
  region, placement of the image in the scanned block); `Settings::show_explanations`.
- `crates/ui`: new `guide` module (card, figure, info marks); small additions to the
  inspector, the series browser, the preset menu and the settings window.
- `crates/testkit`: the CT phantom records a contrast agent, the MR phantom a protocol
  name.
- All new text goes through `summrise_i18n::tr!`; no translations are added here.
- No new dependencies, no file-format changes; one new settings field with a default.
