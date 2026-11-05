## 1. Build

- [ ] 1.1 Evaluate pure-Rust JPEG-LS decoders against CharLS (from `image-decoding-and-volumes` 4.2)
- [ ] 1.2 `summrise` feature `jpeg-ls` forwarding to `summrise-dicom/jpeg-ls`; enabled by `xtask bundle-macos` and the CI package job
- [ ] 1.3 CMake/C++ in CI and a `--features jpeg-ls` test run on macOS, Windows and Linux

## 2. Verification

- [ ] 2.1 JPEG-LS Lossless fixture in the testkit and a bit-exact decode test
- [ ] 2.2 Licence (BSD-3-Clause) and binary-size check
