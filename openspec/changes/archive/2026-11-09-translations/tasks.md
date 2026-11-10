## 1. Catalogues and languages (`summrise-i18n`)

- [x] 1.1 PO parser and writer: contexts, plurals, strings over several lines, escapes, comments, obsolete entries; malformed input is an error with its line (unit tests)
- [x] 1.2 Plural rules for de, es, fr, ru (CLDR, whole numbers) tested at 0, 1, 2, 5, 11, 21, 22, 25 and 101; Plural-Forms headers checked against the code
- [x] 1.3 Languages under their own names; system language tags to a language: `de-AT`, `es-MX`, `fr-CA`, `ru-KZ` select their base language, unknown and unsupported tags English (unit tests)
- [x] 1.4 Catalogues compiled in with `include_str!` and read on first use; untranslated and fuzzy entries show English; `with_catalogue` also stands in for the compiled catalogue of its language (unit tests)
- [x] 1.5 Source scanner (literals of `tr!`, `tr_n!`, `tr_ctx!`, multi-line and escaped literals, `Translators:` comments, test code left out, placeholder checks), template, update and check (unit tests)

## 2. Keeping catalogues in step

- [x] 2.1 `cargo xtask i18n`: writes `messages.pot`, brings the four PO files up to date (keeps translations, drops and lists removed messages) and reports per language
- [x] 2.2 Completeness test in both directions with `const COMPLETE: &[&str] = &[];` (`crates/i18n/tests/completeness.rs`)
- [x] 2.3 Round trip: the PO files are exactly what `cargo xtask i18n` writes; a second run changes nothing (test, and two runs of the command with no change)

## 3. Language setting

- [x] 3.1 `Settings::language` (absent in stored settings = system language; unit test)
- [x] 3.2 Settings row "Language" with "System language (…)" and the five languages under their own names
- [x] 3.3 System languages from `sys-locale`, passed in `AppOptions::system_languages`; applied at the start of every frame
- [x] 3.4 Test: choosing Deutsch in the settings changes the toolbar in the next frame, and back (`crates/ui/tests/language.rs`, test catalogue)
- [x] 3.5 Test: system languages `de-AT`, `de_CH.UTF-8`, `es-MX`, `fr-CA`, `ru-KZ` pick their base language; `pt-BR`, `zh-Hans-CN`, `C` English

## 4. The sweep

- [x] 4.1 imaging: plane names, slab modes, VOI functions, 3D looks (context), volume errors, labels of built-in presets
- [x] 4.2 dicom: errors, decoder reasons, progress stages, warnings with plurals, modality, sex, age, patient position, scanning sequence, summary labels and section titles, attribute-tree texts
- [x] 4.3 core: layouts, tools, views, loader reasons, the recent label, preset labels for display
- [x] 4.4 ui and desktop: every screen, tooltip and status text; sentences as templates, counts with `tr_n!`
- [x] 4.5 Contexts and translator comments for ambiguous short texts (Window, L, W, Bone, Images, Pan, Other, Volume, Scale…)
- [x] 4.6 Identities stay English: built-in presets translated when shown; summary sections found by key; privacy test with a translated patient section title (fails with the old title comparison)
- [x] 4.7 English snapshots of the main screens unchanged in substance (compared pixel by pixel with renderings from before the sweep; the differences are "1 image" for "1 images", the new Language row and timing)

## 5. Catalogue files

- [x] 5.1 `crates/i18n/locales/de.po`, `es.po`, `fr.po`, `ru.po`: every message with source references, contexts and comments, empty translations, header with Language and Plural-Forms
- [x] 5.2 Template `crates/i18n/locales/messages.pot`

## 6. Fonts and snapshots

- [x] 6.1 Glyph test: every translation and language name with the bundled fonts; the main screens in every language with the fonts the app loads
- [x] 6.2 Snapshot helper `crates/ui/tests/translation_snapshots.rs` (`SUMMRISE_LANGUAGE`, `SUMMRISE_SNAPSHOT_DIR`), with a toolbar fit check; run in English and looked at
- [x] 6.3 Glyph test on Windows and Linux (Segoe UI, Noto/DejaVu): `crates/ui/tests/glyphs.rs` passes on the windows-latest and ubuntu-24.04 CI runners since the Windows check is back (PR #8): Segoe UI is on the Windows runner, and the Linux job installs `fonts-noto-core` and `fonts-dejavu-core`
- [x] 6.4 Corner annotations never overlap: blocks in opposite corners share the width and long lines are shortened with "…"; the scale bar is left out where it does not fit (`widgets::share_row`, `widgets::elide_lines`, unit tests; reformat layout looked at at 960 and 1440 points)
- [x] 6.5 The status bar keeps its right-hand text (number of images, source, warnings) in place and shortens the hint on the left ("…", in full on hover) instead of running under it; the playback bar wraps onto another row, each label with its control, and the image-set menu narrows to the room there is (checked from 1440 to 960 points in `viewer/toolbar_tests.rs`, and in every language by the snapshot helper's fit check)

## 7. Verification

- [x] 7.1 `cargo xtask ci` passes
- [x] 7.2 `openspec validate --all --strict` passes

## 8. Translations (second phase, open)

- [x] 8.1 German, Spanish, French and Russian translations, each language added to `COMPLETE` (second phase, one language each; 853 of 853 messages each, snapshot helper looked at in every language)
- 8.2 Moved to `field-validation` 5.14 when this change was archived: Review of the translations by native speakers: the translations are machine-made and not yet reviewed
