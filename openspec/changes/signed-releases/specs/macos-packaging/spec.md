## ADDED Requirements

### Requirement: Gatekeeper-accepted releases
Released DMGs SHALL contain an app signed with a Developer ID certificate, notarized and
stapled, so that Gatekeeper accepts it on a Mac that did not build it.

#### Scenario: First launch on another Mac
- **WHEN** a user downloads the release DMG and opens SumMRIse on a different Mac
- **THEN** `spctl --assess --type execute` accepts the app and no "unidentified developer" warning appears

### Requirement: Intel compatibility verified
Each release SHALL be launched on an Intel Mac or under Rosetta before publication.

#### Scenario: Rosetta smoke test
- **WHEN** the universal executable is started with `arch -x86_64` on Apple Silicon
- **THEN** it opens the synthetic CD and displays the CT series

## MODIFIED Requirements

### Requirement: Application bundle
`cargo xtask bundle-macos` SHALL produce `dist/macos/SumMRIse.app` containing the
release executable built with the release features, an Info.plist (identifier
`com.summrise.Viewer`, short version and build version equal to the workspace version,
`LSMinimumSystemVersion` 11.0, high resolution capable, category medical),
`AppIcon.icns`, `PkgInfo`, and `LICENSE.txt` and `THIRD-PARTY-NOTICES.txt` in its
resources.

#### Scenario: Launch from Finder
- **WHEN** the user double-clicks `SumMRIse.app`
- **THEN** the app starts with its icon in the Dock and shows the welcome screen

#### Scenario: Licence files
- **WHEN** the bundle is built
- **THEN** `SumMRIse.app/Contents/Resources/THIRD-PARTY-NOTICES.txt` equals the file in the repository

### Requirement: Disk image and notarization
With `--dmg` the build SHALL create `SumMRIse-<version>-macos-<arch>.dmg` containing the
app, an Applications link, `LICENSE.txt` and `THIRD-PARTY-NOTICES.txt`; with a Developer
ID identity and either `--notarize <keychain profile>` or an App Store Connect API key
(`--notary-key`, `--notary-key-id`, `--notary-issuer`) the image SHALL be notarized,
stapled and validated.

#### Scenario: Release DMG
- **WHEN** `cargo xtask bundle-macos --universal --dmg` completes
- **THEN** `dist/macos/SumMRIse-0.3.0-macos-universal.dmg` exists and mounts showing SumMRIse, Applications and the two licence files
