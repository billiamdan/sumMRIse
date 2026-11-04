## ADDED Requirements

### Requirement: Local-only processing
The application SHALL NOT open network connections and SHALL NOT transmit images or
metadata anywhere.

#### Scenario: Offline use
- **WHEN** the computer has no network connection
- **THEN** every feature works unchanged

### Requirement: No derived data at rest
The application SHALL NOT write pixel data, thumbnails or DICOM attributes to disk.

#### Scenario: Re-opening a disc
- **WHEN** a study is opened, the app is closed and the cache directories are inspected
- **THEN** no file contains image data or patient attributes

### Requirement: Minimal recent list
The recent list SHALL store only the folder path, study description, study date and
modalities, SHALL be clearable, and SHALL not be written when "Remember opened folders"
is off.

#### Scenario: Label content
- **WHEN** a study of patient `PHANTOM^SYNTHETIC`, ID `SYN-0001` is opened
- **THEN** the recent entry reads "Synthetic phantom study · 2026-01-15 · CT, MR, OT" and contains neither the name nor the ID

### Requirement: No identifiers in logs
Log messages at info level or above SHALL NOT contain patient names, IDs or birth dates.

#### Scenario: Default log output
- **WHEN** the app runs with `RUST_LOG=info` and opens a study
- **THEN** the log shows the folder path, series counts and sizes but no patient attributes

### Requirement: Privacy display mode
A privacy toggle SHALL hide patient name, ID and birth date in viewport overlays, the
series browser and the inspector (Patient section hidden, group 0010 values masked).

#### Scenario: Screenshot for a forum
- **WHEN** privacy mode is on
- **THEN** no patient name, ID or birth date is visible anywhere in the window

### Requirement: Not a medical device
The welcome screen and About dialog SHALL state that the application is not a medical
device and must not be used for diagnosis.

#### Scenario: First launch
- **WHEN** the app starts
- **THEN** the welcome screen shows the disclaimer and the local-processing notice
