//! The languages of the interface, choosing one from the languages the
//! operating system prefers, and their catalogues: the PO files of
//! `crates/i18n/locales/`, compiled into the application and read the first
//! time a language is used.

use std::sync::{OnceLock, PoisonError, RwLock};

use crate::po::{self, PoFile};
use crate::{Catalogue, install, plural_rule};

/// A language the interface can be shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    /// The base language as a BCP 47 tag: `"de"`.
    pub code: &'static str,
    /// The name of the language in the language itself, as the settings list
    /// it (never translated): `"Deutsch"`.
    pub name: &'static str,
    /// The name in English, for the header of its PO file.
    pub english_name: &'static str,
}

/// The language of the source code: no catalogue, every text as written.
pub const ENGLISH: Language = Language { code: "en", name: "English", english_name: "English" };

/// Every language of the interface, English first.
pub const LANGUAGES: &[Language] = &[
    ENGLISH,
    Language { code: "de", name: "Deutsch", english_name: "German" },
    Language { code: "es", name: "Español", english_name: "Spanish" },
    Language { code: "fr", name: "Français", english_name: "French" },
    Language { code: "ru", name: "Русский", english_name: "Russian" },
];

/// The catalogues compiled into the application, in the order of [`LANGUAGES`].
const PO_FILES: [(&str, &str); 4] = [
    ("de", include_str!("../locales/de.po")),
    ("es", include_str!("../locales/es.po")),
    ("fr", include_str!("../locales/fr.po")),
    ("ru", include_str!("../locales/ru.po")),
];

/// The languages that have a catalogue: all but English.
pub fn translated_languages() -> impl Iterator<Item = &'static Language> {
    LANGUAGES.iter().filter(|l| l.code != ENGLISH.code)
}

/// The language with code `code`.
pub fn language(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.code == code)
}

/// The language of the interface for a language tag: its base language if
/// the interface has it. Takes BCP 47 tags (`de-AT`, `es-419`,
/// `zh-Hans-CN`) and POSIX locales (`fr_CA.UTF-8`); `C` and `POSIX` are no
/// language.
pub fn supported(tag: &str) -> Option<&'static Language> {
    let base = tag.split(['-', '_', '.', '@']).next()?.trim().to_ascii_lowercase();
    LANGUAGES.iter().find(|l| l.code == base)
}

/// The language for the system's preferred languages (most preferred
/// first): the first one the interface has, else English. A system that
/// prefers Ukrainian, then Russian, gets Russian, as the platforms do it for
/// their own applications.
pub fn match_language<'a>(preferred: impl IntoIterator<Item = &'a str>) -> &'static Language {
    preferred.into_iter().find_map(supported).unwrap_or(&ENGLISH)
}

/// A catalogue that takes the place of the compiled one of its language
/// (see [`crate::with_catalogue`]).
pub(crate) static OVERRIDE: RwLock<Option<&'static Catalogue>> = RwLock::new(None);

static LOADED: [OnceLock<Option<&'static Catalogue>>; 4] =
    [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];

/// The catalogue of language `code`, read from its PO file the first time
/// and kept for the rest of the program; `None` for English and for codes
/// without a catalogue.
pub fn catalogue(code: &str) -> Option<&'static Catalogue> {
    if let Some(replacement) = *OVERRIDE.read().unwrap_or_else(PoisonError::into_inner)
        && replacement.language() == code
    {
        return Some(replacement);
    }
    let index = PO_FILES.iter().position(|(c, _)| *c == code)?;
    let (code, text) = PO_FILES[index];
    *LOADED[index].get_or_init(|| match po::parse(text) {
        Ok(file) => Some(Catalogue::from_po(code, &file).leak()),
        // The completeness test reads every compiled catalogue, so this does not happen in a
        // released build; the interface then stays in English.
        Err(e) => {
            log::error!("the {code} catalogue cannot be read: {e}");
            None
        }
    })
}

/// Shows the interface in `language` from now on; English for a code
/// without a catalogue.
pub fn set_language(code: &str) {
    install(catalogue(code));
}

/// The PO file of language `code` as compiled into the application.
pub fn compiled_po(code: &str) -> Option<&'static str> {
    PO_FILES.iter().find(|(c, _)| *c == code).map(|(_, text)| *text)
}

