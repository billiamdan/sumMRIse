# image-viewing Specification

## Purpose

Displaying and navigating images in their acquired plane and as multiplanar reformats (including thick-slab projections), with radiological orientation, linked crosshair, annotations and value read-outs.
## Requirements
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
Users SHALL change images with the mouse wheel or two-finger scroll, Up and Down (and
Left and Right, unless the series is split into stacks, where they change the stack),
Page Up/Down (±10), Home/End; zoom with pinch, ⌘/Ctrl-scroll or the zoom tool around the
pointer; pan with middle-drag, Shift-drag or the pan tool; fit with double-click; and play
or pause the images with Space.

#### Scenario: Pinch zoom keeps the point under the fingers
- **WHEN** the user pinches to 200 % with the pointer over a lesion
- **THEN** the lesion stays under the pointer

#### Scenario: Arrow keys of a series with stacks
- **WHEN** image 3 / 12 of echo 1 of a dual-echo series is shown and the user presses Down, then Right
- **THEN** image 4 / 12 of echo 1 is shown, then image 4 / 12 of echo 2

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

### Requirement: Loading placeholder
An image that is not loaded yet SHALL be shown as a loading placeholder with its image
number, never as a blank viewport or as another image.

#### Scenario: Scrolling into unloaded images
- **WHEN** the user scrolls to image 400 while only images 250 to 350 are loaded
- **THEN** the viewport shows "Loading image 400 / 600" and replaces it with the image as soon as it is decoded

#### Scenario: Image that failed to decode
- **WHEN** image 17 of a series cannot be decoded
- **THEN** its position shows "Image 17 could not be decoded" and the other images display normally

### Requirement: Reformats after complete loading
Reformats, slab projections and volume information SHALL become available when all
images of the series are loaded and form a volume; until then the reformat layouts are
disabled with the loading progress as the reason.

#### Scenario: Series still loading
- **WHEN** 300 of 600 images are loaded and the user hovers the MPR layout button
- **THEN** the button is disabled and its tooltip reads "Loading 300 / 600"

#### Scenario: Loading completes
- **WHEN** the last image of a regular 600-slice stack is decoded
- **THEN** the MPR and 2 × 2 layouts become available without re-selecting the series

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

### Requirement: Stack control
For a series split into stacks the viewer SHALL show one stack at a time, with a control
under the images that names the stack shown and switches to the previous, the next or any
other stack; Left and Right SHALL change the stack. Scrolling, Page Up/Down, Home and End
SHALL stay inside the stack, and image numbers SHALL count inside it. Changing the stack
SHALL keep the image number, the crosshair and the reformat planes. Every view of such a
series SHALL name the stack in its annotations and its accessible description.

#### Scenario: Next time point
- **WHEN** image 7 / 12 of time point 1 of a 3 × 12 dynamic series is shown (S 50 mm) and the user presses Right
- **THEN** the control reads "Time point 2 / 3", the viewport shows image 7 / 12 at S 50 mm of time point 2 and its annotations name "Time point 2 / 3"

#### Scenario: Scrolling stays in the stack
- **WHEN** the last image of time point 1 is shown and the user presses Down
- **THEN** image 12 / 12 of time point 1 is still shown, not image 1 of time point 2

#### Scenario: Series without stacks
- **WHEN** a CT series that is not split is shown and the user presses Right
- **THEN** the next image is shown, as before, and no stack control appears

### Requirement: Reformats of the current stack
Reformats, slab projections and the crosshair SHALL use the volume of the stack shown;
the crosshair position SHALL be shared by all stacks.

#### Scenario: Marker of the second time point
- **WHEN** the axial reformat of the 3 × 12 dynamic series is set to the marker at L 40, A 35, S 50 mm, whose value is 600, 900 and 1200 in time points 1, 2 and 3, and the user chooses "Time point 2 / 3"
- **THEN** the reformat reads 900 at the crosshair, and 1200 after one more Right, with the crosshair still at L 40, A 35, S 50 mm

### Requirement: Contrast per stack
Each stack SHALL keep its own window where the stacks differ in contrast (echoes,
diffusion weightings, image types, acquisitions, sequences, parts), starting from its own
default: the first stored window of the first decoded image of that stack, else an
automatic window from it. Presets, the full range and drag sensitivity SHALL follow the
stack shown. Time points and heart phases SHALL share one window.

