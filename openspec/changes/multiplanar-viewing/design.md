## Decisions

- **Canonical MPR bases (LPS):** axial right=+X, down=+Y; coronal right=+X, down=−Z;
  sagittal right=+Y, down=−Z. Hence axial/coronal show patient right on the screen's
  left; sagittal shows anterior on the left; superior is up on coronal/sagittal.
- **General resampling.** Output pixels map to patient space and through the inverse
  volume affine to fractional voxel indices, trilinear interpolation; NaN outside the
  volume (drawn black). This handles any acquisition orientation and shear exactly.
  Pixels are isotropic at the volume's smallest spacing (capped at 768 px per side).
- **Slice stepping** along a view normal uses the projection of the best-aligned voxel
  axis, so axial scrolling of an axial CT moves one acquired slice per step.
- **Crosshair** is a patient-space point shared by all views; the acquired stack index
  follows its projection onto the stack axis and vice versa.
- **Orientation letters** come from direction cosines: components ≥ 0.25 contribute a
  letter (L/R, P/A, S/I) ordered by magnitude, e.g. "RS" for oblique edges.
- **CPU rendering** with rayon; GPU compute and 3D volume rendering are deferred.

## Risks / Trade-offs

- [Thick slabs are slower] → cost grows with thickness/voxel size; acceptable at ≤100 mm.
- [Overlay clutter in small views] → in multi-view layouts patient/study text is shown
  only in the first view; each view has a coloured title.
