## Why

The release packages contain about 470 third-party crates, CharLS (C++), fonts and code
ported from the IJG JPEG library. Their licences (MIT, Apache-2.0, BSD, Zlib, OFL, IJG and
others) require their copyright and licence texts to go with every copy, and the IJG
licence asks for an acknowledgement in the documentation. The About dialog named only
SumMRIse's own MIT licence.

## What Changes

- `cargo xtask notices` writes `THIRD-PARTY-NOTICES.txt`: every crate compiled into the
  release application on any of the four release targets, with the licence SumMRIse uses
  it under and the licence texts that apply, each text once. `--check` fails when the file
  is out of date; CI and `cargo xtask ci` run it.
- Every package carries the file next to `LICENSE.txt` (the DMG and the app's resources,
  the Windows zip and MSI, the Linux tarball and AppImage).
- About ▸ Third-party licences shows it in the application.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `build-system`: the notices are generated and checked.
- `application-shell`: the About dialog shows them.

## Impact

`xtask` (new `notices` module and standard licence texts in `xtask/licenses/`, `serde_json`
for `cargo metadata`), the About dialog, packaging. No change to what the application
does with images or data.
