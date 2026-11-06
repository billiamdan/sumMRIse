//! Reading and writing PO files (GNU gettext), the format of the catalogues
//! in `crates/i18n/locales/`: one file per language, the English text as
//! `msgid`, its translation as `msgstr` (`msgstr[n]` for plural forms).
//!
//! The parser accepts what translation tools write: comments (`#`, `#.`,
//! `#:`, `#,`, `#|`), contexts (`msgctxt`), plural forms, strings continued
//! over several lines and C escapes. Obsolete entries (`#~`) are read and
//! marked as such. Anything else is an error with its line number, never a
//! guess.

use std::collections::HashSet;
use std::fmt::Write as _;

/// An entry of a PO file: a message and its translation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// Comments of translators (`# …`).
    pub translator_comments: Vec<String>,
    /// Comments for translators taken from the source code (`#. …`).
    pub extracted_comments: Vec<String>,
    /// Where the message is used (`#: crates/ui/src/viewer.rs`).
    pub references: Vec<String>,
    /// Flags such as `fuzzy` (`#, …`).
    pub flags: Vec<String>,
    /// `msgctxt`: tells apart equal English texts that need different translations.
    pub context: Option<String>,
    /// `msgid`: the English text.
    pub id: String,
    /// `msgid_plural`: the English plural, for a message that depends on a count.
    pub plural: Option<String>,
    /// `msgstr`, or `msgstr[0]`, `msgstr[1]`, … for a message with a plural.
    pub translations: Vec<String>,
    /// Kept for reference only (`#~`): not used, not part of the catalogue.
    pub obsolete: bool,
}

impl Entry {
    /// What identifies the entry: its context and its English text.
    pub fn key(&self) -> (Option<&str>, &str) {
        (self.context.as_deref(), &self.id)
    }

    /// Marked for review: translation tools and the application do not use it.
    pub fn is_fuzzy(&self) -> bool {
        self.flags.iter().any(|f| f == "fuzzy")
    }

    /// Every form is translated and none is marked for review.
    pub fn is_translated(&self) -> bool {
        !self.is_fuzzy() && !self.translations.is_empty() && self.translations.iter().all(|t| !t.is_empty())
    }
}

/// A PO file: the header (the entry with an empty `msgid`) and the entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoFile {
    pub header: Option<Entry>,
    pub entries: Vec<Entry>,
}

impl PoFile {
    /// A field of the header, e.g. `Language` or `Plural-Forms`.
    pub fn header_field(&self, name: &str) -> Option<&str> {
        let header = self.header.as_ref()?;
        header.translations.first()?.lines().find_map(|line| {
            let (field, value) = line.split_once(':')?;
            (field.trim() == name).then_some(value.trim())
        })
    }
}

/// A PO file that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct PoError {
    /// 1-based line number.
    pub line: usize,
    pub message: String,
}

/// Reads a PO file.
pub fn parse(text: &str) -> Result<PoFile, PoError> {
    let mut parser = Parser::default();
    for (index, raw) in text.lines().enumerate() {
        parser.line = index + 1;
        parser.read_line(raw.trim_end_matches('\r'))?;
    }
    parser.finish_entry()?;
    Ok(parser.file)
}

/// Which string a continuation line (`"…"`) belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Context,
    Id,
    Plural,
    Translation(usize),
}

#[derive(Default)]
struct Parser {
    file: PoFile,
    entry: Entry,
    /// The entry has a keyword (as opposed to comments only).
    started: bool,
    has_id: bool,
    last: Option<Field>,
    /// Line where the current entry's keywords start.
    entry_line: usize,
    line: usize,
    keys: HashSet<(Option<String>, String)>,
}

impl Parser {
    fn error(&self, message: impl Into<String>) -> PoError {
        PoError { line: self.line, message: message.into() }
    }

