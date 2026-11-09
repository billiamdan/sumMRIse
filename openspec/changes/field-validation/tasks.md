## 1. Validation set

- [ ] 1.1 Consent form and PS3.15 de-identification procedure; encrypted storage outside the repository
- [ ] 1.2 Collect discs from ≥ 4 vendors plus one PACS export, including Enhanced multi-frame and JPEG 2000 studies (from `dicom-media-import` 4.2 and `image-decoding-and-volumes` 4.3)

## 2. Comparison with the vendor viewers

- [ ] 2.1 Series list and image counts per study
- [ ] 2.2 Orientation letters, ordering and CT reference values
- [ ] 2.3 Each discrepancy fixed and reproduced as a synthetic testkit fixture

## 3. People

- [ ] 3.1 Screen-reader walkthroughs with VoiceOver, Narrator and Orca (from `adopt-cross-platform-ui` 4.4)
- [ ] 3.2 Usability sessions with non-expert participants — presets, overlays, inspector (from `contrast-windowing` 3.2)

## 4. Privacy

- [ ] 4.1 Review info-level logs produced while opening the validation set (from `privacy-safeguards` 3.3)
- [ ] 4.2 Start a release build with `SUMMRISE_TEST_CRASH=window` on macOS, Windows 10 and 11, and Linux (GNOME and KDE, with and without zenity): the message box "SumMRIse closed unexpectedly" names the report, the report holds no patient data, and the next start shows no notice once the box was closed (without zenity: it shows it once) (from `crash-reports`)
- [ ] 4.3 `SUMMRISE_TEST_CRASH=background` with a study open: "Something went wrong" appears in the window with the path of the report; a release build prints no panic message to standard error (from `crash-reports`)

## 5. Checks moved from the archived changes

Moved on 2026-11-09 when these changes were archived; each keeps its text and says where it came from. They need a real window, real media, a printer, other hardware or people.

- [ ] 5.1 Time to first image measured from a real optical drive (needs hardware; cannot be verified on this Mac without a drive, tracked with `field-validation`) (from `progressive-loading` 4.5)
- [ ] 5.2 The native file dialog with its `zip`/`iso` filter — not verified: the `rfd` dialog cannot be opened by the offscreen tests, and the application window cannot be operated on this machine (from `archive-import` 2.6)
- [ ] 5.3 The `summrise` binary started with an archive argument — not verified: the application is not run here; the same path is covered through `AppOptions::open` (2.5) (from `archive-import` 2.7)
- [ ] 5.4 Archives made on Windows (Explorer's deflate64 for large files) and real hospital downloads — not verified on this machine; tracked with `field-validation` (from `archive-import` 4.8)
- [ ] 5.5 Handles, the feel of dragging and the key bindings tried in the real window on macOS, Windows and Linux (cannot be verified here: the application window cannot be operated from this environment) (from `oblique-reformats` 4.4)
- [ ] 5.6 Review of the texts by patients and by a radiographer or radiologist (needs people; cannot be done on this machine) (from `plain-language-guide` 5.4)
- [ ] 5.7 Check of the classification rules against studies from several vendors (needs real, anonymised discs, which the tests must not use; tracked with `field-validation`) (from `plain-language-guide` 5.5)
- [ ] 5.8 Playback smoothness and the keys tried in the real window on macOS, Windows and Linux (cannot be verified here: the application window cannot be operated from this environment) (from `multi-phase-cine` 5.4)
- [ ] 5.9 Turning with a mouse and a trackpad, the feel of previews and refinement, and the keys tried in the real window on macOS, Windows and Linux (cannot be verified here: the application window cannot be operated from this environment) (from `volume-rendering` 5.4)
- [ ] 5.10 Timing on a slower computer and on a software-rendered Linux virtual machine (cannot be verified here: only this Mac is available; estimated with 2 and 4 threads, design D4) (from `volume-rendering` 5.5)
- [ ] 5.11 The display check and the scanner-graphics key tried in the real window on a calibrated and an uncalibrated screen (cannot be verified here: the application window cannot be operated from this environment) (from `display-fidelity` 7.3)
- [ ] 5.12 A real PET/CT disc from a scanner opened and its SUV compared with the scanner's own values (cannot be verified here: no real patient data may be used; tracked with `field-validation`) (from `pet-support` 6.4)
- [ ] 5.13 The menu, the keys and the overlay tried in the real window on macOS, Windows and Linux (cannot be verified here: the application window cannot be operated from this environment) (from `pet-support` 6.5)
- [ ] 5.14 Review of the translations by native speakers: the translations are machine-made and not yet reviewed (from `translations` 8.2)
- [ ] 5.15 "Show in folder" really opens the file manager on macOS, Windows and Linux. Not verified: the tests check the command and do not start another application, and Windows and Linux are not available on this machine (from `report-viewer` 4.5)
- [ ] 5.16 Clipboard copy when no text is selected (done and tested), with a clear message when the clipboard is unavailable (not possible in general: egui's clipboard route reports no failure; on Linux, where it can refuse images, the message says what to do; design D18) (from `export-and-share` 2.5)
- [ ] 5.17 Printed on a real printer from macOS, and "Save PDF and open…" tried on Windows and Linux without CUPS (cannot be verified here: no printer, and only this Mac; "Save PDF and open…" was run on this Mac against a fake opener only) (from `export-and-share` 4.8)
- [ ] 5.18 Clipboard checked on macOS; Windows and Linux (X11 and Wayland) cannot be verified on this Mac (only the command egui hands to its integration was checked offscreen, not a real paste: the application window cannot be operated here) (from `export-and-share` 5.4)
