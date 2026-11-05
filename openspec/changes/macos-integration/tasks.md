## 1. Menu bar

- [ ] 1.1 Add `muda` (macOS only) and build the menus after `NSApp` exists (from `macos-distribution` 3.1)
- [ ] 1.2 Route menu events to the existing `Action`s; keep shortcuts single-sourced

## 2. Opening from Finder and the Dock

- [ ] 2.1 `platform-macos` crate handling open-document Apple Events with a safe callback (from `macos-distribution` 3.2)
- [ ] 2.2 Declare document types (folders, DICOMDIR, application/dicom) in `Info.plist`
- [ ] 2.3 Verify cold and warm launch via Dock drop and "Open With"

## 3. Optional

- [ ] 3.1 Evaluate a Quick Look preview extension (from `macos-distribution` 3.3)