    fn read_line(&mut self, line: &str) -> Result<(), PoError> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(());
        }
        if let Some(rest) = line.strip_prefix("#~") {
            let rest = rest.trim_start();
            // `#~|`: the previous text of an obsolete entry; not needed.
            if rest.starts_with('|') || rest.is_empty() {
                return Ok(());
            }
            return self.read_keyword_line(rest, true);
        }
        if let Some(rest) = line.strip_prefix('#') {
            // A comment after the strings of an entry belongs to the next entry.
            if self.started && self.has_translation() {
                self.finish_entry()?;
            }
            let text = |rest: &str| rest.strip_prefix(' ').unwrap_or(rest).to_owned();
            match rest.chars().next() {
                Some('.') => self.entry.extracted_comments.push(text(&rest[1..])),
                Some(':') => self.entry.references.extend(rest[1..].split_whitespace().map(str::to_owned)),
                Some(',') => self
                    .entry
                    .flags
                    .extend(rest[1..].split(',').map(str::trim).filter(|f| !f.is_empty()).map(str::to_owned)),
                // `#|`: the previous text of a message, written by merge tools; not needed.
                Some('|') => {}
                _ => self.entry.translator_comments.push(text(rest)),
            }
            return Ok(());
        }
        self.read_keyword_line(line, false)
    }

    fn has_translation(&self) -> bool {
        !self.entry.translations.is_empty()
    }

    fn read_keyword_line(&mut self, line: &str, obsolete: bool) -> Result<(), PoError> {
        if line.starts_with('"') {
            let value = self.string(line)?;
            let field = self.last.ok_or_else(|| self.error("a string without a keyword before it"))?;
            if obsolete != self.entry.obsolete {
                return Err(self.error("an entry is partly obsolete (#~)"));
            }
            self.field_mut(field).push_str(&value);
            return Ok(());
        }
        let (keyword, rest) = line.split_once(|c: char| c.is_whitespace()).unwrap_or((line, ""));
        let value = self.string(rest.trim_start())?;
        match keyword {
            "msgctxt" => {
                if self.started {
                    if !self.has_translation() {
                        return Err(self.error("msgctxt inside an entry"));
                    }
                    self.finish_entry()?;
                }
                self.start(obsolete);
                self.entry.context = Some(value);
                self.last = Some(Field::Context);
            }
            "msgid" => {
                if self.started && self.has_id {
                    if !self.has_translation() {
                        return Err(self.error("msgid without msgstr before it"));
                    }
                    self.finish_entry()?;
                }
                if !self.started {
                    self.start(obsolete);
                }
                self.check_obsolete(obsolete)?;
                self.has_id = true;
                self.entry.id = value;
                self.last = Some(Field::Id);
            }
            "msgid_plural" => {
                self.check_obsolete(obsolete)?;
                if !self.has_id || self.entry.plural.is_some() || self.has_translation() {
                    return Err(self.error("msgid_plural must follow msgid"));
                }
                self.entry.plural = Some(value);
                self.last = Some(Field::Plural);
            }
            "msgstr" => {
                self.check_obsolete(obsolete)?;
                if !self.has_id || self.has_translation() {
                    return Err(self.error("msgstr must follow msgid"));
                }
                if self.entry.plural.is_some() {
                    return Err(self.error("a message with msgid_plural needs msgstr[0], msgstr[1], …"));
                }
                self.entry.translations.push(value);
                self.last = Some(Field::Translation(0));
            }
            _ => {
                let index = keyword
                    .strip_prefix("msgstr[")
                    .and_then(|k| k.strip_suffix(']'))
                    .and_then(|n| n.parse::<usize>().ok())
                    .ok_or_else(|| self.error(format!("unknown keyword {keyword:?}")))?;
                self.check_obsolete(obsolete)?;
                if self.entry.plural.is_none() {
                    return Err(self.error("msgstr[n] needs msgid_plural"));
                }
                if index != self.entry.translations.len() {
                    return Err(self.error(format!("msgstr[{index}] out of order")));
                }
                self.entry.translations.push(value);
                self.last = Some(Field::Translation(index));
            }
        }
        Ok(())
    }

    fn start(&mut self, obsolete: bool) {
        self.started = true;
        self.entry.obsolete = obsolete;
        self.entry_line = self.line;
    }

    fn check_obsolete(&self, obsolete: bool) -> Result<(), PoError> {
        if obsolete == self.entry.obsolete { Ok(()) } else { Err(self.error("an entry is partly obsolete (#~)")) }
    }

    fn field_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Context => self.entry.context.get_or_insert_with(String::new),
            Field::Id => &mut self.entry.id,
            Field::Plural => self.entry.plural.get_or_insert_with(String::new),
            Field::Translation(i) => &mut self.entry.translations[i],
        }
    }

    fn finish_entry(&mut self) -> Result<(), PoError> {
        if !self.started {
            // Comments at the end of the file belong to no entry.
            self.entry = Entry::default();
            return Ok(());
        }
        let entry = std::mem::take(&mut self.entry);
        let line = self.entry_line;
        let error = |message: &str| PoError { line, message: message.to_owned() };
        (self.started, self.has_id, self.last) = (false, false, None);
        if !entry.translations.is_empty() && entry.plural.is_some() && entry.translations.len() < 2 {
            return Err(error("a message with msgid_plural needs at least msgstr[0] and msgstr[1]"));
        }
        if entry.translations.is_empty() {
            return Err(error("an entry without msgstr"));
        }
        if entry.id.is_empty() && entry.context.is_none() && !entry.obsolete {
            if self.file.header.is_some() || !self.file.entries.is_empty() {
                return Err(error("the header (empty msgid) must come first, and only once"));
            }
            if entry.plural.is_some() {
                return Err(error("the header cannot have a plural"));
            }
            self.file.header = Some(entry);
            return Ok(());
        }
        if entry.id.is_empty() {
            return Err(error("an entry with an empty msgid"));
        }
        if !entry.obsolete && !self.keys.insert((entry.context.clone(), entry.id.clone())) {
            return Err(error("the same message twice"));
        }
        self.file.entries.push(entry);
        Ok(())
    }

    /// The value of a quoted string with C escapes; nothing may follow it.
    fn string(&self, text: &str) -> Result<String, PoError> {
        let mut chars = text.strip_prefix('"').ok_or_else(|| self.error("a quoted string is expected"))?.chars();
        let mut out = String::new();
        loop {
            match chars.next() {
                None => return Err(self.error("the string is not closed")),
                Some('"') => break,
                Some('\\') => out.push(self.escape(&mut chars)?),
                Some(c) => out.push(c),
            }
        }
        if !chars.as_str().trim().is_empty() {
            return Err(self.error("text after the closing quote"));
        }
        Ok(out)
    }

    fn escape(&self, chars: &mut std::str::Chars<'_>) -> Result<char, PoError> {
        let c = chars.next().ok_or_else(|| self.error("the string is not closed"))?;
        Ok(match c {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            'a' => '\u{7}',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'v' => '\u{b}',
            '\\' | '"' | '\'' | '?' => c,
            '0'..='7' => {
                let mut value = c.to_digit(8).unwrap_or(0);
                for _ in 0..2 {
                    match chars.clone().next().and_then(|d| d.to_digit(8)) {
                        Some(d) => {
                            value = value * 8 + d;
                            chars.next();
                        }
                        None => break,
                    }
                }
                char::from_u32(value).ok_or_else(|| self.error("an invalid octal escape"))?
            }
            'x' => {
                let digits: String = chars.clone().take_while(char::is_ascii_hexdigit).collect();
                if digits.is_empty() || digits.len() > 6 {
                    return Err(self.error("an invalid hexadecimal escape"));
                }
                for _ in 0..digits.len() {
                    chars.next();
                }
                u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| self.error("an invalid hexadecimal escape"))?
            }
            other => return Err(self.error(format!("an unknown escape \\{other}"))),
        })
    }
}

