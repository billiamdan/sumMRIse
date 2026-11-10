## ADDED Requirements

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
