# Changelog

What changed in each version of SumMRIse, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Each version names the OpenSpec changes it
contains (`openspec/changes/archive/`). The release workflow takes the release notes from
the section of the tagged version.

SumMRIse is not a medical device. It is intended for personal and educational viewing and
must not be used for diagnosis.

## [Unreleased]

## [0.3.0] — 2026-11-10

The first version with downloadable packages.

### Added

- **Downloads for macOS, Windows and Linux**: a universal DMG for macOS 11 or later, a
  per-user MSI installer and a portable zip for Windows 10 or later, and an AppImage and a
  tarball for Linux (glibc 2.35 or later), with a `SHA256SUMS` file, for each tagged version
  (`signed-releases`, `windows-distribution`, `linux-distribution`). `SumMRIse.exe` carries
  its icon and version information. Signing and notarization run when the certificates are
  configured; until then the packages are unsigned, and the release notes say so.
- **JPEG-LS** Lossless and Near-Lossless images are decoded (CharLS) in the release
  packages (`jpeg-ls-support`).
- **Crash reports without patient data**: if SumMRIse stops because of a fault, it says so
  and saves a short report that holds no names, IDs, dates of birth, file names or values
  from the images (`crash-reports`). If only part of the program stopped, the screen that
  was waiting for it says so instead of waiting for ever.
- **Third-party licences**: the licences of the software SumMRIse is built with — the
  crates, the Rust standard library, bundled fonts and CharLS — are shown under About ▸
  Third-party licences and come with every package as `THIRD-PARTY-NOTICES.txt`
  (`third-party-notices`).
- Everything built since 0.2.0, which was never published as a download:
  - images appear while a series is still loading (`progressive-loading`);
  - ZIP archives and ISO 9660 disc images open in place (`archive-import`);
  - reformatted planes can be tilted freely (`oblique-reformats`);
  - a plain-language guide to each series, value and viewing position
    (`plain-language-guide`);
  - series with several time points, echoes or b-values, and cine (`multi-phase-cine`);
  - a 3D view (`volume-rendering`);
  - contrast tables, padding, scanner graphics, lossy-compression marks, the display
    check and the DICOM conformance statement (`display-fidelity`);
  - PET with SUV, colour maps and fusion over CT or MR (`pet-support`);
  - German, Spanish, French and Russian (`translations`; machine translations, not yet
    reviewed by native speakers);
  - two to four series side by side, study comparison and tile grids
    (`series-comparison`);
  - length, angle and region measurements (`measurement-tools`);
  - structured reports, dose reports, PDF and text documents from the disc
    (`report-viewer`);
  - saving, copying and printing pictures and series (`export-and-share`).

### Changed

- Files in a folder are indexed in the order of their names on every system, so a disc
  that holds two files with the same UID shows the same one everywhere.
- On Windows, printing never looks for `lp` on the current drive; PDFs are saved and
  opened in the default application.
- Release builds no longer print panic messages, which could quote data from the opened
  files, to standard error.
- The log no longer names the folder or archive that was opened unless it is switched to
  debug level: such names can be a patient's.

### Security

- The supply-chain check fails when one of the common HTTP, WebSocket or TLS client crates
  named in `deny.toml` enters the dependency graph (`cargo deny`): a guard against network
  access arriving with a dependency.

## [0.2.0] — 2026-11-05

The application rebuilt as a layered Rust workspace with one egui interface for macOS,
Windows and Linux (`restructure-monorepo`, `adopt-cross-platform-ui`), reading patient
media through the DICOMDIR (`dicom-media-import`), decoding images into volumes
(`image-decoding-and-volumes`), multiplanar viewing (`multiplanar-viewing`), contrast
windowing (`contrast-windowing`), the metadata inspector (`metadata-inspector`), privacy
safeguards (`privacy-safeguards`), the macOS app bundle (`macos-distribution`) and CI
quality gates (`ci-quality-gates`). Not published as a download.

## 0.1.0 — 2025-09-11

The first prototype, before the rebuild.

[Unreleased]: https://github.com/billiamdan/sumMRIse/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/billiamdan/sumMRIse/releases/tag/v0.3.0
[0.2.0]: https://github.com/billiamdan/sumMRIse/tree/9d64f17
