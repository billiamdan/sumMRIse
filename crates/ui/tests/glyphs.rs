//! Every non-ASCII character used in UI text must render with egui's bundled
//! fonts alone, so the interface never depends on which system fonts a user
//! (or a CI runner) happens to have. A hard-coded "⌘" once rendered on macOS
//! and Linux but had no glyph in Windows' Segoe UI.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use egui_kittest::Harness;

/// Shown only on macOS, whose UI font has the glyph, and only via
/// `widgets::command_key` / `widgets::shortcut`.
const PLATFORM_ONLY: char = '⌘';

/// Non-ASCII characters inside string literals of the crates whose text is
/// displayed, with the files they occur in. Comments and unit tests (which
/// contain sample data) are ignored.
fn ui_symbols() -> BTreeMap<char, BTreeSet<String>> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut symbols: BTreeMap<char, BTreeSet<String>> = BTreeMap::new();
    for dir in ["ui/src", "core/src", "dicom/src", "imaging/src"] {
        for entry in std::fs::read_dir(crates.join(dir)).expect("source dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let file = format!("{dir}/{}", path.file_name().unwrap_or_default().to_string_lossy());
            let source = std::fs::read_to_string(&path).expect("source file");
            let production = source.split("#[cfg(test)]").next().unwrap_or_default();
            for line in production.lines() {
                let code = line.split("//").next().unwrap_or_default();
                let mut in_string = false;
                let mut escaped = false;
                for ch in code.chars() {
                    match ch {
                        '\\' if in_string => escaped = !escaped,
                        '"' if !escaped => in_string = !in_string,
                        _ => {
                            if in_string && !ch.is_ascii() {
                                symbols.entry(ch).or_default().insert(file.clone());
                            }
                            escaped = false;
                        }
                    }
                }
            }
        }
    }
    symbols
}

#[test]
fn command_glyph_only_comes_from_the_shortcut_helpers() {
    let symbols = ui_symbols();
    let files: Vec<&String> = symbols.get(&PLATFORM_ONLY).map(|f| f.iter().collect()).unwrap_or_default();
    assert_eq!(files, ["ui/src/widgets.rs"], "use widgets::shortcut/command_key instead of a literal ⌘");
}

#[test]
fn ui_symbols_render_with_bundled_fonts_only() {
    let mut symbols = ui_symbols();
    for expected in ['·', '×', '—', '…', '°'] {
        assert!(symbols.contains_key(&expected), "scanner missed {expected:?}; found {:?}", symbols.keys());
    }
    symbols.remove(&PLATFORM_ONLY);

    // kittest uses egui's bundled fonts and panics on any missing glyph.
    let text: String = symbols.into_keys().collect();
    let mut harness = Harness::new_ui(move |ui| {
        ui.label(text.as_str());
    });
    harness.run();
}

/// Every character of every translation (`msgstr` of the compiled PO files)
/// and of the language names in the settings renders with egui's bundled
/// fonts alone, whatever the system has.
#[test]
fn translations_and_language_names_render_with_bundled_fonts_only() {
    let mut characters: BTreeSet<char> = summrise_i18n::LANGUAGES.iter().flat_map(|l| l.name.chars()).collect();
    assert!(characters.contains(&'у') && characters.contains(&'ç') && characters.contains(&'ñ'));
    for language in summrise_i18n::translated_languages() {
        let po = summrise_i18n::compiled_po(language.code).expect("compiled catalogue");
        let file = summrise_i18n::po::parse(po).expect("the catalogue reads");
        for entry in &file.entries {
            characters.extend(entry.translations.iter().flat_map(|t| t.chars()).filter(|c| !c.is_control()));
        }
    }
    characters.remove(&PLATFORM_ONLY);
    let text: String = characters.into_iter().collect();
    let mut harness = Harness::new_ui(move |ui| {
        ui.label(text.as_str());
    });
    harness.run();
}

mod tour;

/// The main screens of the app in every language with the fonts the app
/// loads on this platform; kittest panics on a missing glyph. With the
/// catalogues still empty this shows English everywhere, and once they are
/// filled, every language.
#[test]
fn main_screens_render_in_every_language() {
    summrise_i18n::with_catalogue(None, || {
        for language in summrise_i18n::LANGUAGES {
            let mut tour = tour::Tour::new(language.code, None);
            tour.main_screens();
            assert_eq!(summrise_i18n::active_language(), language.code);
            assert!(tour.problems.is_empty(), "{}: {:?}", language.code, tour.problems);
        }
    });
}
