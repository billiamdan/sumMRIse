## 1. Build

- [x] 1.1 Evaluate pure-Rust JPEG-LS decoders against CharLS (from `image-decoding-and-volumes` 4.2): on 2026-11-09, `pure_jpegls` 2.0.0 decodes lossless JPEG-LS only (no near-lossless), is not a dicom-rs adapter and has not been run against the CharLS conformance images; `radicom-codecs-jpeg-ls` 0.2.0 declares no licence. CharLS stays (design.md)
- [x] 1.2 `summrise` feature `jpeg-ls` forwarding to `summrise-dicom/jpeg-ls`; enabled by `xtask bundle-macos`, `xtask package-windows`, `xtask package-linux` and with them the CI package jobs and the release workflow
- [x] 1.3 CMake/C++ in CI and a `--features jpeg-ls` test run on macOS, Windows and Linux: the check job runs clippy and the tests with `summrise/jpeg-ls` on all three (CMake and a C++ compiler are on every runner image); `cargo xtask ci` does the same

## 2. Verification

- [x] 2.1 JPEG-LS Lossless fixture in the testkit and a bit-exact decode test: `summrise_testkit::transcode` re-encodes the synthetic series with CharLS; `crates/dicom/tests/jpeg_ls.rs` checks the CT bit for bit after JPEG-LS Lossless and the MR within NEAR after near-lossless
- [x] 2.2 Licence (BSD-3-Clause) and binary-size check: CharLS's BSD 3-Clause text is in `THIRD-PARTY-NOTICES.txt` (from `charls/LICENSE.md`; `charls-sys` itself declares MIT, so cargo-deny does not see it); the release binary on macOS arm64 grows from 27,028,224 to 27,392,640 bytes (+356 KB, +1.3 %)
