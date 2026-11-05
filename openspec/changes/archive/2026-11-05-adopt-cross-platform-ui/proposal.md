## Why

The GTK4 UI only worked on Linux, ran blocking DICOM I/O on the UI thread, showed debug
artefacts (red borders, green size text) over patient images, hard-coded a fake API
key in Settings, and had no keyboard, accessibility or layout model. macOS — the
primary target — had no UI at all. Users need a modern, consistent viewer on every
platform that stays responsive while large studies load from slow optical media.

## What Changes

- **BREAKING** Replace GTK4 with a single egui/eframe 0.37 UI rendered through wgpu
  (Metal on macOS, DirectX 12 on Windows, Vulkan/GL on Linux) — see the stack analysis
  in `restructure-monorepo/design.md`.
- Application shell with three screens: Welcome (open folder/disc, drop target, recent
  studies, detected discs), Scanning (progress + cancel), Viewer (toolbar, series
  browser with thumbnails, viewport grid, inspector, status bar).
- All disc and decoding work runs on background threads; the UI wakes on completion.
- Design system: dark theme by default (light/system selectable), platform UI fonts
  loaded at run time (SF on macOS, Segoe UI on Windows, Noto/DejaVu on Linux) with OS
  font fallback for non-Latin patient names, Material Symbols icons, consistent spacing.
- Keyboard shortcuts, drag and drop, command-line path argument, persisted settings.
- Accessibility: every control exposes an accessible name (checked in tests).

## Capabilities

### New Capabilities
- `application-shell`: screens, navigation, opening media, layout panels, shortcuts, appearance, persistence and accessibility of the desktop app.

### Modified Capabilities
<!-- None. -->

## Impact

- New crates `summrise-ui` and `summrise-core`; binary `apps/desktop`.
- Settings are stored by eframe in the platform data directory
  (`~/Library/Application Support/SumMRIse` on macOS).
- Viewing behaviour itself is specified in `multiplanar-viewing`, `contrast-windowing`
  and `metadata-inspector`.
