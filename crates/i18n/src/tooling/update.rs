//! The template and the PO files as `cargo xtask i18n` writes them, and
//! the check of a PO file against the source code.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::po::{Entry, PoFile};
use crate::tooling::Message;
use crate::{ENGLISH, Language, plural_rule};

/// The placeholders of a text as [`crate::fill`] reads them: the names in
/// `{name}`, where `{{` and `}}` stand for braces. A single brace or a name
/// that is not an identifier is an error (it would be shown as it is).
pub fn placeholders(text: &str) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    let mut rest = text;
    while let Some(pos) = rest.find(['{', '}']) {
        let tail = &rest[pos..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            rest = &tail[2..];
            continue;
        }
        if tail.starts_with('}') {
            return Err("a single closing brace (write }} for a brace)".to_owned());
        }
        let Some(end) = tail.find('}') else {
            return Err("a brace that is not closed (write {{ for a brace)".to_owned());
        };
        let name = &tail[1..end];
        let identifier = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !identifier {
            return Err(format!("{{{name}}} is not a placeholder name"));
        }
        names.insert(name.to_owned());
        rest = &tail[end + 1..];
    }
    Ok(names)
}

/// The header of the PO file of `language`, or of the template (`None`).
pub fn header(language: Option<&Language>) -> Entry {
    let rule = plural_rule(language.map_or(ENGLISH.code, |l| l.code));
    let mut comments = match language {
        Some(language) => vec![
            format!("{} translations of SumMRIse.", language.english_name),
            "Brought up to date with the source code by `cargo xtask i18n`; translators fill in the msgstr lines."
                .to_owned(),
            "Rules for translators: design.md of the OpenSpec change `translations`.".to_owned(),
        ],
        None => vec![
            "Template of the messages of SumMRIse, written by `cargo xtask i18n`.".to_owned(),
            "Do not edit: the PO files of the languages are brought up to date from the source code.".to_owned(),
        ],
    };
    if language.is_some() {
        let forms: Vec<String> = (0..rule.forms).map(|i| format!("msgstr[{i}] for {}", rule.examples(i))).collect();
        comments.push(format!("Plural forms: {}.", forms.join("; ")));
    }
    let fields = [
        ("Project-Id-Version", "SumMRIse"),
        ("Language", language.map_or("", |l| l.code)),
        ("MIME-Version", "1.0"),
        ("Content-Type", "text/plain; charset=UTF-8"),
        ("Content-Transfer-Encoding", "8bit"),
        ("Plural-Forms", rule.header),
    ];
    let text: String = fields.iter().map(|(name, value)| format!("{name}: {value}\n")).collect();
    Entry { translator_comments: comments, translations: vec![text], ..Entry::default() }
}

/// The entry of `message` without translations, with `forms` empty
/// translations if it has a plural.
fn fresh_entry(message: &Message, forms: usize) -> Entry {
    Entry {
        extracted_comments: message.comments.clone(),
        references: message.references.clone(),
        context: message.context.clone(),
        id: message.id.clone(),
        plural: message.plural.clone(),
        translations: vec![String::new(); if message.plural.is_some() { forms } else { 1 }],
        ..Entry::default()
    }
}

/// The template (`messages.pot`): every message of the source code, in its
/// order, without translations.
pub fn template(messages: &[Message]) -> PoFile {
    PoFile { header: Some(header(None)), entries: messages.iter().map(|m| fresh_entry(m, 2)).collect() }
}

/// The PO file of `language` brought up to date with the source code: an
/// entry for every message, in the order of the source code, with the
/// comments and references of the source code; translations, translators'
/// comments and flags are kept from `existing` for every message whose
/// English text (and plural) is unchanged. Entries of messages that are no
/// longer in the source code are left out ([`check`] lists them).
pub fn update(messages: &[Message], existing: &PoFile, language: &Language) -> PoFile {
    let forms = plural_rule(language.code).forms;
    let old: HashMap<(Option<&str>, &str), &Entry> =
        existing.entries.iter().filter(|e| !e.obsolete).map(|e| (e.key(), e)).collect();
    let entries = messages
        .iter()
        .map(|message| {
            let mut entry = fresh_entry(message, forms);
            if let Some(old) = old.get(&(message.context.as_deref(), message.id.as_str()))
                && old.plural == message.plural
                && old.translations.len() == entry.translations.len()
            {
                entry.translations.clone_from(&old.translations);
                entry.translator_comments.clone_from(&old.translator_comments);
                entry.flags.clone_from(&old.flags);
            }
            entry
        })
        .collect();
    PoFile { header: Some(header(Some(language))), entries }
}

