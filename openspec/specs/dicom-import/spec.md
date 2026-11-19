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

### Requirement: Archives and disc images open like the folder
The importer SHALL open a ZIP archive or an ISO 9660 disc image of patient media exactly
as it opens the unpacked folder: DICOMDIR first, folder scan as fallback, with the same
studies, series, instances, geometry, counts and warnings, and the same pixel values.

#### Scenario: ZIP download of a patient disc
- **WHEN** a ZIP archive holds `PATIENT CD/DICOMDIR`, `PATIENT CD/DICOM/…`, `PATIENT CD/AUTORUN.INF`, a `VIEWER/` folder and the `__MACOSX/` folder of an archive made on a Mac
- **THEN** exactly the images the DICOMDIR references are indexed, the status bar shows the source "DICOMDIR", and the index equals the one of the unpacked folder

#### Scenario: Disc image with ISO 9660 names
- **WHEN** a disc image names its files `IM000001.;1` in the primary tree, with or without a Joliet tree, and the DICOMDIR references `DICOM\ST000001\SE000001\IM000001`
- **THEN** the file is found and indexed

#### Scenario: Same images as from the folder
- **WHEN** the axial CT of the synthetic disc is loaded from its ZIP archive and from its disc image
- **THEN** both give 24 slices ordered from superior to inferior, the marker at (40, −35, 50) mm (patient left, anterior, superior) reads 300 HU, and every image is pixel-identical to the one loaded from the folder

#### Scenario: Archive without a DICOMDIR
- **WHEN** an archive of extension-less DICOM files plus `README.TXT` has no DICOMDIR
- **THEN** it is scanned like the folder, all images are indexed and no file is reported unreadable

### Requirement: Recognition by content
The importer SHALL recognise a ZIP archive by the signature of its first header and an
ISO 9660 disc image by `CD001` at byte 32769, whatever the file is called; a file with
the DICOM preamble and `DICM` code SHALL be opened as a DICOM file whatever it is called.

#### Scenario: Download without an extension
- **WHEN** the user opens a file called `download` that is a ZIP archive of a disc
- **THEN** it opens as that disc

#### Scenario: Misleading names
- **WHEN** a ZIP archive is called `disc.iso` and a DICOM image is called `image.zip`
- **THEN** the first opens as an archive and the second as a single DICOM image

### Requirement: Reading in memory without unpacking
Files SHALL be read from an archive or disc image into memory when they are needed and
SHALL NOT be written to disk; header indexing SHALL read an entry only up to the Pixel
Data, inflating a compressed entry only that far.

#### Scenario: Nothing written to disk
- **WHEN** a study is opened from a ZIP archive and from a disc image and every image, preview and metadata view of it is loaded
- **THEN** no file is created in the temporary folder or beside the archive

#### Scenario: Headers without pixel data
- **WHEN** an archive of 512 × 512 CT slices is indexed
- **THEN** less than a quarter of the archive is read, and the images themselves are read only when the series is loaded

### Requirement: Concurrent reads from one archive
Reading from one archive or disc image SHALL be safe while header indexing, the series
loader, previews and the metadata inspector read from it at the same time, and SHALL
give each reader the same bytes as reading alone.

#### Scenario: Everything at once
- **WHEN** an archive is indexed twice while its CT is loaded three times over, previews of every series are made and the inspector reads every image
- **THEN** every read returns the bytes of the file, and the index and images equal those of the unpacked folder

### Requirement: Unsupported archive content is explained
When an archive or disc image cannot be opened, the importer SHALL say in plain words why
and what to do instead; when only some of its files cannot be read, it SHALL open the
rest and count the skipped files in a warning.

#### Scenario: Password-protected archive
- **WHEN** every file of `locked.zip` is encrypted
- **THEN** the error reads "locked.zip is protected by a password. Unpack it with the password, then open the unpacked folder."

#### Scenario: Partly unreadable archive
- **WHEN** one image of `partly.zip` is encrypted and the others are not
- **THEN** the study opens without that image and a warning reads "1 file in partly.zip is protected by a password and was skipped"

#### Scenario: Unsupported compression method
- **WHEN** the files of an archive are compressed with LZMA
- **THEN** the error says that the archive is compressed with a method that is not supported (Lzma) and suggests unpacking it

#### Scenario: Archive inside the archive
- **WHEN** `wrapper.zip` holds only `README.TXT` and `disc.iso`
- **THEN** the error reads "wrapper.zip contains another archive (disc.iso), which is not opened automatically. Unpack wrapper.zip, then open disc.iso."

#### Scenario: UDF-only disc image
- **WHEN** a disc image has a UDF file system and no ISO 9660 one
- **THEN** the error says that UDF disc images are not supported and that the image can be opened with the system and the resulting disc opened instead

#### Scenario: Other container formats
- **WHEN** a 7z, RAR, gzip, tar or DMG file is opened, whatever it is called
- **THEN** the error names the format and says that it is not supported

#### Scenario: Interrupted download
- **WHEN** only the first half of a ZIP download arrived
- **THEN** the error says that the archive is damaged or was not downloaded completely

#### Scenario: Disc image cut off
- **WHEN** the last quarter of a disc image is missing
- **THEN** the images that are complete open, and warnings say that part of the image could not be read and how many files were skipped

#### Scenario: No images inside
- **WHEN** a ZIP archive holds only documents
- **THEN** the error "no DICOM images were found in <path>" is shown, as for a folder

### Requirement: Untrusted archives
Reading an archive SHALL NOT reserve more memory for an entry than its declared size,
SHALL NOT read beyond that size, SHALL refuse entries larger than 4 GiB with a warning,
and SHALL never use the name of an entry as a file-system path.

#### Scenario: Absurd declared size
- **WHEN** an entry of `huge.zip` declares 5 GiB
- **THEN** it is skipped without being read and a warning reads "1 file in huge.zip is larger than 4 GB and was skipped"

#### Scenario: Entry longer than declared
- **WHEN** an entry holds more data than its declared size
- **THEN** reading it fails with a message naming the archive and the entry, and no data beyond the declared size is returned

#### Scenario: Names that climb out
- **WHEN** an archive holds an entry named `../../outside/IM000001`
- **THEN** the entry is not listed and nothing is read or written outside the archive

#### Scenario: Archive replaced while open
- **WHEN** the archive file is replaced on disk while a study from it is open
- **THEN** further reads fail with a message that the file has changed since it was opened, instead of returning other data

### Requirement: Document discovery
The importer SHALL index DICOM document objects (structured reports, Encapsulated PDF)
with their study, whether they are found through DICOMDIR records or a folder scan, and
SHALL find report files (PDF, RTF, text, HTML) on the medium outside folders that contain
executable software.

#### Scenario: DICOMDIR with a report record
- **WHEN** the DICOMDIR lists an SR DOCUMENT record under a study
- **THEN** the report is indexed as a document of that study and is not counted among the objects that are not shown

#### Scenario: Report file beside the viewer software
- **WHEN** the medium contains `REPORTS/report_001.rtf` and `VIEWER/readme.txt` next to `VIEWER/viewer.exe`
- **THEN** `report_001.rtf` is indexed as a document and `readme.txt` is not

