## ADDED Requirements

### Requirement: Published releases
Pushing a version tag SHALL publish a GitHub Release containing the macOS DMG, the
Windows zip and the Linux tarball together with a `SHA256SUMS` file.

#### Scenario: Tag v0.3.0
- **WHEN** tag `v0.3.0` is pushed to `main`
- **THEN** a GitHub Release `v0.3.0` lists the three artefacts and `SHA256SUMS`, and every checksum matches its file
