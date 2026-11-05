## ADDED Requirements

### Requirement: Screen-reader operable core flows
The core flows SHALL be operable with VoiceOver (macOS), Narrator (Windows) and Orca
(Linux): opening a study, selecting a series, changing the image, choosing a window
preset and reading the image information.

#### Scenario: VoiceOver walkthrough
- **WHEN** a VoiceOver user moves through the viewer toolbar
- **THEN** every control announces a meaningful name and, for toggles, its state

#### Scenario: Changing the window preset without a mouse
- **WHEN** a keyboard-only user opens the window preset menu and selects "Lung"
- **THEN** the image updates and the screen reader announces the selected preset
