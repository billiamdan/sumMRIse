## ADDED Requirements

### Requirement: Application bundle
`cargo xtask bundle-macos` SHALL produce `dist/macos/SumMRIse.app` containing the
release executable, an Info.plist (identifier `com.summrise.Viewer`, short version and
build version equal to the workspace version, `LSMinimumSystemVersion` 11.0, high
resolution capable, category medical), `AppIcon.icns` and `PkgInfo`.

#### Scenario: Launch from Finder
- **WHEN** the user double-clicks `SumMRIse.app`
- **THEN** the app starts with its icon in the Dock and shows the welcome screen

### Requirement: Universal binary
With `--universal` the executable SHALL contain arm64 and x86_64 slices built for
macOS 11.0 or later.

#### Scenario: Architecture check
- **WHEN** `lipo -info SumMRIse.app/Contents/MacOS/summrise` is run
- **THEN** it reports `x86_64 arm64`, and `otool -l` shows `minos 11.0` for both

### Requirement: Code signing
The bundle SHALL be signed with the hardened runtime and empty entitlements — ad-hoc by
default or with the identity passed to `--sign` (with secure timestamp) — and the
build SHALL fail if `codesign --verify --strict` fails.

#### Scenario: Default local build
- **WHEN** no identity is given
- **THEN** `codesign -dv` reports `Signature=adhoc` and flags `adhoc,runtime`

### Requirement: Disk image and notarization
With `--dmg` the build SHALL create `SumMRIse-<version>-macos-<arch>.dmg` containing the
app and an Applications link; with `--notarize <profile>` and a Developer ID identity
the image SHALL be notarized and stapled.

#### Scenario: Release DMG
- **WHEN** `cargo xtask bundle-macos --universal --dmg` completes
- **THEN** `dist/macos/SumMRIse-0.2.0-macos-universal.dmg` exists and mounts showing SumMRIse and Applications
