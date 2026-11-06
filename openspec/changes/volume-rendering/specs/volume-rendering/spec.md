## ADDED Requirements

### Requirement: 3D layout
The viewer SHALL offer a "3D" layout next to Images, MPR and 2 × 2 that shows the
selected series as one 3D view. It SHALL be available under the condition of the reformat
layouts (a complete series forming a volume of at least 3 slices); otherwise its button
SHALL be disabled with the same reason as tooltip. The choice SHALL be remembered like
the other layouts.

#### Scenario: Series still loading
- **WHEN** 3 of the 48 images of a CT are loaded and the user hovers the "3D" button
- **THEN** the button is disabled and its tooltip reads "Loading 3 / 48"

#### Scenario: Loading completes
- **WHEN** the last image of the CT is decoded and the user chooses "3D"
- **THEN** the 3D view of the CT is shown with the look "Bone" and "3D" is the remembered layout

#### Scenario: Colour images
- **WHEN** an RGB secondary capture is selected
- **THEN** the "3D" button is disabled and its tooltip contains "colour images cannot be reformatted"

### Requirement: Truthful orientation of the 3D view
The letters at the edges of the 3D view SHALL name the patient directions that point
left, right, up and down in the picture on screen, computed from the directions of
screen-right and screen-down of that picture's camera like the letters of every other
view. The first view SHALL look from the front with the head up, as the coronal view;
the 3D picture SHALL NOT be mirrored by any turn.

#### Scenario: First view
- **WHEN** a CT is first shown in 3D
- **THEN** the edges read R (left), L (right), S (top) and I (bottom)

#### Scenario: Marker on the patient's left and towards the head
- **WHEN** the CT phantom with a marker of 300 HU at L 40 mm, A 35 mm, S 50 mm is shown in the first view with the look MIP and the window W 400 / L 40 HU
- **THEN** the marker appears white, right of the centre of the picture and above it, at the pixel the camera gives for that position

#### Scenario: Half a turn about the vertical axis
- **WHEN** a volume with a marker 12 mm to the patient's left of its centre is turned by 180° about the vertical axis of the screen
- **THEN** the marker appears 12 mm left of the centre (within 0.6 mm), a marker 12 mm towards the head still 12 mm above it, and the edges read L (left), R (right), S (top), I (bottom)

#### Scenario: Gantry-tilted volume
- **WHEN** a stack acquired with 20° gantry tilt with a marker at L 8 mm, A 6 mm, S 10 mm is shown in the first view, turned by 35° to the right and 25° up, and from the patient's left
- **THEN** in each picture the marker appears within one pixel of the pixel the camera gives for its patient position

### Requirement: Turning, zooming and panning the 3D view
In the 3D view a drag with the primary button SHALL turn the object, whatever the tool,
so that its near side follows the pointer; the arrow keys SHALL turn it by 10° per press
(45° with Shift) as a drag in their direction does. Zoom (pinch, Command/Ctrl-scroll,
about the pointer) and pan (Shift-drag, middle-drag) SHALL work as in the other views,
double-click SHALL fit the picture again without changing the turn, and a right-drag
SHALL change the contrast window. The keys and the toolbar menu that turn and flip a
picture SHALL NOT act on the 3D view, and the arrow keys SHALL NOT change the image of
the other views.

#### Scenario: Dragging half a turn
- **WHEN** the user drags 360 points to the right across the first view
- **THEN** the object is seen from the back and the edges read L (left), R (right), S (top), I (bottom)

#### Scenario: Turning with the arrow keys
- **WHEN** the user presses Right nine times in the first view
- **THEN** the object is seen from the patient's right and the edges read P (left), A (right), S (top), I (bottom); two more presses of Shift+Right show it from the back (L, R, S, I)

#### Scenario: Turning up
- **WHEN** the user presses Up nine times in the first view
- **THEN** the object is seen from below the feet and the edges read R (left), L (right), A (top), P (bottom)

#### Scenario: Panning and fitting
- **WHEN** the user Shift-drags the 3D view by 40 points to the right and 30 points down and then double-clicks it
- **THEN** the picture first moves with the pointer, right and down, without turning and without a change of contrast, and the double-click centres it again with the turn kept

### Requirement: Standard views
The 3D view SHALL offer six standard views from a toolbar menu and with Shift and the
orientation letter of the side the view looks from: from the front (Shift+A), from the
back (Shift+P), from the patient's left (Shift+L), from the patient's right (Shift+R),
from above the head (Shift+S) and from below the feet (Shift+I). The general reset (R)
SHALL return to the first view and the first look; turning and choosing a look SHALL
count as changes made by the user.

#### Scenario: Letters of the standard views
- **WHEN** the user presses Shift+A, Shift+P, Shift+L, Shift+R, Shift+S and Shift+I in turn
- **THEN** the edges (left, right, top, bottom) read R, L, S, I; L, R, S, I; A, P, S, I; P, A, S, I; L, R, A, P; and R, L, A, P

#### Scenario: Reset
- **WHEN** the object is turned, the look is "Skin" and the zoom 200 %, and the user presses R
- **THEN** the view is from the front again (R, L, S, I), the look is "Bone", the zoom 100 %, and the view counts as unchanged

### Requirement: Looks
For a volume in Hounsfield units the 3D view SHALL offer the looks "Bone", "Skin",
"Vessels and bone" and "MIP"; for other volumes "Surface" and "MIP". The Hounsfield looks
SHALL use fixed value ranges (Bone from 300 HU, Skin from −600 HU, Vessels and bone from
150 HU with bone from 450 HU), whereas the opacity of "Surface" and the grey of "MIP"
SHALL follow the contrast window. L SHALL choose the next look, and the look SHALL be
chosen in the toolbar menu that also holds the standard views.

