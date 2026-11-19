# measurements Specification

## Purpose
Length, angle and region measurements in the unit of the series, honest about their calibration and never saved, marked as approximate and not for diagnosis.
## Requirements
### Requirement: Length measurement
The viewer SHALL provide a length tool that measures the straight-line distance between
two points in millimetres, computed from the image's patient geometry and shown to one
decimal place.

#### Scenario: Known phantom size
- **WHEN** the user measures across the 20 mm marker sphere of the synthetic CT (pixel spacing 0.94 mm) on the slice through its centre
- **THEN** the label reads between 19.0 mm and 21.0 mm

#### Scenario: Non-square pixels
- **WHEN** an image has Pixel Spacing 1.0 \ 0.5 (row spacing 1.0 mm, column spacing 0.5 mm) and the user draws one line spanning 100 columns and another spanning 100 rows
- **THEN** the first reads 50.0 mm and the second 100.0 mm

#### Scenario: Zoom independence
- **WHEN** the same two anatomical points are measured at 100 % and at 400 % zoom
- **THEN** both labels show the same length

### Requirement: Angle measurement
The viewer SHALL provide an angle tool that reports the angle between two lines sharing
a vertex, from 0.0° to 180.0°, computed in patient space.

#### Scenario: Right angle on non-square pixels
- **WHEN** the user places the arms along the row and column directions of an image with Pixel Spacing 1.0 \ 0.5
- **THEN** the label reads 90.0°

### Requirement: Region statistics
The viewer SHALL provide ellipse and rectangle region tools that report the area and the
mean, standard deviation, minimum and maximum of the image values inside the region, in
the image's units and independent of the contrast window.

#### Scenario: Soft tissue and air in CT
- **WHEN** the user draws an ellipse inside the soft tissue of the synthetic CT (40 HU with ±6 HU noise) and another in the surrounding air
- **THEN** the first shows a mean between 37 and 43 HU and the second a mean of −1000 HU with a standard deviation of 0

#### Scenario: Area in physical units
- **WHEN** a rectangle of 40 × 20 pixels is drawn on an image with 0.5 mm square pixels
- **THEN** the area reads 2.0 cm²

#### Scenario: Colour image
- **WHEN** a region is drawn on an RGB image
- **THEN** the area is shown and the statistics are replaced by "not available for colour images"

#### Scenario: Padding and values outside a reformat
- **WHEN** a region of a CT holds pixels of −3024 HU marked as padding, or on a reformat lies partly outside the volume
- **THEN** the statistics leave those pixels out, the smallest value is that of the air at −1000 HU, and the tooltip says how many were left out; a region with nothing else reads "outside the scanned area"

#### Scenario: Unit of the series shown
- **WHEN** a region is drawn on a PET in SUV over its hot sphere of SUV 5, and on a CT with that PET drawn over it, on its marker of 300 HU
- **THEN** the first reads a mean of SUV 5 and the second a mean of 300 HU

#### Scenario: Slab projection
- **WHEN** a region is drawn on a reformat shown as a 10 mm MIP slab
- **THEN** its label carries "MIP" with the statistics of the projected values

### Requirement: Calibration transparency
Every measurement SHALL show its unit, and the viewer SHALL state where the scale comes
from: images without pixel spacing are measured in pixels and marked uncalibrated,
projection images calibrated only at the detector say so, and measurements on reformats
are marked approximate.

#### Scenario: Screenshot without spacing
- **WHEN** a length is drawn on a secondary-capture image that has no pixel spacing
- **THEN** the label reads in pixels, for example "123 px (uncalibrated)"

#### Scenario: Projection X-ray
- **WHEN** an image has Imager Pixel Spacing but no Pixel Spacing
- **THEN** the label is in millimetres with the note "at detector — true size may be smaller"

#### Scenario: Scale bar of a projection X-ray
- **WHEN** an image has Imager Pixel Spacing but no Pixel Spacing
- **THEN** its scale bar is labelled in millimetres at the detector, for example "50 mm at detector"; an image with neither attribute has no scale bar

#### Scenario: Reformat of thick slices
- **WHEN** a 40 mm line is drawn along the head–foot direction on a sagittal reformat of a CT with 5 mm slice spacing
- **THEN** the label reads "≈ 40.0 mm" and its tooltip states the 5 mm slice spacing

### Requirement: Editing measurements
Measurements SHALL be adjustable by dragging their handles, movable as a whole,
selectable, removable with Delete or Backspace, and removable all at once; they stay
attached to the image they were drawn on and follow zoom and pan.

#### Scenario: Scrolling away and back
- **WHEN** a length is drawn on image 31 and the user scrolls to image 32 and back
- **THEN** the measurement is hidden on image 32 and shown again, unchanged, on image 31

#### Scenario: Adjusting an endpoint
- **WHEN** the user drags one endpoint of a 20.0 mm line 5 mm further away along the line
- **THEN** the label updates to 25.0 mm while dragging

#### Scenario: Moving or tilting a reformat
- **WHEN** a length is drawn on a sagittal reformat of a CT whose slices are 5 mm apart, and the reformat moves along its normal or is tilted
- **THEN** the measurement is shown 2.4 mm beside the plane, hidden 2.6 mm beside it, hidden when tilted by 2° and shown again, unchanged, when returned

#### Scenario: Turned and mirrored views
- **WHEN** the same two points are measured on a picture turned by a quarter and mirrored
- **THEN** the length is the same as in the standard orientation, the line lies over the same anatomy and its label is upright

#### Scenario: Image sets
- **WHEN** a length is drawn on an image of one image set of a series and the user changes to another set at the same position
- **THEN** the measurement is hidden, and shown again on returning to the first set

### Requirement: Measurements in tile grids
In a tile grid a measurement SHALL belong to the image of the tile it was drawn on, not to
the current image, and SHALL be shown on the tile of that image wherever that tile is.

#### Scenario: Drawing on a later tile
- **WHEN** a 3 × 3 grid shows images 31 to 39 and the user draws a length on the tile of image 33
- **THEN** there is one measurement, on that tile; one step later it is on the second tile with image 33; when image 33 scrolls out of the grid it is no longer shown; and on image 33 alone it reads the same length

### Requirement: Approximate and session-only
The viewer SHALL present measurements as approximate and not for diagnosis, and SHALL
NOT store them on disk: they are discarded when the study is closed.

#### Scenario: First use
- **WHEN** the user selects a measurement tool for the first time
- **THEN** a notice explains that measurements are approximate and not for diagnosis

#### Scenario: Re-opening the study
- **WHEN** a study with measurements is closed and opened again
- **THEN** no measurements are present

### Requirement: Accessible measurement tools
Each measurement tool SHALL be reachable from the toolbar with an accessible name and a
single-key shortcut shown in its tooltip.

#### Scenario: Keyboard selection
- **WHEN** the user presses the shortcut shown for the length tool
- **THEN** the length tool becomes active and its toolbar button is shown as selected

#### Scenario: Not offered in the 3D view
- **WHEN** the 3D layout is shown
- **THEN** the measurement tools are not offered and their shortcuts do nothing

