## 1. Bundle

- [x] 1.1 Info.plist template, entitlements (hardened runtime, no exceptions), PkgInfo
- [x] 1.2 Icon generator (`cargo xtask icons`) → PNG set, `AppIcon.icns` (iconutil), `summrise.ico`
- [x] 1.3 `bundle-macos`: release build per target, `lipo` universal merge, deployment target 11.0
- [x] 1.4 Ad-hoc signing + strict verification; Developer ID path with timestamp
- [x] 1.5 DMG with Applications link (`diskutil image`, `hdiutil` fallback, LZFSE)
- [x] 1.6 Notarization option (`notarytool` + `stapler`)

## 2. Verification on this Mac (macOS 27.2, Apple Silicon)

- [x] 2.1 Universal bundle built; `lipo -info` → x86_64 arm64; `minos 11.0`
- [x] 2.2 `codesign --verify --strict` passes (adhoc, runtime)
- [x] 2.3 Bundled executable launched against the synthetic CD; series decoded, no errors

## Follow-ups (moved out of this change)

- 2.4 Run the x86_64 slice on an Intel Mac or under Rosetta → `signed-releases`
- 2.5 Developer ID signing and notarization (requires Apple Developer account) → `signed-releases`
- 3.1 Native menu bar (File ▸ Open… ⌘O, View, Window, Help) via `muda` → `macos-integration`
- 3.2 Open folders dropped on the Dock icon / "Open With" (Apple Events `application:openURLs:`) → `macos-integration`
- 3.3 Quick Look preview extension for DICOM files (optional) → `macos-integration`
