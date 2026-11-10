## 1. Generator

- [x] 1.1 `cargo xtask notices [--check]` from `cargo metadata` of the four release targets with `jpeg-ls` (unit tests for licence expressions, file recognition and wrapping)
- [x] 1.2 Standard texts in `xtask/licenses/` with their sources; clarifications for CharLS, the Material Symbols font, the fonts of egui and the IJG code
- [x] 1.3 `THIRD-PARTY-NOTICES.txt` generated (469 components, 194 texts); `--check` in `cargo xtask ci` and the CI supply-chain job

## 2. Shipping and showing

- [x] 2.1 In every package next to `LICENSE.txt`: app resources and DMG, Windows zip and MSI, Linux tarball and AppImage (`xtask` packaging; the CI package jobs check that it is there)
- [x] 2.2 About ▸ Third-party licences, drawn row by row (UI test `crates/ui/tests/licences.rs`, unit test of the text); interface texts translated
