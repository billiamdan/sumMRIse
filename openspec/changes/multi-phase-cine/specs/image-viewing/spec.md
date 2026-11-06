## ADDED Requirements

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

## MODIFIED Requirements

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
