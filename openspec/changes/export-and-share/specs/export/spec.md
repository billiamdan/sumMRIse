## ADDED Requirements

### Requirement: Save the current view as an image
The viewer SHALL save the active view as a PNG or JPEG file through the system save
dialog, either as "image only" — the picture at its native resolution with the current
contrast window and no text — or "with annotations" — as displayed.

#### Scenario: Image-only export of a CT slice
- **WHEN** a 512 × 512 CT slice displayed with the Lung window (W 1500 / L −600) is saved as an "image only" PNG
- **THEN** the file is a 512 × 512 8-bit greyscale image in which −600 HU is mid-grey (127 or 128), −1350 HU and below are black, and +150 HU and above are white

#### Scenario: Non-square pixels
- **WHEN** an image of 256 columns × 128 rows with Pixel Spacing 1.0 \ 0.5 is saved as "image only"
- **THEN** the file is 256 × 256 pixels and shows the anatomy with correct proportions

#### Scenario: Export with annotations
- **WHEN** the user saves "with annotations" while zoomed to 200 %
- **THEN** the file shows the view as displayed, including orientation letters, scale bar and window values

### Requirement: Patient details excluded by default
Exports SHALL NOT contain the patient's name, ID or birth date — in the picture, in the
suggested file name or in file metadata — unless the user enables "Include patient
details" for that export; the choice is not remembered.

#### Scenario: Default export with annotations
- **WHEN** the user saves "with annotations" without changing any option
- **THEN** the picture shows the study and series texts but no patient name, ID or birth date, and the suggested file name has the form `SumMRIse-CT-S1-031.png`

#### Scenario: Details switched on once
- **WHEN** the user ticks "Include patient details" for one export and later opens the export dialog again
- **THEN** the first file contains the patient lines and the option is unticked again

### Requirement: Burned-in text warning
The viewer SHALL warn before exporting an image whose pixels may contain personal
details — Burned In Annotation is YES, or the image is a secondary capture or ultrasound
image — that such details cannot be removed, and SHALL let the user cancel.

#### Scenario: Scanned report page
- **WHEN** the user exports a secondary-capture image
- **THEN** a warning explains that the picture itself may show personal details, with the choices "Export anyway" and "Cancel"

### Requirement: Copy to the clipboard
The viewer SHALL copy the active view to the system clipboard as an image without
patient details, with ⌘C on macOS and Ctrl+C on Windows and Linux.

#### Scenario: Pasting into a message
- **WHEN** the user presses the copy shortcut and pastes into a mail or chat application
- **THEN** the image-only picture of the active view is inserted

### Requirement: Export a series as images
The viewer SHALL export all images of a fully loaded series as consecutively numbered
PNG or JPEG files into a folder chosen by the user, with progress and cancel, using the
current contrast window, and SHALL NOT overwrite existing files without confirmation.

#### Scenario: Sixty-slice series
- **WHEN** a 60-image series is exported as PNG
- **THEN** the folder contains 60 files numbered 001 to 060 in display order

#### Scenario: Cancelling
- **WHEN** the user cancels after 20 files
- **THEN** the export stops, the 20 files remain and a message states that 20 of 60 images were exported

### Requirement: Derived-image caption
Exports "with annotations" SHALL carry the caption "Not for diagnostic use"; "image
only" exports contain no added text.

#### Scenario: Caption present
- **WHEN** a view is saved "with annotations"
- **THEN** the caption "Not for diagnostic use" appears at the lower edge of the picture
