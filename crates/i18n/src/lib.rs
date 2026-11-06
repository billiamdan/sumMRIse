//! Translation of user-visible text.
//!
//! The English text written in the source code is the message key (gettext
//! style): `tr!("Open folder…")` returns the translation of that text in the
//! active language, or the English text itself when no language is installed
//! or the catalogue has no entry. Code stays readable, and a missing
//! translation degrades to English instead of to a message identifier.
//!
//! This crate sits below every other SumMRIse crate, so that labels defined
//! in the imaging, DICOM and application layers can be translated where they
//! are written.
//!
//! Rules for callers:
//! - Every user-visible text goes through [`tr!`], [`tr_n!`] or [`tr_ctx!`]
//!   with a **string literal** (catalogues are checked against the literals
//!   found in the source code).
//! - Values are inserted with named placeholders, never by concatenation:
//!   `tr!("Image {index} / {count}", index = i + 1, count = n)`. Translators
//!   can then reorder them. Format numbers before passing them in.
//! - Text that comes from the disc (names, descriptions, DICOM values) is
//!   never translated.
//! - A short or ambiguous text gets a context, which translators see and
//!   which keeps it apart from the same English word elsewhere:
//!   `tr_ctx!("contrast control", "Window")`. A comment starting with
//!   `Translators:` on the line above (or up to three lines above) a message
//!   is given to translators with it:
//!   `// Translators: "W" is short for the window width (contrast).`
//!
//! The catalogues are PO files in `crates/i18n/locales/`, one per language,
//! compiled into the application ([`LANGUAGES`], [`set_language`]).
//! `cargo xtask i18n` brings them up to date with the source code, and a test
//! checks that every message of the source code has an entry (see
//! [`tooling`]).

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{Display, Write as _};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock};

mod languages;
mod plural;
pub mod po;
pub mod tooling;

pub use languages::{
    ENGLISH, LANGUAGES, Language, catalogue, compiled_po, language, match_language, set_language, supported,
    translated_languages,
};
pub use plural::{PluralRule, plural_rule};

/// Separates a context from the text in a catalogue key (as in gettext).
pub const CONTEXT_SEPARATOR: char = '\u{4}';

/// The translations of one language.
#[derive(Debug)]
pub struct Catalogue {
    language: &'static str,
    messages: HashMap<&'static str, &'static str>,
    plurals: HashMap<&'static str, Vec<&'static str>>,
    plural_form: fn(u64) -> usize,
}

impl Catalogue {
    /// An empty catalogue for `language` (a BCP 47 tag such as `"de"`).
    /// `plural_form` maps a count to the index of the plural form to use.
    pub fn new(language: &'static str, plural_form: fn(u64) -> usize) -> Self {
        Self { language, messages: HashMap::new(), plurals: HashMap::new(), plural_form }
    }

    pub fn language(&self) -> &'static str {
        self.language
    }

    /// Adds the translation of `source` (for a text with a context, the key
    /// is `context`, [`CONTEXT_SEPARATOR`], `text`).
    pub fn insert(&mut self, source: &'static str, translation: &'static str) {
        self.messages.insert(source, translation);
    }

    /// Adds the plural forms of the text whose English singular is `singular`.
    pub fn insert_plural(&mut self, singular: &'static str, forms: Vec<&'static str>) {
        self.plurals.insert(singular, forms);
    }

    pub fn get(&self, source: &str) -> Option<&'static str> {
        self.messages.get(source).copied().filter(|t| !t.is_empty())
    }

    pub fn get_plural(&self, singular: &str, n: u64) -> Option<&'static str> {
        let forms = self.plurals.get(singular)?;
        forms.get((self.plural_form)(n)).or(forms.last()).copied().filter(|t| !t.is_empty())
    }

    pub fn len(&self) -> usize {
        self.messages.len() + self.plurals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Keeps the catalogue for the rest of the program, as [`install`] needs.
    pub fn leak(self) -> &'static Self {
        Box::leak(Box::new(self))
    }
}

/// English plural rule: one form for exactly 1, another for everything else.
pub fn english_plural_form(n: u64) -> usize {
    usize::from(n != 1)
}

static ACTIVE: RwLock<Option<&'static Catalogue>> = RwLock::new(None);

/// Makes `catalogue` the active language; `None` switches back to English.
pub fn install(catalogue: Option<&'static Catalogue>) {
    *ACTIVE.write().unwrap_or_else(PoisonError::into_inner) = catalogue;
}

