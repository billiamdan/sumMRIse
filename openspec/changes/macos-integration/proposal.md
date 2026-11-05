## Why

SumMRIse ships as a signed, universal `.app`, but on macOS it still behaves like a
cross-platform app: there is no native menu bar (File ▸ Open…, Window, Help), and a disc
folder cannot be opened by dropping it on the Dock icon or via Finder's "Open With".

## What Changes

- Native macOS menu bar (App, File, View, Window, Help) wired to the existing actions and
  shortcuts (⌘O, ⌘B, ⌘I, ⌘,), built with `muda`.
- Handle "open documents" Apple Events so dropping a folder, DICOMDIR or DICOM file on the
  Dock icon, or choosing SumMRIse in "Open With", opens it on cold and warm launch.
- Declare document types in `Info.plist` once handling works.
- Evaluate a Quick Look preview extension (optional).

## Capabilities

### New Capabilities
- `macos-integration`: native menu bar and opening files/folders from Finder and the Dock.

### Modified Capabilities
<!-- None. -->

## Impact

macOS-only code in the desktop binary or a small `platform-macos` crate;
`packaging/macos/Info.plist`. Other platforms are unaffected.
