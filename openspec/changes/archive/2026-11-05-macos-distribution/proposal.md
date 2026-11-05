## Why

macOS is the priority platform, yet there was no macOS build at all: the GTK app needed
Homebrew libraries and produced a bare binary without bundle, icon, signature or
installer. Users expect a signed `.app` in a `.dmg` that runs on Apple Silicon and Intel
Macs and passes Gatekeeper.

## What Changes

- `cargo xtask bundle-macos` builds `SumMRIse.app` (Info.plist with bundle id
  `com.summrise.Viewer`, version from the workspace, minimum macOS 11, icon, PkgInfo).
- `--universal` builds `aarch64` + `x86_64` and merges them with `lipo`
  (`MACOSX_DEPLOYMENT_TARGET=11.0`).
- Code signing with hardened runtime: ad-hoc by default, Developer ID with
  `--sign`, plus `codesign --verify --strict`.
- `--dmg` creates a compressed disk image with an Applications link (`diskutil image`,
  falling back to `hdiutil` on older systems); `--notarize <profile>` notarizes and
  staples via `xcrun notarytool`.
- Procedurally generated app icon (`cargo xtask icons` → PNG set, `.icns`, `.ico`).

## Capabilities

### New Capabilities
- `macos-packaging`: producing, signing and distributing the macOS application bundle.

### Modified Capabilities
<!-- None. -->

## Impact

`xtask`, `packaging/macos/{Info.plist,entitlements.plist,AppIcon.icns,README.md}`,
`packaging/icons`. Distribution outside the developer's Mac requires an Apple Developer
account (Developer ID certificate + notarization).
