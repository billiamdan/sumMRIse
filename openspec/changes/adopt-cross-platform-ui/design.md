## Context

A DICOM viewer's central element is a custom-drawn, GPU-backed image canvas; the
surrounding chrome is panels, lists, a toolbar and forms. Large CD studies (hundreds of
slices, often JPEG-compressed) take seconds to decode.

## Goals / Non-Goals

**Goals:** responsive UI under load; identical behaviour on all OSes; modern,
restrained visual design; keyboard and screen-reader accessibility; testable UI.

**Non-Goals:** native widget look-alike per OS; localisation (planned separately);
macOS menu-bar integration (tracked in `macos-distribution`).

## Decisions

- **eframe `App::ui` + panels.** Top toolbar, left browser, right inspector, bottom
  status bar and a central viewport grid (`egui::Panel`, `CentralPanel`). Immediate
  mode keeps view state in plain structs owned by `summrise-core`.
- **Background jobs with a waker.** `core::jobs::Job`/`Stream` run work on named
  threads (rayon inside) and call `ctx.request_repaint()` when done; the UI polls once
  per frame. No async runtime.
- **Render cache.** Each viewport keeps a texture keyed by (slice/position, window,
  invert, slab, interpolation); images are re-rendered only when an input changes.
- **Design tokens** in `crates/ui/src/theme.rs` (palette, type scale, radii, spacing).
  Viewports stay black in both themes for image perception. View colours follow the
  3D Slicer convention (axial red, coronal green, sagittal yellow).
- **Platform fonts at run time** — never redistributed; eframe's `system_fonts`
  provider supplies OS fallback fonts for CJK/Cyrillic names.
- **Icons** from `egui_material_icons` (Apache-2.0 Material Symbols, outlined).
- **Offscreen UI tests** with `egui_kittest` + wgpu: the smoke test opens the synthetic
  CD, switches layouts/tabs/series and renders PNGs; kittest's built-in accessibility
  check fails the test for unlabeled widgets.

## Risks / Trade-offs

- [Custom-drawn widgets feel less native] → platform fonts, native file dialogs (rfd),
  restrained styling; native menu bar planned.
- [CPU resampling for MPR] → ~1–5 ms per 512² view with rayon; slab projections scale
  with thickness. A GPU (wgpu compute) path is a future optimisation, not needed now.