fn active() -> Option<&'static Catalogue> {
    *ACTIVE.read().unwrap_or_else(PoisonError::into_inner)
}

/// BCP 47 tag of the active language (`"en"` when none is installed).
pub fn active_language() -> &'static str {
    active().map_or("en", Catalogue::language)
}

/// Translation of a source-code literal. Use [`tr!`] instead of calling this.
pub fn translate(text: &'static str) -> &'static str {
    active().and_then(|c| c.get(text)).unwrap_or(text)
}

/// Translation of `text` in `context`, for English words that need different
/// translations in different places. `key` is `context`, [`CONTEXT_SEPARATOR`],
/// `text`. Use [`tr_ctx!`] instead of calling this.
pub fn translate_in_context(key: &'static str, text: &'static str) -> &'static str {
    active().and_then(|c| c.get(key)).unwrap_or(text)
}

/// The plural form for `n`. Use [`tr_n!`] instead of calling this.
pub fn translate_plural(singular: &'static str, plural: &'static str, n: u64) -> &'static str {
    active().and_then(|c| c.get_plural(singular, n)).unwrap_or(if english_plural_form(n) == 0 {
        singular
    } else {
        plural
    })
}

/// Translation of an English text that is only known at run time (a label
/// produced elsewhere). Prefer [`tr!`] where the text is written.
pub fn translate_dynamic(text: &str) -> Cow<'_, str> {
    active().and_then(|c| c.get(text)).map_or(Cow::Borrowed(text), Cow::Borrowed)
}