#### Scenario: Two echoes with their own windows
- **WHEN** echo 1 (TE 15 ms) stores W 4000 / L 2000 and echo 2 (TE 90 ms) stores W 700 / L 350, the user sets W 500 / L 200 on echo 2 and returns to echo 1
- **THEN** echo 1 shows W 4000 / L 2000, and going back to echo 2 shows W 500 / L 200

#### Scenario: Time points share the window
- **WHEN** the user sets W 800 / L 400 on time point 1 of a dynamic series and chooses time point 3
- **THEN** time point 3 shows W 800 / L 400

#### Scenario: Reset
- **WHEN** the user presses R on echo 2 after changing the windows of both echoes
- **THEN** echo 2 shows W 700 / L 350 and echo 1 its W 4000 / L 2000 when chosen again, and echo 2 stays the stack shown

### Requirement: Stacks named in the browser and the guide
The series browser SHALL say for a series split into stacks how many stacks of how many
images it holds and what tells them apart, before the series is loaded; the inspector
SHALL show the stack of the displayed image; the plain-language guide SHALL say in one
statement what the stacks are, with its basis.

#### Scenario: Browser row of a dynamic series
- **WHEN** a study with a 3 × 12 dynamic series is opened
- **THEN** the browser row of that series reads "3 time points × 12 images"

#### Scenario: Guide statement
- **WHEN** the dynamic series is selected with explanations on
- **THEN** the card "About this series" says "This series holds 3 time points of the same slices." with the basis "Recorded in the images: temporal position."

### Requirement: Cine playback
The viewer SHALL play the images of the active view as a loop, or, for a series whose
stacks are time points or heart phases, the stacks at the current position. Space and a
Play/Pause button SHALL start and stop playback. The speed SHALL be the Recommended Display
Frame Rate, else the Cine Rate, else 1000 / Frame Time, else 10 images per second, and
SHALL be adjustable from 1 to 60 per second; playback SHALL run as a loop or back and
forth. Playback SHALL wait at an image that is still loading instead of skipping it, and
SHALL stop when another series is selected. Nothing about playback SHALL be saved.

#### Scenario: Frame time of a cine
- **WHEN** a series of 20 images of one slice records Frame Time 40 ms and the user presses Space
- **THEN** the images advance at 25 per second, from image 20 back to image 1

#### Scenario: Back and forth
- **WHEN** a series of 4 images is played back and forth from image 1
- **THEN** the images shown are 1, 2, 3, 4, 3, 2, 1, 2

#### Scenario: Through time at one position
- **WHEN** image 7 / 12 of a 3 × 12 dynamic series is shown and the user presses Space
- **THEN** time points 2, 3, 1, 2 … are shown at image 7 / 12 (S 50 mm), with one window

#### Scenario: An image still loading
- **WHEN** playback reaches an image that is not decoded yet
- **THEN** the image before it stays on screen, the bar says "Waiting for images to load", that image is loaded next, and playback goes on once it is decoded

#### Scenario: Another series
- **WHEN** the user selects another series during playback
- **THEN** playback stops

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

### Requirement: Annotations fit the view
Text annotations in opposite corners of a viewport SHALL NOT overlap, in any language:
where a block in a left corner and the block in the right corner of the same rows do not
fit side by side, the narrower one keeps its width if it takes at most half of the space
and the other is shortened with an ellipsis ("…"), else both are shortened to half of the
space. The scale bar SHALL be left out where it does not fit between the last two lines of
the bottom annotations.

#### Scenario: Narrow reformat view
- **WHEN** the coronal view of the reformat layout is about 120 points wide in the smallest window (960 × 600 points) and shows "Coronal 49 / 96" bottom left and "W 400  L 40 HU" bottom right
- **THEN** both lines are shortened with "…" so that they do not touch, and no scale bar is drawn between them

#### Scenario: Wide view
- **WHEN** the axial view of the reformat layout is about 470 points wide (window 1440 × 800 points)
- **THEN** every annotation is shown in full and the 50 mm scale bar is drawn

### Requirement: Bars fit narrow windows
The status bar SHALL keep its right-hand text (number of images, source, warnings) in full
and shorten the hint or read-out on its left with an ellipsis ("…", shown in full on hover)
where both do not fit; the playback bar under the views SHALL wrap onto further rows, each
label with its control, so that none of its controls leaves the width of the views.

#### Scenario: Playback bar in the smallest window
- **WHEN** a series with three time points is shown in a window of 960 × 800 points with the series list and the inspector open (the view is about 310 points wide)
- **THEN** the image-set control is on one row and Play, the speed and the two ways of playing are on the next, all within the width of the view

