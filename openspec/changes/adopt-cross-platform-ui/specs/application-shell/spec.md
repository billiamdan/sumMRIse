## ADDED Requirements

### Requirement: Opening a study
The application SHALL open a folder, a mounted disc, a `DICOMDIR` or a single DICOM
file chosen via the native folder dialog (⌘O / Ctrl+O), dropped onto the window, given
as the first command-line argument, or picked from the recent and detected-disc lists.

#### Scenario: Drag and drop
- **WHEN** the user drops a folder containing a DICOM CD copy onto the window
- **THEN** a "Drop to open" overlay is shown while hovering and the folder is scanned after the drop

#### Scenario: Command-line path
- **WHEN** the app is launched as `summrise /Volumes/MRI_CD`
- **THEN** it starts scanning that path without showing the welcome screen first

### Requirement: Responsive loading with progress and cancel
Scanning and decoding SHALL run off the UI thread, show the current stage with a
progress bar (done/total), and be cancellable; the UI SHALL keep responding.

#### Scenario: Cancel a slow scan
- **WHEN** the user clicks Cancel while a disc is being indexed
- **THEN** the background work stops and the welcome screen is shown again

#### Scenario: Unreadable folder
- **WHEN** the chosen folder contains no DICOM images
- **THEN** an error notification "no DICOM images were found in <path>" is shown and the welcome screen remains

### Requirement: Viewer layout
The viewer SHALL show a toolbar, a resizable series browser (left), the viewport area,
a resizable image-information inspector (right) and a status bar; the browser and
inspector SHALL be toggled with ⌘B and ⌘I (Ctrl on Windows/Linux).

#### Scenario: Default series
- **WHEN** a study with a localizer, an axial CT stack and a dose report opens
- **THEN** the axial CT stack is selected and displayed automatically

#### Scenario: Several studies on one disc
- **WHEN** the disc contains two studies
- **THEN** a notification says so and the browser offers a study picker

### Requirement: Welcome screen
The welcome screen SHALL offer "Open folder or disc…", list discs/drives whose root
contains a DICOMDIR, list recent studies (with remove and clear), and state that images
stay on the computer and that the app is not a medical device.

#### Scenario: Inserted patient CD
- **WHEN** a CD with a DICOMDIR is mounted while the welcome screen is open
- **THEN** it appears under "Discs and drives with images" with an Open button within a few seconds

### Requirement: Appearance and persistence
The application SHALL support System, Dark and Light appearance (viewports always
black) and SHALL persist settings, window geometry and the recent list across launches.

#### Scenario: Restart keeps preferences
- **WHEN** the user selects Light appearance, hides the inspector and restarts the app
- **THEN** the app starts in Light appearance with the inspector hidden

### Requirement: Accessible controls
Every interactive control SHALL expose an accessible name to the platform screen
reader (AccessKit), including icon-only buttons, combo boxes and numeric fields.

#### Scenario: Automated accessibility check
- **WHEN** the offscreen UI smoke test renders the viewer
- **THEN** no widget without an accessible name is reported

### Requirement: Platform conventions in UI text
Shortcut hints SHALL follow the platform convention (⌘ on macOS, "Ctrl+" on Windows and
Linux), and all other non-ASCII characters in UI text SHALL be renderable with the fonts
bundled in the application, independent of installed system fonts.

#### Scenario: Shortcut hint on Windows
- **WHEN** the user hovers the inspector toggle on Windows
- **THEN** the tooltip reads "Image information (Ctrl+I)"

#### Scenario: Shortcut hint on macOS
- **WHEN** the user hovers the same toggle on macOS
- **THEN** the tooltip reads "Image information (⌘I)"

#### Scenario: Minimal system fonts
- **WHEN** the platform UI font is unavailable and only egui's bundled fonts are present
- **THEN** every label and tooltip renders without missing-glyph boxes

