## 1. Executable

- [x] 1.1 `windows_subsystem = "windows"` for release builds
- [x] 1.2 `packaging/windows/summrise.ico` (16–256 px) from `cargo xtask icons`
- [x] 1.3 `build.rs` with `winresource` to embed icon and version info (built only on Windows hosts; the CI package job checks `ProductName` and `ProductVersion` of `SumMRIse.exe`)

## 2. Distribution

- [x] 2.1 Portable zip from `cargo xtask package-windows`, with `LICENSE.txt` and `THIRD-PARTY-NOTICES.txt`
- [x] 2.2 MSI via WiX 5 (`packaging/windows/SumMRIse.wxs`, `cargo xtask package-windows --msi`): per user in `%LOCALAPPDATA%\Programs\SumMRIse`, Start-menu shortcut, uninstall, upgrades; the CI package job installs it silently, checks the program, the notices and the shortcut, removes it and checks that they are gone (passed in CI run 42684689436, with ProductVersion 0.3.0 in the version resource)
- [ ] 2.3 Authenticode signing step gated on secrets — implemented (`package-windows --sign`, signtool with a SHA-256 timestamp and `signtool verify`, run when `WINDOWS_CERT_PFX` and `WINDOWS_CERT_PASSWORD` exist), but never run: there is no code-signing certificate yet

## 3. Verification (requires Windows)

- [ ] 3.1 Launch on Windows 10 and 11; DX12 and WARP rendering — needs a Windows desktop; CI starts only `SumMRIse.exe --version` and the offscreen UI tests
- [ ] 3.2 Open a patient CD from the optical drive; drive-letter media detection — needs a drive and a disc
- [ ] 3.3 Settings persisted in `%APPDATA%\SumMRIse` — needs the real window
- [ ] 3.4 Install the MSI by double-click on Windows 10 and 11: no administrator prompt, Start-menu entry with icon, removal in Settings ▸ Apps, what SmartScreen says about the unsigned MSI — needs a Windows desktop (CI installs it silently on Windows Server only)

## 4. CI

- [x] 4.1 Re-enable the Windows lint & test job (paused on 2026-11-05 to save CI time; back in the check matrix with JPEG-LS)
