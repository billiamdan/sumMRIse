## ADDED Requirements

### Requirement: Desktop integration
SumMRIse SHALL install a desktop entry and AppStream metadata with application id
`com.summrise.Viewer`, and its windows SHALL use the same app id.

#### Scenario: Launcher and taskbar
- **WHEN** the user starts SumMRIse from the application launcher
- **THEN** the running window is grouped under the SumMRIse icon in the dock/taskbar

### Requirement: Portable packages
Releases SHALL include a tarball and an AppImage that run on current Ubuntu, Fedora and
Debian releases without installing additional libraries beyond a desktop session. Both
are built on Ubuntu 22.04 (glibc 2.35) and carry the licence and
`THIRD-PARTY-NOTICES.txt`; the AppImage is made with pinned, checksum-verified tools and
starts without FUSE when `APPIMAGE_EXTRACT_AND_RUN=1` is set.

#### Scenario: AppImage on a CI runner
- **WHEN** the AppImage is started with `--version` and `APPIMAGE_EXTRACT_AND_RUN=1` on Ubuntu 22.04
- **THEN** it prints `SumMRIse 0.3.0`

#### Scenario: AppImage on Fedora
- **WHEN** the AppImage is made executable and started on Fedora Workstation (Wayland)
- **THEN** the welcome screen appears

### Requirement: Sandboxed distribution
A Flatpak manifest SHALL build SumMRIse against the freedesktop runtime with access to
user-selected files through portals and read access to removable media.

#### Scenario: Open a CD in the Flatpak
- **WHEN** the user chooses the mounted CD in the portal file dialog
- **THEN** the study opens
