## ADDED Requirements

### Requirement: Scanner graphics display
Scanner graphics of an acquired image SHALL be drawn over it at render time in a
contrasting colour (cyan), on by default for every series, never on reformats, slabs or
the 3D view; the key G and a toggle "Scanner graphics" in the status bar (shown while the
series has scanner graphics) SHALL switch them off and on for the series; reset (R) SHALL
switch them on again; the pixel values under them SHALL stay unchanged.

#### Scenario: Graphics on by default
- **WHEN** an image whose overlay plane covers image column 20, row 10 is displayed
- **THEN** that pixel is drawn in the graphics colour and the read-out there shows the pixel's own value (500)

#### Scenario: Toggle with G
- **WHEN** the user presses G
- **THEN** the image is drawn without the graphics, the status-bar toggle "Scanner graphics" is off, and pressing G again draws them again

#### Scenario: Reformats
- **WHEN** a series with scanner graphics is shown in the MPR layout
- **THEN** the reformat views show no graphics

### Requirement: Padding display and read-out
Padding SHALL be drawn as background (black, also when the grey scale is inverted), and
the value read-out over padding, or over a point of a reformat that touches padding, SHALL
say "Outside the scanned area" instead of a value.

#### Scenario: Pointer over CT padding
- **WHEN** the pointer is over a padded corner of a CT slice whose padding would be −3024 HU
- **THEN** the status bar says "Outside the scanned area" and no HU value

#### Scenario: Inverted padded CT
- **WHEN** inversion is switched on for the padded CT slice
- **THEN** the tissue is inverted and the padded corners stay black

### Requirement: Lossy-compression mark
An image that is or was compressed with loss SHALL carry a badge with its label (for
example "Lossy 10:1 (JPEG)") in the badge row below the "Mirrored" row, whether
annotations are shown or not; reformat views SHALL carry it when any image of the series is
lossy; the inspector's "Lossy compression" row SHALL show the same label.

#### Scenario: Lossy image mirrored
- **WHEN** an image recorded as lossy 10:1 with ISO_10918_1 is shown and the user flips it horizontally
- **THEN** the badges "Mirrored" and "Lossy 10:1 (JPEG)" are both shown, one above the other, without covering each other

#### Scenario: Annotations hidden
- **WHEN** annotations are hidden (O) on a lossy image
- **THEN** the lossy badge stays

#### Scenario: Inspector
- **WHEN** the inspector shows the overview of an uncompressed image recorded as lossy 10:1 with ISO_10918_1
- **THEN** its "Lossy compression" row reads "Lossy 10:1 (JPEG)", the label of the badge
