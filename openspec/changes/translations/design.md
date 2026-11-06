## Context

`summrise-i18n` sits below every other crate and already offered `tr!`, `tr_n!` and
`tr_ctx!` (gettext style: the English text is the key), an in-memory `Catalogue` and
`with_catalogue` for tests, but no catalogue files, no way to choose a language and no
check that catalogues match the code. The features of the last rounds (archive import,
oblique reformats, guide, cine, 3D, display fidelity, PET) used the macros; the older
code wrote English literals directly, also in the lower layers (layout and tool names,
plane names, preset names, modality names, summary labels, warnings, errors).

## Goals / Non-Goals

**Goals:**
- Every text the application says can be translated, where it is written, without
  changing the code again when the translations arrive.
- Translators edit one standard file per language; one command brings all files up to
  date after a change of the code; a test fails when code and files disagree.
- Changing the language takes effect at once and changes nothing else.

**Non-Goals:**
- The translations themselves (the second phase, one language each).
- Right-to-left languages, CJK fonts, localised number and date formats (numbers keep a
  decimal point, dates stay ISO `YYYY-MM-DD`; the application shows no month names),
  translated DICOM attribute names, documentation in other languages.

## Decisions

### D1 — Gettext style on `summrise-i18n` (binding)
The English text is the key; `tr!`, `tr_n!` and `tr_ctx!` are the only entry points;
values go in named placeholders (`tr!("Image {number} / {count}", …)`), numbers are
formatted before they are passed in. `tr_ctx!` now also takes values.

Why: the code stays readable (the text is where it is shown); a missing translation
falls back to English for free; labels of the lower layers are translated where they are
written, without a UI-side mapping; PO is the standard format translators and their
tools know; no new run-time dependency.

What it costs: changing the English wording of a message invalidates its translations
(`cargo xtask i18n` drops the old entry and lists it, the new one is untranslated); there
are no per-language number formats.

Alternatives considered:
- Fluent (`fluent-rs`) — rejected: new dependencies, message ids instead of readable
  text in the code, and its strengths (selectors, number formatting) are not needed here.
- Key-based catalogues (`tr("toolbar.contrast")`) — rejected: unreadable code, a key and
  an English catalogue to maintain, and a missing entry shows a key instead of English.

### D2 — PO catalogues compiled in, a parser of our own, plural rules in code (binding)
`crates/i18n/locales/{de,es,fr,ru}.po` are compiled in with `include_str!` and read the
first time a language is used (`summrise_i18n::catalogue`); the parsed catalogue is kept
for the rest of the program (its strings are leaked once per language, which bounds the
memory to the size of the catalogues). The parser (`po.rs`) reads msgctxt, msgid,
msgid_plural, msgstr, msgstr[n], strings over several lines, C escapes and every comment
kind; obsolete entries (`#~`) are read and marked; anything else is an error with its
line number. Fuzzy and untranslated entries are left out of the catalogue, so they show
English.

Plural rules (`plural.rs`) are the CLDR cardinal rules for whole numbers: German and
Spanish one form for 1, French one form for 0 and 1, Russian three forms (1, 21, 31…;
2–4, 22–24…; 0, 5–20, 25–30…). CLDR gives Spanish and French a third category, "many",
for exact multiples of one million only ("un millón de imágenes"); the application
writes counts as digits and never counts that many images or files, so that category is
merged into "other", and the two languages keep the two forms translators and tools
expect (`msgstr[1]` is the plural). The `Plural-Forms` header of each file states the
same rule for tools, and a test compares it with the code.

Alternatives considered:
- Loading catalogues from files next to the program — rejected: one more thing to
  install and to keep in step with the binary.
- The `gettext`/`polib`-style crates — rejected: a new dependency for some 300 lines.
- Evaluating the `Plural-Forms` expression — rejected: an expression parser for four
  fixed languages; the rules are tested instead.

### D3 — The language setting and the system language
`Settings::language: Option<String>`: `None` is "System language" (also for settings
stored before the field existed), `Some(code)` a language of `summrise_i18n::LANGUAGES`;
an unknown code (from a later version) counts as "System language". The settings window
lists "System language (Deutsch)" — naming the language it stands for — then every
language under its own name, never translated, so that everyone finds theirs.

The system's preferred languages come from `sys-locale` in the UI layer
(`summrise_ui::system_languages()`), called by the binary and passed in
`AppOptions::system_languages`, most preferred first. The first one whose base language
the interface has wins (`de-AT`, `de_CH.UTF-8` → German; Ukrainian then Russian →
Russian, as the platforms choose for their own applications); none (`pt-BR`, `zh-Hans`,
`C`) → English. Tests and anything else that leaves `system_languages` empty get English,
whatever the machine's language: tests never depend on where they run.

