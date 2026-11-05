## Context

The shipped changes were verified on synthetic phantoms (`crates/testkit`) and by the
offscreen UI test on macOS, Windows and Linux runners. Items moved here from the archived
changes: real-CD field test (`dicom-media-import` 4.2), Enhanced multi-frame/JPEG 2000
sample set (`image-decoding-and-volumes` 4.3), VoiceOver check (`adopt-cross-platform-ui`
4.4), usability check (`contrast-windowing` 3.2), log review with real data
(`privacy-safeguards` 3.3).

## Goals / Non-Goals

**Goals:** evidence that SumMRIse opens and displays real patient media correctly and is
usable with assistive technology and by non-experts.

**Non-Goals:** clinical validation or any diagnostic claim; collecting data beyond what
participants consent to.

## Decisions

- **De-identification before anything leaves the participant's machine**, using the
  PS3.15 Basic Profile (names, IDs, dates shifted, UIDs remapped, burned-in annotations
  checked by eye). Data is stored encrypted and outside the repository.
- **The disc's own viewer is the reference.** Each study gets a checklist (series and
  counts, orientation letters on two views, three CT reference values, MPR availability).
- **Findings become fixtures.** Each quirk is reproduced synthetically in the testkit so
  CI guards it without real data.

## Risks / Trade-offs

- [Few vendors available] → start with what participants have and widen over time; the
  requirement lists the target vendors.
- [Burned-in PHI in pixel data] → manual review of every image series before sharing.
