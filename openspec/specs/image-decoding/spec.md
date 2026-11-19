# image-decoding Specification

## Purpose

Turning DICOM pixel data into correctly scaled (modality units such as Hounsfield units), correctly ordered frames and regular volumes — robustly and within a memory budget.
## Requirements
### Requirement: Supported transfer syntaxes
The decoder SHALL decode Implicit/Explicit VR Little Endian, Explicit VR Big Endian,
Deflated Explicit VR Little Endian, JPEG Baseline, Extended and Lossless, JPEG 2000
(lossless and lossy), RLE Lossless and — in release builds — JPEG-LS Lossless and
Near-Lossless; series using any other transfer syntax SHALL be listed with the transfer
syntax name and a "not supported by this build" notice.

#### Scenario: JPEG-compressed localizer
- **WHEN** a series is stored as JPEG Baseline (1.2.840.10008.1.2.4.50)
- **THEN** all its frames decode and display

#### Scenario: JPEG-LS lossless series in a release build
- **WHEN** a release build opens a series stored as JPEG-LS Lossless (1.2.840.10008.1.2.4.80)
- **THEN** all its frames decode and their values equal those of the uncompressed original

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
oriented so that the lowest Instance Number comes first. In a series split into stacks
this order SHALL apply inside every stack, in the direction found for the first stack,
so that image n of every stack lies at the same position; the first stack comes first.

#### Scenario: Files written in random order
- **WHEN** slice files are stored in shuffled order with Instance Number 1 at the most superior slice
- **THEN** image 1 is the most superior slice and positions decrease monotonically

#### Scenario: Stacks in the same direction
- **WHEN** the images of a 3 × 12 dynamic series are stored in shuffled order and Instance Number 1 is the slice at S 20 mm of the first time point
- **THEN** in every stack image 1 lies at S 20 mm and image 7 at S 50 mm

### Requirement: Volume assembly
A series, or each stack of a series split into stacks, SHALL become a volume only if its
frames share size, orientation and pixel spacing, have no duplicate positions and are
evenly spaced (deviation ≤ max(10 %, 0.05 mm)); otherwise the reason SHALL be reported
and only the acquired images shown. The stacks of a series SHALL share one geometry, and
an image that fails to decode in any stack SHALL make reformatting unavailable for the
whole series.

#### Scenario: Missing slice
- **WHEN** one file of an evenly spaced 2 mm stack is missing from the disc
- **THEN** reformatting is unavailable with the reason "slices are not evenly spaced … a slice may be missing"

#### Scenario: Gantry tilt
- **WHEN** a CT stack is acquired with 15° gantry tilt
- **THEN** a volume is built with the tilt represented as shear and the inspector shows "Gantry tilt 15.0° (corrected in reformats)"

#### Scenario: One volume per stack
- **WHEN** all 36 images of a 3 × 12 dynamic series are decoded
- **THEN** there are 3 volumes of 48 × 48 × 12 voxels with the same geometry, and the volume of the second stack holds the images of time point 2

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

### Requirement: Progressive series loading
When a series is selected, its first image SHALL be displayed as soon as that image is
decoded, and the remaining images SHALL become viewable as they are decoded, without
waiting for the whole series.

#### Scenario: Large series from slow media
- **WHEN** a 600-image CT series is selected
- **THEN** the image at the initial position is displayed after one image has been decoded, and the viewport shows the loading progress, for example "Loading 1 / 600"

#### Scenario: Order fixed in advance
- **WHEN** the images of a series arrive in any order
- **THEN** the image counter shows the final total from the start (for example "Image 300 / 600"), and an image already displayed never changes its number or its slice position

### Requirement: Load prioritisation
Loading SHALL favour the images nearest to the one being displayed and SHALL
re-prioritise when the user moves to an image that is not loaded yet.

#### Scenario: Jumping ahead
- **WHEN** images 280 to 320 are loaded and the user jumps to image 550
- **THEN** image 550 and its neighbours are decoded before the remaining images between 321 and 549

### Requirement: Stable display while loading
The viewer SHALL NOT change the contrast window, zoom, pan or position automatically
while further images arrive: the window chosen when the first image appears and
everything the user sets are kept.

#### Scenario: Window adjusted during loading
- **WHEN** the user sets W 1500 / L −600 while a CT series is still loading
- **THEN** the window is still W 1500 / L −600 when loading completes

### Requirement: Pause and resume
Selecting another series SHALL pause loading of the previous one promptly, and
re-selecting a partly loaded series SHALL resume it without decoding its loaded images
again, unless the series was evicted by the memory budget.

#### Scenario: Switching back
- **WHEN** a series is left with 200 of 600 images loaded and is selected again later
- **THEN** the 200 images are available immediately and loading continues with the remaining 400