The app compares the wanted language with the active one at the start of every frame and
installs the catalogue when they differ, then asks for a repaint: a choice made in the
settings shows in the next frame. The active language is global (`ACTIVE` in the i18n
crate) because labels are produced in the lower layers and in background threads (scan
progress, warnings), which must speak the same language as the UI.

Alternatives considered:
- An enum of languages in the settings — rejected: settings stored by a later version
  with another language would fail to load as a whole.
- Detecting the system language inside `SumMRIseApp::new` — rejected: the tests build
  the app the same way and would follow the developer's system language.
- A thread-local language — rejected: background threads produce user-visible text.

### D4 — The sweep (binding scope)
Every user-visible string of `crates/imaging`, `crates/dicom`, `crates/core`,
`crates/ui` and `apps/desktop` goes through the macros, where it is written: label
functions (`Layout::label`, `Tool::label`, `ViewKind::label`, plane names, slab modes,
VOI functions, modality, sex, age units, patient position, scanning sequence),
summary labels and section titles, warnings and errors (`DicomError` and `VolumeError`
now implement `Display` with `tr!`; the `thiserror` attributes cannot call macros),
progress stages, decoder reasons shown to the user. Sentences glued from fragments
became one template; counts use `tr_n!`, which also fixed English "1 images" and
"image(s)" (now "1 image", "2 images").

