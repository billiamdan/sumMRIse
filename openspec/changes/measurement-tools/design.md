## Context

`ui/placement.rs` (`Placement`) is the only mapping between screen and picture, and it knows
display orientation (turn, mirror). `Displayed` (`ui/render.rs`) holds what a picture shows and
converts between pixels and patient coordinates, and the status bar reads the value under the
pointer. Tools are `WindowLevel`, `Pan` and `Zoom`. Acquired frames hold modality values
(`VoxelBuffer`), addressed among all image sets by `ViewState::slot_index`; a series records the
values that are padding (`Padding`). PET values are in SUV where they can be computed
(`SeriesData::units`). Reformatted images hold resampled values with NaN outside the volume, and
their planes can be tilted (`ViewState::plane_rotation`, `PlaneBasis`). The index stored one
`pixel_spacing` per image that silently fell back from Pixel Spacing to Imager Pixel Spacing, so
the origin of the scale was not known, and the scale bar showed detector millimetres without
saying so.

## Goals / Non-Goals

**Goals:**
- Correct, simple measurements with honest units.
- Identical results at any zoom, in any display orientation and under non-square pixels.
- Nothing persisted.

**Non-Goals:**
- Cobb angle, polygon or freehand regions, volumes, measurements across slices.
- Saving measurements as DICOM presentation states or structured reports.
- User calibration, ultrasound region calibration, Pixel Aspect Ratio: such images count as
  uncalibrated and are measured in pixels (see D4).
- Measurements on the 3D view: the tools are not offered there.

## Decisions

### D1 — Points live in patient space
A measurement stores its points in patient coordinates (mm, LPS) together with the picture it
belongs to; images without patient geometry store pixel coordinates (with their spacing if they
have one) and the image. Length is the Euclidean distance of the patient points, which equals
pixel deltas scaled by row and column spacing.

Alternative considered: storing screen or pixel coordinates only — rejected: wrong under
non-square pixels and unusable on another view of the same plane.

The code keeps the layers: the maths in `summrise_imaging::measure`, the model and the wording in
`summrise_core::measure`, the pointer and the drawing in `ui/measure.rs`. The UI speaks to the
model in fractional pixels of the picture, obtained from `Placement`; screen positions never
reach the model.

### D2 — Where a measurement is visible
A measurement drawn on an acquired image is shown on that image only, addressed by its index among
all images of the series, so an image set is simply other images. One drawn on a reformat belongs
to its plane: it is shown while the reformat has the same orientation (the same screen-right and
screen-down directions in patient space, within about 0.0001°), the same image set, and is within
half a step of the position where it was drawn. Tilting or moving away hides it; returning shows it
again. An edit made up to half a step beside the plane stays on the plane, so it cannot lengthen a
line by the distance between the two.

Alternative considered: keying a reformat's measurements by its view (axial, coronal, sagittal)
and slice number — rejected: slice numbers change when the planes are tilted, and a tilted view
keeps its name.

### D3 — Source of region statistics
On acquired images statistics use the stored modality values of the pixels whose centres lie inside
the shape, without interpolation. On reformats they use the displayed resampled values, and when a
slab projection is active the result carries the projection name (for example "MIP"). Padding and
values outside the volume are left out, and the tooltip says how many pixels counted and how many
were left out; a region with nothing but those says "outside the scanned area" rather than showing
a number. The unit is that of the series shown: on a CT with a PET drawn over it the statistics are
of the CT, in HU; on a PET in SUV where the series has them. A colour image has no statistics, and
the label says so.

The area is that of the shape, not a count of pixels: a rectangle of 40 × 20 pixels of 0.5 mm is
exactly 2.0 cm², although 41 × 21 pixel centres lie on or inside it. The standard deviation is the
population form: the values are all the pixels of the region, not a sample of them.

Statistics are computed whenever the picture is drawn, without a cache. A region as large as a
7.5-megapixel radiograph takes 25 ms in a development build, and egui draws on input only.
Alternative considered: caching them per measurement — rejected: reformat values change with the
slab, and a stale value is worse than a slower frame.

Alternative considered: always computing from original voxels — rejected for reformats: an oblique
region has no exact voxel set, and "statistics of what is displayed" is easier to explain.

### D4 — Calibration ladder
1. Pixel Spacing (0028,0030): millimetres.
2. Imager Pixel Spacing (0018,1164) only: millimetres "at detector — true size may be
   smaller".
3. Neither: pixels, "uncalibrated".