impl Catalogue {
    /// The translations of a PO file for language `language`. Entries that
    /// are obsolete, marked fuzzy or not translated are left out, so that
    /// their English text is shown.
    pub fn from_po(language: &'static str, file: &PoFile) -> Self {
        let mut catalogue = Self::new(language, plural_rule(language).form);
        let leak = |s: &str| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) };
        for entry in file.entries.iter().filter(|e| !e.obsolete && !e.is_fuzzy()) {
            if entry.translations.iter().all(String::is_empty) {
                continue;
            }
            let key = match &entry.context {
                Some(context) => leak(&format!("{context}{}{}", crate::CONTEXT_SEPARATOR, entry.id)),
                None => leak(&entry.id),
            };
            if entry.plural.is_some() {
                catalogue.insert_plural(key, entry.translations.iter().map(|t| leak(t)).collect());
            } else if let Some(translation) = entry.translations.first() {
                catalogue.insert(key, leak(translation));
            }
        }
        catalogue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{active_language, tr, tr_ctx, tr_n, with_catalogue};

    fn code(tags: &[&str]) -> &'static str {
        match_language(tags.iter().copied()).code
    }

    #[test]
    fn regional_variants_select_their_base_language() {
        assert_eq!(code(&["de-AT"]), "de");
        assert_eq!(code(&["es-MX"]), "es");
        assert_eq!(code(&["fr-CA"]), "fr");
        assert_eq!(code(&["ru-KZ"]), "ru");
        assert_eq!(code(&["de_CH.UTF-8"]), "de", "POSIX locales as on Linux");
        assert_eq!(code(&["FR"]), "fr");
        assert_eq!(code(&["en-GB", "de-DE"]), "en", "English is a language of the interface too");
    }

    #[test]
    fn unknown_and_unsupported_system_languages_fall_back_to_english() {
        assert_eq!(code(&[]), "en");
        assert_eq!(code(&["pt-BR"]), "en");
        assert_eq!(code(&["zh-Hans-CN", "ja-JP"]), "en");
        assert_eq!(code(&["C"]), "en");
        assert_eq!(code(&["POSIX"]), "en");
        assert_eq!(code(&[""]), "en");
        assert_eq!(code(&["-"]), "en");
        // The first language the interface has, as the platforms pick for their own applications.
        assert_eq!(code(&["uk-UA", "ru-RU", "en-US"]), "ru");
        assert!(supported("deu").is_none(), "three-letter codes are not German");
    }

    #[test]
    fn languages_are_listed_under_their_own_names() {
        let names: Vec<&str> = LANGUAGES.iter().map(|l| l.name).collect();
        assert_eq!(names, ["English", "Deutsch", "Español", "Français", "Русский"]);
        let codes: Vec<&str> = translated_languages().map(|l| l.code).collect();
        assert_eq!(codes, PO_FILES.map(|(code, _)| code));
    }

    #[test]
    fn every_compiled_catalogue_loads() {
        for language in translated_languages() {
            let catalogue = catalogue(language.code).expect("the catalogue reads");
            assert_eq!(catalogue.language(), language.code);
        }
        assert!(catalogue("en").is_none() && catalogue("it").is_none());
    }

    #[test]
    fn a_po_file_becomes_a_catalogue_with_contexts_and_plural_forms() {
        let file = po::parse(
            "msgid \"Open\"\nmsgstr \"Открыть\"\n\nmsgctxt \"contrast control\"\nmsgid \"Window\"\nmsgstr \"Окно\"\n\n\
             msgid \"{n} image\"\nmsgid_plural \"{n} images\"\nmsgstr[0] \"{n} снимок\"\nmsgstr[1] \"{n} снимка\"\n\
             msgstr[2] \"{n} снимков\"\n\nmsgid \"Empty\"\nmsgstr \"\"\n\n#, fuzzy\nmsgid \"Unsure\"\nmsgstr \"?\"\n",
        )
        .expect("parses");
        let test = Catalogue::from_po("ru", &file).leak();
        assert_eq!(test.len(), 3, "untranslated and fuzzy entries are left out");
        with_catalogue(Some(test), || {
            assert_eq!(active_language(), "ru");
            assert_eq!(tr!("Open"), "Открыть");
            assert_eq!(tr_ctx!("contrast control", "Window"), "Окно");
            assert_eq!(tr!("Window"), "Window", "the context tells the two apart");
            assert_eq!(tr_n!("{n} image", "{n} images", 21), "21 снимок");
            assert_eq!(tr_n!("{n} image", "{n} images", 22), "22 снимка");
            assert_eq!(tr_n!("{n} image", "{n} images", 11), "11 снимков");
            assert_eq!(tr!("Empty"), "Empty");
            assert_eq!(tr!("Unsure"), "Unsure");
            // The test catalogue takes the place of the compiled Russian one.
            set_language("en");
            assert_eq!(tr!("Open"), "Open");
            set_language("ru");
            assert_eq!(tr!("Open"), "Открыть");
        });
        assert_eq!(active_language(), "en");
        assert!(catalogue("ru").is_some_and(|c| !std::ptr::eq(c, test)), "the compiled catalogue is back");
    }
}