/// Writes a PO file the way `cargo xtask i18n` keeps the catalogues: one
/// entry per paragraph, references one per line, strings on one line unless
/// they contain line breaks. Reading the result gives `file` again.
pub fn write(file: &PoFile) -> String {
    let mut out = String::new();
    let mut entries = file.header.iter().chain(&file.entries).peekable();
    while let Some(entry) = entries.next() {
        write_entry(&mut out, entry);
        if entries.peek().is_some() {
            out.push('\n');
        }
    }
    out
}

fn write_entry(out: &mut String, entry: &Entry) {
    for comment in &entry.translator_comments {
        push_comment(out, "#", comment);
    }
    for comment in &entry.extracted_comments {
        push_comment(out, "#.", comment);
    }
    for reference in &entry.references {
        push_comment(out, "#:", reference);
    }
    if !entry.flags.is_empty() {
        push_comment(out, "#,", &entry.flags.join(", "));
    }
    let prefix = if entry.obsolete { "#~ " } else { "" };
    if let Some(context) = &entry.context {
        push_string(out, prefix, "msgctxt", context);
    }
    push_string(out, prefix, "msgid", &entry.id);
    if let Some(plural) = &entry.plural {
        push_string(out, prefix, "msgid_plural", plural);
        for (i, translation) in entry.translations.iter().enumerate() {
            push_string(out, prefix, &format!("msgstr[{i}]"), translation);
        }
    } else {
        push_string(out, prefix, "msgstr", entry.translations.first().map_or("", String::as_str));
    }
}

