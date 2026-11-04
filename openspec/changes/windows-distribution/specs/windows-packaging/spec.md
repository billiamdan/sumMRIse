## ADDED Requirements

### Requirement: Native Windows executable
The release `summrise.exe` SHALL run without a console window and SHALL carry the
SumMRIse icon and version information.

#### Scenario: Explorer view
- **WHEN** the user views `SumMRIse.exe` in Explorer
- **THEN** it shows the SumMRIse icon and the file properties list version 0.2.0

### Requirement: Installer
An MSI installer SHALL install SumMRIse per user, add a Start-menu shortcut and register
an uninstaller; a portable zip SHALL also be provided.

#### Scenario: Uninstall
- **WHEN** the user removes SumMRIse in Settings ▸ Apps
- **THEN** the program files and shortcut are removed (user settings may remain)

### Requirement: Signed binaries
When a code-signing certificate is configured in CI, the executable and MSI SHALL be
Authenticode-signed with a timestamp.

#### Scenario: SmartScreen
- **WHEN** a signed MSI is downloaded and opened
- **THEN** Windows shows the publisher name instead of "Unknown publisher"
