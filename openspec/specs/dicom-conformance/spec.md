# dicom-conformance Specification

## Purpose
The DICOM conformance statement kept with the code: which media, SOP classes, transfer syntaxes and attributes SumMRIse reads, checked against the code by a test.
## Requirements
### Requirement: Conformance statement
The repository SHALL contain a DICOM conformance statement,
`docs/dicom-conformance-statement.md`, in the layout of PS3.2 reduced to what applies:
overview, networking (none), media storage with the File-set Reader role only, supported
SOP classes, transfer syntaxes, photometric interpretations, attributes read and ignored,
character sets, security, and the annexes that apply.

#### Scenario: Reader role only
- **WHEN** a reader looks up the media roles in the statement
- **THEN** File-set Reader (FSR) is supported and File-set Creator (FSC) and Updater (FSU) are not

#### Scenario: No networking
- **WHEN** a reader looks up network services
- **THEN** the statement says that SumMRIse supports no DICOM network service and opens no network connection

### Requirement: Transfer syntaxes in step with the code
The statement's transfer-syntax table SHALL list every transfer syntax known to the DICOM
library with whether this build decodes it and whether it is always lossy, and a test SHALL
fail when a row disagrees with `format::transfer_syntax_supported`,
`format::transfer_syntax_is_lossy` or the build options, or when a known transfer syntax is
missing.

#### Scenario: JPEG Baseline row
- **WHEN** the test reads the row of 1.2.840.10008.1.2.4.50
- **THEN** it says "Yes" for decoding and "Yes" for lossy, as the code does

#### Scenario: JPEG-LS without the build option
- **WHEN** the default build reads the row of 1.2.840.10008.1.2.4.80
- **THEN** it says "With the jpeg-ls build option" and `transfer_syntax_supported` is false

