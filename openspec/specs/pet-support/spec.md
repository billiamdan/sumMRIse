# pet-support Specification

## Purpose
PET images: standardised uptake values computed from the headers with their inputs shown, colour maps, and PET drawn over the CT or MR of the same scan.
## Requirements
### Requirement: Standardised uptake values
For a PET series whose headers allow it, the viewer SHALL show values as standardised
uptake values by body weight (SUVbw), computed once per series with the vendor-neutral
calculation published by QIBA: the injected dose is decay-corrected from the
radiopharmaceutical start time to the series time with the radionuclide half-life, and
SUVbw = activity concentration [Bq/ml] × patient weight [g] / decayed dose [Bq]. The
calculation SHALL require Units BQML, Corrected Image with ATTN and DECY, and Decay
Correction START. The one factor per series SHALL be applied where decoded images enter
the series, so that the window, presets, read-out, value range, statistics and reformats
are all in SUV and the unit is reported as "SUV".

#### Scenario: Hot sphere of SUV 5
- **WHEN** a PET series records Units BQML, Corrected Image ATTN and DECY, Decay Correction START, patient weight 70 kg, injected dose 370 MBq, half-life 6586.2 s, series time 10:15:00 and injection time 08:25:13.8 (one half-life earlier), and the pointer is over a voxel of 13 214.3 Bq/ml
- **THEN** the read-out shows "SUV 5" (13 214.3 × 70 000 / 185 000 000 = 5.000), the window fields and annotations are in SUV, and the decoded value range of the series is in SUV

#### Scenario: Values stored as SUV
- **WHEN** a PET series records Units GML and no SUV Type, or SUV Type BW
- **THEN** its values are shown unchanged (factor 1) with the unit "SUV", and the inspector says that they were stored as uptake values in the images

#### Scenario: Reformats in SUV
- **WHEN** an axial PET series with a computed SUV factor is reformatted and the pointer is over the hot sphere in the coronal view
- **THEN** the read-out shows the same SUV as in the acquired image at that position

### Requirement: Uptake values are refused when an input is missing
The viewer SHALL NOT compute SUV when an input is missing, implausible or unsupported,
and SHALL NOT guess a missing input. The values SHALL then stay in the stored unit
("Bq/ml" for BQML), and the inspector SHALL say which input stops the calculation. The
reasons are: the unit is not recorded or is not Bq/ml; the images are not corrected for
attenuation or for decay; the decay correction is not recorded or is not to the start of
the scan; the stored uptake values are not by body weight; the patient weight (or a
weight of 0), the injected dose, the half-life, the injection time or the series date
and time is not recorded; the injection time is later than the series time; more than
ten half-lives lie between the injection and the series time; an image was acquired
before the series time.

#### Scenario: Weight not recorded
- **WHEN** a PET series in Bq/ml records everything except the patient weight
- **THEN** the read-out over a voxel of 13 214.3 Bq/ml shows "13214.3 Bq/ml", no SUV preset is offered, and the inspector's uptake section reads that SUV is not computed because the patient's weight is not recorded

#### Scenario: Injection after the series time
- **WHEN** the injection time is 11:00:00 and the series time 10:15:00 on the same date
- **THEN** SUV is not computed and the reason is that the injection time is later than the series time

#### Scenario: Counts and decay correction to the injection
- **WHEN** a PET series records Units CNTS, or Decay Correction ADMIN
- **THEN** SUV is not computed and the inspector names the unit CNTS, or the decay correction ADMIN, as the reason

#### Scenario: Not attenuation corrected
- **WHEN** a PET series records Corrected Image DECY without ATTN
- **THEN** SUV is not computed and the reason is that the images are not corrected for attenuation

### Requirement: Uptake values are approximate and not diagnostic
The application SHALL present SUV as approximate and not for diagnosis: it depends on
the weight, the injected dose and the times entered by staff, on the scanner's
calibration and on the time between injection and scan. The inspector SHALL list every
input with its recorded value next to the result, and the explanation of SUV SHALL say
in two sentences what SUV is and that it is computed from values entered by staff. The
patient weight SHALL be hidden in privacy mode and SHALL never be written to logs.

#### Scenario: Inputs listed
- **WHEN** a PET series with a computed SUV is displayed
- **THEN** the inspector's "Uptake values" section shows that the values are SUV computed by SumMRIse, the weight 70 kg, the injected dose 370 MBq, the injection time 08:25:13, the reference time 10:15:00, the half-life 6586.2 s, the decay correction START, the corrections and the stored unit BQML

#### Scenario: Privacy mode
- **WHEN** privacy mode is on while that series is displayed
- **THEN** the weight row shows that the value is hidden, and no other row of the window shows the weight

