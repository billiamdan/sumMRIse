## MODIFIED Requirements

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
- **WHEN** a build without the `jpeg-ls` feature reads the row of 1.2.840.10008.1.2.4.80
- **THEN** it says "Yes (release builds)" and `transfer_syntax_supported` is false

#### Scenario: JPEG-LS in a release build
- **WHEN** a build with the `jpeg-ls` feature, as the release packages are, reads that row
- **THEN** it says "Yes (release builds)" and `transfer_syntax_supported` is true
