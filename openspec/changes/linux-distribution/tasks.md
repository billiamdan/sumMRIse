## 1. Integration

- [x] 1.1 `packaging/linux/com.summrise.Viewer.desktop` (Exec with %f, MimeType application/dicom)
- [x] 1.2 `packaging/linux/com.summrise.Viewer.metainfo.xml` (validated as well-formed XML)
- [x] 1.3 Window app id `com.summrise.Viewer` (`ViewportBuilder::with_app_id`)

## 2. Packages

- [x] 2.1 Tarball in the CI `package` job (configured, not yet run)
- [ ] 2.2 AppImage (e.g. `linuxdeploy`) built on Ubuntu 22.04
- [ ] 2.3 Flatpak manifest + `appstreamcli validate`

## 3. Verification (requires Linux)

- [ ] 3.1 X11 and Wayland sessions (GNOME, KDE)
- [ ] 3.2 Media detection under `/media/$USER` and `/run/media/$USER`
- [ ] 3.3 Portal file dialog inside Flatpak