#### Scenario: Bone
- **WHEN** the CT phantom is shown with the look "Bone"
- **THEN** from the front the ray through the bone cylinder of 700 HU is opaque (opacity at least 250 of 255) and ivory, a ray through soft tissue of 40 HU and lung of −820 HU only is transparent, and from the side the ray through the contrast-filled vessel of 300 HU is transparent

#### Scenario: Skin
- **WHEN** the CT phantom is shown with the look "Skin"
- **THEN** rays through the body, which is 200 mm wide, are opaque and skin-coloured, and rays 110 mm to the left or right of the centre are transparent

#### Scenario: Vessels and bone
- **WHEN** the CT phantom is shown from the patient's left with the look "Vessels and bone"
- **THEN** the vessel of 300 HU is red (green below half of red) and the bone cylinder of 700 HU is clearly less red

#### Scenario: MIP through the window
- **WHEN** a volume with a marker of 600 is shown with the look "MIP"
- **THEN** every pixel shows the largest sample along its ray (as found by sampling the whole ray, within 0.05), the marker is grey 153 of 255 in a window from 0 to 1000, white in a window from 0 to 600, and grey 102 when inverted

#### Scenario: MR
- **WHEN** an MR series without units is shown in 3D
- **THEN** the looks on offer are "Surface" and "MIP", it opens with "Surface", and choosing the stored window W 1000 / L 900 instead of W 1400 / L 700 shows the same view computed with the new window

#### Scenario: Surface follows the window
- **WHEN** a volume with a body of signal 320 and a marker of 1500 is shown with the look "Surface"
- **THEN** with a window from 0 to 400 the body is opaque, and with a window from 1000 to 2000 the body is transparent and only the marker is opaque

### Requirement: Quick and full pictures
The 3D view SHALL show a quick picture of the current view at once whenever the object
is turned, zoomed or panned or its look or contrast is changed, and SHALL compute the
full picture — the viewport's size, at most 1024 pixels on the longest side — in the
background once the view stays the same for a frame. A full picture SHALL replace the
quick one only if it is the one still wanted; a full picture that is no longer wanted
SHALL be cancelled and never shown. The interface SHALL never wait for a full picture.

#### Scenario: Dragging
- **WHEN** the user drags the object over 24 frames
- **THEN** each frame shows a quick picture of the turn of that frame, and no full picture is started while the pointer moves

#### Scenario: Resting
- **WHEN** the pointer rests after the drag
- **THEN** the full picture of the same turn replaces the quick picture

#### Scenario: Stale full picture
- **WHEN** the full picture of one turn is in progress, the user turns further, and that full picture arrives after the quick picture of the new turn
- **THEN** it is not shown, and the full picture of the new turn follows

#### Scenario: Size of the full picture
- **WHEN** the viewport measures 2048 × 1536 physical pixels
- **THEN** the full picture is 1024 × 768 pixels

### Requirement: Illustration and the guide
The 3D view SHALL carry the caption "Illustration: colours and surfaces are computed",
also when annotations are hidden, and SHALL show no value read-out, crosshair, scale bar
or patient details. Its annotations SHALL name the look of the picture on screen. The
inspector's "Where is this image?" SHALL describe the side from which the body is seen in
the 3D view, title it "3D view", and mark no image in its figure.

#### Scenario: Guide for the first view
- **WHEN** the chest CT is shown in 3D from the front
- **THEN** the guide reads "3D view", "You are looking from the front of the body towards the back." and "The patient's right side is on the left of the screen.", and the body outline is described as "Schematic body outline: the examined region is highlighted."

#### Scenario: Guide from the back
- **WHEN** the object is turned to be seen from the back
- **THEN** the guide reads "You are looking from the back of the body towards the front." and "The patient's left side is on the left of the screen."

#### Scenario: Annotations hidden
- **WHEN** the user hides the annotations with O in the 3D view
- **THEN** the edge letters and the look are hidden and the caption is still shown

### Requirement: Renderer API
`summrise_imaging::render3d::render` SHALL render a prepared volume through an
orthographic camera, in a style (look, window, inversion), into a premultiplied RGBA
picture of the requested size, the same bytes for the same request whatever the number of
threads, sampling the volume's own slices without copying them. Empty-space skipping
SHALL NOT change the picture and early ray termination SHALL change no channel by more
than one grey level. A render whose cancellation turns true SHALL return no picture.

#### Scenario: Deterministic
- **WHEN** the same request is rendered twice, and once more in a thread pool of one thread
- **THEN** the three pictures are identical byte for byte

#### Scenario: Shortcuts
- **WHEN** the CT phantom (all CT looks) and an MR-like volume (Surface, MIP) are rendered from the front, turned by 35° and 20°, and from above, as previews and full pictures, with and without the shortcuts
- **THEN** with empty-space skipping the pictures are identical to those of the plain ray caster, and with early termination as well they differ by at most one grey level per channel

#### Scenario: No copy
- **WHEN** a volume is prepared for rendering
- **THEN** the prepared volume holds the very same slice buffers as the volume, plus a grid of the smallest and largest value of each block of 8 × 8 × 8 voxels

#### Scenario: Cancelled
- **WHEN** a render is asked for with a cancellation that is already true
- **THEN** no picture is returned
