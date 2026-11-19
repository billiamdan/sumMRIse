## MODIFIED Requirements

### Requirement: Crash notice
After a fault in the program, the application SHALL tell the user in plain words what
happened, that the crash report holds no patient data, and where the report is: in a
message box of the system when the fault closed the window, in the window when the fault
stopped part of the program and the window lives on, and at the next start when the
message box could not be shown. When the window cannot be opened, a message box SHALL say
so and why. A screen that was waiting for background work that a fault stopped — opening
a folder, the image information, the search for printers — SHALL say that it stopped and
SHALL NOT go on waiting.

#### Scenario: Fault on the window's thread
- **WHEN** a panic on the main thread ends the event loop
- **THEN** a message box titled "SumMRIse closed unexpectedly" names the path of the report, and the next start shows no notice once the user has closed it

#### Scenario: Fault on a background thread
- **WHEN** a background task panics while the window is open
- **THEN** the window shows "Something went wrong" with the path of the report and a "Copy path" button

#### Scenario: Message box not available
- **WHEN** the message box after a crash could not be shown
- **THEN** the next start shows "SumMRIse closed unexpectedly" with the path of the report, once

#### Scenario: The scan stops
- **WHEN** the thread that scans an opened folder ends without a result
- **THEN** "Opening stopped unexpectedly." is shown and the welcome screen is back

### Requirement: Third-party licences
The About dialog SHALL name SumMRIse's licence and offer "Third-party licences…", which
shows the text of `THIRD-PARTY-NOTICES.txt` from within the application in place of the
dialog, with lines of at most 100 characters; it SHALL scroll with the mouse and with
Page Up, Page Down, the arrow keys, Home and End, and Close and Escape SHALL close it.

#### Scenario: Reading the licences
- **WHEN** the user presses F1 and chooses "Third-party licences…"
- **THEN** the window explains that the licences follow in their original wording and that the same text is in `THIRD-PARTY-NOTICES.txt`, and shows that text from its beginning

#### Scenario: From the keyboard
- **WHEN** the user presses End and then Home in that window
- **THEN** the last line of the text is shown, and then the first again
