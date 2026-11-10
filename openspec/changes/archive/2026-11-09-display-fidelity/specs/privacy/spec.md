## ADDED Requirements

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