/// What [`check`] finds in a PO file. Each item names the message.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Check {
    /// Messages of the source code without an entry.
    pub missing: Vec<String>,
    /// Entries of messages that are not (or no longer) in the source code.
    pub obsolete: Vec<String>,
    /// Entries without a translation of every form, or marked fuzzy.
    pub untranslated: Vec<String>,
    /// Wrong entries and a wrong header: the plural, the number of forms,
    /// placeholders of the translation.
    pub errors: Vec<String>,
    /// Entries that match a message.
    pub entries: usize,
}

impl Check {
    /// Every message has an entry, no entry is obsolete and none is wrong;
    /// translations may be missing.
    pub fn is_consistent(&self) -> bool {
        self.missing.is_empty() && self.obsolete.is_empty() && self.errors.is_empty()
    }
}

fn describe(context: Option<&str>, id: &str) -> String {
    match context {
        Some(context) => format!("{id:?} (context {context:?})"),
        None => format!("{id:?}"),
    }
}

/// Compares the PO file of `language` with the messages of the source code,
/// in both directions.
pub fn check(messages: &[Message], file: &PoFile, language: &Language) -> Check {
    let rule = plural_rule(language.code);
    let mut result = Check::default();
    if file.header_field("Language") != Some(language.code) {
        result.errors.push(format!("the header must say Language: {}", language.code));
    }
    if file.header_field("Plural-Forms") != Some(rule.header) {
        result.errors.push(format!("the header must say Plural-Forms: {}", rule.header));
    }
    let entries: HashMap<(Option<&str>, &str), &Entry> =
        file.entries.iter().filter(|e| !e.obsolete).map(|e| (e.key(), e)).collect();
    let known: HashSet<(Option<&str>, &str)> = messages.iter().map(|m| (m.context.as_deref(), m.id.as_str())).collect();
    for message in messages {
        let name = describe(message.context.as_deref(), &message.id);
        let Some(entry) = entries.get(&(message.context.as_deref(), message.id.as_str())) else {
            result.missing.push(name);
            continue;
        };
        result.entries += 1;
        if entry.plural != message.plural {
            result.errors.push(format!("{name}: the plural must be {:?}", message.plural));
            continue;
        }
        let forms = if message.plural.is_some() { rule.forms } else { 1 };
        if entry.translations.len() != forms {
            result.errors.push(format!("{name}: {forms} translations (msgstr) expected"));
            continue;
        }
        if !entry.is_translated() {
            result.untranslated.push(name.clone());
        }
        let source = placeholders(&message.id).and_then(|mut all| {
            if let Some(plural) = &message.plural {
                all.extend(placeholders(plural)?);
            }
            Ok(all)
        });
        let Ok(source) = source else { continue };
        for (index, translation) in entry.translations.iter().enumerate().filter(|(_, t)| !t.is_empty()) {
            let form = if message.plural.is_some() { format!("msgstr[{index}]") } else { "msgstr".to_owned() };
            let found = match placeholders(translation) {
                Ok(found) => found,
                Err(e) => {
                    result.errors.push(format!("{name}: {form}: {e}"));
                    continue;
                }
            };
            let needed: BTreeSet<&str> = source
                .iter()
                .map(String::as_str)
                .filter(|&p| message.plural.is_none() || p != "n" || rule.form_has_several_counts(index))
                .collect();
            let found: BTreeSet<&str> = found.iter().map(String::as_str).collect();
            let unknown: Vec<&&str> = found.iter().filter(|p| !source.contains(**p)).collect();
            let lost: Vec<&&str> = needed.difference(&found).collect();
            if !unknown.is_empty() || !lost.is_empty() {
                result.errors.push(format!(
                    "{name}: {form} {translation:?} must keep the placeholders {needed:?} (unknown: {unknown:?}, missing: {lost:?})"
                ));
            }
        }
    }
    for entry in &file.entries {
        if entry.obsolete || !known.contains(&entry.key()) {
            result.obsolete.push(describe(entry.context.as_deref(), &entry.id));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::po::{parse, write};

    fn message(context: Option<&str>, id: &str, plural: Option<&str>) -> Message {
        Message {
            context: context.map(str::to_owned),
            id: id.to_owned(),
            plural: plural.map(str::to_owned),
            references: vec!["crates/ui/src/app.rs".to_owned()],
            comments: Vec::new(),
        }
    }

    fn sources() -> Vec<Message> {
        vec![
            message(None, "Open", None),
            message(Some("contrast control"), "Window", None),
            message(None, "{n} image", Some("{n} images")),
            message(None, "Image {index} / {count}", None),
        ]
    }

    fn russian() -> &'static Language {
        crate::language("ru").expect("Russian")
    }

    #[test]
    fn placeholders_are_read_as_fill_reads_them() {
        let names = |t: &str| placeholders(t).map(|p| p.into_iter().collect::<Vec<_>>());
        assert_eq!(names("Image {index} / {count}"), Ok(vec!["count".to_owned(), "index".to_owned()]));
        assert_eq!(names("{{literal}} {n}"), Ok(vec!["n".to_owned()]));
        assert!(names("{index von {count}").is_err_and(|e| e.contains("not a placeholder name")));
        assert!(names("a } b").is_err_and(|e| e.contains("closing brace")));
        assert!(names("a { b").is_err_and(|e| e.contains("not closed")));
        assert!(names("{x:.1}").is_err());
    }

    #[test]
    fn updating_adds_new_messages_keeps_translations_and_drops_obsolete_entries() {
        let existing = parse(
            "msgid \"\"\nmsgstr \"Language: ru\\n\"\n\n# Checked.\nmsgid \"Open\"\nmsgstr \"Открыть\"\n\n\
             msgid \"{n} image\"\nmsgid_plural \"{n} pictures\"\nmsgstr[0] \"a\"\nmsgstr[1] \"b\"\nmsgstr[2] \"c\"\n\n\
             msgid \"Gone\"\nmsgstr \"Нет\"\n",
        )
        .expect("parses");
        let before = check(&sources(), &existing, russian());
        assert_eq!(before.missing, ["\"Window\" (context \"contrast control\")", "\"Image {index} / {count}\""]);
        assert_eq!(before.obsolete, ["\"Gone\""]);
        assert!(before.errors.iter().any(|e| e.contains("the plural must be")), "{:?}", before.errors);
        assert!(before.errors.iter().any(|e| e.contains("Plural-Forms")));

        let updated = update(&sources(), &existing, russian());
        let after = check(&sources(), &updated, russian());
        assert!(after.is_consistent(), "{after:?}");
        assert_eq!(after.entries, 4);
        assert_eq!(after.untranslated.len(), 3, "only \"Open\" is translated");
        let open = &updated.entries[0];
        assert_eq!(
            (open.translations.as_slice(), open.translator_comments.as_slice()),
            (["Открыть".to_owned()].as_slice(), ["Checked.".to_owned()].as_slice())
        );
        // The plural changed: the old forms do not fit, three empty Russian forms instead.
        assert_eq!(updated.entries[2].translations, ["", "", ""]);
        assert_eq!(updated.header_field("Language"), Some("ru"));
        // Up to date stays up to date: writing, reading and updating again changes nothing.
        let written = write(&updated);
        assert_eq!(write(&update(&sources(), &parse(&written).expect("parses"), russian())), written);
    }

    #[test]
    fn translations_must_keep_their_placeholders_and_forms() {
        let messages = sources();
        let mut file = update(&messages, &PoFile::default(), russian());
        file.entries[3].translations[0] = "Снимок {index} из {total}".to_owned();
        file.entries[2].translations = vec!["{n} снимок".into(), "{n} снимка".into(), "снимков".into()];
        let result = check(&messages, &file, russian());
        assert_eq!(result.errors.len(), 2, "{:?}", result.errors);
        assert!(result.errors[0].contains("msgstr[2]") && result.errors[0].contains("missing: [\"n\"]"));
        assert!(result.errors[1].contains("unknown: [\"total\"]"));
        // German "ein Bild": the form for exactly one may leave out the count.
        let german = crate::language("de").expect("German");
        let mut file = update(&messages, &PoFile::default(), german);
        file.entries[2].translations = vec!["ein Bild".into(), "{n} Bilder".into()];
        assert!(check(&messages, &file, german).errors.is_empty());
        file.entries[2].translations.push("{n} Bilder".into());
        assert!(check(&messages, &file, german).errors[0].contains("2 translations"));
    }

    #[test]
    fn the_template_lists_every_message_with_two_empty_plural_forms() {
        let template = template(&sources());
        assert_eq!(template.header_field("Language"), Some(""));
        assert_eq!(template.entries.len(), 4);
        assert_eq!(template.entries[2].translations, ["", ""]);
        assert_eq!(template.entries[1].references, ["crates/ui/src/app.rs"]);
        let ru = header(Some(russian()));
        assert!(ru.translator_comments.iter().any(|c| c.starts_with("Plural forms: msgstr[0] for 1, 21, 31")));
    }
}