Not translated: text from the disc; DICOM attribute names and keywords ("Private
Creator" is one); orientation letters; units (mm, HU, SUV, kV, ms…); codes and technical
names (DICOMDIR, JPEG, VR, UID values); the application's name; log messages; text that
only tests see; key letters of shortcuts. The names of modifier keys ("Ctrl", "Alt",
"Option") and "Shift+{key}" are translatable, because keyboards print them differently
(German: Strg).

Ambiguous short texts have a context (`tr_ctx!("contrast control", "Window")`,
`("window level", "L")`, `("3D look", "Bone")` next to `("CT window", "Bone")`,
`("layout", "Images")`, `("mouse tool", "Pan")`, `("sex", "Other")` next to
`("modality", "Other")`, `("inspector section", "Patient")`…) or a comment for
translators: a line comment starting with `Translators:` up to three lines above the
message, which the scanner copies into the PO files as `#.` (a comment that no message
follows is an error). The PO files also give each entry its source files (`#:`).

### D5 — Names that are identities stay English
- Built-in window presets (the CT windows, "Auto", "Full range") are identified by their
  English name (`ViewState::preset`, compared by `is_chosen` and `is_pristine`, stored in
  the PET overlay's scale). They are translated only when shown:
  `WindowPreset::label`, `window::builtin_label`, `ViewState::preset_label`,
  `ContrastChoice::label`. A window stored in the images keeps its name from the disc,
  also when it reads like a built-in one ("LUNG" is shown as the disc says it).
  Alternative considered: translating the names when the presets are made — rejected:
  after a language change the chosen window would no longer be recognised as chosen, and
  `CT_PRESETS` is a constant that cannot call `tr!`.
- Summary sections carry a stable `key` (`Patient`, `Study`, …, `Uptake`), as the rows
  already did (`SummaryRow::key`). Privacy mode hides the section with the key `Patient`;
  the old comparison with the English title would have shown the patient's details in
  every other language. The settings window has a fixed id, so that its title can change.

### D6 — When text follows a language change
Text made on every frame follows at once: everything drawn by the UI, label functions,
the guide (described per series and language), volume errors. The inspector reads the
metadata of the displayed image again when the language changes. Text made once stays in
the language it was made in until the study is opened again: warnings found while reading
the disc (status bar), notifications already shown, the names given to unnamed series and
stored windows ("Series 3", "Window 1"), the reason a series could not be loaded, and the
label of a recent study, which is stored.

Alternative considered: storing every warning as data and translating it when shown —
rejected: a restructuring of the import code for a rare action (changing the language
with a study open); reopening the study gives the new language.

### D7 — Completeness test and `cargo xtask i18n` (binding)
The tooling (`summrise_i18n::tooling`) scans the Rust sources under `src/` of every crate
and app except the testkit, with a small tokenizer (comments, raw and escaped string
literals over several lines, characters and lifetimes). `#[cfg(test)]` items and test
modules (and the modules they declare) are left out. It checks every use: string
literals where the macros take them, values passed as `name = value`, a value for every
placeholder and no value without one, `{n}` in every plural, no braces in a text without
values.

`crates/i18n/tests/completeness.rs` requires, for every language, an entry for every
message (same plural, the language's number of forms, no obsolete entry, the header's
Language and Plural-Forms), every non-empty translation with the placeholders of its
message (a plural form used for one count only may leave out `{n}`: German "ein Bild"),
and, for the languages in `const COMPLETE: &[&str] = &[];`, every entry translated and
not fuzzy. Each translation adds its language to that one line.

`cargo xtask i18n` writes `messages.pot` and every PO file as the test expects them: new
messages added untranslated, translations and translators' comments kept, entries of
removed messages dropped and listed (so that no translation is lost unnoticed), and per
language a summary of what is translated, added, removed or wrong; for a language with at
least one translation it lists what is still untranslated. Its output is canonical — no
dates, references one per line without line numbers (so that moving code does not change
the files), strings on one line unless they contain line breaks — and a second test
checks that the files in the repository are exactly what it writes.

Alternative considered: keeping removed entries as `#~` comments, as `msgmerge` does —
rejected: they would accumulate, and the test requires that no entry is obsolete; the
command lists what it drops instead.

### D8 — Fonts for every language (binding)
`crates/ui/tests/glyphs.rs` renders every character of every translation and of the
language names with egui's bundled fonts alone (Cyrillic included), and the main screens
(welcome, settings, display check, About, the viewer with the CT, reformats, inspector and
guide) in every language with the fonts the application loads on the platform it runs on;
kittest panics on a missing glyph. Until the catalogues are filled this checks the
language names and English.

### D9 — Snapshot helper for translators (binding)
`crates/ui/tests/translation_snapshots.rs` does nothing unless `SUMMRISE_LANGUAGE` is set;
then it shows the app in that language on a system that prefers it (the welcome screen,
the settings, the display check, the About dialog, the viewer with inspector and guide
for the CT, axial and MPR, the toolbar at 1440, 1280 and 960 points with reformats and a
thick slab and with the 3D view, the PET menu, an archive that cannot be opened, the
playback bar of a series with time points) and, with `SUMMRISE_SNAPSHOT_DIR`, saves each
as `<code>-<screen>.png`. It fails, after saving, when a toolbar control is cut off or
covers another. The tour is shared with the glyph test (`tests/tour/mod.rs`).

## Translation quality rules (binding for the translations)

- Translate meaning, not words. Use the terms a patient information leaflet of the
  country would use, and keep the established imaging terms: MRT/CT in German, IRM and
  TDM (or scanner) in French, RM/TC in Spanish, МРТ/КТ in Russian.
- Formal address where the language distinguishes it: Sie, usted, vous, вы.
- Keep placeholders (`{name}`), units, numbers and DICOM terms intact. Placeholders may
  move within the sentence; `{n}` must appear in every plural form that more than one
  count uses (the test says which).
- The statements that SumMRIse is not a medical device and not for diagnosis must stay
  unambiguous and at least as strong as in English.
- Keep labels short enough for the toolbar and the buttons; look at the layouts with the
  snapshot helper and shorten where a label does not fit.
- Orientation letters on images (R, L, A, P, S, I, H, F) are not translated: they are the
  DICOM and radiological convention and must match the scanner's own annotations. The
  same holds for the letters of the coordinate read-out.
- Contexts (`msgctxt`) and comments (`#.`) say where a short text is used; the source
  files (`#:`) show it in the code.
- Use typographic characters the bundled fonts have (the glyph test checks it); prefer a
  plain space to narrow no-break spaces.
- The translations are machine-made and have not been reviewed by native speakers. Say so
  wherever they are presented, until such a review has taken place.

Recurring concepts, as the English text uses them (choose one term per concept and keep
it): *window* — the contrast setting of the grey values (width and level), never a window
of the screen; *level* — the centre of the window (brightness); *reformat* — an image
computed through the volume in another plane (MPR: multiplanar reformatting); *slab* —
several reformat slices shown as one projection (MIP, MinIP, average); *series* — one set
of images the scanner made in one run; *study* — all series of one examination; *image* —
one picture of a series; *scan* — the examination or its images, as in "CT scan"; *uptake
value* — the PET value SUV (standardised uptake value); *colour map* — the colours a PET is
shown in; *fusion* — a PET drawn in colour over a CT or MR; *padding* — the values outside
the scanned circle of a CT; *scanner graphics* — marks and text the scanner stored with the
image (overlays); *lossy compression* — compression that discards detail; *privacy mode* —
hiding the patient's name, ID and birth date; *display check* — the test picture for the
screen.

## Risks / Trade-offs

- [English wording changes invalidate translations] → `cargo xtask i18n` lists dropped
  entries with their text; the completeness test fails for complete languages until the
  new text is translated.
- [Longer translations break layouts] → the toolbar wraps by design; the snapshot helper
  checks the toolbar in the chosen language and shows every main screen; the glyph test
  checks fonts in CI.
- [Global language state in tests] → tests that switch languages hold the
  `with_catalogue` lock and run in their own test binaries (`language`, `glyphs`,
  `translation_snapshots`); everything else runs in English because `AppOptions` gives no
  system language by default.
- [Text made once stays in the old language] → documented (D6); reopening the study
  gives the new language.
- [A changed catalogue rebuilds the workspace] → the catalogues are compiled into the
  lowest crate; acceptable for files that change rarely.
