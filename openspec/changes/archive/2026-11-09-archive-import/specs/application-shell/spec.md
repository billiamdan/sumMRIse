## ADDED Requirements

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