fn push_comment(out: &mut String, marker: &str, text: &str) {
    if text.is_empty() {
        out.push_str(marker);
    } else {
        // Writing to a String cannot fail.
        let _ = write!(out, "{marker} {text}");
    }
    out.push('\n');
}

/// `keyword "text"`; a text with line breaks inside is written as `""`
/// followed by one line per line of the text, as gettext tools do.
fn push_string(out: &mut String, prefix: &str, keyword: &str, text: &str) {
    let breaks = text.trim_end_matches('\n').contains('\n');
    if !breaks {
        let _ = writeln!(out, "{prefix}{keyword} \"{}\"", quoted(text));
        return;
    }
    let _ = writeln!(out, "{prefix}{keyword} \"\"");
    for line in text.split_inclusive('\n') {
        let _ = writeln!(out, "{prefix}\"{}\"", quoted(line));
    }
}

/// `text` with C escapes, for writing between quotes.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(out, "\\{:03o}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# German translations.
msgid ""
msgstr ""
"Language: de\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

# Checked by Anna.
#. "W" is short for window width.
#: crates/ui/src/viewer.rs
#: crates/ui/src/pet.rs
#, fuzzy
msgctxt "window width"
msgid "W"
msgstr "B"

#: crates/ui/src/app.rs
msgid ""
"Line one\n"
"Line \"two\"\tend"
msgstr "Zeile eins\nZeile \"zwei\"\tEnde"

msgid "{n} image"
msgid_plural "{n} images"
msgstr[0] "{n} Bild"
msgstr[1] "{n} "
"Bilder"

