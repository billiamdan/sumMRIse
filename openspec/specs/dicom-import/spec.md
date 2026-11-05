# dicom-import Specification

## Purpose

Discovering DICOM files on patient media and in folders (DICOMDIR first, folder scan as fallback) and indexing them into patients' studies, series and instances.

## Requirements
### Requirement: DICOMDIR is authoritative
When a `DICOMDIR` exists within three levels of the chosen folder, the importer SHALL
index exactly the files it references (active IMAGE-type records) and SHALL NOT parse
other files on the medium.

#### Scenario: Patient CD with viewer software
- **WHEN** the disc root contains `DICOMDIR`, `DICOM/…` images, `AUTORUN.INF` and a `VIEWER/` folder
- **THEN** only the referenced images are indexed and the status bar shows the source "DICOMDIR"

#### Scenario: Case-insensitive resolution
- **WHEN** DICOMDIR references `DICOM\ST000001\IM000001` but the mounted file is `dicom/st000001/im000001;1`
- **THEN** the file is found and indexed

#### Scenario: Broken DICOMDIR
- **WHEN** more than 10 % of the referenced files are missing or the DICOMDIR cannot be parsed
- **THEN** the folder is scanned instead and a warning explains why

### Requirement: Folder scan fallback
Without a usable DICOMDIR the importer SHALL recursively scan the folder, accept files
with the 128-byte preamble followed by `DICM`, and skip hidden entries below the chosen
folder and files with known non-DICOM extensions.

#### Scenario: Copied images without DICOMDIR
- **WHEN** a folder of extension-less DICOM files plus `README.TXT` is opened
- **THEN** all images are indexed and no file is reported unreadable

### Requirement: Header-only parallel indexing
Indexing SHALL read attributes only up to Pixel Data, in parallel, and SHALL group
instances by Study Instance UID and Series Instance UID, ignoring duplicate SOP
Instance UIDs and counting non-image objects separately.

#### Scenario: Grouping and order
- **WHEN** a disc with series numbers 3, 1, 99, 2 is indexed
- **THEN** series are listed 1, 2, 3, 99 and instances within a series by Instance Number

#### Scenario: Malformed file
- **WHEN** one file is truncated or makes the parser panic
- **THEN** it is counted as unreadable, a warning is shown and all other files are indexed

### Requirement: Text decoding
Text attributes SHALL be decoded using the file's Specific Character Set.

#### Scenario: UTF-8 institution name
- **WHEN** Specific Character Set is `ISO_IR 192` and Institution Name is "Klinikum Musterstadt — Röntgen"
- **THEN** the name is shown with the em dash and umlaut intact

### Requirement: Progress and cancellation
Discovery and indexing SHALL report the current stage and done/total counts and SHALL
stop promptly when cancelled.

#### Scenario: Cancelled scan
- **WHEN** cancellation is requested during a folder scan
- **THEN** the scan returns a "cancelled" outcome without an index

### Requirement: Default series
On opening a study the application SHALL select the grey-scale series with patient
geometry and the most frames, excluding localizers.

#### Scenario: CT study with scout
- **WHEN** a study has a 3-image localizer and a 120-slice axial series
- **THEN** the 120-slice series is shown first

### Requirement: Removable media detection
The application SHALL detect mounted volumes whose root contains a DICOMDIR, polling
off the UI thread.

#### Scenario: Optical drive spin-up
- **WHEN** an optical drive takes several seconds to respond
- **THEN** the UI keeps rendering and the disc appears once it is readable

