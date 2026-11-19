# translations Specification

## Purpose
The interface in English, German, Spanish, French and Russian: the language follows the system and can be chosen in the settings; text from the disc is never translated. The translations are machine-made and not yet reviewed by native speakers (the README says so; review is tracked in `field-validation`).
## Requirements
### Requirement: Translated interface
Everything the application itself says SHALL be shown in the language of the interface
when its catalogue translates it: menus, buttons, tooltips, layouts and tools,
annotations, status and progress text, warnings and errors, the welcome screen, the
settings, the inspector's labels and section titles, the plain-language guide, preset
names, the 3D looks, the PET colour maps and fusion texts and the display check. Text
made on every frame SHALL follow a change of language from the next frame on; text made
once while a study was read (warnings, names given to unnamed series and windows) MAY stay
in the language it was made in until the study is opened again.

#### Scenario: Toolbar in German
- **WHEN** the interface language is German and the German catalogue translates the mouse tool "Contrast" as "Kontrast"
- **THEN** the tool button in the toolbar reads "Kontrast" in the frame after the language was chosen

#### Scenario: Count with the right plural form
- **WHEN** the interface language is Russian and a series has 21, 22 or 25 images
- **THEN** the series browser uses the Russian forms for 21 (one), 22 (few) and 25 (many) images, and in French 0 and 1 image take the singular

### Requirement: Text from the disc is never translated
Text from the disc SHALL be shown as it is, in every language: patient and study text,
series descriptions, DICOM values, the names of windows and contrast tables stored in the
images and DICOM attribute names in the full attribute list. A built-in preset SHALL be
translated when shown and SHALL keep its identity: changing the language SHALL NOT change
the window in use.

#### Scenario: Stored window that reads like a built-in one
- **WHEN** a CT stores a window W 1500 / L -600 HU explained as "LUNG" and the interface language is German
- **THEN** the contrast menu shows the stored window as "Lung" (as stored, in title case) and only the built-in window W 1500 / L -600 HU under its German name

#### Scenario: Language change keeps the chosen window
- **WHEN** the built-in lung window (W 1500 / L -600 HU) is chosen and the language is changed from German to English
- **THEN** the lung window stays chosen, its values stay W 1500 / L -600 HU and its name reads "Lung"

### Requirement: Orientation letters stay as on the scanner
Orientation letters on images (R, L, A, P, S, I, H, F) and in the position read-out SHALL
NOT be translated in any language, so that they match the scanner's own annotations.

#### Scenario: Axial CT in Russian
- **WHEN** the interface language is Russian and an axial CT image is shown in the standard orientation
- **THEN** its edges read R (left edge), L (right edge), A (top) and P (bottom), and the read-out of a point 12.5 mm left, 30 mm posterior and 40 mm superior of the origin reads "L 12.5  P 30.0  S 40.0 mm"

### Requirement: English fallback
A message without a translation in the active language SHALL be shown in English, never
as a key, a context or an empty label; translations marked fuzzy SHALL count as missing.

#### Scenario: Catalogue without the message
- **WHEN** the interface language is German and the German catalogue has no translation of "Multiplanar reformatting unavailable: {reason}"
- **THEN** the tooltip of the disabled MPR button reads the English text with the reason filled in

### Requirement: Catalogues match the source code
The catalogues SHALL be PO files, one per language (German, Spanish, French, Russian),
compiled into the application, each with an entry for every message the source code
passes to `tr!`, `tr_n!` or `tr_ctx!`, the plural forms of its language and no entry for
a message the code no longer has. A test SHALL enforce this in both directions and SHALL
require, for the languages listed as complete, a translation of every message with the
same placeholders. `cargo xtask i18n` SHALL bring the template and every PO file up to
date with the source code, keeping existing translations, and SHALL leave up-to-date files
unchanged.

#### Scenario: New message in the code
- **WHEN** a developer adds `tr!("Export images")` and runs the tests
- **THEN** the completeness test fails naming "Export images" for every language, and after `cargo xtask i18n` it passes with the message untranslated (shown in English) in every PO file

#### Scenario: Translation with a lost placeholder
- **WHEN** the German entry of "Image {number} / {count}" is translated without `{count}`
- **THEN** the completeness test fails naming the entry and the missing placeholder

### Requirement: Privacy in every language
Privacy mode SHALL hide the patient's name, ID and birth date in every language,
independent of how the titles of the inspector are translated.

#### Scenario: Translated section title
- **WHEN** the interface language is German, the patient section of the image information is titled "Patient (de)" and privacy mode is switched on
- **THEN** the patient section, the name "SYNTHETIC PHANTOM" and the ID "SYN-0001" are no longer shown, and the other sections stay

### Requirement: Fonts for every language
Every character of the interface in every language SHALL render with the fonts the
application loads on each platform, without missing-glyph boxes; the language names and
the translations SHALL render with the fonts bundled in the application alone.

#### Scenario: Language names
- **WHEN** the settings list English, Deutsch, Español, Français and Русский
- **THEN** every character of the five names renders with the bundled fonts

