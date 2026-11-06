//! The catalogues match the source code, in both directions: every message
//! that the code passes to `tr!`, `tr_n!` or `tr_ctx!` has an entry in every
//! PO file, no entry is left over, and the languages in [`COMPLETE`] translate
//! every message with the same placeholders and the right number of plural
//! forms. `cargo xtask i18n` brings the PO files up to date after a change of
//! the source code.

use std::path::{Path, PathBuf};

use summrise_i18n::{po, tooling, translated_languages};

/// The languages whose translation is complete. A translation adds its
/// language here when every message of its PO file is translated.
const COMPLETE: &[&str] = &["de", "es", "fr", "ru"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn messages() -> Vec<tooling::Message> {
    tooling::scan_workspace(&root()).unwrap_or_else(|errors| {
        let errors: Vec<String> = errors.iter().map(ToString::to_string).collect();
        panic!("uses of the translation macros that the catalogues cannot follow:\n{}", errors.join("\n"))
    })
}

fn po_file(code: &str) -> (PathBuf, String) {
    let path = root().join(format!("crates/i18n/locales/{code}.po"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    (path, text)
}

fn listed(kind: &str, items: &[String]) -> String {
    let mut out: Vec<String> = items.iter().take(20).map(|item| format!("  {kind}: {item}")).collect();
    if items.len() > 20 {
        out.push(format!("  {kind}: … and {} more", items.len() - 20));
    }
    out.join("\n")
}

#[test]
fn every_message_has_an_entry_and_complete_languages_are_translated() {
    let messages = messages();
    assert!(messages.len() > 100, "the scanner found only {} messages", messages.len());
    let mut problems = Vec::new();
    for language in translated_languages() {
        let (path, text) = po_file(language.code);
        let file = po::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let check = tooling::check(&messages, &file, language);
        let mut lines = vec![listed("missing", &check.missing), listed("obsolete", &check.obsolete)];
        lines.push(listed("wrong", &check.errors));
        if COMPLETE.contains(&language.code) {
            lines.push(listed("untranslated", &check.untranslated));
        }
        let lines: Vec<String> = lines.into_iter().filter(|l| !l.is_empty()).collect();
        if !lines.is_empty() {
            problems.push(format!("{}:\n{}", path.display(), lines.join("\n")));
        }
    }
    assert!(
        problems.is_empty(),
        "the catalogues do not match the source code (run `cargo xtask i18n`, then translate):\n{}",
        problems.join("\n")
    );
}

#[test]
fn complete_names_languages_with_a_catalogue() {
    for code in COMPLETE {
        assert!(translated_languages().any(|l| l.code == *code), "{code} has no catalogue");
    }
}

/// The round trip of `cargo xtask i18n`: on files that are up to date it
/// changes nothing, so the PO files in the repository are exactly what it
/// writes, and the application compiles the same files.
#[test]
fn po_files_are_as_cargo_xtask_i18n_writes_them() {
    let messages = messages();
    let template = po::write(&tooling::template(&messages));
    let pot = root().join("crates/i18n/locales/messages.pot");
    let on_disk = std::fs::read_to_string(&pot).unwrap_or_default();
    assert!(on_disk == template, "{} is out of date: run `cargo xtask i18n`", pot.display());
    for language in translated_languages() {
        let (path, text) = po_file(language.code);
        let file = po::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let updated = po::write(&tooling::update(&messages, &file, language));
        assert!(updated == text, "{} is not as `cargo xtask i18n` writes it: run it", path.display());
        let again = tooling::update(&messages, &po::parse(&updated).expect("reads"), language);
        assert_eq!(po::write(&again), updated, "updating twice changes nothing");
        assert_eq!(summrise_i18n::compiled_po(language.code), Some(text.as_str()), "the application compiles the file");
    }
}
