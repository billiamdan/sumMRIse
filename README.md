# SumMRIse

A native desktop viewer for the **MRI, CT and PET scans hospitals give patients on CDs,
DVDs and USB sticks**. Open the disc — or the ZIP file or disc image you were sent — see
every series in the correct anatomical orientation, adjust contrast with clinical
presets, reformat volumes into any plane, look at them in 3D, and read what each series
and parameter means in plain language.

Runs on **macOS** (Apple Silicon and Intel, macOS 11+), **Windows** and **Linux** from a
single Rust code base. Everything happens on your computer — nothing is uploaded.

> **SumMRIse is not a medical device.** It is intended for personal and educational
> viewing and must not be used for diagnosis. Discuss your images with your doctor.

![Viewer showing a synthetic CT phantom](docs/screenshots/viewer-ct.png)

| Multiplanar reformatting (2 × 2) | Sagittal MR with reformats |
| --- | --- |
| ![MPR](docs/screenshots/mpr-quad.png) | ![MR](docs/screenshots/mr-sagittal.png) |

*Screenshots use the synthetic phantom from `crates/testkit`; no real patient data.*

## Features

- **Opens patient media the standard way** — reads the `DICOMDIR` index (PS3.10/12),
  ignores the viewer software and autorun files on the disc, falls back to a folder
  scan (and says why), resolves ISO 9660 file names, detects inserted discs
  automatically, and shows the first images while the rest of a series loads.
- **Downloads too** — opens ZIP archives and disc images (ISO 9660, with Joliet names)
  in place: nothing is unpacked to disk; password-protected archives and UDF-only
  images are refused with a reason.
- **Correct images** — dicom-rs 0.10 decoding of uncompressed, deflate, JPEG
  (baseline/extended/lossless), JPEG 2000 and RLE data; Hounsfield units for CT;
  MONOCHROME1/2, colour and palette colour; Modality and VOI lookup tables
  ("contrast tables"); padding outside the scanned area; scanner graphics (overlay
  planes, G); a mark on images that were compressed lossily; multi-frame and Enhanced
  multi-frame geometry. What is supported is listed in the
  [DICOM conformance statement](docs/dicom-conformance-statement.md).
- **All projections** — acquired images in their own plane plus axial, coronal and
  sagittal reformats in radiological orientation for any acquisition plane (also
  oblique and gantry-tilted CT), planes that can be tilted freely, linked crosshair,
  MIP/MinIP/average slabs; any view can be turned and mirrored, and a mirrored view
  says so.
- **Multi-phase series and cine** — series with several time points, echoes or
  b-values are split into image sets (echoes and b-values keep a contrast each); cine
  and dynamic series play at the rate the scanner recorded.
- **3D view** — the volume as a shaded object (CT: bone, skin, vessels and bone; MR:
  surface; maximum intensity projection), turned with the mouse or keys, with standard
  views; it is labelled as a computed illustration.
- **PET** — standardised uptake values (SUV, body weight) computed from the headers, with
  every input listed and the reason when they cannot be computed; inverted grey, hot
  iron and rainbow colour maps; a PET drawn over the CT or MR of the same scan,
  aligned by the scanner's positions only.
- **Contrast** — DICOM VOI functions (LINEAR, LINEAR_EXACT, SIGMOID), windows stored by
  the scanner, 11 clinical CT presets, SUV windows for PET, robust auto window,
  drag/numeric adjustment, invert.
- **Plain-language guide** — what each series is ("CT scan", "MR scan, T2-weighted"),
  where the image on screen lies in the body, and what the parameters and contrast
  presets mean; it can be switched off in the settings.
- **Metadata** — curated overview (patient, study, series, equipment, MR/CT/PET
  acquisition parameters with units, image geometry) and the complete searchable
  attribute tree.
- **Private by default** — no network access, no image cache on disk, privacy mode for
  screenshots (with a warning on images whose pixels contain patient details),
  minimal recent-studies list.
- **Five languages** — English, German, Spanish, French and Russian; the interface
  follows the language of the system and can be changed in the settings. Text from the
  disc is never translated. The translations are machine-made and have not yet been
  reviewed by native speakers.