### Requirement: Stacks of images at the same positions
A series whose slice positions repeat evenly SHALL be split into stacks from its headers,
before any pixel data is read: a greyscale series whose frames all have geometry, one
orientation, one size and one pixel spacing, and whose P ≥ 2 distinct slice positions
each hold exactly k ≥ 2 frames, becomes k stacks of P images that each hold every
position exactly once. What tells the stacks apart SHALL be
taken from the first of these that partitions the frames so: temporal position, trigger
time, echo (number and time), diffusion b-value, image type, acquisition number, sequence
name; failing all of them, the order in which the frames occur at each position. The
stacks SHALL be ordered by that value (image type and sequence name: by first
occurrence) and SHALL share one geometry; a series that cannot be split so SHALL be
displayed as before.

#### Scenario: Time points of a dynamic scan
- **WHEN** an axial MR series holds 36 images with Temporal Position Identifier 1 to 3 at 12 positions from S 20 mm to S 75 mm, 5 mm apart
- **THEN** it is shown as 3 stacks labelled "Time point 1 / 3" to "Time point 3 / 3", image 1 of every stack lies at S 20 mm and image 12 at S 75 mm, and the stacks share one volume geometry of 48 × 48 × 12 voxels of 5 × 5 × 5 mm with its first voxel at R 120, A 120, S 20 mm

#### Scenario: Two echoes
- **WHEN** a series holds 24 images at 12 positions with Echo Number 1 and 2 and Echo Time 15 ms and 90 ms
- **THEN** it is shown as 2 stacks labelled "Echo 1 / 2 · TE 15 ms" and "Echo 2 / 2 · TE 90 ms"

#### Scenario: Diffusion weightings
- **WHEN** a series holds images with Diffusion b-value 0 and 1000 s/mm² at the same 12 positions, all with the same echo time
- **THEN** the echo time does not split them (one value) and the stacks are labelled "b = 0 s/mm²" and "b = 1000 s/mm²"

#### Scenario: Nothing recorded tells the sets apart
- **WHEN** a series holds 3 images at each of 12 positions and no discriminator differs between them
- **THEN** it is shown as 3 stacks labelled "Part 1 / 3" to "Part 3 / 3", taken in the order the images occur at each position

#### Scenario: One image missing
- **WHEN** one of the 36 images of a 3 × 12 dynamic series is missing from the disc
- **THEN** the series is not split: its 35 images are listed in instance order and reformatting is unavailable with the reason "several images share the same slice position (multi-echo, dynamic or repeated scan)"

#### Scenario: All images at one position
- **WHEN** a series holds 20 images of one slice (a cine of the heart)
- **THEN** it is not split and its images are listed in time order

### Requirement: Per-frame values of Enhanced objects
The values that tell stacks apart SHALL be read per frame while the headers are indexed:
for Enhanced multi-frame objects from the per-frame functional groups (Frame Content,
Cardiac Synchronization, MR Echo, MR Diffusion, Frame Type), else from the shared group,
else from the top-level attributes. The frames of one file MAY then belong to different
stacks.

#### Scenario: Enhanced MR object with two echoes
- **WHEN** one Enhanced MR file holds 12 frames at 6 positions, the frames of Effective Echo Time 30 ms and 100 ms interleaved
- **THEN** the series is shown as 2 stacks "Echo 1 / 2 · TE 30 ms" and "Echo 2 / 2 · TE 100 ms" of 6 images each, ordered by position, and the file is read once

### Requirement: Palette colour
Images with Photometric Interpretation PALETTE COLOR SHALL be decoded to colour through
their Red, Green and Blue Palette Color Lookup Tables (8- or 16-bit entries, first index
mapped as recorded, indices beyond the table taking its first or last entry) and treated as
colour images (no windowing, no reformats); images that only carry segmented palette data
SHALL fail with "Segmented palette colour images are not supported yet".

#### Scenario: 8-bit palette image
- **WHEN** an 8-bit palette image whose table maps index 3 to red 200, green 30, blue 60 is decoded
- **THEN** every pixel with stored index 3 is RGB (200, 30, 60) and the series has no window controls

#### Scenario: 16-bit palette image
- **WHEN** a 16-bit palette image whose 16-bit tables map index 3 to (51400, 7710, 15420) is decoded
- **THEN** every pixel with stored index 3 is RGB (200, 30, 60)

#### Scenario: Segmented palette
- **WHEN** a palette image holds only Segmented Red/Green/Blue Palette Color Lookup Table Data
- **THEN** its images are reported as "Segmented palette colour images are not supported yet" and the rest of the study displays

### Requirement: Stored values masked to Bits Stored
Before any modality transform, every grey-scale sample SHALL be reduced to its Bits
Stored (shifted by High Bit + 1 − Bits Stored for native pixel data) and sign-extended when
Pixel Representation is 1, so that bits outside Bits Stored never change a value.

