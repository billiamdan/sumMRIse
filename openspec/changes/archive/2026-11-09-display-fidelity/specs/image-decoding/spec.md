## ADDED Requirements

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