- **Modern, accessible UI** — dark/light themes, platform fonts, keyboard shortcuts,
  screen-reader labels, trackpad gestures, drag and drop, a toolbar that wraps in narrow
  windows, and a display check to see whether the screen shows all grey levels.

## Download and build

Prerequisite: [rustup](https://rustup.rs). The exact toolchain (Rust 1.99.1) is pinned
in `rust-toolchain.toml` and installs itself on first use.

```bash
cargo run --release -p summrise                 # run the app
cargo run --release -p summrise -- /Volumes/CD  # open a disc/folder directly
```

### macOS app bundle

```bash
cargo xtask bundle-macos --universal --dmg
```

Produces `dist/macos/SumMRIse.app` (universal arm64 + x86_64, ad-hoc signed, hardened
runtime) and `dist/macos/SumMRIse-<version>-macos-universal.dmg`. Developer ID signing
and notarization: see [packaging/macos/README.md](packaging/macos/README.md).

### Windows and Linux

```bash
cargo build --release -p summrise
```

Linux needs the usual X11/Wayland development packages (Debian/Ubuntu:
`libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev`).
Desktop entry and AppStream metadata live in `packaging/linux`. CI builds a Windows zip
and a Linux tarball; installers (MSI, AppImage, Flatpak) are planned — see the plan below.

## Using SumMRIse

| Action | Mouse / trackpad | Keyboard |
| --- | --- | --- |
| Next / previous image | scroll, two-finger swipe | ↑ ↓ (← → too, unless the series has image sets), Page Up/Down (±10), Home/End |
| Image sets (time points, echoes) and cine | play bar under the views | ← → switch sets, Space = play/pause |
| Contrast (window/level) | drag with *Contrast* tool, or right-drag | presets and contrast tables 1–9, A = auto, R = reset, I = invert |
| Zoom | pinch, ⌘/Ctrl-scroll, *Zoom* tool | double-click = fit |
| Pan | middle-drag, Shift-drag, *Pan* tool | |
| Move crosshair (MPR) | click | |
| Tilt a reformatted plane | drag the handles on the crosshair lines | Alt+← → (1°, with Shift 5°), Alt+R = straighten |
| Turn and mirror a view | *Rotate and flip* menu | ] and [ = quarter turn right / left, H = mirror left–right, V = mirror top–bottom |
| 3D view | drag = turn, Shift-drag = pan | arrows = turn (Shift: 45°), Shift+A/P/L/R/S/I = standard views, L = next look |
| PET | *PET colours and fusion* menu | C = next colour map, F = PET over CT/MR and back |
| Scanner graphics | toggle in the status bar | G |
| Layouts | toolbar *Images / MPR / 2 × 2 / 3D* | M toggles MPR |
| Panels | | ⌘B browser, ⌘I inspector, O annotations, ⌘, settings, F1 about |
| Open | drop a folder, ZIP or disc image on the window | ⌘O folder or disc, ⇧⌘O archive or disc image |

On Windows and Linux, Ctrl takes the place of ⌘.

## Architecture

```text
sumMRIse/
├── apps/desktop/        summrise binary: window, logging, CLI argument
├── crates/
│   ├── i18n/            translation macros, PO catalogues (de, es, fr, ru), completeness checks
│   ├── imaging/         pure maths: LPS geometry, volumes, MPR, 3D rendering, VOI windowing, colour maps
│   ├── dicom/           DICOM I/O (dicom-rs): DICOMDIR/scan, ZIP/ISO media, indexing, decoding, metadata, SUV
│   ├── core/            UI-agnostic app layer: session, background jobs, view state, settings, guide texts
│   ├── ui/              egui/eframe interface (wgpu → Metal / DirectX 12 / Vulkan)
│   └── testkit/         synthetic, PHI-free DICOM CDs for tests and QA
├── xtask/               cargo xtask: icons, bundle-macos, fixtures, ci
├── docs/                DICOM conformance statement, screenshots
├── packaging/           macOS Info.plist & icns, Windows ico, Linux desktop/AppStream
└── openspec/            specification-driven change plan (see below)
```

Dependencies point one way: `i18n ← imaging ← dicom ← core ← ui ← apps/desktop`. Only `ui` and
the binary know about egui, so the domain layers can be reused by another front-end
(for example a native SwiftUI shell via FFI) without changes. Why one cross-platform UI
instead of a different stack per platform, and why egui rather than Qt, GTK, Tauri or
Slint, is documented in
[openspec/changes/restructure-monorepo/design.md](openspec/changes/restructure-monorepo/design.md).

## Development

```bash
cargo xtask ci                      # fmt check + clippy -D warnings + all tests
cargo test --workspace              # unit, integration and offscreen UI tests
cargo xtask fixtures /tmp/test-cd   # write a synthetic patient CD for manual testing
cargo xtask icons                   # regenerate app icons
cargo xtask i18n                    # update the PO catalogues after changing interface text
SUMMRISE_SNAPSHOT_DIR=/tmp/shots cargo test -p summrise-ui --test smoke   # keep UI renders
```

Never commit real imaging data — `.gitignore` blocks `DICOMDIR` and `*.dcm`; use the
testkit fixtures instead. Licences and advisories are checked with `cargo deny check`.

## Plan (OpenSpec)

Work is organised as [OpenSpec](https://github.com/Fission-AI/OpenSpec) changes
(`openspec list`, `openspec show <change>`; in Claude Code use `/opsx:propose`,
`/opsx:apply`, `/opsx:archive`). Shipped changes are archived in
`openspec/changes/archive/`, and their requirements form the baseline specifications in
`openspec/specs/`: build system, application shell, DICOM import, image decoding, image
viewing, contrast windowing, metadata inspector, privacy, macOS packaging and continuous
integration.

**Shipped** (PR #1, archived 2026-11-05): `restructure-monorepo`,
`adopt-cross-platform-ui`, `dicom-media-import`, `image-decoding-and-volumes`,
`multiplanar-viewing`, `contrast-windowing`, `metadata-inspector`, `privacy-safeguards`,
`macos-distribution`, `ci-quality-gates`.

**Implemented, not yet archived:** `progressive-loading` (PR #4, #5); `archive-import`,
`oblique-reformats`, `plain-language-guide`, `multi-phase-cine`, `volume-rendering`,
`display-fidelity`, `pet-support`, `translations` (branch `feat/tier2-tier3`). Their open tasks — above
all checks with real vendor discs and in the real window on every platform — are
listed in each change's `tasks.md`.

**Planned — viewing features** (from a comparison with professional viewers; suggested
order as listed, `series-comparison` before `measurement-tools`):

| Change | Scope |
| --- | --- |
| `report-viewer` | Structured reports, PDF and text documents, dose reports |
| `series-comparison` | Side-by-side layouts, synchronised scrolling, cross-reference lines, compare studies |
| `measurement-tools` | Length, angle and region statistics, presented as approximate |
| `export-and-share` | Save and copy views, export a series as images, private by default |

**Planned — releases and platforms:**

| Change | Scope |
| --- | --- |
| `signed-releases` | Developer ID signing and notarization, Intel check, tagged GitHub Releases with checksums, no-network dependency gate |
| `macos-integration` | Native menu bar; open folders from the Dock and Finder ("Open With") |
| `jpeg-ls-support` | JPEG-LS decoding (CharLS) in release builds |
| `field-validation` | Real vendor CDs, screen readers, usability sessions, log privacy review |
| `windows-distribution` | Executable resources, MSI installer, Authenticode signing; re-enable the Windows CI check (paused to save CI time) |
| `linux-distribution` | AppImage and Flatpak |

Later ideas (not yet proposed): de-identified DICOM export, and — only with
de-identification and explicit consent — AI explanations.

## Licence

MIT — see [LICENSE](LICENSE). Third-party crates are permissively licensed (checked by
`cargo deny`); platform UI fonts are loaded from the operating system at run time.