#### Scenario: Overlay bit and garbage in unused bits
- **WHEN** a 16-bit-allocated, 12-bit-stored CT pixel holds 0xF52C (stored value 1324, an overlay bit at bit 12 and garbage in bits 13–15) with slope 1 and intercept −1024
- **THEN** its value is 300 HU, the same as for a pixel holding 0x052C

#### Scenario: Signed 12-bit data
- **WHEN** a signed image with Bits Stored 12 holds 0x0FFF in a pixel
- **THEN** the stored value is −1

### Requirement: Modality LUT Sequence
When the Modality LUT Sequence is present, its table SHALL replace Rescale Slope/Intercept:
stored values are mapped through the table (values outside its input range taking the first
or last entry), and Modality LUT Type SHALL be the unit unless it is "US".

#### Scenario: Non-linear modality table
- **WHEN** an image with a Modality LUT Sequence (4096 entries from 0, entry i = round(i² / 16), type OD) has stored value 100 in a pixel
- **THEN** that pixel's value is 625 OD and the value read-out shows "625 OD"

### Requirement: Presentation LUT Shape INVERSE
Images with Presentation LUT Shape INVERSE SHALL open inverted like MONOCHROME1 images;
MONOCHROME1 images with INVERSE SHALL open inverted once, not twice.

#### Scenario: MONOCHROME2 with INVERSE
- **WHEN** a MONOCHROME2 radiograph records Presentation LUT Shape INVERSE
- **THEN** the series opens inverted: its smallest value is white

### Requirement: Pixel padding
Padding SHALL be recorded for each image, without storing a second copy of the image:
pixels whose stored value equals Pixel Padding Value, or lies between Pixel Padding Value
and Pixel Padding Range Limit; padding SHALL be left out of the automatic window, of the
value range and of the voxels that reformats and slabs sample.

#### Scenario: CT padded outside the scan circle
- **WHEN** a signed CT slice with intercept −1024 has Pixel Padding Value −2000 outside a circle and values from −1000 to 700 HU inside it
- **THEN** its value range is −1000 to 700 HU, its automatic window equals the automatic window of its pixels inside the circle, and −3024 HU appears nowhere

#### Scenario: Padding range
- **WHEN** Pixel Padding Value is 0 and Pixel Padding Range Limit is 10 on unsigned data
- **THEN** pixels with stored values 0 to 10 are padding and a pixel with stored value 11 is not

### Requirement: Scanner graphics planes
Overlay planes (groups 6000–601E) SHALL be read as 1-bit planes kept with each decoded
image, both from Overlay Data (60xx,3000) and from the retired form in unused bits of the
pixel data; a plane with Number of Frames in Overlay SHALL apply to the image frames from
Image Frame Origin on, a plane without it to every frame; a malformed plane SHALL be
skipped without failing the image; overlay bits SHALL never enter pixel values.

#### Scenario: Overlay Data at an origin
- **WHEN** an image holds in (6000,3000) a 16 × 8 rectangle outline with Overlay Origin row 11, column 21
- **THEN** the decoded image carries one plane whose top-left set bit lies over image column 20, row 10 (0-based), and the pixel values are those of the image without the plane

#### Scenario: Overlay in bit 12
- **WHEN** a 12-bit image carries an overlay plane in bit 12 of its 16-bit pixel data
- **THEN** the plane is read from that bit and every pixel value equals the value of its 12 stored bits

#### Scenario: Multi-frame overlay
- **WHEN** a 3-frame image has an overlay of 2 frames with Image Frame Origin 2
- **THEN** frame 1 has no scanner graphics, frame 2 shows overlay frame 1 and frame 3 overlay frame 2

### Requirement: Lossy compression facts
Each image header SHALL record whether the image is or was compressed with loss — Lossy
Image Compression "01", or a transfer syntax that is always lossy — with the ratios and
methods recorded in Lossy Image Compression Ratio and Method, named as "JPEG", "JPEG-LS",
"JPEG 2000", "HTJ2K", "MPEG-2", "H.264", "HEVC" or "JPEG XL".

#### Scenario: Recorded ratio and method
- **WHEN** an uncompressed file records Lossy Image Compression 01, ratio 10 and method ISO_10918_1
- **THEN** its label is "Lossy 10:1 (JPEG)"

#### Scenario: Lossy transfer syntax without attributes
- **WHEN** a file is stored as JPEG Baseline (1.2.840.10008.1.2.4.50) and records no lossy attributes
- **THEN** its label is "Lossy (JPEG)"

#### Scenario: JPEG 2000 that may be lossless
- **WHEN** a file is stored as JPEG 2000 (1.2.840.10008.1.2.4.91) without Lossy Image Compression
- **THEN** it is not labelled lossy

