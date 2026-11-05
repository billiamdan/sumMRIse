## ADDED Requirements

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

### Requirement: Displayed series stay loaded
Series shown in any viewport SHALL NOT be evicted by the memory budget; when the
displayed series together exceed the budget the viewer SHALL say so and name the setting
that raises it.

#### Scenario: Budget smaller than two series
- **WHEN** two displayed series need 900 MB together and the memory limit is 512 MB
- **THEN** both remain displayed and a notice explains that the memory limit is exceeded and can be raised in Settings
