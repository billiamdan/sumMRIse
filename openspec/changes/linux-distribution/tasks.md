## 1. Integration

- [x] 1.1 `packaging/linux/com.summrise.Viewer.desktop` (Exec with %f, MimeType application/dicom)
- [x] 1.2 `packaging/linux/com.summrise.Viewer.metainfo.xml` (validated as well-formed XML)
- [x] 1.3 Window app id `com.summrise.Viewer` (`ViewportBuilder::with_app_id`)

## 2. Packages

- [x] 2.1 Tarball from `cargo xtask package-linux`, with `LICENSE.txt` and `THIRD-PARTY-NOTICES.txt`
- [x] 2.2 AppImage built on Ubuntu 22.04 with appimagetool 1.9.1 and the type2 runtime 20251108, both pinned by SHA-256 (`package-linux --appimagetool … --appimage-runtime …`); the CI package job starts it with `--version` (extract-and-run, no FUSE)
- [x] 2.3 `desktop-file-validate` and `appstreamcli validate --no-net` in the CI package job (passed with Ubuntu 22.04's appstream 0.15.2: one pedantic note, no warnings)
- [ ] 2.4 Flatpak manifest — not written: building and testing one needs flatpak-builder with the freedesktop SDK and the Rust extension on a Linux machine, and offline cargo sources for Flathub; left for a Linux session

## 3. Verification (requires Linux)

- [ ] 3.1 X11 and Wayland sessions (GNOME, KDE), tarball and AppImage — needs a Linux desktop
- [ ] 3.2 Media detection under `/media/$USER` and `/run/media/$USER` — needs removable media
- [ ] 3.3 Portal file dialog inside Flatpak — needs 2.4
