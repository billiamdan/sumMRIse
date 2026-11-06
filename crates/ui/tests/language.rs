//! The language of the interface in the real app, with a German test
//! catalogue in place of the compiled one (`summrise_i18n::with_catalogue`;
//! the active language is global, so these tests run one at a time, in a test
//! binary of their own): the setting changes the text from the next frame
//! on, "System language" follows the languages the system prefers, and
//! privacy mode hides the patient details whatever the language.

use std::path::PathBuf;
use std::time::Duration;

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable, by};
use egui_material_icons::icons::{ICON_BADGE, ICON_CONTRAST};
use summrise_i18n::{Catalogue, active_language, english_plural_form, with_catalogue};
use summrise_testkit::{PhantomOptions, write_phantom_study};

mod tour;

use tour::{App, choose, settle, wait_for};

/// A few German words of the toolbar, the settings and the inspector.
fn german() -> &'static Catalogue {
    let mut catalogue = Catalogue::new("de", english_plural_form);
    for (english, german) in [
        ("mouse tool\u{4}Contrast", "Kontrast"),
        ("Language", "Sprache"),
        ("Restore defaults", "Standard wiederherstellen"),
        ("Open folder or disc…", "Ordner oder Datenträger öffnen…"),
        ("inspector section\u{4}Patient", "Patient (de)"),
        ("inspector section\u{4}Study", "Untersuchung"),
    ] {
        catalogue.insert(english, german);
    }
    catalogue.leak()
}

fn phantom() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    write_phantom_study(dir.path(), &PhantomOptions::small()).expect("fixture");
    let path = dir.path().to_path_buf();
    (dir, path)
}

fn shows(app: &App, text: &str) -> bool {
    app.query_all_by_label_contains(text).next().is_some()
}

/// The tool button "Contrast" in the toolbar, in the language shown.
fn contrast_tool(word: &str) -> String {
    format!("{} {word}", ICON_CONTRAST.codepoint)
}

#[test]
fn choosing_a_language_in_the_settings_changes_the_toolbar_in_the_next_frame() {
    with_catalogue(Some(german()), || {
        let (_dir, path) = phantom();
        // No system languages are given: "System language" is English.
        let mut app = tour::app("", Some(path), [1440.0, 900.0], false);
        wait_for(&mut app, tour::CT);
        assert_eq!(active_language(), "en");
        assert!(app.query_all_by_label(&contrast_tool("Contrast")).next().is_some());

        app.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
        wait_for(&mut app, "Restore defaults");
        let setting = || app.query_all(by().label("Language").role(Role::ComboBox)).next().expect("the language list");
        assert_eq!(setting().accesskit_node().value().as_deref(), Some("System language (English)"), "named");
        setting().click();
        settle(&mut app, Duration::from_millis(200));
        app.query_all_by_label("Deutsch").next().expect("Deutsch in the list").click();
        // The frame that takes the click still shows English; the next one is German.
        app.step();
        assert!(app.query_all_by_label(&contrast_tool("Contrast")).next().is_some());
        app.step();
        assert_eq!(active_language(), "de");
        assert!(app.query_all_by_label(&contrast_tool("Kontrast")).next().is_some(), "the toolbar is German");
        assert!(shows(&app, "Standard wiederherstellen") && shows(&app, "Sprache"));
        // Text without a translation stays English, never a key or an empty label.
        assert!(app.query_all_by_label(tour::CT).next().is_some(), "text from the disc is never translated");
        assert!(shows(&app, "MPR") && shows(&app, "All attributes"));

        // And back: English from the next frame on.
        choose(&mut app, "Sprache", "English");
        assert_eq!(active_language(), "en");
        assert!(app.query_all_by_label(&contrast_tool("Contrast")).next().is_some());
        assert!(!shows(&app, "Kontrast") && shows(&app, "Restore defaults"));
    });
}

#[test]
fn system_language_picks_the_base_language_and_falls_back_to_english() {
    with_catalogue(Some(german()), || {
        for (system, expected) in [
            ("de-AT", "de"),
            ("de_CH.UTF-8", "de"),
            ("es-MX", "es"),
            ("fr-CA", "fr"),
            ("ru-KZ", "ru"),
            ("pt-BR", "en"),
            ("zh-Hans-CN", "en"),
            ("C", "en"),
        ] {
            let mut app = tour::app(system, None, [1280.0, 800.0], false);
            settle(&mut app, Duration::from_millis(100));
            assert_eq!(active_language(), expected, "a system that prefers {system}");
            // The welcome screen speaks that language: the test catalogue stands in for
            // German, the compiled catalogues give the others.
            let welcome = summrise_i18n::tr!("Open folder or disc…");
            assert_eq!(welcome == "Open folder or disc…", expected == "en", "{system}: {welcome:?}");
            assert!(expected != "de" || welcome == "Ordner oder Datenträger öffnen…", "the test catalogue");
            assert!(shows(&app, welcome), "{system}: {welcome:?}");
        }
    });
}

/// Privacy mode finds the patient section by its key, not by its title,
/// which is translated.
#[test]
fn privacy_mode_hides_the_patient_details_in_every_language() {
    with_catalogue(Some(german()), || {
        let (_dir, path) = phantom();
        let mut app = tour::app("de-DE", Some(path), [1440.0, 900.0], false);
        wait_for(&mut app, "Patient (de)");
        assert!(shows(&app, "SYNTHETIC PHANTOM"), "the name is shown outside privacy mode");
        assert!(shows(&app, "Untersuchung"));

        tour::click(&mut app, ICON_BADGE.codepoint);
        settle(&mut app, Duration::from_millis(300));
        assert!(!shows(&app, "Patient (de)"), "the translated patient section is hidden");
        assert!(!shows(&app, "SYNTHETIC PHANTOM") && !shows(&app, summrise_testkit::PATIENT_ID));
        assert!(shows(&app, "Untersuchung"), "the other sections stay");
    });
}
