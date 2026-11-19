## ADDED Requirements

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
