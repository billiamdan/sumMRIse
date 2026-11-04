## ADDED Requirements

### Requirement: Cross-platform verification
Every push and pull request SHALL run clippy with warnings as errors and the full test
suite with `--locked` on macOS, Windows and Linux, and a formatting check.

#### Scenario: Platform-specific regression
- **WHEN** a change compiles on macOS but not on Windows
- **THEN** the Windows `check` job fails and the pull request shows a failing status

### Requirement: Supply-chain checks
CI SHALL fail when a dependency has a disallowed licence, a known security advisory, a
yanked version, or comes from an unknown registry or git source.

#### Scenario: GPL-only dependency
- **WHEN** a pull request adds a crate licensed only under GPL-3.0
- **THEN** the `supply-chain` job fails citing the licence

### Requirement: Packaged artefacts
Pushes to `main`, version tags and manual runs SHALL produce a macOS universal DMG, a
Windows x64 zip and a Linux x64 tarball as downloadable workflow artefacts; the macOS
build SHALL be Developer ID–signed when signing secrets are configured.

#### Scenario: Tagged release
- **WHEN** tag `v0.2.0` is pushed
- **THEN** the run publishes `SumMRIse-macos-universal`, `SumMRIse-windows-x64` and `SumMRIse-linux-x64` artefacts
