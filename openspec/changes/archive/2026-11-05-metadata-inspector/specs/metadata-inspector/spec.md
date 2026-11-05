## ADDED Requirements

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
