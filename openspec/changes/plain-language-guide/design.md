## Context

The index (`summrise_dicom::Series`) knows the modality, the description, the Body Part
Examined and, per image, the plane and the Image Type. Acquisition parameters (repetition
time, kernel, …) are only read by `read_metadata`, per displayed file, for the inspector,
whose summary rows are `(label, value)` pairs. The viewer knows the geometry of what each
viewport shows (`render::Displayed`: screen axes and pixel-to-patient mapping). Nothing
explains any of it to a lay user. `summrise-i18n` (`tr!`) exists for user-visible text.

## Goals / Non-Goals

**Goals:**
- A patient can read what each series is, what the parameters and presets mean and from
  where the body is seen on screen.
- Every statement is true or marked as likely, and shows what it rests on.
- The logic is UI-agnostic and unit-tested with concrete values.

**Non-Goals:**
- Interpreting findings, saying what is normal or abnormal, diagnosis, treatment.
- Detecting anatomy or the kind of scan from pixel data.
- Vendor private attributes (b-values, sequence details in private groups).
- Translations (a later change translates the texts written here).
- The side of a paired body part (left or right knee): Laterality is not read, both sides
  are highlighted in the figure. A wrong side would be worse than none.

## Decisions

### D1 — Three layers
- `crates/dicom/src/acquisition.rs`: what the files record (`Acquisition`).
- `crates/core/src/guide.rs`: classification, every text, viewing direction, body
  region and the placement of the image in the scanned block. No egui types.
- `crates/ui/src/guide.rs`: the card, the figure (egui shapes), the info marks.

Alternative considered: classification inside the UI crate — rejected: it could not be
reused by another front-end and would be tested through the UI only.

