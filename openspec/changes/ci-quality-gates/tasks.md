## 1. Workflow

- [x] 1.1 `check` matrix with fmt, clippy `-D warnings`, tests (`--locked`), Linux lavapipe
- [x] 1.2 `supply-chain` job with cargo-deny and `deny.toml`
- [x] 1.3 `package` matrix (macOS DMG with optional signing, Windows zip, Linux tarball)
- [x] 1.4 Least-privilege permissions, concurrency, caching; YAML validated locally
- [x] 1.5 `cargo xtask ci` local equivalent

## 2. Not verifiable on this machine

- [ ] 2.1 First green run on GitHub Actions (push the branch)
- [ ] 2.2 Confirm the offscreen UI test renders on Windows (WARP) and Linux (lavapipe) runners
- [ ] 2.3 Pin third-party actions to commit SHAs
- [ ] 2.4 Publish tagged builds to GitHub Releases with checksums
