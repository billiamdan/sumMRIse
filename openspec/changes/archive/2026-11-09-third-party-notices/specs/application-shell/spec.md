## ADDED Requirements

### Requirement: Third-party licences
The About dialog SHALL name SumMRIse's licence and offer "Third-party licences…", which
shows the text of `THIRD-PARTY-NOTICES.txt` from within the application in place of the
dialog; Close and Escape SHALL close it.

#### Scenario: Reading the licences
- **WHEN** the user presses F1 and chooses "Third-party licences…"
- **THEN** the window explains that the licences follow in their original wording and that the same text is in `THIRD-PARTY-NOTICES.txt`, and shows that text from its beginning
