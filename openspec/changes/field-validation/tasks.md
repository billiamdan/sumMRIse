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
