## 1. macOS signing

- [ ] 1.1 Apple Developer ID Application certificate and notarization credentials (from `macos-distribution` 2.5) — needs an Apple Developer Program membership
- [ ] 1.2 CI secrets and a notarize + staple step in the macOS package job — implemented (`package.yml` imports the certificate into a temporary keychain and `bundle-macos --notary-key` notarizes, staples and validates the DMG with an App Store Connect API key when `APPLE_API_KEY_P8`, `APPLE_API_KEY_ID` and `APPLE_API_ISSUER` exist), but never run: needs 1.1
- [ ] 1.3 Verify `spctl --assess` on a second Mac — needs a signed, notarized build; the package job runs it when signing secrets exist
- [ ] 1.4 Run the x86_64 slice on an Intel Mac or under Rosetta (from `macos-distribution` 2.4) — the package job starts `arch -x86_64 summrise --version` where the runner has Rosetta (it printed `SumMRIse 0.3.0` in CI run 42684689436); opening the synthetic CD needs the real window

## 2. Publishing

- [x] 2.1 Tag-triggered GitHub Release with DMG, MSI, zip, AppImage, tarball and `SHA256SUMS` (from `ci-quality-gates` 2.4): `.github/workflows/release.yml` checks that the tag names the workspace version and is on `main`, builds the packages with `package.yml` and publishes them; the packaging was checked with a manual run of CI (run 42684689436: all three packages built and checked; the verify and publish jobs run at the first tag)
- [x] 2.2 Release notes generated from the merged OpenSpec changes: `cargo xtask release-notes` takes the version's section of `CHANGELOG.md`, which names the changes, and says which packages are not signed
- [ ] 2.3 First tag: the release lists the files and `sha256sum -c SHA256SUMS` passes on the downloads — tagging is the maintainer's decision

## 3. Privacy gate

- [x] 3.1 `cargo-deny` bans for HTTP/network client crates (from `privacy-safeguards` 3.2): HTTP, WebSocket and TLS clients in `deny.toml`; a probe crate depending on `minreq` fails with "crate 'minreq = 2.14.1' is explicitly banned"
