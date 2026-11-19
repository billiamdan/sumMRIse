## MODIFIED Requirements

### Requirement: Third-party notices
`cargo xtask notices` SHALL write `THIRD-PARTY-NOTICES.txt` listing the Rust standard
library and every crate compiled into the release application on the release targets, each
with the licence it is used under and the licences of what it contains (fonts, C++
libraries, protocol definitions), followed by the licence texts that apply, each once and
under the name of its own licence. Files of a crate that only point to its licences SHALL
be kept as notices, with their copyright lines; the text of a licence that a crate offers
as an alternative and that is not chosen SHALL NOT be included. A crate that publishes no
licence file SHALL get the standard text of its licence with a note saying so. `cargo
xtask notices --check` SHALL fail when the file differs from what would be written, and CI
SHALL run it.

#### Scenario: Dependency update that changes a licence text
- **WHEN** a pull request updates a crate whose licence file changed, without running `cargo xtask notices`
- **THEN** the "Licences & advisories" check fails saying that `THIRD-PARTY-NOTICES.txt` is out of date

#### Scenario: Bundled C++ library
- **WHEN** the notices are generated
- **THEN** they list `charls-sys` as `MIT AND BSD-3-Clause` with the BSD 3-Clause text of CharLS by Jan de Vaan and Victor Derks, and state that the software is based in part on the work of the Independent JPEG Group

#### Scenario: Font inside a crate
- **WHEN** the notices are generated
- **THEN** `sctk-adwaita` is listed as `MIT AND OFL-1.1` with the copyright of the Cantarell font and the text of the SIL Open Font License 1.1

#### Scenario: Copyright in a file that points to the licences
- **WHEN** a crate used under MIT has a `COPYING` file that names its copyright holders and points to `LICENSE-MIT` and `LICENSE-APACHE`
- **THEN** that file is among the crate's texts (for `siphasher`: "Copyright 2016-2026 Frank Denis")

#### Scenario: Alternative licence not chosen
- **WHEN** a crate is offered under `Apache-2.0 OR GPL-2.0-only` and used under Apache-2.0
- **THEN** the notices do not contain the GNU General Public License

#### Scenario: Protocol definitions under LGPL
- **WHEN** the notices are generated
- **THEN** `wayland-protocols-plasma` is listed as `MIT AND LGPL-2.1-or-later`, with a note naming the KDE protocol definition winit uses and the text of the LGPL 2.1
