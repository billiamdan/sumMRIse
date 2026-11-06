//! The language of the interface: the setting ("System language" or a
//! language by its own name) and the languages the operating system
//! prefers. Changing the setting shows the interface in the new language
//! from the next frame on.

use egui::{ComboBox, Response, Ui};
use summrise_core::Settings;
use summrise_i18n::{LANGUAGES, Language, tr};

/// The languages the operating system prefers, most preferred first, as
/// BCP 47 tags (`de-AT`). Given to the application at start-up.
pub fn system_languages() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// The language the interface is shown in with `settings`: the chosen one,
/// or for "System language" (and a stored language the interface does not
/// have) the first of the system's languages that it has, else English.
pub(crate) fn wanted(settings: &Settings, system: &[String]) -> &'static Language {
    settings
        .language
        .as_deref()
        .and_then(summrise_i18n::language)
        .unwrap_or_else(|| summrise_i18n::match_language(system.iter().map(String::as_str)))
}

/// Shows the interface in the language of `settings` from the next frame on.
pub(crate) fn apply(ctx: &egui::Context, settings: &Settings, system: &[String]) {
    let wanted = wanted(settings, system);
    if summrise_i18n::active_language() != wanted.code {
        log::info!("interface language: {}", wanted.code);
        summrise_i18n::set_language(wanted.code);
        ctx.request_repaint();
    }
}

/// The language setting: "System language" with the language it stands
/// for, then every language under its own name (never translated, so that
/// everyone finds theirs).
pub(crate) fn choice(ui: &mut Ui, language: &mut Option<String>, system: &[String]) -> Response {
    let system_language = summrise_i18n::match_language(system.iter().map(String::as_str));
    let system_entry = tr!("System language ({language})", language = system_language.name);
    let selected = match language.as_deref().and_then(summrise_i18n::language) {
        Some(chosen) => chosen.name.to_owned(),
        None => system_entry.clone(),
    };
    ComboBox::from_id_salt("language")
        .selected_text(selected)
        .width(200.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(language, None, system_entry);
            for entry in LANGUAGES {
                ui.selectable_value(language, Some(entry.code.to_owned()), entry.name);
            }
        })
        .response
}
