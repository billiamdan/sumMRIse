## ADDED Requirements

### Requirement: Acquired-image display
The viewer SHALL display the acquired frames of the selected series with their physical
aspect ratio from Pixel Spacing, centred and fitted to the viewport.

#### Scenario: Anisotropic pixels
- **WHEN** a frame of 256 columns × 128 rows has Pixel Spacing 1.0 \ 0.5 (row spacing 1.0 mm, column spacing 0.5 mm)
- **THEN** it is displayed as a square (128 mm × 128 mm), not as a 2:1 rectangle

### Requirement: Orientation labels
Each viewport SHALL show patient-orientation letters at its four edges derived from the
image direction cosines (L/R, A/P, S/I; combined letters for oblique directions).

#### Scenario: Standard axial image
- **WHEN** Image Orientation (Patient) is 1\0\0\0\1\0
- **THEN** the edges read R (left), L (right), A (top), P (bottom)

#### Scenario: Sagittal acquisition
- **WHEN** Image Orientation (Patient) is 0\1\0\0\0\-1
- **THEN** the edges read A (left), P (right), S (top), I (bottom)

### Requirement: Navigation and interaction
Users SHALL change images with the mouse wheel or two-finger scroll, arrow keys,
Page Up/Down (±10), Home/End; zoom with pinch, ⌘/Ctrl-scroll or the zoom tool around the
pointer; pan with middle-drag, Shift-drag or the pan tool; and fit with double-click.

#### Scenario: Pinch zoom keeps the point under the fingers
- **WHEN** the user pinches to 200 % with the pointer over a lesion
- **THEN** the lesion stays under the pointer

### Requirement: Multiplanar reformatting
For series forming a volume of at least 3 slices the viewer SHALL offer axial, coronal
and sagittal reformats in radiological orientation independent of the acquisition plane;
otherwise the MPR layouts SHALL be disabled with the reason as tooltip.

#### Scenario: Marker in the left-anterior-superior octant
- **WHEN** a CT phantom has a bright marker at patient position (40, −35, 50) mm (LPS)
- **THEN** it appears on the screen's right and upper half in the axial and coronal views, and on the screen's left and upper half in the sagittal view

#### Scenario: Colour images
- **WHEN** an RGB secondary capture is selected
- **THEN** the MPR and 2 × 2 layout buttons are disabled with "colour images cannot be reformatted"

### Requirement: Linked crosshair
A click in any view SHALL set a shared patient-space cursor; the other reformats SHALL
move to it, the acquired stack SHALL show the nearest acquired slice, and each view
SHALL draw crosshair lines in the partner views' colours (axial red, coronal green,
sagittal yellow).

#### Scenario: Scrolling the acquired stack
- **WHEN** the user scrolls the acquired axial stack to image 4
- **THEN** the coronal and sagittal crosshair lines move to that slice's z position

### Requirement: Thick-slab projections
Reformatted views SHALL support MIP, MinIP and Average over a slab of 2–100 mm centred
on the current position.

#### Scenario: MIP reveals an out-of-plane structure
- **WHEN** a bright marker lies 6 mm outside the current axial plane and a 16 mm MIP slab is enabled
- **THEN** the marker becomes visible in the axial view

### Requirement: Annotations and read-out
Viewports SHALL show patient (unless privacy mode is on), study and series details,
image number/position, window and zoom, and a scale bar when pixel spacing is known;
annotations SHALL be toggled with O. The status bar SHALL show the hovered pixel,
its patient position in mm with R/L, A/P, S/I and its value with units.

#### Scenario: Hover over air in CT
- **WHEN** the pointer is over air outside the patient
- **THEN** the status bar shows a value of about −1000 HU