The index records which case applies (`InstanceHeader::calibration`) instead of merging both
attributes into one value without saying so. `pixel_spacing` keeps its meaning for display. The
scale bar uses the ladder too: on an image calibrated only at the detector it reads, for example,
"50 mm at detector"; images with neither have no scale bar.

A Pixel Spacing that is present but unusable (not positive) gives an uncalibrated image, as it gave
no spacing before; it is not replaced by the detector's scale. An image with both attributes counts
as Pixel Spacing, as D4 says, although some X-ray systems store the detector's value in both.

Pixel Aspect Ratio and ultrasound regions are out of scope: such images have no spacing for this
ladder, so they count as uncalibrated, in pixels, and are displayed with square pixels as before.

Alternative considered: adding the source to `FrameInfo` — rejected: it is per file, as
`Fidelity` is, and the viewer finds it through `FrameInfo::instance` as it does the lossy mark.

### D5 — Precision and approximation marks
Lengths and angles are shown to one decimal; areas in mm² below 100 mm², otherwise in cm². Pixels
are whole numbers. Measurements on reformats are prefixed with "≈" (every line of a region's label),
and their tooltip states the slice spacing, because resolution across slices is coarser than within
them. An angle on an uncalibrated image is marked too: it assumes square pixels.

### D6 — Interaction
Click-drag creates a length or a region; an angle takes three clicks (the corner second), and a
click that moves a little on its way still places its point. Handles have an 8-point hit radius and
lines a 5-point one; a handle moves with the pointer from where it was taken hold of, instead of
jumping to it. Rectangles have their four corners as handles, ellipses the ends of their axes. A
press inside a selected region moves it; a click inside any region selects it; a drag inside a
region that is not selected draws there, so that a length can be measured inside an ellipse. What
is drawn stays inside the picture, and a change that would leave nothing (coinciding points, no
extent) is not made. Nothing can be started on the placeholder of an image that is not decoded.

Esc cancels what is being drawn and returns what is being edited; otherwise it is left to others.
Delete or Backspace removes the selection, and the menu removes all of a series. A measurement tool
stays active until another tool is chosen; choosing another drops what is being drawn. Right-drag
keeps adjusting contrast, and middle-drag and Shift-drag keep panning. Double-click does not fit the
picture while a measuring tool is chosen, and a click does not move the crosshair. Playback stops
when a measurement is drawn, so that the picture stays put under it.

Measurements can be changed only while a measuring tool is chosen. Alternative considered: taking
hold of them with any tool — rejected: a stray contrast drag that starts near a line would change a
result without the user noticing.

The crosshair and its tilt handles are not drawn while a measuring tool is chosen in a reformat.
Alternative considered: leaving them, with the tilt handles on top — rejected: a line could not
start near a handle, and a stray drag would tilt the plane the measurement belongs to, which hides
it.

### D7 — Session-only storage and notice
Measurements are kept in the per-series view state and dropped when the study closes, so the privacy
specification is unchanged. A view with measurements is in use (`ViewState::is_pristine`); "reset"
(R) keeps them, since it says it resets contrast, zoom, pan, rotation and planes, and ends what is
being drawn. A one-time notice and a status-bar hint while a measurement tool is active say
"Approximate — not for diagnosis"; only the fact that the notice was shown is remembered
(`Settings::measure_notice_shown`). The hint stays on the right of the status bar, because the left
shows the read-out while the pointer is on a picture.

Alternative considered for the notice: a notification that fades — rejected: it can be missed, and
it only comes once.

### D8 — Per picture
`ui/measure.rs::show` takes one picture's `Placement` and what it displays, and is called from
`viewport::show` for every picture drawn. Which picture it is comes from `Displayed` (the image
index, or the plane), not from the view state, so a measurement on image 33 appears wherever image
33 is drawn: in a single view and in a tile of a grid. Labels are drawn upright at their screen
position, and are not annotations: hiding annotations (O) keeps them.

The measuring tools are `Tool::Measure(kind)`, with one menu button "Measure" after the Contrast,
Pan and Zoom tools, which shows the chosen tool while one is active. The 3D layout does not offer it.

## Risks / Trade-offs

- [False confidence in sizes] → notice, calibration label, one-decimal precision, tests
  against the phantom.
- [Coarse slice spacing makes reformat measurements imprecise] → "≈" prefix and slice
  spacing in the tooltip.
- [Magnification in projection X-ray images] → "at detector" note.
- [Where users click depends on the contrast window] → documented limitation; results
  themselves do not depend on the window.
- [Labels of measurements close together can overlap] → accepted; each stays readable on hover and
  to assistive technology, and a measurement can be moved.
