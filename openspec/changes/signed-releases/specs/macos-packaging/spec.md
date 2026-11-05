## ADDED Requirements

### Requirement: Gatekeeper-accepted releases
Released DMGs SHALL contain an app signed with a Developer ID certificate, notarized and
stapled, so that Gatekeeper accepts it on a Mac that did not build it.

#### Scenario: First launch on another Mac
- **WHEN** a user downloads the release DMG and opens SumMRIse on a different Mac
- **THEN** `spctl --assess --type execute` accepts the app and no "unidentified developer" warning appears

### Requirement: Intel compatibility verified
Each release SHALL be launched on an Intel Mac or under Rosetta before publication.

#### Scenario: Rosetta smoke test
- **WHEN** the universal executable is started with `arch -x86_64` on Apple Silicon
- **THEN** it opens the synthetic CD and displays the CT series
