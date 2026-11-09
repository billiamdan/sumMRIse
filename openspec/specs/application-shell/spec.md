# application-shell Specification

## Purpose

The desktop application shell shared by all platforms: opening studies from media, folders or the command line, screens and panels, responsive background loading, appearance, persistence, accessibility and platform conventions.
## Requirements
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

### Requirement: Opening an archive or a disc image
The application SHALL open a ZIP archive or an ISO 9660 disc image chosen with "Open
archive or disc image…" on the welcome screen (⌘Shift+O on macOS, Ctrl+Shift+O on
Windows and Linux, a file dialog that offers `.zip` and `.iso` files), dropped onto the
window, given as the command-line argument, or picked from the recent list, without
unpacking it.

#### Scenario: Welcome screen
- **WHEN** the welcome screen is shown
- **THEN** "Open archive or disc image…" is offered below "Open folder or disc…", its tooltip says that the file is opened without unpacking it and names the shortcut, and the drop hint mentions ZIP archives and disc images

#### Scenario: Dropped download
- **WHEN** the user drops `MRI download.zip` onto the window
- **THEN** the study opens with the same series as the unpacked folder, and the archive appears in the recent list with its path

#### Scenario: Archive on the command line
- **WHEN** the app is launched as `summrise "MRI download.zip"`
- **THEN** it opens the study in the archive without showing the welcome screen first

#### Scenario: Archive that cannot be opened
- **WHEN** the user drops a password-protected archive onto the window
- **THEN** an error notification explains it and the welcome screen remains

#### Scenario: Shortcuts do not collide
- **WHEN** the user presses Ctrl+Shift+O (⌘Shift+O on macOS)
- **THEN** only the dialog for archives and disc images opens, not the folder dialog of Ctrl+O

### Requirement: Display check
The application SHALL offer a display check, opened from the settings window and from the
About dialog and closed with Escape or Close: a picture generated in memory for the pixel
size of the area it fills and drawn 1:1 without smoothing — grey steps 0, 10 … 100 %, a 5 %
square inside the 0 % step and a 95 % square inside the 100 % step, a continuous ramp from
black to white, fine line pairs and a border — with plain instructions on what should be
visible and what to change if it is not, described as a quick check in the style of the
SMPTE pattern and not as a calibration or a compliance claim; it SHALL involve no patient
data.

#### Scenario: Patches at their pixels
- **WHEN** the picture is generated for 800 × 600 pixels
- **THEN** the 0 % step reads 0 around the 5 % square, which spans columns 43–72 and rows 102–131 and reads 13, the 50 % step reads 128 at column 399, row 117, and the 95 % square reads 242

#### Scenario: Another window size
- **WHEN** the picture is generated for 1280 × 720 pixels
- **THEN** every step reads round(p × 255) for its level p at its centre, and the outermost pixel row is white

#### Scenario: Opening from the settings
- **WHEN** the user presses Ctrl/Cmd+comma, moves to "Check the display…" with Tab and presses Space
- **THEN** the display check covers the window with its instructions, including "It is not a calibration", and Escape closes it

### Requirement: Interface language
The settings SHALL offer the language of the interface: "System language" (the default,
naming the language it stands for) and English, Deutsch, Español, Français and Русский,
each listed under its own name. "System language" SHALL use the first of the languages
the operating system prefers whose base language the interface has, else English. A
change SHALL take effect from the next frame without restarting and SHALL be remembered
across launches.

#### Scenario: Austrian German system
- **WHEN** the operating system prefers de-AT and the setting is "System language"
- **THEN** the interface is German and the setting reads "System language (Deutsch)"

#### Scenario: Unsupported system language
- **WHEN** the operating system prefers pt-BR, then zh-Hans
- **THEN** the interface is English

#### Scenario: Choosing a language
- **WHEN** the user chooses "Français" in the settings while a study is open
- **THEN** the toolbar and the open panels are French from the next frame on, and the app starts in French at the next launch

#### Scenario: Settings stored by an earlier version
- **WHEN** the stored settings have no language
- **THEN** the setting is "System language"

### Requirement: Crash notice
After a fault in the program, the application SHALL tell the user in plain words what
happened, that the crash report holds no patient data, and where the report is: in a
message box of the system when the fault closed the window, in the window when the fault
stopped part of the program and the window lives on, and at the next start when the
message box could not be shown. When the window cannot be opened, a message box SHALL say
so and why.

#### Scenario: Fault on the window's thread
- **WHEN** a panic on the main thread ends the event loop
- **THEN** a message box titled "SumMRIse closed unexpectedly" names the path of the report, and the next start shows no notice once the user has closed it

#### Scenario: Fault on a background thread
- **WHEN** a background task panics while the window is open
- **THEN** the window shows "Something went wrong" with the path of the report and a "Copy path" button

#### Scenario: Message box not available
- **WHEN** the message box after a crash could not be shown
- **THEN** the next start shows "SumMRIse closed unexpectedly" with the path of the report, once

### Requirement: Third-party licences
The About dialog SHALL name SumMRIse's licence and offer "Third-party licences…", which
shows the text of `THIRD-PARTY-NOTICES.txt` from within the application in place of the
dialog; Close and Escape SHALL close it.

#### Scenario: Reading the licences
- **WHEN** the user presses F1 and chooses "Third-party licences…"
- **THEN** the window explains that the licences follow in their original wording and that the same text is in `THIRD-PARTY-NOTICES.txt`, and shows that text from its beginning

