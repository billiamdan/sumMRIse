## Context

Moved from `macos-distribution` (3.1 menu bar, 3.2 Dock/"Open With", 3.3 Quick Look).
The UI is egui/eframe on winit; winit creates a default app menu but exposes no API for
custom menus or for open-document Apple Events.

## Decisions

- **Menus with `muda`** (MIT/Apache, used by Tauri): create the menu after `NSApp`
  exists (in the eframe creation callback) and forward menu events to the existing
  `Action`s through a channel, so shortcuts stay defined in one place.
- **Apple Events in an isolated crate.** Registering a handler for `kAEOpenDocuments`
  needs Objective-C runtime calls (`objc2`). The workspace forbids `unsafe`, so this lives
  in a tiny `platform-macos` crate with a reviewed `#![allow(unsafe_code)]`, exposing a
  safe `on_open(paths)` callback.
- **Quick Look** would be a separate Swift app extension; only pursue it if cheap.

## Risks / Trade-offs

- [Double-handled shortcuts (menu accelerator and egui)] → menu items carry the
  accelerator for display; egui ignores the shortcut when the menu consumed it.
- [Events arriving before the UI exists on cold launch] → queue paths until the app is created.
