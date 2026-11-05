## ADDED Requirements

### Requirement: Native menu bar
On macOS the application SHALL provide a native menu bar with File ▸ Open… (⌘O) and Close
Study, View toggles for the series browser (⌘B), inspector (⌘I) and annotations, a
Window menu and Help ▸ About, each performing the same action as the in-window control.

#### Scenario: Open from the menu bar
- **WHEN** the user chooses File ▸ Open… in the macOS menu bar
- **THEN** the folder dialog opens exactly as with ⌘O

### Requirement: Open from Finder and the Dock
The application SHALL open a folder, DICOMDIR or DICOM file dropped onto its Dock icon
or chosen via Finder's "Open With", whether or not the app is already running.

#### Scenario: Cold launch from the Dock
- **WHEN** the user drops a CD folder on the Dock icon while SumMRIse is closed
- **THEN** the app launches and starts scanning that folder

#### Scenario: App already running
- **WHEN** the user chooses SumMRIse in "Open With" for a DICOMDIR while a study is open
- **THEN** the new disc is scanned and replaces the current study
