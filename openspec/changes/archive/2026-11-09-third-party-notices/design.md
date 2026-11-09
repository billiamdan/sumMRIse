## Context

`cargo deny` already checks that every licence in the graph is allowed, but it does not
collect the texts. Eighty crates of the release graph publish no licence file (egui,
dicom-rs, objc2, AccessKit among them), and `charls-sys` declares MIT while the CharLS
code it compiles is BSD-3-Clause.

## Decisions

- **Our own generator in xtask** reading `cargo metadata` for each release target.
  *Rejected:* `cargo-about` — another tool to install for contributors and CI, and its
  output needs the same clarifications for CharLS and the fonts.
- **One file for all packages**: the union of the four targets; a superset is correct for
  each package.
- **No versions in the file**: a version bump changes it only when a licence text
  changes, so Dependabot updates rarely need it regenerated.
- **Published texts first**: the crate's top-level licence files for the licence chosen
  (MIT where a choice includes it), and licence files further down for bundled code and
  fonts. A crate without a file gets the standard text from `xtask/licenses/` and a note
  saying so. A short table names what files do not say (CharLS, the Material Symbols font,
  the IJG code in `jpeg-encoder`).
- **Shown from memory**: the text is compiled into the application and drawn row by row,
  so it can be read in every package, including the portable ones.

## Risks / Trade-offs

- [A new licence without a standard text] → the generator stops and names it.
- [Clarifications drift] → they are checked by the unit test of the About text (CharLS
  copyright, IJG acknowledgement, OFL) and reviewed when dependencies change.
