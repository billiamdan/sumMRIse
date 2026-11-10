## ADDED Requirements

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
