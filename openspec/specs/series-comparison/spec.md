# series-comparison Specification

## Purpose
Several series side by side with linked scrolling, zoom and pan by patient position, cross-reference lines, study comparison and tile grids.
## Requirements
### Requirement: Multi-series layouts
The viewer SHALL offer layouts with one, two (side by side or stacked) and four
viewports, each showing a series the user assigns by dragging it from the series browser
or by selecting the viewport and then the series.

#### Scenario: Two MR series side by side
- **WHEN** the user picks the side-by-side layout and drags "T1 SAG" onto the left viewport and "T2 SAG" onto the right
- **THEN** both series are displayed, each with its own image counter, orientation letters and contrast window

#### Scenario: Empty viewport
- **WHEN** a layout has more viewports than assigned series
- **THEN** each unassigned viewport shows "Drop a series here" and accepts a dropped series

### Requirement: Active viewport
Exactly one viewport SHALL be active; toolbar actions, keyboard navigation and the
inspector SHALL apply to the active viewport's series, and the active viewport SHALL be
visibly highlighted.

#### Scenario: Switching the active viewport
- **WHEN** the user clicks the right viewport showing "T2 SAG"
- **THEN** it gets the highlight border, the inspector shows the metadata of the displayed T2 image, and the arrow keys scroll the T2 series

### Requirement: Viewport identification
In layouts showing more than one series, every viewport SHALL always display its series
description and study date, even when other annotations are hidden.

#### Scenario: Annotations switched off
- **WHEN** annotations are hidden while two studies are shown side by side
- **THEN** each viewport still shows its series description and study date, for example "T2 TSE SAG · 2025-03-02" and "T2 TSE SAG · 2026-01-15"

### Requirement: Synchronised scrolling by position
Linked viewports SHALL scroll together when their series share a Frame of Reference UID
and their slice directions differ by less than 30°: each shows the slice nearest to the
anatomical position of the active viewport's slice. Linking SHALL be on by default for
such series and switchable per viewport.

#### Scenario: Different slice spacing
- **WHEN** an axial series with slices every 5 mm is scrolled to its slice at z = −40 mm and a linked axial series has slices every 2.5 mm
- **THEN** the linked viewport shows its slice at z = −40 mm, and one further step of the first series to z = −45 mm moves the linked viewport two slices to z = −45 mm

#### Scenario: Position outside the other series
- **WHEN** the active series is scrolled beyond the coverage of a linked series
- **THEN** the linked viewport stays on its nearest end slice and shows "outside range"

#### Scenario: Unlinking
- **WHEN** the user switches the link off for a viewport
- **THEN** scrolling the active viewport no longer moves it

### Requirement: Linked zoom and pan
Viewports linked to the active viewport SHALL follow its zoom and pan: a zoom multiplies
the zoom of each linked viewport by the same factor and a pan moves each by the same
distance on screen. Turn, mirror and contrast SHALL NOT be linked, and a viewport whose
link is off SHALL be left alone.

#### Scenario: Zooming two linked series
- **WHEN** two linked viewports show axial series and the user zooms the left one by a factor of 1.5 and pans it 40 points to the right
- **THEN** the right viewport's zoom also grows by a factor of 1.5 and its picture moves 40 points to the right

#### Scenario: Link switched off
- **WHEN** the link of the right viewport is off and the user zooms the left one
- **THEN** the right viewport keeps its zoom and pan

### Requirement: Manual synchronisation
For series without a common frame of reference the viewer SHALL offer "link from here",
which keeps the offset between the viewports at the moment of linking and moves them by
equal distances afterwards.

#### Scenario: Two studies aligned by hand
- **WHEN** the user scrolls both viewports to the same vertebra, chooses "link from here" and scrolls the active viewport by 10 mm
- **THEN** the other viewport moves to its slice nearest to 10 mm in the same direction and its link indicator reads "manual"

### Requirement: Cross-reference lines
Each viewport SHALL draw the line along which the active viewport's current slice plane
intersects its own displayed image, for series in the same frame of reference whose
planes are not parallel; the lines SHALL follow scrolling and be hidden together with
annotations.

