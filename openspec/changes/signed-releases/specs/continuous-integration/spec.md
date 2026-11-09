## ADDED Requirements

### Requirement: Published releases
Pushing a version tag SHALL publish a GitHub Release containing the macOS DMG, the
Windows MSI and zip and the Linux AppImage and tarball together with a `SHA256SUMS` file,
with release notes from the version's section of `CHANGELOG.md` that say which packages
are not signed. The tag SHALL name the workspace version and be on `main`.

#### Scenario: Tag v0.3.0
- **WHEN** tag `v0.3.0` is pushed to `main`
- **THEN** a GitHub Release `v0.3.0` lists the five packages and `SHA256SUMS`, and every checksum matches its file

#### Scenario: Tag that does not match the version
- **WHEN** tag `v0.3.1` is pushed while `Cargo.toml` says `0.3.0`
- **THEN** the release workflow fails before building anything and no release is published

## MODIFIED Requirements

### Requirement: Packaged artefacts
Pushes to `main`, version tags and manual runs SHALL produce a macOS universal DMG, a
Windows x64 MSI and zip and a Linux x64 AppImage and tarball as downloadable workflow
artefacts, built with the release features, each package carrying `LICENSE.txt` and
`THIRD-PARTY-NOTICES.txt`; the macOS build SHALL be Developer ID–signed and notarized and
the Windows files Authenticode-signed when the signing secrets are configured, and the
packages SHALL still be built without them. The workflow SHALL start each packaged
program with `--version`, check the version information of `SumMRIse.exe`, and install and
remove the MSI.

#### Scenario: Manual run
- **WHEN** the CI workflow is run manually on a branch
- **THEN** the run has the artefacts `SumMRIse-macos-universal`, `SumMRIse-windows-x64` and `SumMRIse-linux-x64`

#### Scenario: No signing secrets
- **WHEN** the repository has no signing secrets
- **THEN** the packages are built unsigned and the run notes that they are not signed
