## ADDED Requirements

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
