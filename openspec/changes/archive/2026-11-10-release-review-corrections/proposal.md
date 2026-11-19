## Why

A review of the release-readiness work (PR 8) found that the third-party notices missed
things the packages contain, that a fault in background work could leave a screen waiting
for ever, that the log named what was opened although such names can be a patient's, and
that several specifications and documents said more than the code did.

## What Changes

- **Third-party notices** now cover the Rust standard library, the font inside
  `sctk-adwaita` (Cantarell, OFL-1.1) and the KDE protocol definitions that reach the Linux
  build through winit (one of them LGPL-2.1-or-later). Files that only point to a crate's
  licences are kept with their copyright lines; a licence a crate merely offers as an
  alternative (GPL) is no longer printed; font and Unicode licences are no longer labelled
  MIT.
- **Background faults**: when the thread behind the scan, the image information or the
  printer search dies, the screen says so and returns instead of waiting.
- **Privacy**: the paths of opened folders and archives are logged at debug level only.
  The crash report goes to a crash-report folder (the log folder on macOS only), and the
  application also writes the name of the newest report for the next start.
- **Third-party licences window**: it scrolls from the keyboard, and long lines are broken.
- **Conformance specification**: its JPEG-LS scenario follows the statement's wording.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `build-system`: what the third-party notices cover.
- `application-shell`: background faults end what was waiting; the licences window from the keyboard.
- `privacy`: no paths of opened media in the log at its default level; crash-report wording.
- `dicom-conformance`: the JPEG-LS scenario.

## Impact

`xtask` (notices), `crates/ui` (app, inspector, print, licences), `crates/dicom` and
`crates/ui` logging, documents. Patient-safety impact: none on image display; privacy is
tightened.
