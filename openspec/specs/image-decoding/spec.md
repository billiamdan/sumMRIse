# image-decoding Specification

## Purpose

Turning DICOM pixel data into correctly scaled (modality units such as Hounsfield units), correctly ordered frames and regular volumes — robustly and within a memory budget.

## Requirements
### Requirement: Supported transfer syntaxes
The decoder SHALL decode Implicit/Explicit VR Little Endian, Explicit VR Big Endian,
Deflated Explicit VR Little Endian, JPEG Baseline, Extended and Lossless, JPEG 2000
(lossless and lossy) and RLE Lossless; series using any other transfer syntax SHALL be
listed with the transfer syntax name and a "not supported by this build" notice.

#### Scenario: JPEG-compressed localizer
- **WHEN** a series is stored as JPEG Baseline (1.2.840.10008.1.2.4.50)
- **THEN** all its frames decode and display

### Requirement: Modality values
Grey-scale samples SHALL be converted with the Modality LUT (Rescale Slope/Intercept,
including per-frame and shared functional groups); CT data SHALL be labelled "HU".

#### Scenario: CT stored with intercept −1024
- **WHEN** a CT pixel has stored value 1324 with slope 1 and intercept −1024
- **THEN** its displayed value is 300 HU

### Requirement: Photometric interpretation
MONOCHROME1 images SHALL initially display inverted (minimum value white); RGB and
YBR_FULL/YBR_FULL_422 images SHALL display in colour regardless of planar configuration.

#### Scenario: Secondary-capture dose report
- **WHEN** an RGB secondary capture is selected
- **THEN** it displays in colour and window controls are hidden

### Requirement: Spatial ordering
Frames with patient geometry SHALL be ordered by position along the slice normal,
oriented so that the lowest Instance Number comes first.

#### Scenario: Files written in random order
- **WHEN** slice files are stored in shuffled order with Instance Number 1 at the most superior slice
- **THEN** image 1 is the most superior slice and positions decrease monotonically

### Requirement: Volume assembly
A series SHALL become a volume only if its frames share size, orientation and pixel
spacing, have no duplicate positions and are evenly spaced (deviation ≤ max(10 %,
0.05 mm)); otherwise the reason SHALL be reported and only the acquired images shown.

#### Scenario: Missing slice
- **WHEN** one file of an evenly spaced 2 mm stack is missing from the disc
- **THEN** reformatting is unavailable with the reason "slices are not evenly spaced … a slice may be missing"

#### Scenario: Gantry tilt
- **WHEN** a CT stack is acquired with 15° gantry tilt
- **THEN** a volume is built with the tilt represented as shear and the inspector shows "Gantry tilt 15.0° (corrected in reformats)"

### Requirement: Robust decoding
A corrupt or undecodable file SHALL NOT crash the application or prevent other images
of the series from displaying; the number of failed images SHALL be reported.

#### Scenario: One corrupt slice
- **WHEN** one of 100 slices fails to decode
- **THEN** 99 slices display and a warning "1 image(s) could not be decoded" appears

### Requirement: Memory budget
Decoded series SHALL be kept in memory up to a configurable budget, evicting the least
recently used series that is not displayed.

#### Scenario: Budget exceeded
- **WHEN** opening a second large series would exceed the budget
- **THEN** the first series is evicted and decoded again if re-selected

