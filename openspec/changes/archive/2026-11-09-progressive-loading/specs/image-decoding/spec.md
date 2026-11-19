## ADDED Requirements

### Requirement: Progressive series loading
When a series is selected, its first image SHALL be displayed as soon as that image is
decoded, and the remaining images SHALL become viewable as they are decoded, without
waiting for the whole series.

#### Scenario: Large series from slow media
- **WHEN** a 600-image CT series is selected
- **THEN** the image at the initial position is displayed after one image has been decoded, and the viewport shows the loading progress, for example "Loading 1 / 600"

#### Scenario: Order fixed in advance
- **WHEN** the images of a series arrive in any order
- **THEN** the image counter shows the final total from the start (for example "Image 300 / 600"), and an image already displayed never changes its number or its slice position

### Requirement: Load prioritisation
Loading SHALL favour the images nearest to the one being displayed and SHALL
re-prioritise when the user moves to an image that is not loaded yet.

#### Scenario: Jumping ahead
- **WHEN** images 280 to 320 are loaded and the user jumps to image 550
- **THEN** image 550 and its neighbours are decoded before the remaining images between 321 and 549

### Requirement: Stable display while loading
The viewer SHALL NOT change the contrast window, zoom, pan or position automatically
while further images arrive: the window chosen when the first image appears and
everything the user sets are kept.

#### Scenario: Window adjusted during loading
- **WHEN** the user sets W 1500 / L −600 while a CT series is still loading
- **THEN** the window is still W 1500 / L −600 when loading completes

### Requirement: Pause and resume
Selecting another series SHALL pause loading of the previous one promptly, and
re-selecting a partly loaded series SHALL resume it without decoding its loaded images
again, unless the series was evicted by the memory budget.

#### Scenario: Switching back
- **WHEN** a series is left with 200 of 600 images loaded and is selected again later
- **THEN** the 200 images are available immediately and loading continues with the remaining 400
