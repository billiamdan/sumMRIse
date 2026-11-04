## 1. Workspace layout

- [x] 1.1 Remove `gui-linux`, `gui-macos`, `gui-windows`, `rendering`, `llm`, `src/`, `Cargo_simple.toml`, `build.sh`
- [x] 1.2 Create `crates/{imaging,dicom,core,ui,testkit}`, `apps/desktop`, `xtask`, `packaging/`
- [x] 1.3 Virtual root manifest: edition 2024, resolver 3, `[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`, profiles
- [x] 1.4 Pin toolchain in `rust-toolchain.toml` (1.99.1 + rustfmt, clippy; host target only, xtask adds cross targets)

## 2. Dependencies

- [x] 2.1 Upgrade dicom-rs 0.6 → 0.10.1 with pure-Rust codecs (`native`, `openjp2`)
- [x] 2.2 Replace gtk4/glib/cairo with eframe/egui 0.37 (wgpu backend)
- [x] 2.3 Drop tokio, reqwest, config, uuid, chrono, toml, dirs, nalgebra, bytemuck, base64
- [x] 2.4 thiserror 2, env_logger 0.11, image 0.25 (xtask), rfd 0.17, glam 0.34
- [x] 2.5 Licence audit of the full graph and `deny.toml` allow-list (no GPL/LGPL-only crates)

## 3. Tooling

- [x] 3.1 Commit `Cargo.lock`; fix `.gitignore` (stop ignoring `config.toml`, block DICOM data)
- [x] 3.2 `rustfmt.toml`, `clippy.toml`, `.editorconfig`, `.cargo/config.toml` (`cargo xtask` alias)
- [x] 3.3 `cargo xtask ci|icons|bundle-macos|fixtures`
- [x] 3.4 `cargo clippy --workspace --all-targets -- -D warnings` passes
- [x] 3.5 `cargo test --workspace` passes on macOS
- [ ] 3.6 Same commands pass on Windows and Linux (verified by the `ci-quality-gates` workflow; not verifiable on this Mac)
