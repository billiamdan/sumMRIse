## Context

Starting point (commit `56e43a3`): Rust workspace, GTK4 0.7 UI in a single file,
dicom-rs 0.6 used with a hand-rolled (and incorrect, big-endian) pixel decoder, Tokio
"full" pulled into every crate without any async I/O, `thiserror` 1, `reqwest` 0.11,
`wgpu` 0.19 / `winit` 0.29 unused. The user's brief: one monorepo that builds macOS,
Linux and Windows apps, current libraries, modern design, best-practice architecture,
macOS first.

## Goals / Non-Goals

**Goals**
- One repository, one command per platform to build a distributable app.
- Clear layering so domain logic is testable without a UI and reusable by any front-end.
- Reproducible builds: pinned toolchain, committed lockfile, CI on all three OSes.

**Non-Goals**
- Mobile or web targets (the layering keeps them possible, nothing more).
- Shipping platform installers for Windows/Linux in this change (separate changes).

## Decisions

### D1 — Stack: keep Rust, one cross-platform GPU UI (egui/eframe on wgpu)

The brief assumed a Qt app; the code was Rust + GTK4. Options evaluated:

| Option | Verdict |
| --- | --- |
| **Rust + egui/eframe (wgpu)** | **Chosen.** Pure Rust, no system UI toolkit to bundle; wgpu → Metal/DX12/Vulkan; image viewports are custom-drawn anyway; immediate mode suits interactive viewers (precedent: Rerun); MIT/Apache licence matches the project; AccessKit screen-reader support; offscreen UI testing with egui_kittest. |
| Rust + GTK4/libadwaita (status quo) | Native only on Linux. On macOS it needs Homebrew GTK, dozens of dylibs in the bundle and looks foreign; on Windows needs MSYS2 runtime. |
| Qt 6 (C++ or cxx-qt) | Industry-proven for medical viewers, but a full rewrite in C++/QML or a large FFI layer; heavy SDK/licensing (LGPL bundling) and CI cost for a small team. |
| Native per platform (SwiftUI + WinUI 3 + GTK) via UniFFI | Best native look, but three UIs to build, test and keep in parity; display logic would diverge per OS. |
| Tauri 2 + web UI (e.g. Cornerstone3D) | Modern web look; but WebKitGTK WebGL is weak, large pixel buffers cross IPC, two languages, and DICOM decoding would split between Rust and JS. |
| Slint | Good declarative UI, but custom GPU viewports depend on unstable wgpu integration features, and the royalty-free licence requires attribution. |

### D2 — Do not split the tech stack per platform

Only packaging and OS integration differ (bundle formats, signing, menus, file
associations). A single rendering path is a *safety* property for a medical viewer:
contrast, orientation and geometry maths must give identical pixels everywhere, and one
test suite proves it. Because `core` is UI-agnostic, a native shell (e.g. SwiftUI via
UniFFI) can still be added later for one platform without touching the domain layers.

### D3 — Layered crates

```
imaging  (pure maths, no I/O)          ← unit tests, deterministic
   ↑
dicom    (dicom-rs I/O → domain types) ← integration tests with synthetic CDs
   ↑
core     (session, jobs, view state)   ← no UI types
   ↑
ui       (egui)                        ← offscreen render tests
   ↑
apps/desktop (binary, platform glue)
```
`testkit` (dev-only) writes PHI-free fixtures; `xtask` automates builds without extra
tools (uses lipo/codesign/iconutil/diskutil that ship with macOS).

### D4 — No async runtime

All I/O is local file reading, CPU-bound decoding and resampling. Background threads +
rayon + channels (with a repaint "waker") are simpler and faster than Tokio here.

### D5 — Dependency updates

| Crate | Before | After | Note |
| --- | --- | --- | --- |
| dicom-rs (`dicom-object`, `-pixeldata`, …) | 0.5/0.6 | 0.10.1 | real codecs (JPEG, JPEG 2000 via pure-Rust openjp2, RLE, deflate), functional groups |
| gtk4 / gio / glib / cairo | 0.7 / 0.18–0.19 | removed | replaced by eframe/egui 0.37 + wgpu 30 |
| wgpu / winit | 0.19 / 0.29 | (via eframe) | no direct dependency |
| thiserror | 1 | 2 | |
| env_logger | 0.10 | 0.11 | |
| image | 0.24 | 0.25 (xtask only) | |
| tokio, reqwest, config, uuid, chrono, toml, dirs, nalgebra, bytemuck, base64 | various | removed | unused or superseded (glam for geometry, eframe persistence for settings) |
| rfd | — | 0.17 | native file dialogs |
| rust toolchain | 1.70+ (unpinned) | 1.99.1 pinned | egui 0.37 needs ≥ 1.95 |

## Risks / Trade-offs

- [egui widgets are custom-drawn, not native] → platform UI fonts are loaded at run time,
  a restrained design system (`crates/ui/src/theme.rs`) and native file dialogs/menus.
- [egui API churn between minor versions] → versions pinned in the workspace; updates
  are one deliberate change with the offscreen UI test as a safety net.
- [Pinned toolchain download on first build] → one-time ~250 MB, local to the project.

## Migration Plan

Old crates are deleted in the same change; history keeps them. There was no persisted
user data worth migrating (the old JSON cache is intentionally dropped, see
`privacy-safeguards`).

## Open Questions

- Should a SwiftUI shell be built later for macOS-specific polish (Quick Look plug-in,
  Finder "Open With")? Decide after user feedback on the egui build.