/// Replaces `{name}` placeholders in `template`. `{{` and `}}` stand for
/// literal braces; a placeholder without a value is left as it is.
pub fn fill(template: &str, values: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(pos) = rest.find(['{', '}']) {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
        } else if let Some(end) = tail.strip_prefix('{').and_then(|t| t.find('}')) {
            let name = &tail[1..=end];
            match values.iter().find(|(n, _)| *n == name) {
                // Writing to a String cannot fail.
                Some((_, value)) => drop(write!(out, "{value}")),
                None => out.push_str(&tail[..end + 2]),
            }
            rest = &tail[end + 2..];
        } else {
            out.push_str(&tail[..1]);
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// A count as the plural rules see it (negative counts are treated as 0).
pub fn count<T: TryInto<u64>>(n: T) -> u64 {
    n.try_into().unwrap_or(0)
}

/// Translates a user-visible text.
///
/// `tr!("Open folder…")` gives a `&'static str`;
/// `tr!("Image {index} / {count}", index = 3, count = 24)` gives a `String`
/// with the values inserted into the translated text.
#[macro_export]
macro_rules! tr {
    ($text:literal) => {
        $crate::translate($text)
    };
    ($text:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::fill(
            $crate::translate($text),
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}

/// Translates a text that depends on a count; the count is available as
/// `{n}`: `tr_n!("{n} image", "{n} images", count)`. More values can follow
/// as in [`tr!`]. Gives a `String`.
#[macro_export]
macro_rules! tr_n {
    ($singular:literal, $plural:literal, $n:expr $(, $name:ident = $value:expr)* $(,)?) => {{
        let n: u64 = $crate::count($n);
        $crate::fill(
            $crate::translate_plural($singular, $plural, n),
            &[("n", &n as &dyn ::std::fmt::Display) $(, (stringify!($name), &$value as &dyn ::std::fmt::Display))*],
        )
    }};
}

/// Translates a text whose English wording is ambiguous, giving translators
/// the context: `tr_ctx!("toolbar", "Open")` gives a `&'static str`;
/// `tr_ctx!("window level", "L {value}", value = 40)` a `String` with the
/// values inserted, as [`tr!`].
#[macro_export]
macro_rules! tr_ctx {
    ($context:literal, $text:literal) => {
        $crate::translate_in_context(concat!($context, "\u{4}", $text), $text)
    };
    ($context:literal, $text:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::fill(
            $crate::translate_in_context(concat!($context, "\u{4}", $text), $text),
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}

static EXCLUSIVE: Mutex<()> = Mutex::new(());

/// Runs `f` with `catalogue` active and restores English afterwards. While
/// `f` runs, `catalogue` also takes the place of the compiled catalogue of
/// its language, so that choosing that language (in the settings, say)
/// shows it. The active language is global, so callers are serialised;
/// meant for tests.
pub fn with_catalogue<R>(catalogue: Option<&'static Catalogue>, f: impl FnOnce() -> R) -> R {
    struct Restore<'a>(#[allow(dead_code)] MutexGuard<'a, ()>);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            *languages::OVERRIDE.write().unwrap_or_else(PoisonError::into_inner) = None;
            install(None);
        }
    }
    let _restore = Restore(EXCLUSIVE.lock().unwrap_or_else(PoisonError::into_inner));
    *languages::OVERRIDE.write().unwrap_or_else(PoisonError::into_inner) = catalogue;
    install(catalogue);
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn german() -> &'static Catalogue {
        let mut c = Catalogue::new("de", english_plural_form);
        c.insert("Open folder…", "Ordner öffnen…");
        c.insert("Image {index} / {count}", "Bild {index} von {count}");
        c.insert("toolbar\u{4}Open", "Öffnen");
        c.insert("Untranslated", "");
        c.insert_plural("{n} image", vec!["{n} Bild", "{n} Bilder"]);
        c.leak()
    }

    /// Russian-style rule with three forms.
    fn three_forms(n: u64) -> usize {
        match (n % 10, n % 100) {
            (1, r) if r != 11 => 0,
            (2..=4, r) if !(12..=14).contains(&r) => 1,
            _ => 2,
        }
    }

    #[test]
    fn english_is_returned_when_no_language_is_installed() {
        with_catalogue(None, || {
            assert_eq!(tr!("Open folder…"), "Open folder…");
            assert_eq!(tr!("Image {index} / {count}", index = 3, count = 24), "Image 3 / 24");
            assert_eq!(tr_n!("{n} image", "{n} images", 1usize), "1 image");
            assert_eq!(tr_n!("{n} image", "{n} images", 24usize), "24 images");
            assert_eq!(tr_ctx!("toolbar", "Open"), "Open");
            assert_eq!(active_language(), "en");
        });
    }

    #[test]
    fn installed_language_translates_and_falls_back_to_english() {
        with_catalogue(Some(german()), || {
            assert_eq!(active_language(), "de");
            assert_eq!(tr!("Open folder…"), "Ordner öffnen…");
            assert_eq!(tr!("Image {index} / {count}", index = 3, count = 24), "Bild 3 von 24");
            assert_eq!(tr_n!("{n} image", "{n} images", 1u32), "1 Bild");
            assert_eq!(tr_n!("{n} image", "{n} images", 5u32), "5 Bilder");
            assert_eq!(tr_ctx!("toolbar", "Open"), "Öffnen");
            // Missing and empty entries fall back to the English text.
            assert_eq!(tr!("Not in the catalogue"), "Not in the catalogue");
            assert_eq!(tr!("Untranslated"), "Untranslated");
            assert_eq!(translate_dynamic("Open folder…"), "Ordner öffnen…");
            assert_eq!(translate_dynamic("CT THORAX"), "CT THORAX");
        });
        assert_eq!(tr!("Open folder…"), "Open folder…");
    }

    #[test]
    fn plural_rules_with_three_forms_pick_the_right_form() {
        let mut c = Catalogue::new("ru", three_forms);
        c.insert_plural("{n} image", vec!["{n} снимок", "{n} снимка", "{n} снимков"]);
        let c = c.leak();
        with_catalogue(Some(c), || {
            assert_eq!(tr_n!("{n} image", "{n} images", 1), "1 снимок");
            assert_eq!(tr_n!("{n} image", "{n} images", 3), "3 снимка");
            assert_eq!(tr_n!("{n} image", "{n} images", 11), "11 снимков");
            assert_eq!(tr_n!("{n} image", "{n} images", 21), "21 снимок");
            assert_eq!(tr_n!("{n} image", "{n} images", 25), "25 снимков");
        });
    }

    #[test]
    fn placeholders_are_filled_by_name_in_any_order() {
        let (a, b) = (1.5, "mm");
        assert_eq!(fill("{b} {a} {a}", &[("a", &a), ("b", &b)]), "mm 1.5 1.5");
        assert_eq!(fill("100 %, {{literal}}, {unknown}, {open", &[]), "100 %, {literal}, {unknown}, {open");
        assert_eq!(fill("", &[]), "");
        assert_eq!(tr_n!("{n} of {total} file", "{n} of {total} files", 2, total = 9), "2 of 9 files");
        assert_eq!(count(-3), 0);
    }
}
