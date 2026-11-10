## ADDED Requirements

### Requirement: Third-party notices
`cargo xtask notices` SHALL write `THIRD-PARTY-NOTICES.txt` listing every crate compiled
into the release application on the release targets with the licence it is used under,
followed by the licence texts that apply, each once; a crate that publishes no licence
file SHALL get the standard text of its licence with a note saying so. `cargo xtask
notices --check` SHALL fail when the file differs from what would be written, and CI
SHALL run it.

#### Scenario: Dependency update that changes a licence text
- **WHEN** a pull request updates a crate whose licence file changed, without running `cargo xtask notices`
- **THEN** the "Licences & advisories" check fails saying that `THIRD-PARTY-NOTICES.txt` is out of date

#### Scenario: Bundled C++ library
- **WHEN** the notices are generated
- **THEN** they list `charls-sys` with the BSD 3-Clause text of CharLS by Jan de Vaan and Victor Derks, and state that the software is based in part on the work of the Independent JPEG Group