#~ msgid "Gone"
#~ msgstr "Weg"
"#;

    #[test]
    fn reads_comments_contexts_continuations_plurals_and_obsolete_entries() {
        let file = parse(SAMPLE).expect("parses");
        assert_eq!(file.header_field("Language"), Some("de"));
        assert_eq!(file.header_field("Plural-Forms"), Some("nplurals=2; plural=(n != 1);"));
        assert_eq!(
            file.header.as_ref().map(|h| h.translator_comments.clone()),
            Some(vec!["German translations.".into()])
        );
        assert_eq!(file.entries.len(), 4);

        let w = &file.entries[0];
        assert_eq!(w.key(), (Some("window width"), "W"));
        assert_eq!(w.translator_comments, ["Checked by Anna."]);
        assert_eq!(w.extracted_comments, ["\"W\" is short for window width."]);
        assert_eq!(w.references, ["crates/ui/src/viewer.rs", "crates/ui/src/pet.rs"]);
        assert!(w.is_fuzzy() && !w.is_translated(), "fuzzy entries do not count as translated");

        let lines = &file.entries[1];
        assert_eq!(lines.id, "Line one\nLine \"two\"\tend");
        assert_eq!(lines.translations, ["Zeile eins\nZeile \"zwei\"\tEnde"]);
        assert!(lines.is_translated());

        let plural = &file.entries[2];
        assert_eq!(plural.plural.as_deref(), Some("{n} images"));
        assert_eq!(plural.translations, ["{n} Bild", "{n} Bilder"]);

        let gone = &file.entries[3];
        assert!(gone.obsolete);
        assert_eq!((gone.id.as_str(), gone.translations.as_slice()), ("Gone", ["Weg".to_owned()].as_slice()));
    }

    #[test]
    fn escapes_are_decoded() {
        let file = parse(r#"msgid "a\\b \"q\" \n\t\r \101\x42 \'\?" msgstr """#).expect_err("two keywords on a line");
        assert_eq!(file.line, 1);
        let file = parse("msgid \"a\\\\b \\\"q\\\" \\n\\t\\r \\101\\x42 \\'\\?\"\nmsgstr \"\"").expect("parses");
        assert_eq!(file.entries[0].id, "a\\b \"q\" \n\t\r AB '?");
    }

    #[test]
    fn writing_and_reading_gives_the_same_file() {
        let file = parse(SAMPLE).expect("parses");
        let written = write(&file);
        assert_eq!(parse(&written).expect("parses again"), file);
        // Line breaks inside a text are written one line each; others stay on one line.
        assert!(written.contains("msgid \"\"\n\"Line one\\n\"\n\"Line \\\"two\\\"\\tend\"\n"), "{written}");
        assert!(written.contains("msgstr[1] \"{n} Bilder\"\n"));
        assert!(written.contains("#~ msgid \"Gone\"\n#~ msgstr \"Weg\"\n"));
        assert_eq!(write(&parse(&written).expect("parses")), written, "writing is stable");
        // A control character without a short escape survives as an octal escape.
        let bell = PoFile {
            header: None,
            entries: vec![Entry { id: "a\u{7}b".into(), translations: vec![String::new()], ..Entry::default() }],
        };
        assert_eq!(write(&bell), "msgid \"a\\007b\"\nmsgstr \"\"\n");
        assert_eq!(parse(&write(&bell)).expect("parses"), bell);
    }

    #[test]
    fn malformed_files_are_errors_with_their_line() {
        let cases: &[(&str, usize, &str)] = &[
            ("msgid \"a\"\nmsgstr \"b", 2, "not closed"),
            ("msgid \"a\"\nmsgstr \"b\" x", 2, "after the closing quote"),
            ("msgid \"a\\q\"\nmsgstr \"\"", 1, "unknown escape"),
            ("msgid \"a\"\nmsgid \"b\"\nmsgstr \"\"", 2, "without msgstr"),
            ("msgid \"a\"\n", 1, "without msgstr"),
            ("msgstr \"b\"", 1, "must follow msgid"),
            ("msgid \"a\"\nmsgstr \"b\"\nmsgstr \"c\"", 3, "must follow msgid"),
            ("msgid \"a\"\nmsgid_plural \"as\"\nmsgstr \"b\"", 3, "needs msgstr[0]"),
            ("msgid \"a\"\nmsgid_plural \"as\"\nmsgstr[1] \"b\"", 3, "out of order"),
            ("msgid \"a\"\nmsgid_plural \"as\"\nmsgstr[0] \"b\"", 1, "at least msgstr[0] and msgstr[1]"),
            ("msgid \"a\"\nmsgstr[0] \"b\"", 2, "needs msgid_plural"),
            ("msgid \"a\"\nmsgstr \"b\"\n\nmsgid \"a\"\nmsgstr \"c\"", 4, "twice"),
            ("msgid \"a\"\nmsgstr \"b\"\nmsgid \"\"\nmsgstr \"x\"", 3, "header"),
            ("\"orphan\"", 1, "without a keyword"),
            ("msgid \"a\"\n#~ msgstr \"b\"", 2, "partly obsolete"),
            ("msgfoo \"a\"", 1, "unknown keyword"),
            ("msgid a\nmsgstr \"\"", 1, "quoted string"),
            ("msgid \"a\\x\"\nmsgstr \"\"", 1, "hexadecimal"),
        ];
        for (text, line, message) in cases {
            let error = parse(text).expect_err(text);
            assert_eq!(error.line, *line, "{text:?}: {error}");
            assert!(error.message.contains(message), "{text:?}: {error}");
        }
    }

    #[test]
    fn entries_without_blank_lines_and_trailing_comments_are_fine() {
        let file = parse("msgid \"a\"\nmsgstr \"b\"\n# next\nmsgid \"c\"\nmsgstr \"d\"\n# trailing\n").expect("parses");
        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[1].translator_comments, ["next"]);
        assert!(parse("").expect("an empty file").entries.is_empty());
    }
}
