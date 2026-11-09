## ADDED Requirements

### Requirement: Stacks of images at the same positions
A series whose slice positions repeat evenly SHALL be split into stacks from its headers,
before any pixel data is read: a greyscale series whose frames all have geometry, one
orientation, one size and one pixel spacing, and whose P ≥ 2 distinct slice positions
each hold exactly k ≥ 2 frames, becomes k stacks of P images that each hold every
position exactly once. What tells the stacks apart SHALL be
taken from the first of these that partitions the frames so: temporal position, trigger
time, echo (number and time), diffusion b-value, image type, acquisition number, sequence
name; failing all of them, the order in which the frames occur at each position. The
stacks SHALL be ordered by that value (image type and sequence name: by first
occurrence) and SHALL share one geometry; a series that cannot be split so SHALL be
displayed as before.

#### Scenario: Time points of a dynamic scan
- **WHEN** an axial MR series holds 36 images with Temporal Position Identifier 1 to 3 at 12 positions from S 20 mm to S 75 mm, 5 mm apart
- **THEN** it is shown as 3 stacks labelled "Time point 1 / 3" to "Time point 3 / 3", image 1 of every stack lies at S 20 mm and image 12 at S 75 mm, and the stacks share one volume geometry of 48 × 48 × 12 voxels of 5 × 5 × 5 mm with its first voxel at R 120, A 120, S 20 mm

#### Scenario: Two echoes
- **WHEN** a series holds 24 images at 12 positions with Echo Number 1 and 2 and Echo Time 15 ms and 90 ms
- **THEN** it is shown as 2 stacks labelled "Echo 1 / 2 · TE 15 ms" and "Echo 2 / 2 · TE 90 ms"

#### Scenario: Diffusion weightings
- **WHEN** a series holds images with Diffusion b-value 0 and 1000 s/mm² at the same 12 positions, all with the same echo time
- **THEN** the echo time does not split them (one value) and the stacks are labelled "b = 0 s/mm²" and "b = 1000 s/mm²"

#### Scenario: Nothing recorded tells the sets apart
- **WHEN** a series holds 3 images at each of 12 positions and no discriminator differs between them
- **THEN** it is shown as 3 stacks labelled "Part 1 / 3" to "Part 3 / 3", taken in the order the images occur at each position

#### Scenario: One image missing
- **WHEN** one of the 36 images of a 3 × 12 dynamic series is missing from the disc
- **THEN** the series is not split: its 35 images are listed in instance order and reformatting is unavailable with the reason "several images share the same slice position (multi-echo, dynamic or repeated scan)"

#### Scenario: All images at one position
- **WHEN** a series holds 20 images of one slice (a cine of the heart)
- **THEN** it is not split and its images are listed in time order

### Requirement: Per-frame values of Enhanced objects
The values that tell stacks apart SHALL be read per frame while the headers are indexed:
for Enhanced multi-frame objects from the per-frame functional groups (Frame Content,
Cardiac Synchronization, MR Echo, MR Diffusion, Frame Type), else from the shared group,
else from the top-level attributes. The frames of one file MAY then belong to different
stacks.

#### Scenario: Enhanced MR object with two echoes
- **WHEN** one Enhanced MR file holds 12 frames at 6 positions, the frames of Effective Echo Time 30 ms and 100 ms interleaved
- **THEN** the series is shown as 2 stacks "Echo 1 / 2 · TE 30 ms" and "Echo 2 / 2 · TE 100 ms" of 6 images each, ordered by position, and the file is read once

## MODIFIED Requirements

### Requirement: Spatial ordering
Frames with patient geometry SHALL be ordered by position along the slice normal,
oriented so that the lowest Instance Number comes first. In a series split into stacks
this order SHALL apply inside every stack, in the direction found for the first stack,
so that image n of every stack lies at the same position; the first stack comes first.

#### Scenario: Files written in random order
- **WHEN** slice files are stored in shuffled order with Instance Number 1 at the most superior slice
- **THEN** image 1 is the most superior slice and positions decrease monotonically

#### Scenario: Stacks in the same direction
- **WHEN** the images of a 3 × 12 dynamic series are stored in shuffled order and Instance Number 1 is the slice at S 20 mm of the first time point
- **THEN** in every stack image 1 lies at S 20 mm and image 7 at S 50 mm

### Requirement: Volume assembly
A series, or each stack of a series split into stacks, SHALL become a volume only if its
frames share size, orientation and pixel spacing, have no duplicate positions and are
evenly spaced (deviation ≤ max(10 %, 0.05 mm)); otherwise the reason SHALL be reported
and only the acquired images shown. The stacks of a series SHALL share one geometry, and
an image that fails to decode in any stack SHALL make reformatting unavailable for the
whole series.

#### Scenario: Missing slice
- **WHEN** one file of an evenly spaced 2 mm stack is missing from the disc
- **THEN** reformatting is unavailable with the reason "slices are not evenly spaced … a slice may be missing"

#### Scenario: Gantry tilt
- **WHEN** a CT stack is acquired with 15° gantry tilt
- **THEN** a volume is built with the tilt represented as shear and the inspector shows "Gantry tilt 15.0° (corrected in reformats)"

#### Scenario: One volume per stack
- **WHEN** all 36 images of a 3 × 12 dynamic series are decoded
- **THEN** there are 3 volumes of 48 × 48 × 12 voxels with the same geometry, and the volume of the second stack holds the images of time point 2
