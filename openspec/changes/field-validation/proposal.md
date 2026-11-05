## Why

Everything shipped so far is verified with synthetic, PHI-free phantoms and automated
tests. Real patient media differ by vendor and CD-burning software (Enhanced multi-frame
objects, JPEG 2000, private tags, unusual DICOMDIRs); screen readers behave differently
from the automated AccessKit check; and non-experts may not understand window presets.
Before a 1.0 release these must be validated with real data and real users — without
compromising anyone's privacy.

## What Changes

- Build a consented, de-identified validation set of patient discs from several vendors
  (at least Siemens, GE, Philips, Canon and one PACS export), including Enhanced MR/CT
  multi-frame and JPEG 2000 studies, stored outside the repository.
- Compare SumMRIse with the viewer shipped on each disc: series list and image counts,
  orientation letters, ordering, CT values at reference points, reformat availability.
- Screen-reader walkthroughs (VoiceOver, Narrator, Orca) of the core flows.
- Usability sessions with non-expert participants.
- Privacy review of info-level logs produced while opening the real studies.
- Every finding becomes a fix plus a synthetic testkit fixture reproducing the quirk.

## Capabilities

### New Capabilities
- `real-world-compatibility`: correct behaviour on media produced by real scanners and PACS exporters.
- `assistive-technology`: operability of the core flows with platform screen readers.

### Modified Capabilities
<!-- None: this change verifies existing dicom-import, image-decoding, contrast-windowing and privacy requirements. -->

## Impact

Process plus whatever fixes the validation finds; `crates/testkit` gains vendor-quirk
fixtures. Needs participants' consent and a de-identification procedure (DICOM PS3.15
Basic Application Level Confidentiality Profile). No real data ever enters the repository.
