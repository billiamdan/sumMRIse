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

## Risks / Trade-offs

- [C++ toolchain on Windows CI] → use the MSVC toolchain already on the runner; fall
  back to `charls-vcpkg` if CMake builds are fragile.
- [Binary size] → measure; CharLS adds a few hundred KB.
