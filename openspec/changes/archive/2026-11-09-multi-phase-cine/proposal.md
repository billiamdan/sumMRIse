## Why

Some series hold several sets of images at the same slice positions: the time points of a
dynamic scan, the phases of the heart cycle, two echoes, diffusion weightings (b = 0 and
b = 1000) or a derived map next to the originals. Today such a series is one long list in
which neighbouring images can be 20 time points apart, reformats are switched off for it
("several images share the same slice position") and nothing tells the user which set an
image belongs to. There is also no way to play images as a moving loop, which is how
dynamic and cardiac data, ultrasound clips and angiography runs are meant to be seen.

## What Changes

- A series whose slice positions repeat evenly is split into stacks of equal length, one
  per time point, heart phase, echo, diffusion weighting, image type, acquisition or
  sequence, decided from the headers before any pixel is read. Each stack is a normal
  set of slices: scrolling stays inside it, and reformats, slabs and the crosshair work
  again, on the stack shown.
- A control under the images switches the stack and says what distinguishes it ("Time
  point 3 / 20", "Echo 2 / 2 · TE 90 ms", "b = 1000 s/mm²", "Part 2 / 3" when nothing
  recorded tells the sets apart). Left and Right change the stack of such a series; Up
  and Down change the image as before. The annotations, the inspector and the
  plain-language guide name the stack; the series browser says that a series has stacks
  ("3 time points × 12 images").
- Stacks that differ in contrast (echoes, diffusion weightings, image types, unknown)
  keep a window each, starting from their own default; time points and heart phases share
  one window, so that a loop through them does not flicker.
- Cine: Play/Pause (Space) loops through the images of the active view, or through the
  time points or heart phases at the current position. The speed comes from the images
  when they record it (Recommended Display Frame Rate, Cine Rate, Frame Time) and can be
  set from 1 to 60 images per second; loop or back and forth. Playback waits for images
  that are still loading and stops when another series is selected.
- Series whose positions do not repeat evenly, and series whose images all lie at one
  position (a cine of one slice, ultrasound, angiography), keep today's list and
  explanation; cine plays them as they are.

Patient safety: the split is made only when every stack holds every slice position
exactly once and all stacks share one geometry; otherwise nothing changes. An image is
always shown with the name of the stack it belongs to, so a time point or an echo is never
mistaken for another. Stacks told apart by nothing recorded are called "Part n / k", never
given a meaning they do not have. Cine changes only which image is shown; positions,
orientation and values stay true. Nothing new is written to disk or to the settings, and
the stack labels hold no patient data.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `image-decoding`: adds the split of a series into stacks from the headers (discriminators in a fixed order, per-frame values of Enhanced objects) and per-stack volumes; volume assembly applies to each stack.
- `image-viewing`: adds the stack control, per-stack contrast, cine playback, and the stack named in annotations, browser and guide; Left/Right change the stack of a series with several.

## Impact

- `crates/dicom`: per-frame discriminator values read during header indexing; a new
  `stacks` module with the split rule; `SeriesPlan` and `LoadedSeries` carry the stacks;
  frame-rate attributes in `Acquisition`.
- `crates/core`: `SeriesData` with stack accessors and one volume per stack; `ViewState`
  with the current stack and per-stack windows; the session knows the stacks of every
  series for the browser; one statement in the guide.
- `crates/ui`: playback bar with the stack control and cine; keys; render, viewport,
  inspector rows and browser name the stack.
- `crates/testkit`: a study of seven series with repeated positions.
- No new dependency, no file-format or settings change; cine state is never saved.
