## Why

JPEG-LS (lossless and near-lossless) is used by some scanners and PACS exporters. The
current release build has no JPEG-LS decoder — the only mature one, CharLS, is C++ — so
such series show "not supported by this build" instead of images.

## What Changes

- Add a `jpeg-ls` feature to the desktop app that forwards to `summrise-dicom/jpeg-ls`
  (CharLS via dicom-rs), enabled for release builds (`cargo xtask bundle-macos`, CI
  packaging) while plain developer builds keep working without CMake.
- Install CMake and a C++ toolchain in CI and run the tests with the feature on all
  three platforms.
- Add a JPEG-LS fixture to the testkit and a decode test.
- Re-evaluate a pure-Rust decoder if one becomes production ready.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `image-decoding`: "Supported transfer syntaxes" additionally covers JPEG-LS Lossless and Near-Lossless in release builds.

## Impact

`apps/desktop` and `crates/dicom` features, `xtask`, CI workflow, testkit. Adds CharLS
(BSD-3-Clause, already allowed by `deny.toml`) to release binaries.
