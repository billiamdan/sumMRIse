## 1. Workflow

- [x] 1.1 `check` matrix with fmt, clippy `-D warnings`, tests (`--locked`), Linux lavapipe
- [x] 1.2 `supply-chain` job with cargo-deny and `deny.toml`
- [x] 1.3 `package` matrix (macOS DMG with optional signing, Windows zip, Linux tarball)
- [x] 1.4 Least-privilege permissions, concurrency, caching; YAML validated locally
- [x] 1.5 `cargo xtask ci` local equivalent
- [x] 1.6 Advisory policy in `deny.toml`: vulnerabilities and yanked crates fail; "unmaintained" fails for direct dependencies only, known transitive ones documented; internal path dependencies allowed as wildcards (`publish = false`)
- [x] 1.7 Reproduce the CI cargo-deny run locally with the same version (0.20.2) and arguments
- [x] 1.8 Cross-lint the workspace for Windows and Linux targets from macOS (`cargo clippy --target …`)

## 2. Not verifiable on this machine

- [ ] 2.1 First green run on GitHub Actions (push the branch)
- [ ] 2.2 Confirm the offscreen UI test renders on Windows (WARP) and Linux (lavapipe) runners — Linux and macOS confirmed; Windows rendered the welcome screen, full test pending the glyph fix
- [x] 2.3 Pin third-party actions to commit SHAs (checkout v7.0.1, rust-cache v2.9.3, cargo-deny-action v2.1.1, upload-artifact v7.0.2; Node 24)
- [ ] 2.4 Publish tagged builds to GitHub Releases with checksums
