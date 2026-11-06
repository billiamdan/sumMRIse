//! Plural rules of the languages with catalogues: which form of a message a
//! count takes. They follow the CLDR cardinal rules for whole numbers
//! (counts are never fractions here) and are written in code; the
//! `Plural-Forms` header of each PO file states the same rule for
//! translation tools.
//!
//! CLDR gives Spanish and French a third category, "many", that applies only
//! to exact multiples of one million (as in "un millón de imágenes"); the
//! application writes counts as digits and never counts that many images or
//! files, so that category is merged into "other" and the two languages keep
//! the two forms translators and tools expect (see
//! `openspec/changes/translations/design.md`).

/// How a language picks among the plural forms of a message.
#[derive(Debug, Clone, Copy)]
pub struct PluralRule {
    /// Number of forms (`nplurals`), the number of `msgstr[n]` of a plural entry.
    pub forms: usize,
    /// The `Plural-Forms` header of the language's PO file: the same rule for tools.
    pub header: &'static str,
    /// The index of the form for a count.
    pub form: fn(u64) -> usize,
}

/// One form for exactly 1, another for every other count: English, German, Spanish.
const ONE_OTHER: PluralRule =
    PluralRule { forms: 2, header: "nplurals=2; plural=(n != 1);", form: crate::english_plural_form };

/// French: the singular for 0 and 1.
const FRENCH: PluralRule = PluralRule { forms: 2, header: "nplurals=2; plural=(n > 1);", form: french };

/// Russian: one (1, 21, 31, …), few (2–4, 22–24, …) and many (0, 5–20, 25–30, …).
const RUSSIAN: PluralRule = PluralRule {
    forms: 3,
    header: "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<12 || n%100>14) ? 1 : 2);",
    form: russian,
};

/// The plural rule of `language` (a code from [`crate::LANGUAGES`]); the
/// English rule for any other.
pub fn plural_rule(language: &str) -> PluralRule {
    match language {
        "fr" => FRENCH,
        "ru" => RUSSIAN,
        _ => ONE_OTHER,
    }
}

fn french(n: u64) -> usize {
    usize::from(n > 1)
}

fn russian(n: u64) -> usize {
    match (n % 10, n % 100) {
        (1, r) if r != 11 => 0,
        (2..=4, r) if !(12..=14).contains(&r) => 1,
        _ => 2,
    }
}

impl PluralRule {
    /// Whether form `index` is used for more than one count, so that its
    /// text must show the count (`{n}`). German "1 Bild" may be written
    /// "ein Bild"; Russian form 0 is also used for 21, 31, …
    pub fn form_has_several_counts(&self, index: usize) -> bool {
        (0..1000u64).filter(|&n| (self.form)(n) == index).nth(1).is_some()
    }

    /// The first counts that take form `index`, for the explanation in the
    /// header of a PO file: "1, 21, 31, 41, …".
    pub fn examples(&self, index: usize) -> String {
        let counts: Vec<String> =
            (0..1000u64).filter(|&n| (self.form)(n) == index).take(6).map(|n| n.to_string()).collect();
        let more = (0..1000u64).filter(|&n| (self.form)(n) == index).nth(6).is_some();
        if more { format!("{}, …", counts.join(", ")) } else { counts.join(", ") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTS: [u64; 9] = [0, 1, 2, 5, 11, 21, 22, 25, 101];

    fn forms(language: &str) -> Vec<usize> {
        let rule = plural_rule(language);
        COUNTS.iter().map(|&n| (rule.form)(n)).collect()
    }

    #[test]
    fn counts_take_the_cldr_form_in_every_language() {
        //                         0  1  2  5 11 21 22 25 101
        assert_eq!(forms("en"), [1, 0, 1, 1, 1, 1, 1, 1, 1]);
        assert_eq!(forms("de"), [1, 0, 1, 1, 1, 1, 1, 1, 1]);
        assert_eq!(forms("es"), [1, 0, 1, 1, 1, 1, 1, 1, 1]);
        // French: 0 is singular ("0 image").
        assert_eq!(forms("fr"), [0, 0, 1, 1, 1, 1, 1, 1, 1]);
        // Russian: 1 снимок, 2 снимка, 5 снимков, 11 снимков, 21 снимок, 22 снимка, 25 снимков, 101 снимок.
        assert_eq!(forms("ru"), [2, 0, 1, 2, 2, 0, 1, 2, 0]);
        assert_eq!((plural_rule("ru").form)(12), 2);
        assert_eq!((plural_rule("ru").form)(111), 2);
        assert_eq!((plural_rule("ru").form)(1_000_004), 1);
        assert_eq!(forms("pt"), forms("en"), "other languages use the English rule");
    }

    #[test]
    fn rules_name_their_forms_for_translators() {
        let ru = plural_rule("ru");
        assert_eq!(ru.forms, 3);
        assert_eq!(ru.examples(0), "1, 21, 31, 41, 51, 61, …");
        assert_eq!(ru.examples(1), "2, 3, 4, 22, 23, 24, …");
        assert_eq!(ru.examples(2), "0, 5, 6, 7, 8, 9, …");
        assert_eq!(plural_rule("fr").examples(0), "0, 1");
        assert_eq!(plural_rule("de").examples(0), "1");
        // German "ein Bild" needs no count; French and Russian singulars do.
        assert!(!plural_rule("de").form_has_several_counts(0));
        assert!(plural_rule("de").form_has_several_counts(1));
        assert!(plural_rule("fr").form_has_several_counts(0));
        assert!(ru.form_has_several_counts(0) && ru.form_has_several_counts(1) && ru.form_has_several_counts(2));
    }
}
