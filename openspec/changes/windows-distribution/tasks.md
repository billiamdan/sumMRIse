## 1. Executable

- [x] 1.1 `windows_subsystem = "windows"` for release builds
- [x] 1.2 `packaging/windows/summrise.ico` (16–256 px) from `cargo xtask icons`
- [ ] 1.3 `build.rs` with `winresource` to embed icon and version info (Windows-only build dependency)

## 2. Distribution

- [x] 2.1 Portable zip in the CI `package` job (configured, not yet run)
- [ ] 2.2 MSI via WiX 4 or `cargo-packager` (per-user, Start menu, uninstall)
- [ ] 2.3 Authenticode signing step gated on secrets

## 3. Verification (requires Windows)

- [ ] 3.1 Launch on Windows 10 and 11; DX12 and WARP rendering
- [ ] 3.2 Open a patient CD from the optical drive; drive-letter media detection
- [ ] 3.3 Settings persisted in `%APPDATA%\SumMRIse`
