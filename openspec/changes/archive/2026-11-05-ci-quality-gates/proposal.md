## Why

There was no CI. With three target platforms and a medical domain, every change must be
formatted, lint-clean, tested on macOS, Windows and Linux, licence-checked, and turned
into installable artefacts automatically.

## What Changes

- GitHub Actions workflow `.github/workflows/ci.yml`:
  - `check` matrix (macOS, Windows, Ubuntu): fmt (once), clippy `-D warnings`, full test
    suite (`--locked`); Linux installs Mesa lavapipe so the offscreen UI test can render.
  - `supply-chain`: `cargo-deny` (licences, advisories, sources) with `deny.toml`.
  - `package` matrix on pushes/tags: universal macOS DMG (signed when secrets exist),
    Windows zip, Linux tarball; uploaded as workflow artefacts.
- Least-privilege `permissions: contents: read`, concurrency cancellation, Rust cache.
- Local equivalent: `cargo xtask ci`.

## Capabilities

### New Capabilities
- `continuous-integration`: automated verification and packaging on every change.

### Modified Capabilities
<!-- None. -->

## Impact

New workflow file; optional repository secrets `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`,
`MACOS_SIGNING_IDENTITY` for signed macOS builds.