### Requirement: PET display
A PET or nuclear-medicine series SHALL open with the inverted grey scale customary for
PET, so that high uptake is dark (a MONOCHROME1 series, already inverted, opens with the
plain grey scale). The colour maps "Grey", "Inverted grey", "Hot iron" and "Rainbow"
SHALL be selectable for these series from the toolbar and with C; they SHALL NOT be
offered for other modalities. The colour map is part of the view state: reset restores
the default and it is never saved.

#### Scenario: PET opens inverted
- **WHEN** a PET series with window SUV 0–5 is opened
- **THEN** a voxel of SUV 0 is white, a voxel of SUV 5 or more is black, and the area outside a reformatted volume stays black

#### Scenario: Hot iron
- **WHEN** the user presses C until "Hot iron" is chosen
- **THEN** SUV 0 is black, SUV 5 is (255, 254, 252) and SUV 2.5 is red; R restores the inverted grey scale

### Requirement: PET fusion is offered for the same Frame of Reference only
The viewer SHALL offer to draw a PET series over a CT or MR series of the same study
(from the CT or MR) and to show the PET over the CT or MR (from the PET) when both record
the same Frame of Reference UID. A PET with another or no Frame
of Reference UID SHALL NOT be offered. Fusion SHALL use the positions recorded by the
scanners only; no registration is attempted. A note SHALL say that the positions come
from the scanners and that movement between the two scans can shift them.

#### Scenario: Same Frame of Reference
- **WHEN** a CT and a PET series of a study record the same Frame of Reference UID
- **THEN** the CT's fusion menu offers the PET, and the PET's menu offers to show it over the CT

#### Scenario: Other Frame of Reference
- **WHEN** a second PET series of the study records another Frame of Reference UID
- **THEN** it is not offered over the CT

### Requirement: Drawing the PET over the anatomy
The overlay SHALL be resampled trilinearly from the PET series' volume at the patient
position of every pixel of the picture shown — the acquired image or the reformat
plane, tilted or not — colour-mapped through its colour scale and blended with its
opacity; values below the lower threshold SHALL be transparent. It SHALL follow
scrolling, zoom, pan, tilted planes and the display orientation, because it is part of
the picture placed on screen. The read-out SHALL show both values, for example "CT 40 HU
· PET SUV 3.1". The overlay starts at 50 % opacity with the Hot iron colour map, the
colour scale SUV 0–5 for a PET in SUV (else the PET's own opening window) and the
threshold 10 % into the colour scale. Opacity, colour map, colour scale and threshold are
part of the view state of the CT or MR series; reset switches the overlay off, and it is
never saved. A 3D view SHALL NOT show the overlay.

#### Scenario: Hot sphere on the marker in every view
- **WHEN** the PET's hot sphere of SUV 5 lies at L 40, A 35, S 50 mm, where the CT has its 300 HU marker, and the PET is drawn over the axial CT image at S 50 mm, over each reformat through the marker and over a coronal plane tilted by 30°
- **THEN** in each picture the PET's maximum lies within one PET voxel (4 mm) of the marker, and the read-out over the marker shows "CT 300 HU · PET SUV 5"

#### Scenario: Opacity 0 and threshold
- **WHEN** the opacity is 0, or the threshold is SUV 2 and a pixel's PET value is SUV 1
- **THEN** the CT pixel is shown unchanged

#### Scenario: Turned and mirrored view
- **WHEN** a view with the overlay is turned a quarter turn and flipped horizontally
- **THEN** the PET stays on the marker, the read-out over the marker still shows "CT 300 HU · PET SUV 5", and the edge letters give the true directions

### Requirement: Overlay loading and availability
The overlay series SHALL load in the background like any series, progressively and
within the memory budget, with its progress shown in a status badge on the picture;
while it is drawn it SHALL NOT be evicted, and once switched off it can be evicted again.
The pausing of the displayed series is unchanged. If the PET series cannot form a
volume, or the displayed series and the PET do not fit into the memory budget together,
the overlay SHALL be unavailable and the viewport and the menu SHALL say why: the badge
briefly, its tooltip and the menu in full.

#### Scenario: Loading
- **WHEN** the PET overlay is switched on while 12 of its 50 images are decoded
- **THEN** the badge reads "PET loading 12 / 50", whether annotations are shown or not, and the overlay appears when the PET volume is complete

#### Scenario: A slice missing
- **WHEN** the PET series lacks one slice in the middle of its stack
- **THEN** no PET image is loaded, the badge reads "PET overlay unavailable: no regular volume", and its tooltip and the menu say that the PET series does not form a volume because its slices are not evenly spaced

#### Scenario: Memory budget
- **WHEN** the memory budget is smaller than the displayed CT and the PET together
- **THEN** the badge reads "PET overlay unavailable: not enough memory", the menu names the memory needed and the budget, and no PET image is loaded

#### Scenario: Not evicted while in use
- **WHEN** the budget is reduced while the PET is drawn over the CT
- **THEN** neither the CT nor the PET is evicted; after the overlay is switched off, the PET is evicted

