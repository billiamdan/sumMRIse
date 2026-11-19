## 1. Third-party notices

- [x] 1.1 The Rust standard library as a component, with the MIT text and where its own notices are
- [x] 1.2 What a crate contains is named and given: Cantarell in `sctk-adwaita` (OFL-1.1, new `xtask/licenses/OFL-1.1.txt`), the KDE protocol definitions in `wayland-protocols-plasma` (LGPL-2.1-or-later), CharLS, Material Symbols
- [x] 1.3 Licences are recognised by their own words: files that only point to licences are kept as notices (`siphasher`, `parking`, `utf8_iter`…), the GNU licences are named, font and Unicode licences are not MIT; `LICENSES/` folders are read (unit tests in `xtask/src/notices.rs`; `crates/ui/src/licences.rs` checks the generated text)
- [x] 1.4 The AppImage runtime is named as a separate program in the heading of the notices

## 2. Background faults

- [x] 2.1 The scan, the image information and the printer search say that they stopped when their thread ended without a result (`a_scan_that_died_goes_back_to_the_welcome_screen` in `crates/ui/src/app.rs`)

## 3. Privacy

- [x] 3.1 Paths of opened media at debug level only, in the application and in DICOMDIR warnings (`crates/ui/tests/log_privacy.rs`, with a folder named after the patient)
- [x] 3.2 Documents say what the recent list keeps (the path as it is) and what the network gate checks (the client crates named in `deny.toml`)

## 4. Third-party licences window

- [x] 4.1 Page Up, Page Down, arrows, Home and End; lines broken at 100 characters (`crates/ui/tests/licences.rs`, unit test of the line breaking)

## 5. Specifications and documents

- [x] 5.1 `dicom-conformance`: the JPEG-LS scenario says "Yes (release builds)"
- [x] 5.2 Settings folder named correctly (`com.summrise.Viewer`), dead README link, print queue on macOS and Linux only, glibc 2.35, `--ignore-missing` for the checksums
