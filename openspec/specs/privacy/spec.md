# privacy Specification

## Purpose

Handling of patient-identifying information: local-only processing, no derived data at rest, identifier-free logs and recent list, a privacy display mode, and the not-a-medical-device notice.
## Requirements
### Requirement: Local-only processing
The application SHALL NOT open network connections and SHALL NOT transmit images or
metadata anywhere.

#### Scenario: Offline use
- **WHEN** the computer has no network connection
- **THEN** every feature works unchanged

### Requirement: No derived data at rest
The application SHALL NOT write pixel data, thumbnails or DICOM attributes to disk,
except files the user explicitly exports, which are written only to the location chosen
in the save dialog for that export. Pages the user sends to the printer are handed to the
system's print system without a temporary file of the application's own. Besides its
settings and the recent list, the only files the application writes on its own are crash
reports, which hold no patient data, and the name of the newest one for the next start.

#### Scenario: Re-opening a disc
- **WHEN** a study is opened, the app is closed and the cache directories are inspected
- **THEN** no file contains image data or patient attributes

#### Scenario: Explicit export
- **WHEN** the user saves the current view to a folder chosen in the save dialog
- **THEN** exactly that file is written there and no other copy of the image is stored anywhere else

#### Scenario: Printing
- **WHEN** the user prints a series
- **THEN** the application writes no file; the system's print system holds the job under the neutral title `SumMRIse`

#### Scenario: Crash
- **WHEN** the application stops because of a fault while a study is open
- **THEN** the only new files are the crash report and `unseen.txt` naming it in the crash-report folder, and neither holds image data or patient attributes

### Requirement: Minimal recent list
The recent list SHALL store only the folder path, study description, study date and
modalities, SHALL be clearable, and SHALL not be written when "Remember opened folders"
is off.

#### Scenario: Label content
- **WHEN** a study of patient `PHANTOM^SYNTHETIC`, ID `SYN-0001` is opened
- **THEN** the recent entry reads "Synthetic phantom study · 2026-01-15 · CT, MR, OT" and contains neither the name nor the ID

### Requirement: No identifiers in logs
Log messages at info level or above SHALL NOT contain patient names, IDs or birth dates,
nor the paths of opened folders, files or archives, which can be named after the patient.

#### Scenario: Default log output
- **WHEN** the app runs with `RUST_LOG=info` and opens a study from the folder `PHANTOM^SYNTHETIC SYN-0001`
- **THEN** the log shows that something was opened, series counts and sizes, but neither patient attributes nor the folder's path

#### Scenario: Unreadable DICOMDIR
- **WHEN** the DICOMDIR of the opened folder cannot be read
- **THEN** the warning says so without the folder's path, which is logged at debug level only

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

### Requirement: Burned-in text notice
The acquired view SHALL say "Patient details may be part of the picture itself and cannot
be hidden." at the place of the hidden patient details when privacy mode is on and the
image shown records Burned In Annotation (0028,0301) as YES, whether annotations are shown
or not; it SHALL say nothing when privacy mode is off or the attribute is absent or NO.

#### Scenario: Privacy mode and burned-in text
- **WHEN** privacy mode is on and an ultrasound image with Burned In Annotation YES is displayed
- **THEN** the patient details are hidden and the notice "Patient details may be part of the picture itself and cannot be hidden." is shown on the image

#### Scenario: Privacy mode off
- **WHEN** privacy mode is off for the same image
- **THEN** no notice is shown

### Requirement: Crash reports without patient data
When a fault in the program ends it or one of its threads, the application SHALL write a
crash report to the user's crash-report folder (`~/Library/Logs/SumMRIse` on macOS,
`%LOCALAPPDATA%\SumMRIse\Crash reports` on Windows, `$XDG_STATE_HOME/summrise/crash-reports`
elsewhere), keeping at most five. The report SHALL contain only the version, the operating
system and processor, the time in UTC, the program's own name for the thread, the location
in the source code, a backtrace of function names, and the panic message only when that
message is literal text of the program. It SHALL NOT contain patient names, IDs or birth
dates, the paths of opened files or folders, or values read from them. A panic caught
because a file is damaged SHALL NOT produce a report.

#### Scenario: Message composed from patient data
- **WHEN** a thread panics with a message built from the study of patient `PHANTOM^SYNTHETIC`, ID `SYN-0001`, born `19700101`, opened from the folder `PHANTOM^SYNTHETIC SYN-0001`
- **THEN** the report says "Message: withheld" and contains neither the name, the ID, the birth date nor the folder's path

#### Scenario: Literal message
- **WHEN** a thread panics with the literal message "the volume has no slices"
- **THEN** the report repeats that message, the thread name and the source location

#### Scenario: Damaged file
- **WHEN** the DICOM library panics while decoding a damaged file
- **THEN** the image is reported as not decodable and no crash report is written

#### Scenario: Damaged DICOMDIR
- **WHEN** the DICOM library panics while reading a damaged DICOMDIR
- **THEN** the folder is scanned instead and no crash report is written