#### Scenario: Axial slice on a sagittal series
- **WHEN** the active viewport shows the axial slice at z = 12 mm and another viewport shows a sagittal series of the same examination
- **THEN** the sagittal viewport shows a horizontal line at the image row corresponding to z = 12 mm, which moves up when the axial view is scrolled towards the head

#### Scenario: Parallel planes
- **WHEN** both viewports show axial series
- **THEN** no cross-reference line is drawn

### Requirement: Compare studies
When the opened media contain two or more studies of the same patient, the viewer SHALL
offer "Compare studies", which shows matching series of two studies side by side with
the earlier study on the left.

#### Scenario: Follow-up MR on the same disc
- **WHEN** the disc holds MR studies dated 2025-03-02 and 2026-01-15 that both contain a sagittal T2 series and the user chooses "Compare studies"
- **THEN** the two sagittal T2 series open side by side, the 2025 study on the left, each labelled with its study date

#### Scenario: No matching series
- **WHEN** the two studies have no series with the same modality and plane
- **THEN** both viewports open with each study's default series and the user can drop other series onto them

### Requirement: Tile layout
A viewport showing acquired images SHALL be able to show consecutive images at once as a
grid of 2 × 2, 3 × 3, 4 × 4 or 5 × 5 tiles, with the current image in the first tile (top
left) and the following images of the same image set in reading order. Every tile SHALL
show its image number, and all tiles SHALL share the contrast window, inversion, zoom, pan
and display orientation of the viewport. One scroll step or arrow key SHALL move the grid
by one image, and Page Up and Page Down by a whole grid. Reformats and the 3D view SHALL
NOT offer tiles, and a grid whose tiles would be narrower or lower than 96 points SHALL NOT
be offered. The choice SHALL NOT be saved.

#### Scenario: Nine tiles of a CT series
- **WHEN** the 3 × 3 grid is chosen while image 31 of a 60-image CT series is shown
- **THEN** the tiles show images 31 to 39 in reading order, each labelled with its number; one scroll step shows 32 to 40, and Page Down from there shows 41 to 49

#### Scenario: End of the series
- **WHEN** the 3 × 3 grid reaches image 58 of 60
- **THEN** the first three tiles show images 58, 59 and 60 and the other six stay empty, and the grid can be scrolled no further than image 60 in the first tile

#### Scenario: Opening one tile
- **WHEN** the user double-clicks the tile of image 35
- **THEN** the viewport shows one image again, image 35

#### Scenario: Value under the pointer
- **WHEN** the pointer rests on the tile of image 33 over a point at 40 HU
- **THEN** the status bar gives the pixel position, the patient position and the value of image 33

#### Scenario: Not while the tiles would be too small
- **WHEN** a viewport is 320 points wide
- **THEN** 2 × 2 and 3 × 3 are offered, and 4 × 4 (80 points per tile) and 5 × 5 are not

### Requirement: PET overlay in a multi-series layout
In a multi-series layout a PET SHALL be drawn over the active viewport's series only. A
viewport beside it whose series has a PET overlay set SHALL show its images alone and SHALL
say so, and the overlay SHALL come back when that viewport becomes active. The layouts of one
series SHALL keep their PET overlay as before.

#### Scenario: Another viewport becomes active
- **WHEN** a CT with a PET overlay at 50 % opacity and a PET series are side by side and the user clicks the PET viewport
- **THEN** the CT shows its images alone with the badge "PET overlay only in the active viewport", and clicking the CT again draws the overlay at 50 %

### Requirement: Displayed series stay loaded
Series shown in any viewport SHALL NOT be evicted by the memory budget; when the
displayed series together exceed the budget the viewer SHALL say so and name the setting
that raises it.

#### Scenario: Budget smaller than two series
- **WHEN** two displayed series need 900 MB together and the memory limit is 512 MB
- **THEN** both remain displayed and a notice explains that the memory limit is exceeded and can be raised in Settings

