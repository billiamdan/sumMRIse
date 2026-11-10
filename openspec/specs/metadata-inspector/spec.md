# metadata-inspector Specification

## Purpose

Presenting the DICOM metadata of the displayed image: a curated, human-readable overview and the complete, searchable attribute tree.
## Requirements
### Requirement: Inspector follows the displayed image
The inspector SHALL show metadata of the instance whose frame is currently displayed,
reload when the image changes, and load in the background without blocking the UI.

#### Scenario: Scrolling through a series
- **WHEN** the user scrolls from image 3 to image 4
- **THEN** the Image section shows instance 4's Instance Number and Slice Location

### Requirement: Curated overview
The Overview tab SHALL group formatted values into Patient, Study, Series, Equipment,
Acquisition, Image and Identifiers sections, omitting empty values, expanding coded
values and showing units.

#### Scenario: MR acquisition parameters
- **WHEN** an MR image has RepetitionTime 4000, EchoTime 96, ScanningSequence SE, MagneticFieldStrength 1.5
- **THEN** the overview shows "Repetition time (TR) 4000 ms", "Echo time (TE) 96 ms", "Scanning sequence Spin echo" and "Field strength 1.5 T"

#### Scenario: Human-readable patient values
- **WHEN** Patient's Name is `PHANTOM^SYNTHETIC`, Birth Date `19700101`, Age `056Y`
- **THEN** the overview shows "SYNTHETIC PHANTOM", "1970-01-01" and "56 years"

### Requirement: Complete attribute tree
The All attributes tab SHALL list every attribute of the file meta information and
data set with tag, dictionary name, keyword, VR and value, nest sequence items,
summarise binary values and truncate values longer than 512 characters.

#### Scenario: Private attribute
- **WHEN** an image contains (0029,1010) with private creator "SIEMENS CSA HEADER" in (0029,0010)
- **THEN** the attribute is listed as "Private (SIEMENS CSA HEADER)"

### Requirement: Search and copy
A search field SHALL filter attributes by tag number (with or without punctuation),
name, keyword or value, including nested sequences, expanding matching sequences;
values SHALL be copyable via the context menu.

#### Scenario: Find echo time
- **WHEN** the user types "echo"
- **THEN** Echo Time, Echo Train Length and Echo Numbers remain visible and other attributes are hidden

### Requirement: Parameter explanations
Rows of the Overview tab that have an explanation SHALL show an info mark after their
label. Pointing at the mark, or giving it keyboard focus, SHALL show one or two plain
sentences about the value, including what its unit means. Explanations SHALL be looked up
by a stable key of the row — the keyword of the DICOM attribute it shows — and not by its
label. Rows of the Patient section SHALL have no explanation.

#### Scenario: Repetition time
- **WHEN** an MR image has RepetitionTime 4000
- **THEN** the row "Repetition time (TR) 4000 ms" carries an info mark named "About Repetition time (TR)", and its explanation says that this is the time between two successive excitations of the same slice, in milliseconds (ms, thousandths of a second)

#### Scenario: Tube voltage
- **WHEN** the user points at the info mark of the row "Tube voltage 120 kV" of a CT image
- **THEN** the explanation reads "The voltage of the X-ray tube, in kilovolts (kV, thousands of volts). It sets how penetrating the X-rays are."

#### Scenario: Keyboard
- **WHEN** the info mark of the row "Kernel" receives keyboard focus
- **THEN** its explanation is shown, and it is hidden again when the focus moves on

#### Scenario: Stable keys
- **WHEN** the overview of an MR image is built
- **THEN** the row "Repetition time (TR)" has the key `RepetitionTime`, the row "Orientation" the key `ImageOrientationPatient`, and the rows "Matrix" and "Field of view", which combine attributes, the keys `Matrix` and `FieldOfView`

#### Scenario: Patient rows
- **WHEN** the Patient section shows Name, Patient ID, Birth date, Age and Sex
- **THEN** none of these rows carries an info mark

