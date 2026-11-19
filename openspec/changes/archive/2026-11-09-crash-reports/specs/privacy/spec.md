## ADDED Requirements

### Requirement: Crash reports without patient data
When a fault in the program ends it or one of its threads, the application SHALL write a
crash report to the user's log folder, keeping at most five. The report SHALL contain only
the version, the operating system and processor, the time in UTC, the program's own name
for the thread, the location in the source code, a backtrace of function names, and the
panic message only when that message is literal text of the program. It SHALL NOT contain
patient names, IDs or birth dates, the paths of opened files or folders, or values read
from them. A panic caught because a file is damaged SHALL NOT produce a report.

#### Scenario: Message composed from patient data
- **WHEN** a thread panics with a message built from the study of patient `PHANTOM^SYNTHETIC`, ID `SYN-0001`, born `19700101`, opened from the folder `PHANTOM^SYNTHETIC SYN-0001`
- **THEN** the report says "Message: withheld" and contains neither the name, the ID, the birth date nor the folder's path

#### Scenario: Literal message
- **WHEN** a thread panics with the literal message "the volume has no slices"
- **THEN** the report repeats that message, the thread name and the source location

#### Scenario: Damaged file
- **WHEN** the DICOM library panics while decoding a damaged file
- **THEN** the image is reported as not decodable and no crash report is written

## MODIFIED Requirements

### Requirement: No derived data at rest
The application SHALL NOT write pixel data, thumbnails or DICOM attributes to disk,
except files the user explicitly exports, which are written only to the location chosen
in the save dialog for that export. Pages the user sends to the printer are handed to the
system's print system without a temporary file of the application's own. Besides its
settings and the recent list, the only file the application writes on its own is a crash
report, which holds no patient data.

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
- **THEN** the only new file is the crash report in the log folder, and it holds no image data or patient attributes
