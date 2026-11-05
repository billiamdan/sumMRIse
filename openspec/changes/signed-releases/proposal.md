## Why

CI builds unsigned artefacts only for workflow runs: on other Macs Gatekeeper rejects the
ad-hoc signed app, its x86_64 slice has never been run, nothing is published as a
release, and the "no network access" privacy promise is not enforced automatically.

## What Changes

- Developer ID signing, notarization and stapling of the macOS DMG in CI (App Store
  Connect API key or keychain profile from repository secrets).
- Verify each release on an Intel Mac or under Rosetta.
- Tag-triggered GitHub Releases with the macOS DMG, Windows zip and Linux tarball plus a
  `SHA256SUMS` file.
- Supply-chain gate: `cargo-deny` bans HTTP/network client crates so the app cannot
  silently gain network access.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `macos-packaging`: adds Gatekeeper acceptance and Intel verification of releases.
- `continuous-integration`: adds published, checksummed releases.
- `privacy`: adds an automated gate against network client dependencies.

## Impact

CI workflow, repository secrets, `deny.toml`, release documentation. Requires an Apple
Developer Program membership.
