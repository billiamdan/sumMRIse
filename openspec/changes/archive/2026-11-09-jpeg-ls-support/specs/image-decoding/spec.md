## MODIFIED Requirements

### Requirement: Supported transfer syntaxes
The decoder SHALL decode Implicit/Explicit VR Little Endian, Explicit VR Big Endian,
Deflated Explicit VR Little Endian, JPEG Baseline, Extended and Lossless, JPEG 2000
(lossless and lossy), RLE Lossless and — in release builds — JPEG-LS Lossless and
Near-Lossless; series using any other transfer syntax SHALL be listed with the transfer
syntax name and a "not supported by this build" notice.

#### Scenario: JPEG-compressed localizer
- **WHEN** a series is stored as JPEG Baseline (1.2.840.10008.1.2.4.50)
- **THEN** all its frames decode and display

#### Scenario: JPEG-LS lossless series in a release build
- **WHEN** a release build opens a series stored as JPEG-LS Lossless (1.2.840.10008.1.2.4.80)
- **THEN** all its frames decode and their values equal those of the uncompressed original
