## Why

The repository was a Cargo workspace in name only: the real application was a 1 165-line
GTK4 `src/main.rs`, while `gui-macos`/`gui-windows` were `todo!()` stubs and the
`rendering` (wgpu) and `llm` crates were never called. Dependency versions conflicted
(glib 0.18 vs 0.19), `Cargo.lock` was git-ignored, `.gitignore` excluded every
`config.toml` (including `.cargo/config.toml`), `Cargo_simple.toml` pointed at a file
that does not exist, and the macOS build required Homebrew GTK. None of the three
platforms could be built and shipped reproducibly from one repository.

## What Changes

- **BREAKING** Replace the root package + seven crates with a layered workspace
  monorepo: `crates/{imaging,dicom,core,ui,testkit}`, `apps/desktop` (the `summrise`
  binary), `xtask` (build automation) and `packaging/{macos,windows,linux,icons}`.
- **BREAKING** Remove dead code: `gui-linux`, `gui-macos`, `gui-windows`, `rendering`,
  `llm`, `Cargo_simple.toml`, `build.sh` (kept in git history).
- Pin the toolchain (`rust-toolchain.toml`, Rust 1.99.1; cross targets are added on
  demand by `cargo xtask`) and move to edition 2024 with resolver 3.
- Centralise versions in `[workspace.dependencies]`, lints in `[workspace.lints]`
  (no `unsafe`, clippy `-D warnings`), build profiles (thin LTO for release, optimised
  dependencies in dev).
- Update every retained library to its current release and drop unused ones
  (see design.md, "Dependency updates").
- Commit `Cargo.lock`; add `rustfmt.toml`, `clippy.toml`, `deny.toml`, `.editorconfig`,
  a corrected `.gitignore` that also blocks real DICOM data from being committed.
- Add `cargo xtask` (icons, macOS bundle, synthetic fixtures, local CI).

## Capabilities

### New Capabilities
- `build-system`: how the monorepo is organised, built, linted and tested reproducibly on macOS, Windows and Linux.

### Modified Capabilities
<!-- None: no specs existed before this change. -->

## Impact

- Every source file moves; the GTK4 UI is replaced (see `adopt-cross-platform-ui`).
- Contributors need only `rustup` (the pinned toolchain installs itself) and, on Linux,
  the usual X11/Wayland development headers. Homebrew GTK is no longer required.
- Downstream changes in this plan build on this layout.
