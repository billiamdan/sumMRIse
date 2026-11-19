## Why

Selecting a series shows nothing until every image of it has been decoded, and the
series opened by default is the largest one on the disc. From an optical drive that can
take tens of seconds for a 600-slice CT (estimated from drive read speeds, not yet
measured). Professional viewers show images as they arrive.

## What Changes

- The first image of a series appears as soon as it is decoded; the remaining images
  become viewable as they arrive, in a display order known in advance from the headers.
- Loading concentrates on the images around the one being viewed and re-prioritises when
  the user scrolls elsewhere; an image that is not available yet shows a loading
  placeholder.
- Contrast, zoom and position stay stable while images arrive.
- Reformats, slab projections and volume information switch on when the series is
  complete; until then their controls show how far loading is.
- Switching to another series pauses loading; coming back resumes it.

Patient safety: the display order and slice positions are fixed from the headers before
any pixel data is read, so images never change place while loading, and an image that is
not loaded is shown as such, never as a neighbouring image.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `image-decoding`: adds progressive availability, load prioritisation, stable display parameters during loading, and pause and resume.
- `image-viewing`: adds the loading placeholder and the rule that reformats become available when loading completes.

## Impact

- `crates/dicom`: series loading splits into planning (order and volume geometry from
  headers) and per-file decoding.
- `crates/core`: incremental series store and prioritised loader in `Session`; memory
  accounting per image.
- `crates/ui`: placeholders, progress in the viewport and the series browser, reformat
  controls while loading.
- No file-format or settings changes.
