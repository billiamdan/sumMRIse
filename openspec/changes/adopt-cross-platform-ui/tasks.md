## 1. Shell

- [x] 1.1 `SumMRIseApp` with Welcome / Scanning / Viewer screens and central action handling
- [x] 1.2 Native folder dialog (rfd), drag & drop with overlay, CLI path argument
- [x] 1.3 Background `Job`/`Stream` with UI waker; scanning progress and cancel
- [x] 1.4 Notifications (info/warning/error toasts, click to dismiss)
- [x] 1.5 Settings window (appearance, annotations, privacy, interpolation, recent list, memory limit) and About modal
- [x] 1.6 Persistence of settings and recent list via eframe storage

## 2. Viewer chrome

- [x] 2.1 Toolbar: layouts, tools, window presets, W/L fields, invert, reset, slab, panel toggles
- [x] 2.2 Series browser: study header, modality chips, thumbnails (streamed), load progress, errors
- [x] 2.3 Status bar: hover pixel/patient position/value, source (DICOMDIR or scan), index warnings
- [x] 2.4 Keyboard shortcuts (⌘O, ⌘I, ⌘B, ⌘,, F1, arrows, PgUp/PgDn, Home/End, R, I, O, M, A, 1–9)

## 3. Design system

- [x] 3.1 Palette, typography scale, spacing and radii for dark and light themes
- [x] 3.2 Platform UI/monospace fonts at run time; OS fallback for other scripts
- [x] 3.3 Vector logo matching the app icon
- [x] 3.4 Removable-media detection on a background thread (macOS `/Volumes`, Linux mounts, Windows drives)

## 4. Verification

- [x] 4.1 `egui_kittest` offscreen smoke test (welcome, CT, MPR, 2×2, attributes, MR, colour image)
- [x] 4.2 Accessibility check passes (labels for preset combo, W/L fields, slab controls)
- [x] 4.2a Platform shortcut labels via `widgets::shortcut`/`command_key` (⌘ only on macOS; Windows CI caught a hard-coded ⌘)
- [x] 4.2b Glyph test: every non-ASCII character in UI text renders with egui's bundled fonts; ⌘ only from the helpers
- [x] 4.3 Manual run of the bundled macOS app against the synthetic CD
- [ ] 4.4 Manual check with a screen reader (VoiceOver) — not yet done
