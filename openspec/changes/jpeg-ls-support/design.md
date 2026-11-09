## Context

Moved from `image-decoding-and-volumes` 4.2. dicom-rs exposes JPEG-LS through the
`charls` feature of `dicom-transfer-syntax-registry`, which builds CharLS from source
(CMake, C++), or `charls-vcpkg` on Windows.

## Decisions

- **Feature-gated, on for releases.** `summrise` gets a `jpeg-ls` feature; xtask and the
  CI package job enable it. Contributors without CMake can still build and test.
- **CharLS over a pure-Rust port for now** — it is the reference implementation used by
  DCMTK and GDCM; revisit when a pure-Rust decoder passes the CharLS conformance images.
- **Fixture via encoding.** The testkit transcodes an existing phantom series to
  JPEG-LS Lossless with the CharLS encoder, so the test checks bit-exact round trips.

## Evaluation (2026-11-09)

- `pure_jpegls` 2.0.0 (MIT OR Apache-2.0): lossless JPEG-LS only, so near-lossless
  (1.2.840.10008.1.2.4.81) would still be missing; not a dicom-rs pixel data adapter; no
  sign of testing against the CharLS conformance images.
- `radicom-codecs-jpeg-ls` 0.2.0: no licence declared, so it cannot be used.
- CharLS 2.4.5 through `charls-sys`: decodes both transfer syntaxes, encodes them for the
  testkit, builds with the CMake and C++ compiler of every CI runner, and cross-compiles for
  the x86_64 slice of the universal macOS app. It adds 356 KB to the release binary.

## Risks / Trade-offs

- [C++ toolchain on Windows CI] → use the MSVC toolchain already on the runner; fall
  back to `charls-vcpkg` if CMake builds are fragile.
- [Binary size] → measure; CharLS adds a few hundred KB.
