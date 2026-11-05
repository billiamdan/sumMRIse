## 1. macOS signing

- [ ] 1.1 Apple Developer ID Application certificate and notarization credentials (from `macos-distribution` 2.5)
- [ ] 1.2 CI secrets and a notarize + staple step in the macOS package job
- [ ] 1.3 Verify `spctl --assess` on a second Mac
- [ ] 1.4 Run the x86_64 slice on an Intel Mac or under Rosetta (from `macos-distribution` 2.4)

## 2. Publishing

- [ ] 2.1 Tag-triggered GitHub Release with DMG, zip, tarball and `SHA256SUMS` (from `ci-quality-gates` 2.4)
- [ ] 2.2 Release notes generated from the merged OpenSpec changes

## 3. Privacy gate

- [ ] 3.1 `cargo-deny` bans for HTTP/network client crates (from `privacy-safeguards` 3.2)
