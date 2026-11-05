## ADDED Requirements

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
