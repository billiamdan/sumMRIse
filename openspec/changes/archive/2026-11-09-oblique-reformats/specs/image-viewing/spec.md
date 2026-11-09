## ADDED Requirements

### Requirement: Tilted reformat planes
In the reformat layouts the user SHALL be able to tilt the three reformat planes together
about the crosshair. The planes stay mutually perpendicular, each passes through the
crosshair, and tilting never moves the crosshair. Tilting in one view turns the two other
planes about that view's normal and leaves that view's own picture unchanged; tilting in
two views gives a double-oblique plane. "Reset planes" and the general reset SHALL return
the views to axial, coronal and sagittal.

#### Scenario: Tilting in the axial view
- **WHEN** the crosshair is on a marker of 300 HU at L 40 mm, A 35 mm, S 50 mm and the planes are turned by 30° in the axial view
- **THEN** the axial picture is unchanged, the coronal and sagittal views show planes turned by 30° about the superior–inferior axis, the crosshair is still at L 40 mm, A 35 mm, S 50 mm, and all three views show 300 HU at the crosshair

#### Scenario: Double-oblique plane
- **WHEN** the planes are turned by 30° counter-clockwise in the axial view and then by 25° clockwise in the coronal view
- **THEN** the axial view shows a plane tilted by 25° from axial, the coronal view is still tilted by 30°, and the three planes are still mutually perpendicular

#### Scenario: Reset planes
- **WHEN** the user chooses "Reset planes" after tilting
- **THEN** the views are titled AXIAL, CORONAL and SAGITTAL again, their edges read R, L, A, P (axial), R, L, S, I (coronal) and A, P, S, I (sagittal) for left, right, top and bottom, and the crosshair has not moved

### Requirement: Reslicing on a tilted plane
`summrise_imaging::reslice_plane` SHALL resample a volume on a plane given by its
directions of screen-right and screen-down and its normal in patient space. The picture
covers the bounding box of the eight volume corners projected on those directions; slice
positions and slab projections run along the normal. For the three standard planes the
result SHALL be bit-identical to `reslice`.

#### Scenario: Standard planes are unchanged
- **WHEN** a volume is resliced with the standard orientation of the axial, coronal or sagittal plane
- **THEN** size, pixel spacing, origin and every value are bit-identical to the result of `reslice` for that plane, with and without a slab

#### Scenario: Marker on a plane turned by 45°
- **WHEN** a volume centred on the patient origin has a bright marker at L 6 mm, A 6 mm, S 6 mm and is resliced through the origin on the coronal plane turned by 45° about the superior–inferior axis, so that screen-right points to the patient's left and anterior
- **THEN** the marker lies in the plane and appears 8.5 mm to the right of the centre of the picture and 6 mm above it

#### Scenario: Extent of a tilted plane
- **WHEN** a volume of 20 × 20 × 20 voxels of 1 mm is resliced on the coronal plane turned by 45° about the superior axis
- **THEN** the picture is 27 pixels wide (19 mm × √2 = 26.9 mm) and 20 pixels high

#### Scenario: Slab along the tilted normal
- **WHEN** the marker lies 6 mm from a tilted plane along its normal and a 16 mm MIP slab is enabled
- **THEN** the marker becomes visible at the pixel where the normal through the marker meets the plane

#### Scenario: Gantry-tilted volume
- **WHEN** a stack acquired with 20° gantry tilt is resliced on a double-oblique plane through a marker at L 6 mm, A 6 mm, S 6 mm
- **THEN** the marker appears within one pixel of the pixel computed from its patient position

### Requirement: Plane lines and tilt handles
Each reformat view SHALL draw, through the crosshair, the lines in which the two other
planes cut its own plane, in the colours of those views (axial red, coronal green,
sagittal yellow). The two lines are perpendicular on screen. Each line SHALL carry
handles; dragging a handle SHALL turn the two other planes about this view's normal so
that the line follows the pointer. Clicking SHALL still move the crosshair, and
scrolling SHALL move it along this view's own normal.

#### Scenario: Lines after tilting in the axial view
- **WHEN** a handle in the axial view is dragged by 30° clockwise around the crosshair
- **THEN** the coronal (green) line in the axial view runs from upper left to lower right at 30° to the horizontal, the sagittal (yellow) line is perpendicular to it, and the lines in the coronal and sagittal views are still horizontal and vertical

#### Scenario: Scrolling a tilted view
- **WHEN** the coronal view of a volume with 5 × 5 × 8.33 mm voxels is tilted by 30° about the superior–inferior axis, the crosshair is on one of its slices and the user scrolls it by one step
- **THEN** the crosshair moves by 4.33 mm (the largest projection of a voxel edge on the tilted normal) along that normal to the next slice, and not along the patient's anterior–posterior axis

