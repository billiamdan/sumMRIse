# Contributing to SumMRIse

Thank you for helping. SumMRIse is a viewer for the scans hospitals give to patients; its
users are patients, relatives and students. It is **not a medical device** and must never
present anything as diagnostic.

## Ground rules

- **No real patient data, ever** — not in the repository, not in issues, not in tests, not
  in screenshots. Tests use the synthetic, PHI-free fixtures of `crates/testkit`
  (`cargo xtask fixtures <folder>` writes a synthetic disc). `.gitignore` blocks `DICOMDIR`
  and `*.dcm`.
- **No network access.** The application must not gain an HTTP or other network client;
  `cargo deny check` enforces it.
- **Nothing derived from patient data on disk** unless the user explicitly exports it, and
  no patient names, IDs or birth dates in logs above debug level or in crash reports.
- **No `unsafe` code** (`unsafe_code = "forbid"` in the workspace lints).
- **Every user-visible string goes through `summrise_i18n::tr!`** (`tr_n!`, `tr_ctx!`),
  and is translated in `crates/i18n/locales/{de,es,fr,ru}.po`: run `cargo xtask i18n`
  after changing interface text, then fill in the new entries. The translations are
  machine-made and not yet reviewed by native speakers; improvements from native speakers
  are very welcome.

## Getting started

Install [rustup](https://rustup.rs); the pinned toolchain installs itself. The release
packages decode JPEG-LS with CharLS, a C++ library, which needs **CMake** and a C++
compiler (Xcode command-line tools, Visual Studio Build Tools, or `build-essential`).

```bash
cargo run -p summrise                 # run the app (debug build, without JPEG-LS)
cargo test --workspace                # tests without CMake
cargo xtask ci                        # what CI checks: fmt, clippy and tests with JPEG-LS, notices
cargo deny check                      # licences, advisories, banned crates
```

On Linux, install the windowing development packages listed in the README.

## Changes

Work is planned as [OpenSpec](https://github.com/Fission-AI/OpenSpec) changes in
`openspec/changes/`: a proposal, a design with the alternatives considered, delta specs
with scenarios, and tasks that can each be verified by a test, a command or a visible
result. Run `openspec validate --all --strict` before opening a pull request. Tasks that
need a real disc, a real window, a printer or a person stay unticked, with the reason, or
move to `field-validation`.

A pull request should:

1. pass `cargo xtask ci` and `openspec validate --all --strict`;
2. update `THIRD-PARTY-NOTICES.txt` with `cargo xtask notices` when dependencies change
   (CI checks it);
3. add a line to the `Unreleased` section of `CHANGELOG.md` for anything a user notices;
4. say plainly what was and was not verified, and on which systems.

## Releases

1. Move the `Unreleased` entries of `CHANGELOG.md` into a section `## [x.y.z] — date`, set
   `version` in `[workspace.package]` of `Cargo.toml`, run `cargo check` (it updates
   `Cargo.lock`), and add the release to `packaging/linux/com.summrise.Viewer.metainfo.xml`.
2. Merge to `main` and let CI pass.
3. Tag the merge commit `vx.y.z` and push the tag. The release workflow checks the tag
   against the version, builds and checks the packages, writes `SHA256SUMS` and publishes
   the GitHub Release with the notes from `CHANGELOG.md`.

Signing secrets (all optional; without them the packages are unsigned): `MACOS_CERT_P12`,
`MACOS_CERT_PASSWORD`, `MACOS_SIGNING_IDENTITY`, `APPLE_API_KEY_P8`, `APPLE_API_KEY_ID`,
`APPLE_API_ISSUER` (see `packaging/macos/README.md`), `WINDOWS_CERT_PFX` and
`WINDOWS_CERT_PASSWORD` (base64 of the PFX file and its password).

## Licence

By contributing you agree that your contribution is licensed under the MIT licence of the
project.
