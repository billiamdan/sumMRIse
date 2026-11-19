## ADDED Requirements

### Requirement: Published releases
Pushing a version tag SHALL prepare a GitHub Release as a draft containing the macOS DMG,
the Windows MSI and zip and the Linux AppImage and tarball together with a `SHA256SUMS`
file, with release notes from the version's section of `CHANGELOG.md` that say which
packages are not signed; the maintainer publishes the draft. The tag SHALL name the
workspace version and be on `main`, and CI SHALL have passed for its commit.

#### Scenario: Tag v0.3.0
- **WHEN** tag `v0.3.0` is pushed to `main` after CI has passed for that commit
- **THEN** a draft GitHub Release `v0.3.0` lists the five packages and `SHA256SUMS`, every checksum matches its file, and nothing is public until the maintainer publishes it

#### Scenario: Tag on a commit whose CI has not passed
- **WHEN** a tag is pushed for a commit on `main` whose CI run failed or is still running
- **THEN** the release workflow fails before building anything, saying so

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
program with `--version`, check the version information of `SumMRIse.exe`, validate the
MSI with Windows Installer's consistency checks, and install and remove it.

#### Scenario: Manual run
- **WHEN** the CI workflow is run manually on a branch
- **THEN** the run has the artefacts `SumMRIse-macos-universal`, `SumMRIse-windows-x64` and `SumMRIse-linux-x64`

#### Scenario: No signing secrets
- **WHEN** the repository has no signing secrets
- **THEN** the packages are built unsigned and the run notes that they are not signed
