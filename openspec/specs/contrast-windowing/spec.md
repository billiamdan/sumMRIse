# contrast-windowing Specification

## Purpose

Mapping modality values to display intensity with the DICOM VOI functions, stored and clinical presets, automatic windows and interactive adjustment.
## Requirements
### Requirement: Standard VOI functions
Display intensity SHALL be computed with the DICOM VOI LUT functions: LINEAR (default),
LINEAR_EXACT and SIGMOID as defined in PS3.3 C.11.2.1.2, using the VOI LUT Function
stored in the image when present.

#### Scenario: LINEAR boundaries
- **WHEN** the window is centre 2048, width 4096
- **THEN** value 0 maps to black, 4095 to white and 2047.5 to 50 % grey

#### Scenario: Brain window on CT
- **WHEN** the Brain preset (width 80, level 40) is applied to CT data
- **THEN** −1000 HU (air) is black, +1000 HU (bone) is white and 40 HU is mid-grey

### Requirement: Initial window
A series SHALL open with the first Window Center/Width stored in its images, named by
Window Center & Width Explanation (or "Window n"); without stored windows it SHALL open
with an automatic window spanning the 0.5–99.5 percentile of non-background values.

#### Scenario: CT with two stored windows
- **WHEN** the files store centres 40\-600, widths 400\1500, explanations MEDIASTINUM\LUNG
- **THEN** the series opens at W 400 / L 40 labelled "Mediastinum" and "Lung" is offered as the next preset

### Requirement: Presets
The preset menu SHALL list stored windows, then — for Hounsfield data only — Brain
(80/40), Subdural (200/75), Stroke (40/40), Head soft tissue (380/40), Temporal bone
(2800/600), Lung (1500/−600), Mediastinum (350/50), Abdomen (400/50), Liver (150/30),
Bone (1800/400) and Angio (600/300), followed by Auto and Full range; keys 1–9 SHALL
select the first nine presets and A the automatic window.

#### Scenario: MR series
- **WHEN** an MR series is displayed
- **THEN** no CT presets are offered

### Requirement: Interactive adjustment
Dragging with the contrast tool (or right-dragging with any tool) SHALL change width
horizontally and level vertically at (value range / 1024) units per point, with a minimum
width of 1; numeric W and L fields SHALL allow exact entry; any manual change SHALL mark
the window as "Custom".

#### Scenario: Drag to the right
- **WHEN** the user drags 100 points to the right on a CT with range −1024…3071 HU
- **THEN** the window width increases by 400 HU and the preset label shows "Custom window"

### Requirement: Inversion and reset
The user SHALL be able to invert the grey scale (I) and reset contrast, zoom and pan to
the series defaults (R); MONOCHROME1 series SHALL start inverted; pixels outside a
reformatted volume SHALL stay black when inverted.

#### Scenario: Inverted reformat
- **WHEN** inversion is enabled on a coronal reformat
- **THEN** tissue appears inverted but the area outside the volume remains black

### Requirement: Contrast tables
Every table of the VOI LUT Sequence SHALL be offered as a contrast choice named by its LUT
Explanation (title-cased, or "Contrast table n"), listed after the stored windows and
before the built-in windows, Auto and Full range, with keys 1–9 counting through that list;
the first table SHALL be the default when the image stores no window; a table SHALL map the
modality value (rounded; first or last entry outside its input range) to grey in proportion
to its entry (of 2^bits − 1 of its descriptor), applied to acquired images only, while
reformats and slabs keep the window; choosing a window, dragging, editing W or L, or A SHALL
switch to an ordinary window; where stacks keep a window each they SHALL keep a table choice
each; reset SHALL restore the default.

#### Scenario: Image with two tables and no window
- **WHEN** a radiograph without window values has the tables NORMAL (entry i = round(4095 × √(i / 4095)), 12 bits) and HARDER
- **THEN** it opens with "Normal", the menu offers "Normal" and "Harder" first (keys 1 and 2), and a pixel with stored value 100 is drawn with grey level 40 of 255 (entry 640 of 4095)

#### Scenario: Adjusting contrast by hand
- **WHEN** the user drags with the contrast tool while "Normal" is active
- **THEN** the image is shown with an ordinary window that the drag changes, and the window is labelled "Custom window"

#### Scenario: Read-out with a table
- **WHEN** the pointer is over the pixel with stored value 100 while "Harder" is active
- **THEN** the read-out shows 100, the value, not the table's output

### Requirement: Uptake presets
The preset menu SHALL list, for a series whose values are standardised uptake values
(unit "SUV"), the stored windows (converted to SUV), then the built-in windows "SUV 0–5",
"SUV 0–7", "SUV 0–10" and "SUV 0–15", each spanning from SUV 0 to its upper limit,
followed by Auto and Full range; no CT preset is offered. Keys 1–9 and A keep their
meaning. A series in SUV that stores no window SHALL open with "SUV 0–5" instead of the
automatic window. The W and L fields and the annotations SHALL show SUV with up to two
decimals.

#### Scenario: PET series in SUV opens with SUV 0–5
- **WHEN** a PET series in SUV that stores no window is opened
- **THEN** it is shown with the window "SUV 0–5" (W 5 / L 2.5 SUV), and Auto is still offered in the menu

#### Scenario: PET series in SUV without stored windows
- **WHEN** a PET series in SUV without stored windows is displayed and the user presses 2
- **THEN** the window "SUV 0–7" is applied (W 7 / L 3.5 SUV): SUV 0 is at one end of the grey scale and SUV 7 and above at the other

#### Scenario: PET series kept in Bq/ml
- **WHEN** SUV cannot be computed for a PET series
- **THEN** no SUV preset is offered

### Requirement: Colour maps
For PET and nuclear-medicine series the display intensity SHALL be shown through a
selectable colour map of 256 entries, indexed by the display level of the window: Grey
(black to white), Inverted grey (white to black; the default), Hot iron (black through
red, orange and yellow to (255, 254, 252)) and Rainbow (black through blue, cyan, green
and yellow to red). Pixels outside a reformatted volume SHALL stay black with every
colour map, and inversion (I) SHALL apply before the colour map.

#### Scenario: End points
- **WHEN** the window is SUV 0–5 and the colour map is Rainbow
- **THEN** SUV 0 is black, SUV 5 and above is red (255, 0, 0), and SUV 2 (two fifths of the window) is cyan (0, 255, 255)

