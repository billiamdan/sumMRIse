## Why

The interface speaks English only. The users are patients and their relatives; many do
not read English well, and a viewer they cannot read is one they cannot use safely.

## What Changes

- A language setting: "System language" (the default) and English, Deutsch, Español,
  Français and Русский, each listed under its own name. Changing it takes effect from the
  next frame, without restarting, and is remembered. "System language" picks the first of
  the languages the operating system prefers that the interface has (`de-AT` → German),
  else English.
- Everything the application says goes through the translation catalogues: menus,
  buttons, tooltips, layouts and tools, annotations, status and progress text, warnings
  and errors, the welcome screen, the settings, the inspector's labels and section
  titles, the plain-language guide, preset names, the 3D looks, the PET colour maps and
  fusion texts, the display check. What comes from the disc is never translated: patient
  and study text, series descriptions, DICOM values, the names of windows and contrast
  tables stored in the images, DICOM attribute names in the full attribute list.
- A missing translation shows the English text, never a key or an empty label.
- The catalogues are standard PO files, one per language, kept in step with the source
  code by one command (`cargo xtask i18n`) and checked by a test in both directions.
- This change delivers the machinery and the catalogues with every message and no
  translation yet: until the translations arrive (a second phase, one language each),
  every language shows English. The translations will be machine-made and not reviewed
  by native speakers; that review stays open.

Patient safety: orientation letters on the images (R, L, A, P, S, I, H, F) are never
translated, so they keep matching the scanner's own annotations; units, DICOM terms and
values are left as they are; the "not a medical device / not for diagnosis" statements
must stay at least as strong in every language (a rule for the translations). The names
of built-in windows are translated only when shown, so a language change never changes
the window in use.

Privacy: privacy mode found the patient section of the inspector by its English title,
which a translation would have broken; it now finds it by a key that is the same in
every language, and a test covers it with a translated title.

## Capabilities

### New Capabilities
- `translations`: translated interface, text from the disc untranslated, English fallback, plural forms per language, catalogue completeness, fonts for every language.

### Modified Capabilities
- `application-shell`: adds the interface language setting.

## Impact

- `crates/i18n`: PO parser and writer, plural rules, the languages and their compiled
  catalogues (`locales/de.po`, `es.po`, `fr.po`, `ru.po`, template `messages.pot`), the
  tooling behind the completeness test and `cargo xtask i18n`. New dependencies of the
  crate: `thiserror` and `log` (already in the workspace).
- `xtask`: new command `i18n`.
- `crates/imaging`, `crates/dicom`, `crates/core`, `crates/ui`, `apps/desktop`: every
  user-visible string goes through `tr!`, `tr_n!` or `tr_ctx!`.
- API: `Settings::language`, `AppOptions::system_languages`,
  `summrise_ui::system_languages()`, `SummarySection::key`, `WindowPreset::label`,
  `window::builtin_label`, `ViewState::preset_label` / `contrast_label`,
  `ContrastChoice::label`; `DicomError` and `VolumeError` display translated text.
- New dependency of `summrise-ui`: `sys-locale` 0.3 (MIT OR Apache-2.0), already in the
  dependency tree through eframe.
- Settings gain a field (`language`, absent in stored settings = system language); no
  other file-format change.
