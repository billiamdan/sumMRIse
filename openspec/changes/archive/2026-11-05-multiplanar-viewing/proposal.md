## Why

The user asked to see "all the possible projections in the data". The old viewer showed
each series as a 400×500 thumbnail with ◀/▶ buttons, ignored pixel spacing (wrong
aspect ratio), had no orientation labels, zoom, pan or keyboard navigation, and could
not reconstruct other planes. Patients need to browse their scans the way their images
were acquired *and* in the other anatomical planes, with unambiguous orientation.

## What Changes

- Acquired-image ("stack") view with correct physical aspect ratio, orientation letters
  at all four edges, slice navigation by wheel/trackpad/keys, zoom, pan, fit.
- Multiplanar reformatting (axial, coronal, sagittal) for regular volumes, resampled on
  canonical patient axes with radiological display conventions, whatever the acquisition
  plane (including oblique and gantry-tilted stacks).
- Linked crosshair across views; acquired stack and reformats stay in sync.
- Thick-slab projections: MIP, MinIP and average, 2–100 mm.
- Layouts: Images, MPR (three views) and 2 × 2 (acquired + three reformats).
- Radiological overlays (patient/study/series, image position, window, zoom, scale bar),
  hover read-out of pixel, patient position and value.

## Capabilities

### New Capabilities
- `image-viewing`: displaying, navigating and reformatting images with orientation-correct overlays.

### Modified Capabilities
<!-- None. -->

## Impact

`crates/imaging/src/{geometry,mpr}.rs`, `crates/core/src/view.rs`,
`crates/ui/src/{viewer,viewport,render}.rs`. **Patient safety:** orientation labels and
reformat directions are covered by unit and integration tests with an asymmetric
phantom.
