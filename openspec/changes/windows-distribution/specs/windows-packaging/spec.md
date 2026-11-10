## ADDED Requirements

### Requirement: Native Windows executable
The release `summrise.exe` SHALL run without a console window and SHALL carry the
SumMRIse icon and version information.

#### Scenario: Explorer view
- **WHEN** the user views `SumMRIse.exe` in Explorer
- **THEN** it shows the SumMRIse icon and the file properties list version 0.3.0

### Requirement: Installer
An MSI installer SHALL install SumMRIse per user, without administrator rights, into
`%LOCALAPPDATA%\Programs\SumMRIse`, add a Start-menu shortcut and register an
uninstaller; a newer version SHALL replace an older one. A portable zip SHALL also be
provided. Both carry `LICENSE.txt` and `THIRD-PARTY-NOTICES.txt`.

#### Scenario: Uninstall
- **WHEN** the user removes SumMRIse in Settings ▸ Apps
- **THEN** the program files and shortcut are removed (user settings may remain)

#### Scenario: Silent install and removal
- **WHEN** `msiexec /i SumMRIse-0.3.0-windows-x64.msi /qn` and then `msiexec /x … /qn` run
- **THEN** `SumMRIse.exe`, `THIRD-PARTY-NOTICES.txt` and the Start-menu shortcut exist after the first and are gone after the second

### Requirement: Signed binaries
When a code-signing certificate is configured in CI, the executable and MSI SHALL be
Authenticode-signed with a timestamp.

#### Scenario: SmartScreen
- **WHEN** a signed MSI is downloaded and opened
- **THEN** Windows shows the publisher name instead of "Unknown publisher"
