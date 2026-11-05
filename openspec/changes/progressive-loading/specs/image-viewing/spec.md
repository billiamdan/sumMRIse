## ADDED Requirements

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