### D2 — Acquisition parameters come from the index
`Acquisition` (a `Default` struct: repetition, echo and inversion time, flip angle,
scanning sequence and variant, sequence and protocol name, MR acquisition type, field
strength, diffusion b-value, contrast agent, convolution kernel, slice thickness, kVp,
image type, body part) is read in `index.rs::read_header` with one added call and kept
in the new field `Series::acquisition`. As for the other series attributes, the values
kept are those of the first file indexed for the series: the parameters are read once
per series, not merged over its images. The card says so in a footnote ("read from one
image of the series; other images of the series may differ"), which matters for series
that mix echo times or b-values.

Each value is looked for in the classic attribute, then in the Enhanced multi-frame
functional groups: the macro in the shared group, the shared group itself (some files
store values flat, as PR 5 found for the rescale values), the macro of the first frame.
The helper lives in `attrs.rs`. Enhanced MR objects have no Scanning Sequence; it is
derived from Echo Pulse Sequence (SPIN → SE, GRADIENT → GR, BOTH → SE and GR), Inversion
Recovery (YES → IR) and Echo Planar Pulse Sequence (YES → EP), so the rules below need
one code set only.

Alternatives considered:
- Reading the parameters from the displayed file in the UI (as the inspector does) —
  rejected: the browser line is needed for every series before any is displayed, and it
  would put file I/O behind a description.
- Merging over all files of a series to detect mixed parameters — rejected for now: it
  needs a second edit in the indexing loop, which another change is reworking; the
  footnote states the limitation instead.

### D3 — Kind of series
First match wins:

| Kind | Rule |
|---|---|
| Dose report image | Secondary Capture SOP class and the description contains the word "dose" (or "dosis"), or "patient" and "protocol"; or Image Type contains SECONDARY and the description contains "dose" with "report", "info", "summary", "record" or "protocol", or "patient" and "protocol" |
| Secondary capture | Secondary Capture SOP class (single- or multi-frame) |
| Localizer | Image Type contains LOCALIZER, or the description or protocol name contains one of the words localizer, localiser, scout, survey, topogram, scanogram, surview, loc |
| CT, MR, PET (PT), nuclear medicine (NM), ultrasound (US, IVUS), mammogram (MG), X-ray fluoroscopy or angiography (XA, RF), X-ray projection (CR, DX, RG, PX, IO) | Modality |
| Unknown | anything else: the guide says it does not recognise the kind |

Words are matched as whole words (the text is split at every character that is not a
letter, a digit or `*`), never as substrings: "LOW DOSE CHEST" on a CT image is not a
dose report, and "location" is not "loc".

### D4 — MR weighting: recorded, named, inferred or unknown
Only for series of kind MR (a localizer gets no weighting). In this order; the first
that applies wins.

1. **ADC map, recorded**: Image Type contains ADC.
2. **ADC map, named**: a word of the series description or protocol name begins with
   `adc`. This comes before the b-value, because ADC maps often carry the b-value of the
   diffusion images they were calculated from.
3. **Diffusion-weighted, recorded**: a b-value above 0 is recorded, or Image Type
   contains DIFFUSION (and neither FA nor TENSOR, which are other derived maps). A
   recorded value is a fact, not an inference, so it is not worded "likely".
4. **Named**: the series description, else the protocol name, names a contrast. Within
   one name, in this order: diffusion (`dw`, `diff`, `dti`, `trace`, `tracew`, or a word
   beginning with `dwi` or `diffusion`); STIR (a word ending in `stir`); FLAIR (a word
   containing `flair`, `darkfluid`, or `dark` followed by `fluid`) — T1 FLAIR when a T1
   word and no T2 word is present, else T2 FLAIR; T2* (a word beginning with `t2*` or
   `t2star`, or `t2` followed by `star`); otherwise exactly one of T1, T2 and proton
   density. T1 words are `t1` alone or followed by one of `w`, `wi`, `c`, `ce`, `gd`,
   `km`, `fs`, `se`, `tse`, `fse`, `weighted`, and words ending in `t1w` with one or two
   letters before it (Philips writes `sT1W`); T2 words likewise; proton density is `pd`,
   `pdw`, `pdwi`, `pdfs`, `pdweighted` or `proton` followed by `density`. Two different
   weightings in one name (a dual-echo "PD T2") name nothing, and other measurements
   (`t1map`, `T1rho`) are no weighting words. A name wins over inference and is reported
   as named by the scanner operator, with the name quoted.
5. **Inferred from timing** ("Likely …", with the values shown). TR, TE and TI in ms.
   The scanning sequence must be recorded.
   - Spin-echo family (SE or IR, no GR; EP only together with SE), no inversion (no IR
     and no TI above 0):
     - TR ≤ 1000 and TE ≤ 30 → T1-weighted.
     - TR ≥ 2000 and TE ≥ 80 → T2-weighted.
     - TR ≥ 2000 and TE ≤ 30 → proton-density-weighted.
   - Spin-echo family with inversion:
     - TI 70…260 and TR ≥ 1500 → STIR (fat suppressed).
     - TI 1500…3000, TR ≥ 4000 and TE ≥ 70 → T2 FLAIR (fluid suppressed).
   - Gradient echo (GR, and none of SE, IR, EP): TE ≥ 15 and flip angle ≤ 30° →
     T2*-weighted.
6. **Unknown**, with the reason: no scanning sequence recorded; timing missing; the
   values fit no rule (contradictory, such as short TR with long TE, or in between); an
   inversion time outside both ranges, or an inversion pulse without its time; a
   gradient-echo, echo-planar or mixed sequence for which timing alone does not settle
   the weighting.

Reasoning for the thresholds:
- With a short echo time, T1 differences dominate as long as the repetition time is not
  much longer than tissue T1 values (about 500–1500 ms at 1.5–3 T): TR ≤ 1000 ms is
  clearly T1-weighted, TR ≥ 2000 ms lets most tissues recover. Between 1000 and 2000 ms
  the contrast is mixed, so the guide cannot tell.
- TE ≤ 30 ms keeps T2 decay small; TE ≥ 80 ms is the textbook lower bound of
  T2-weighting. Between them lies "intermediate weighting", which hospitals name
  inconsistently, so the guide cannot tell.
- Fat is nulled at TI ≈ 0.69 × T1 of fat: about 100 ms at 0.35 T, 150–170 ms at 1.5 T,
  200–230 ms at 3 T, hence 70…260 ms. Fluid is nulled at TI ≈ 1800–2800 ms with long
  TR, hence 1500…3000 ms, and FLAIR needs a long echo time to be T2 FLAIR. An inversion
  time between the ranges (T1 FLAIR, MPRAGE-like preparation) is not classified, and with
  an inversion pulse the spin-echo rules are never applied, because the inversion
  changes the contrast.
- Gradient echo: T1-, T2*- and mixed-weighted variants share timing ranges (a spoiled
  3D sequence with a small flip angle and a 4 ms TR is T1-weighted; a balanced
  steady-state sequence with the same timing is not). Only the combination of a long
  echo time and a small flip angle is unambiguous, so it is the only gradient-echo rule.
  Short-TE gradient echo (time-of-flight angiography, in/opposed phase) stays unknown.
- Echo planar without SE (often gradient-echo EPI for perfusion or functional imaging)
  would be mislabelled by the spin-echo rules, so it stays unknown.

Alternatives considered:
- A wider rule set (inferring T1 from MPRAGE-like timing, weighting from vendor
  sequence names such as `*tse2d1_15`) — rejected: vendor-specific and easy to get
  wrong; the brief asks to prefer "cannot tell".
- Using the name only to confirm the inference — rejected: names are right far more
  often than timing rules, and many valid sequences fit no rule.

### D5 — Statements carry their basis
`describe_series` returns a headline (also the browser line) and `Statement { text,
basis }` values. The basis says where the statement comes from: an attribute recorded in
the images, a name typed at the scanner (quoted), or an inference with its values. The
card shows the basis under each statement. Statements: kind; plane of the slices (axial,
coronal, sagittal, tilted, mixed) in everyday words; MR weighting with what tends to
appear bright or dark; MR 2D or 3D acquisition; slice thickness; CT kernel as recorded;
contrast agent recorded or not (not recorded "does not rule one out"). The plane is
stated for CT, MR, PET and nuclear medicine series and for MR localizers; a CT
localizer is a projection through the body, so nothing is said about slices. A card
that rests on acquisition settings (scans and localizers) carries the footnote of D2;
stored pictures and projection images do not.

`Acquisition::kvp`, `field_strength_t` and `sequence_name` are read as the brief asks
but are not shown in the card: they appear in the inspector rows, which carry their own
explanations. The flip angle and the scanning sequence feed the rules of D4.

### D6 — Stable keys for explanations
`SummarySection::rows` becomes `Vec<SummaryRow { label, value, key }>`. `key` is the
DICOM keyword of the attribute the row shows (`RepetitionTime`); rows built from a tag
get it from the dictionary automatically, computed rows name their main attribute
(`PixelSpacing`, `ImageOrientationPatient`, `BitsStored`, `RescaleSlope`, `WindowCenter`,
`TransferSyntaxUID`), and the two rows that combine attributes use the documented keys
`Matrix` and `FieldOfView`. `guide::explain_parameter(key)` returns the explanation;
labels can be reworded or translated without losing it.

Contrast presets have no keyword. `guide::explain_preset` identifies a built-in CT
window by its `PresetSource::Clinical` source and its width and level (Brain is 80/40),
not by its name; stored, automatic and full-range windows are identified by their
source. A test checks that every entry of `CT_PRESETS` has its own explanation, so a
changed preset cannot silently lose it.

Alternatives considered:
- Looking explanations up by label — rejected (binding): labels are display text and
  will be translated.
- A parallel `keys` vector beside the `(label, value)` tuples — rejected: two vectors
  that must stay in step.
- A key field on `WindowPreset` — rejected: it changes a type that the view state and
  the decoder construct, for eleven constants that a (source, width, level) match
  identifies exactly.

### D7 — Viewing direction from the screen axes
`viewing_direction(right, down)` takes the patient-space directions of screen-right and
screen-down of the image on screen. The viewer looks along `right × down`. The dominant
patient axis of that vector gives "You are looking from the feet towards the head" (and
five more); the dominant axis of `-right` gives "The patient's right side is on the left
of the screen" (and five more). When the direction is more than 10° off that axis
(dominant component below cos 10°), the sentence is the "roughly" variant ("The image
is tilted: you are looking roughly from …", "Roughly, the patient's right side …"). The
title names the plane as `AnatomicalPlane::from_normal` classifies it (more than about
37° off every axis is "Tilted (oblique) image"), so a CT with a 20° gantry tilt reads
"Axial image" with the "roughly" sentence. The sentences are separate messages, so that
they can be translated without gluing clauses.

Alternative considered: one sentence per view kind (axial, coronal, sagittal) — rejected:
acquired images are displayed as stored, so an axial image stored head-to-feet or
mirrored would get a false sentence.

### D8 — Where the geometry comes from
The UI builds a `ScreenPlane` (screen axes and the four corners of the image in patient
space) from `render::Displayed` of the active view — what is actually rendered — through
`screen_axes`, `pixel_size` and `pixel_to_patient` only. The scanned block is the
bounding box, along the patient axes, of the corners of every image of the series, from
the indexed headers. `place(block, plane)` expresses the corners as fractions of the
block (0…1 from the patient's right to left, front to back, feet to head).

Alternative considered: deriving the plane from `ViewKind` and `MprAxis::basis()` —
rejected: it would be silently wrong as soon as reformats can be rotated (another change
is adding oblique reformats); a changed `Displayed` API fails to compile instead.

### D9 — The figure
- Recognisable body part (`body_region`, from Body Part Examined or, for Enhanced
  objects, the Anatomic Region code meaning; case, spaces and punctuation ignored; the
  common values and spellings, such as CHEST, THORAX, LSPINE, L SPINE or "Lumbar spine",
  mapped to 34 regions; anything else is not recognised): a front view of a body outline
  made of convex shapes, patient's right on the left with R and L marked, the region
  highlighted. The block is fitted, with its true proportions, into the region; the
  corners of the image are projected onto the front view. An image seen edge-on (axial,
  sagittal) is a line, an image seen face-on (coronal) or tilted is a band.
- Otherwise: the scanned block as a box in an oblique projection with its proportions
  (no side drawn thinner than 15 % of the longest, so that a single slice still reads as
  a box), the image as a sheet inside it, and the letters R, L, A, P, S, I at its faces —
  the letters the viewports show.
- No geometry (projections, screenshots): no plane; the text says that the images carry
  no position information.
- Captions state "Schematic, not to scale" and what the highlight, line and letters
  mean. Where the scan really lies inside the body is unknown, which is why the figure is
  a schematic: the highlight is the region the operator entered, the line is the image's
  place within the scanned block.

Alternatives considered:
- A body outline for every series, guessed from the description — rejected: guessing
  anatomy is what the brief excludes.
- Front and side outlines together, to show the depth of a coronal image — rejected for
  now: twice the drawing for one case; the direction sentence covers it.

### D10 — Info marks and tooltips
A row with an explanation gets a small info mark after its label. It is a focusable
widget named "About <label>"; the explanation shows while the pointer is over it and
while it has keyboard focus (egui tooltips are hover-only, so focus shows the tooltip
explicitly); it scrolls into view when focused. The tooltip of each entry of the preset
menu shows the existing short hint ("Standard CT window") followed by the explanation;
with explanations off it is the short hint alone, as before.

Alternative considered: making the whole label the hover target without a mark —
rejected: nothing would tell the user that an explanation exists, and a label is not a
keyboard stop.

### D11 — Text rules
Every text is a `tr!` literal with named placeholders; numbers are formatted before they
are inserted; text from the disc (description, kernel, agent) is inserted as it is and
never translated. An explanation is one or two sentences. A test runs every explanation
and every statement of the synthetic series against a list of interpretive words
(normal, abnormal, tumour, lesion, disease, diagnosis, …).

### D12 — Setting
`Settings::show_explanations` (default `true`; the struct has `#[serde(default)]`, so
stored settings without the field load as `true`) with a checkbox "Show explanations of
series, values and the viewing position" in a row "Explanations" of the settings window
(reachable with Tab and Space like the other checkboxes). Off: no browser line, no card, no figure, no info marks, no preset
explanations.

### D13 — Privacy
`describe_series`, `scanned_block` and the other functions take a `Series`, which holds
no patient attributes (they live in `Study::patient`), so the guide cannot use them and
privacy mode needs no special case. No patient row has an explanation. A test compares
the guide texts of the synthetic study against the patient's name, ID and birth date.

## Risks / Trade-offs

- [A wrong label misleads a patient] → conservative rules, "likely" wording with the
  values, names quoted, "cannot tell" otherwise; the fixed line says the guide is not an
  interpretation.
- [Series that mix parameters (dual echo, several b-values) are described by one image]
  → footnote in the card; merging over the images is a possible follow-up.
- [Body Part Examined is entered by hand and can be wrong or imprecise] → the caption
  says the highlight is the region recorded in the images, and quotes the value.
- [The figure suggests a precise location] → "Schematic, not to scale", and no outline
  when the body part is not recognised.
- [Many info marks make the inspector busy and add tab stops] → small, weak-coloured
  marks; one setting hides everything.
- [The inspector shows what the active view rendered in the previous frame] → the same
  one-frame latency the Display rows already have; every input repaints.
