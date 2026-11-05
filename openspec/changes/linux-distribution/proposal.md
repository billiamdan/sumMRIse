## Why

The old app was Linux-first but shipped only as `cargo run`. Linux users expect a
desktop entry, an icon, AppStream metadata and a package that runs on common
distributions (AppImage and/or Flatpak), on both X11 and Wayland.

## What Changes

- Desktop entry `com.summrise.Viewer.desktop` and AppStream
  `com.summrise.Viewer.metainfo.xml`; the window app id matches for taskbar grouping —
  implemented.
- CI tarball with binary, desktop entry, metainfo and icon — configured.
- AppImage built in CI (oldest supported glibc runner).
- Flatpak manifest (freedesktop runtime; file access via the document portal, which the
  native file dialog already uses).
- Verify on X11 and Wayland sessions, and media detection under `/media`, `/run/media`.

## Capabilities

### New Capabilities
- `linux-packaging`: packaging and desktop integration on Linux.

### Modified Capabilities
<!-- None. -->

## Impact

`packaging/linux`, CI `package` job. Not verifiable on the macOS development machine.
