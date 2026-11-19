## MODIFIED Requirements

### Requirement: No derived data at rest
The application SHALL NOT write pixel data, thumbnails or DICOM attributes to disk,
except files the user explicitly exports, which are written only to the location chosen
in the save dialog for that export. Pages the user sends to the printer are handed to the
system's print system without a temporary file of the application's own.

#### Scenario: Re-opening a disc
- **WHEN** a study is opened, the app is closed and the cache directories are inspected
- **THEN** no file contains image data or patient attributes

#### Scenario: Explicit export
- **WHEN** the user saves the current view to a folder chosen in the save dialog
- **THEN** exactly that file is written there and no other copy of the image is stored anywhere else

#### Scenario: Printing
- **WHEN** the user prints a series
- **THEN** the application writes no file; the system's print system holds the job under the neutral title `SumMRIse`
