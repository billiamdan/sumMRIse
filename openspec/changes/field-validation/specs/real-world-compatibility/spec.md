## ADDED Requirements

### Requirement: Vendor media compatibility
The application SHALL open studies from patient discs produced by Siemens, GE, Philips
and Canon scanners and common PACS exporters, listing every image series the disc's own
viewer lists, without unreadable-file warnings for image objects.

#### Scenario: Disc from a major vendor
- **WHEN** a de-identified validation disc from one of the target vendors is opened
- **THEN** the browser lists the same image series with the same image counts as the disc's viewer

### Requirement: Agreement with the vendor viewer
For each validation study, orientation letters, image ordering and pixel values SHALL
agree with the viewer supplied on the disc.

#### Scenario: CT value at a reference point
- **WHEN** the pointer is placed on the same pixel in SumMRIse and in the disc's viewer
- **THEN** both show the same CT value within 1 HU

### Requirement: Enhanced multi-frame and JPEG 2000 objects
Real scanners' Enhanced MR/CT multi-frame objects and JPEG 2000 series SHALL decode
with correct per-frame geometry.

#### Scenario: Enhanced MR series
- **WHEN** an Enhanced MR Image Storage object with per-frame functional groups is opened
- **THEN** its frames are ordered spatially and reformatting is available when the stack is regular