### Requirement: Truthful labelling of tilted views
A reformat view whose plane is tilted SHALL be titled "OBLIQUE" with its tilt angle and
the standard plane it is nearest to, SHALL NOT carry the name of a standard plane alone,
and its edge letters SHALL be computed from the patient directions actually shown on
screen. The title of a reformat view SHALL stay visible when annotations are hidden.

#### Scenario: Coronal and sagittal views tilted by 30°
- **WHEN** the planes are turned by 30° counter-clockwise in the axial view
- **THEN** the coronal view is titled "OBLIQUE 30.0° · NEAR CORONAL" and its edges read RP (left), LA (right), S (top), I (bottom); the sagittal view is titled "OBLIQUE 30.0° · NEAR SAGITTAL" and its edges read AR (left), PL (right), S (top), I (bottom); the axial view is still titled "AXIAL" with R, L, A, P

#### Scenario: Small tilt
- **WHEN** the coronal view is tilted by 8°
- **THEN** its title reads "OBLIQUE 8.0° · NEAR CORONAL" although its edge letters still read R, L, S, I (a direction gets a second letter only from about 15°)

### Requirement: Rotating and flipping the display
The display of any view (acquired images and reformats) SHALL be turnable left and right
in 90° steps and flippable horizontally and vertically, per view and per series. The
picture, the edge letters, the crosshair lines, the value and position under the pointer,
clicking, zooming about the pointer and the scale bar SHALL stay consistent with each
other in all eight orientations, including images whose pixels are not square. The
orientation SHALL NOT be saved; reset SHALL restore the standard orientation, and
double-click SHALL fit the picture without changing its orientation.

#### Scenario: One right turn of an axial image
- **WHEN** an axial image whose edges read R (left), L (right), A (top), P (bottom) is turned right once
- **THEN** its edges read P (left), A (right), R (top), L (bottom)

#### Scenario: Horizontal flip of an axial image
- **WHEN** the same image is flipped horizontally instead
- **THEN** its edges read L (left), R (right), A (top), P (bottom) and the badge "Mirrored" is shown

#### Scenario: Pointer after a turn
- **WHEN** a CT image with a marker of 300 HU at L 40 mm, A 35 mm is turned right once and the pointer is moved onto the marker, which is now in the lower right quarter
- **THEN** the status bar shows 300 HU and a position near L 40 mm, A 35 mm

#### Scenario: Pixels that are not square
- **WHEN** a frame of 200 columns × 100 rows with Pixel Spacing 2.0 \ 0.5 (100 mm wide, 200 mm high) is turned right once
- **THEN** it is displayed 200 mm wide and 100 mm high, the pixel of column 0 and row 0 is at the top right corner, and one step along a row on the image is 0.5 mm downwards on screen

#### Scenario: Another series
- **WHEN** a view of one series is turned and flipped and the user selects another series
- **THEN** the other series is shown in the standard orientation, without a badge

### Requirement: Mirrored badge
While a view is mirrored (after an odd number of flips) the badge "Mirrored" SHALL be
shown on its picture, whether annotations are shown or hidden, and SHALL disappear as
soon as the picture is no longer mirrored.

#### Scenario: Annotations hidden
- **WHEN** an axial image is flipped vertically, so that its edges read R (left), L (right), P (top), A (bottom), and annotations are switched off with O
- **THEN** the badge "Mirrored" is still shown

#### Scenario: Two flips are a half turn
- **WHEN** an axial image is flipped horizontally and then vertically
- **THEN** its edges read L (left), R (right), P (top), A (bottom), which is a turn by 180°, and no badge is shown

### Requirement: Keys for orientation and tilt
The following keys SHALL act on the active view: `]` turns it right by 90° and `[` left;
`H` flips it horizontally and `V` vertically. With a reformat view active, Alt (Option on
macOS) with Right or Left SHALL tilt the planes by 1° clockwise or counter-clockwise on
screen about that view's normal, and by 5° with Shift; Alt with R SHALL reset the planes
only. `R` SHALL, besides contrast, zoom and pan, restore the planes and the standard
orientation of every view of the series. The same actions SHALL be available from the
toolbar menu "Rotate and flip", which names the view it acts on when several are shown.

#### Scenario: Tilting by keyboard
- **WHEN** the axial reformat is active and the user presses Alt+Right three times and Alt+Shift+Right once
- **THEN** the planes are turned by 8° clockwise in the axial view and the coronal view is titled "OBLIQUE 8.0° · NEAR CORONAL"

#### Scenario: Resetting the planes only
- **WHEN** the axial view is turned right once, the planes are tilted by 8° and the user presses Alt+R
- **THEN** the views are axial, coronal and sagittal again and the axial view is still turned right once (edges P, A, R, L for left, right, top, bottom)

#### Scenario: General reset
- **WHEN** the user then presses R
- **THEN** the axial view is back in the standard orientation with edges R, L, A, P
