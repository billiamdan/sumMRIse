//! Finds the messages of the source code: the string literals passed to
//! `tr!`, `tr_n!` and `tr_ctx!`, with the comments for translators above
//! them. A small tokenizer reads Rust well enough for that: comments,
//! string literals (escaped, continued over lines, raw), characters and
//! lifetimes. Code that only tests compile (`#[cfg(test)]` items and test
//! modules) is left out: strings that only tests see are not translated.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::tooling::update::placeholders;

/// A message of the source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The context of `tr_ctx!`.
    pub context: Option<String>,
    /// The English text (the singular of `tr_n!`).
    pub id: String,
    /// The English plural of `tr_n!`.
    pub plural: Option<String>,
    /// The files that use the message, relative to the workspace root, in
    /// the order of first use.
    pub references: Vec<String>,
    /// Comments for translators (`// Translators: …` above a use).
    pub comments: Vec<String>,
}

/// A use of the macros that the catalogues cannot follow, or a placeholder
/// without a value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file}:{line}: {message}")]
pub struct ScanError {
    pub file: String,
    pub line: usize,
    pub message: String,
}

/// The Rust files whose messages the application shows: everything under
/// `src/` of the crates in `crates/` and `apps/`, except the testkit (test
/// data only), sorted by path.
pub fn source_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for group in ["apps", "crates"] {
        let Ok(crates) = std::fs::read_dir(root.join(group)) else { continue };
        for krate in crates {
            let krate = krate?.path();
            if krate.file_name().is_some_and(|name| name == "testkit") {
                continue;
            }
            collect_rust_files(&krate.join("src"), &mut files)?;
        }
    }
    files.sort();
    Ok(files)
}

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// The messages of the workspace at `root`, in the order of the files and of
/// their first use, or every problem found.
pub fn scan_workspace(root: &Path) -> Result<Vec<Message>, Vec<ScanError>> {
    let error = |file: &str, message: String| ScanError { file: file.to_owned(), line: 0, message };
    let files = source_files(root).map_err(|e| vec![error(&root.display().to_string(), e.to_string())])?;
    let mut scans = Vec::new();
    let mut errors = Vec::new();
    for path in &files {
        let relative = path.strip_prefix(root).unwrap_or(path).components().map(|c| c.as_os_str().to_string_lossy());
        let relative = relative.collect::<Vec<_>>().join("/");
        match std::fs::read_to_string(path) {
            Ok(source) => scans.push((path.clone(), relative.clone(), scan_source(&relative, &source))),
            Err(e) => errors.push(error(&relative, e.to_string())),
        }
    }
    // Files that only tests compile: test modules, and the modules they declare.
    let mut test_files: HashSet<PathBuf> = HashSet::new();
    for (path, _, scan) in &scans {
        if scan.test_only {
            test_files.insert(path.clone());
        }
        for (name, test) in &scan.modules {
            if *test {
                test_files.extend(module_files(path, name));
            }
        }
    }
    loop {
        let before = test_files.len();
        for (path, _, scan) in &scans {
            if test_files.contains(path) {
                for (name, _) in &scan.modules {
                    test_files.extend(module_files(path, name));
                }
            }
        }
        if test_files.len() == before {
            break;
        }
    }

    let mut messages: Vec<Message> = Vec::new();
    let mut index: HashMap<(Option<String>, String), usize> = HashMap::new();
    for (path, relative, scan) in scans {
        if test_files.contains(&path) {
            continue;
        }
        errors.extend(scan.errors);
        for found in scan.found {
            let key = (found.context.clone(), found.id.clone());
            match index.get(&key) {
                Some(&i) => {
                    let message = &mut messages[i];
                    if message.plural != found.plural {
                        errors.push(ScanError {
                            file: relative.clone(),
                            line: found.line,
                            message: format!(
                                "{:?} is used with another plural ({:?}) elsewhere ({:?} here)",
                                found.id, message.plural, found.plural
                            ),
                        });
                    }
                    if !message.references.contains(&relative) {
                        message.references.push(relative.clone());
                    }
                    if let Some(comment) = found.comment
                        && !message.comments.contains(&comment)
                    {
                        message.comments.push(comment);
                    }
                }
                None => {
                    index.insert(key, messages.len());
                    messages.push(Message {
                        context: found.context,
                        id: found.id,
                        plural: found.plural,
                        references: vec![relative.clone()],
                        comments: found.comment.into_iter().collect(),
                    });
                }
            }
        }
    }
    if errors.is_empty() { Ok(messages) } else { Err(errors) }
}

/// The files that can hold module `name` declared in `file`.
fn module_files(file: &Path, name: &str) -> [PathBuf; 2] {
    let parent = file.parent().unwrap_or(Path::new(""));
    let own_dir = matches!(file.file_name().and_then(|n| n.to_str()), Some("lib.rs" | "main.rs" | "mod.rs"));
    let dir = if own_dir { parent.to_path_buf() } else { parent.join(file.file_stem().unwrap_or_default()) };
    [dir.join(format!("{name}.rs")), dir.join(name).join("mod.rs")]
}

/// What one file contributes.
#[derive(Debug, Default)]
pub struct FileScan {
    /// The messages, in the order of the file.
    pub found: Vec<Found>,
    /// Modules declared with `mod name;`, and whether only tests compile them.
    pub modules: Vec<(String, bool)>,
    /// The file is compiled for tests only (`#![cfg(test)]`).
    pub test_only: bool,
    pub errors: Vec<ScanError>,
}

/// One use of a macro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub context: Option<String>,
    pub id: String,
    pub plural: Option<String>,
    pub line: usize,
    pub comment: Option<String>,
}

/// Finds the messages of one Rust file (`file` names it in errors).
pub fn scan_source(file: &str, source: &str) -> FileScan {
    let mut scan = FileScan::default();
    let error = |line: usize, message: String| ScanError { file: file.to_owned(), line, message };
    let (tokens, mut comments) = tokenize(source);
    let mut skipped: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(end) = cfg_test_attribute(&tokens, i) {
            if tokens.get(i + 1).is_some_and(|t| t.is_punct('!')) {
                scan.test_only = true;
                return scan;
            }
            let (item_end, module) = skip_item(&tokens, end + 1);
            if let Some(name) = module {
                scan.modules.push((name, true));
            }
            skipped.push((tokens[i].line, tokens.get(item_end).map_or(usize::MAX, |t| t.line)));
            i = item_end + 1;
            continue;
        }
        if tokens[i].is_ident("mod")
            && let Some(Token { kind: Kind::Ident(name), .. }) = tokens.get(i + 1)
            && tokens.get(i + 2).is_some_and(|t| t.is_punct(';'))
        {
            scan.modules.push((name.clone(), false));
        }
        if let Kind::Ident(name) = &tokens[i].kind
            && matches!(name.as_str(), "tr" | "tr_n" | "tr_ctx")
            && tokens.get(i + 1).is_some_and(|t| t.is_punct('!'))
            && tokens.get(i + 2).is_some_and(|t| t.is_punct('('))
        {
            match invocation(name, &tokens, i + 2) {
                Ok(found) => scan.found.push(found),
                Err(message) => scan.errors.push(error(tokens[i].line, message)),
            }
        }
        i += 1;
    }
    // Comments for translators go with the first message in the next three lines.
    comments.retain(|c| !skipped.iter().any(|&(from, to)| c.end_line >= from && c.end_line <= to));
    for comment in comments {
        let next = scan.found.iter_mut().find(|f| f.line > comment.end_line);
        match next {
            Some(found) if found.line <= comment.end_line + 3 && found.comment.is_none() => {
                found.comment = Some(comment.text);
            }
            _ => scan.errors.push(error(
                comment.start_line,
                "a `Translators:` comment needs a message in the next three lines".to_owned(),
            )),
        }
    }
    scan
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Ident(String),
    Punct(char),
    /// A string literal and its value.
    Str(String),
    /// Numbers, characters, lifetimes, byte and C strings.
    Other,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    line: usize,
}

impl Token {
    fn is_punct(&self, c: char) -> bool {
        self.kind == Kind::Punct(c)
    }

    fn is_ident(&self, name: &str) -> bool {
        matches!(&self.kind, Kind::Ident(n) if n == name)
    }
}

/// A `// Translators: …` comment, continued by the line comments right below it.
#[derive(Debug)]
struct TranslatorComment {
    start_line: usize,
    end_line: usize,
    text: String,
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn tokenize(source: &str) -> (Vec<Token>, Vec<TranslatorComment>) {
    let chars: Vec<char> = source.chars().collect();
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    let mut tokens = Vec::new();
    let mut comments: Vec<TranslatorComment> = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < chars.len() {
        let c = chars[i];
        let after_ident = i > 0 && is_ident_char(chars[i - 1]);
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && at(i + 1) == '/' {
            let end = (i..chars.len()).find(|&j| chars[j] == '\n').unwrap_or(chars.len());
            let text: String = chars[i + 2..end].iter().collect();
            // Doc comments (`///`, `//!`) are documentation, not notes for translators.
            if !text.starts_with('/') && !text.starts_with('!') {
                let text = text.trim();
                if let Some(note) = text.strip_prefix("Translators:") {
                    comments.push(TranslatorComment { start_line: line, end_line: line, text: note.trim().to_owned() });
                } else if let Some(open) = comments.last_mut().filter(|c| c.end_line + 1 == line)
                    && !text.is_empty()
                {
                    open.text.push(' ');
                    open.text.push_str(text);
                    open.end_line = line;
                }
            }
            i = end;
        } else if c == '/' && at(i + 1) == '*' {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    line += usize::from(chars[i] == '\n');
                    i += 1;
                }
            }
        } else if !after_ident && let Some((value, end, lines, plain)) = raw_string(&chars, i) {
            tokens.push(Token { kind: if plain { Kind::Str(value) } else { Kind::Other }, line });
            line += lines;
            i = end;
        } else if !after_ident && (c == '"' || (matches!(c, 'b' | 'c') && at(i + 1) == '"')) {
            let plain = c == '"';
            let (value, end, lines) = string(&chars, if plain { i } else { i + 1 });
            tokens.push(Token { kind: if plain { Kind::Str(value) } else { Kind::Other }, line });
            line += lines;
            i = end;
        } else if c == '\'' {
            // A character ('a', '\n', '\u{4}') or a lifetime or label ('a, 'static).
            if at(i + 1) == '\\' {
                let mut j = i + 3;
                while j < chars.len() && chars[j] != '\'' {
                    j += 1;
                }
                i = j + 1;
            } else if at(i + 2) == '\'' {
                i += 3;
            } else {
                i += 1;
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
            }
            tokens.push(Token { kind: Kind::Other, line });
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && is_ident_char(chars[i]) {
                i += 1;
            }
            tokens.push(Token { kind: Kind::Ident(chars[start..i].iter().collect()), line });
        } else if c.is_ascii_digit() {
            while i < chars.len() && (is_ident_char(chars[i]) || (chars[i] == '.' && at(i + 1).is_ascii_digit())) {
                i += 1;
            }
            tokens.push(Token { kind: Kind::Other, line });
        } else {
            tokens.push(Token { kind: Kind::Punct(c), line });
            i += 1;
        }
    }
    (tokens, comments)
}

/// A raw string (`r"…"`, `r#"…"#`, also `br"…"` and `cr"…"`) starting at
/// `i`: its value, the index after it, the lines it spans and whether it is
/// a plain string (not a byte or C string).
fn raw_string(chars: &[char], i: usize) -> Option<(String, usize, usize, bool)> {
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    let (plain, mut j) = match (chars[i], at(i + 1)) {
        ('r', _) => (true, i + 1),
        ('b' | 'c', 'r') => (false, i + 2),
        _ => return None,
    };
    let mut hashes = 0;
    while at(j) == '#' {
        hashes += 1;
        j += 1;
    }
    if at(j) != '"' {
        return None;
    }
    let start = j + 1;
    let mut k = start;
    while k < chars.len() {
        if chars[k] == '"' && (1..=hashes).all(|h| at(k + h) == '#') {
            let value: String = chars[start..k].iter().collect();
            let lines = value.matches('\n').count();
            return Some((value, k + 1 + hashes, lines, plain));
        }
        k += 1;
    }
    Some((chars[start..].iter().collect(), chars.len(), 0, plain))
}

/// The string literal whose opening quote is at `i`: its value, the index
/// after it and the lines it spans. Escapes are decoded; a backslash at the
/// end of a line skips the line break and the indentation after it.
fn string(chars: &[char], i: usize) -> (String, usize, usize) {
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    let (mut value, mut j, mut lines) = (String::new(), i + 1, 0);
    while j < chars.len() && chars[j] != '"' {
        let c = chars[j];
        if c == '\n' {
            lines += 1;
        }
        if c != '\\' {
            value.push(c);
            j += 1;
            continue;
        }
        let e = at(j + 1);
        j += 2;
        match e {
            'n' => value.push('\n'),
            'r' => value.push('\r'),
            't' => value.push('\t'),
            '0' => value.push('\0'),
            'x' => {
                let hex: String = chars.get(j..j + 2).unwrap_or_default().iter().collect();
                value.extend(u8::from_str_radix(&hex, 16).ok().map(char::from));
                j += 2;
            }
            'u' => {
                let close = (j..chars.len()).find(|&k| chars[k] == '}').unwrap_or(j);
                let hex: String = chars.get(j + 1..close).unwrap_or_default().iter().collect();
                value.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
                j = close + 1;
            }
            '\n' => {
                lines += 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    lines += usize::from(chars[j] == '\n');
                    j += 1;
                }
            }
            other => value.push(other),
        }
    }
    (value, j + 1, lines)
}

/// If an attribute `#[cfg(test)]` (or `#![cfg(test)]`) starts at `i`, the
/// index of its closing bracket.
fn cfg_test_attribute(tokens: &[Token], i: usize) -> Option<usize> {
    if !tokens[i].is_punct('#') {
        return None;
    }
    let start = if tokens.get(i + 1).is_some_and(|t| t.is_punct('!')) { i + 2 } else { i + 1 };
    let expected = [Kind::Punct('['), Kind::Ident("cfg".into()), Kind::Punct('('), Kind::Ident("test".into())];
    let rest = [Kind::Punct(')'), Kind::Punct(']')];
    let tail = tokens.get(start..start + 6)?;
    let kinds: Vec<&Kind> = tail.iter().map(|t| &t.kind).collect();
    (kinds[..4].iter().copied().eq(expected.iter()) && kinds[4..].iter().copied().eq(rest.iter())).then_some(start + 5)
}

/// Skips the item that starts at `i` (after an attribute): further
/// attributes, then everything up to a `;` or the end of a `{…}` block.
/// Gives the index of its last token and, for `mod name;`, the name.
fn skip_item(tokens: &[Token], mut i: usize) -> (usize, Option<String>) {
    while tokens.get(i).is_some_and(|t| t.is_punct('#')) && tokens.get(i + 1).is_some_and(|t| t.is_punct('[')) {
        i = closing(tokens, i + 1);
        i += 1;
    }
    let start = i;
    let mut depth = 0usize;
    while i < tokens.len() {
        match tokens[i].kind {
            Kind::Punct('(' | '[') => depth += 1,
            Kind::Punct(')' | ']') => depth = depth.saturating_sub(1),
            Kind::Punct('{') => depth += 1,
            Kind::Punct('}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return (i, None);
                }
            }
            Kind::Punct(';') if depth == 0 => {
                let module = tokens[start..i].windows(2).find_map(|w| match (&w[0].kind, &w[1].kind) {
                    (Kind::Ident(m), Kind::Ident(name)) if m == "mod" => Some(name.clone()),
                    _ => None,
                });
                return (i, module);
            }
            _ => {}
        }
        i += 1;
    }
    (tokens.len(), None)
}

/// The index of the bracket that closes the one at `open`.
fn closing(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        match token.kind {
            Kind::Punct('(' | '[' | '{') => depth += 1,
            Kind::Punct(')' | ']' | '}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}

/// The message of the macro `name` whose `(` is at `open`, checked: string
/// literals where the macro takes them, values passed as `name = value`, and
/// a value for every placeholder.
fn invocation(name: &str, tokens: &[Token], open: usize) -> Result<Found, String> {
    let close = closing(tokens, open);
    let mut segments: Vec<Vec<&Token>> = vec![Vec::new()];
    let mut depth = 0usize;
    for token in tokens.get(open + 1..close).unwrap_or_default() {
        match token.kind {
            Kind::Punct('(' | '[' | '{') => depth += 1,
            Kind::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            Kind::Punct(',') if depth == 0 => {
                segments.push(Vec::new());
                continue;
            }
            _ => {}
        }
        if let Some(segment) = segments.last_mut() {
            segment.push(token);
        }
    }
    if segments.last().is_some_and(Vec::is_empty) {
        segments.pop();
    }
    let literal = |index: usize, what: &str| -> Result<String, String> {
        match segments.get(index).map(Vec::as_slice) {
            Some([Token { kind: Kind::Str(value), .. }]) => Ok(value.clone()),
            _ => Err(format!("the {what} of {name}! must be a string literal")),
        }
    };
    let (context, id, plural, first_value) = match name {
        "tr_n" => {
            if segments.get(2).is_none_or(Vec::is_empty) {
                return Err("tr_n! needs a count after the two texts".to_owned());
            }
            (None, literal(0, "singular")?, Some(literal(1, "plural")?), 3)
        }
        "tr_ctx" => (Some(literal(0, "context")?), literal(1, "text")?, None, 2),
        _ => (None, literal(0, "text")?, None, 1),
    };
    let mut values = BTreeSet::new();
    for segment in segments.iter().skip(first_value) {
        match segment.as_slice() {
            [Token { kind: Kind::Ident(value), .. }, eq, rest @ ..]
                if eq.is_punct('=') && rest.first().is_some_and(|t| !t.is_punct('=')) =>
            {
                if !values.insert(value.clone()) {
                    return Err(format!("{value} is given twice"));
                }
            }
            _ => return Err(format!("values of {name}! are passed as `name = value`")),
        }
    }
    if id.is_empty() {
        return Err(format!("{name}! of an empty text"));
    }
    let mut used = placeholders(&id).map_err(|e| format!("{id:?}: {e}"))?;
    if let Some(plural) = &plural {
        if values.contains("n") {
            return Err("tr_n! gives {n} itself; another value cannot be called n".to_owned());
        }
        let in_plural = placeholders(plural).map_err(|e| format!("{plural:?}: {e}"))?;
        if !in_plural.contains("n") {
            return Err(format!("the plural {plural:?} must show the count {{n}}"));
        }
        used.extend(in_plural);
        values.insert("n".to_owned());
    } else if values.is_empty() && (id.contains('{') || id.contains('}')) {
        return Err(format!("{id:?}: a text without values cannot contain braces"));
    }
    if used != values {
        let missing: Vec<&String> = used.difference(&values).collect();
        let unused: Vec<&String> = values.difference(&used).collect();
        return Err(format!(
            "{id:?}: placeholders without a value {missing:?}, values without a placeholder {unused:?}"
        ));
    }
    Ok(Found { context, id, plural, line: tokens[open].line, comment: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(source: &str) -> Vec<Found> {
        let scan = scan_source("test.rs", source);
        assert!(scan.errors.is_empty(), "{:?}", scan.errors);
        scan.found
    }

    fn errors(source: &str) -> Vec<String> {
        scan_source("test.rs", source).errors.into_iter().map(|e| e.message).collect()
    }

    #[test]
    fn finds_the_literals_of_the_three_macros() {
        let source = r##"
            //! Docs show `tr!("Not a message")`.
            /// Also `tr!("Not one either")`.
            fn f() {
                let a = tr!("Open folder…");
                let b = summrise_i18n::tr!("Image {index} / {count}", index = i + 1, count = n);
                let c = tr_n!("{n} image", "{n} images", files.len(), );
                let d = tr_ctx!("contrast control", "Window");
                let e = tr_ctx!("window level", "L {value}", value = 40);
                let s = "tr!(\"inside a string\")";
                let r = r#"tr!("inside a raw string")"#;
                /* tr!("in a block comment /* nested */ still") */
                let x = 'a'; let y = '\''; let z = '\u{4}'; fn g<'a>(v: &'a str) {}
                let nested = tr!("{a} and {b}", a = tr!("one"), b = f(x, (1, 2)));
                let raw = tr!(r"C:\path");
            }
        "##;
        let found = messages(source);
        let ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "Open folder…",
                "Image {index} / {count}",
                "{n} image",
                "Window",
                "L {value}",
                "{a} and {b}",
                "one",
                r"C:\path"
            ]
        );
        assert_eq!(found[2].plural.as_deref(), Some("{n} images"));
        assert_eq!(found[3].context.as_deref(), Some("contrast control"));
        assert_eq!(found[0].line, 5, "line numbers");
    }

    #[test]
    fn multi_line_and_escaped_literals_are_read_as_the_compiler_reads_them() {
        let source = "fn f() {\n    tr!(\"A viewer for MRI and CT studies, \\\n         with \\\"presets\\\".\\n\\tEnd \\u{2014} \\x41\");\n    tr!(\"two\nlines\");\n    tr!(\"after\");\n}\n";
        let found = messages(source);
        assert_eq!(found[0].id, "A viewer for MRI and CT studies, with \"presets\".\n\tEnd — A");
        assert_eq!(found[1].id, "two\nlines");
        assert_eq!(found[2].line, 6, "lines inside literals are counted");
    }

    #[test]
    fn code_compiled_only_for_tests_is_left_out() {
        let source = r#"
            fn shown() { tr!("Shown"); }
            #[cfg(test)]
            mod tests {
                fn helper() { tr!("Only in a test"); }
            }
            #[cfg(test)]
            #[allow(dead_code)]
            fn test_helper() -> &'static str { tr!("Test helper") }
            #[cfg(test)]
            mod more_tests;
            mod real;
            fn also_shown() { tr!("Also shown"); }
        "#;
        let scan = scan_source("crates/x/src/viewer.rs", source);
        let ids: Vec<&str> = scan.found.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["Shown", "Also shown"]);
        assert_eq!(scan.modules, [("more_tests".to_owned(), true), ("real".to_owned(), false)]);
        assert!(scan_source("t.rs", "#![cfg(test)]\nfn f() { tr!(\"x\"); }").test_only);
        let [own, nested] = module_files(Path::new("crates/x/src/viewer.rs"), "more_tests");
        assert_eq!(own, Path::new("crates/x/src/viewer/more_tests.rs"));
        assert_eq!(nested, Path::new("crates/x/src/viewer/more_tests/mod.rs"));
        assert_eq!(module_files(Path::new("crates/x/src/lib.rs"), "a")[0], Path::new("crates/x/src/a.rs"));
    }

    #[test]
    fn comments_for_translators_go_with_the_next_message() {
        let source = r#"
            // Translators: "W" is short for the window width,
            // the contrast of the image.
            let w = tr_ctx!("window width", "W");
            let l = tr_ctx!("window level", "L");
            // A plain comment.
            ui.label(
                // Translators: shown under the progress bar.
                tr_n!("{n} file found", "{n} files found", n),
            );
        "#;
        let found = messages(source);
        assert_eq!(
            found[0].comment.as_deref(),
            Some("\"W\" is short for the window width, the contrast of the image.")
        );
        assert_eq!(found[1].comment, None, "one comment, one message");
        assert_eq!(found[2].comment.as_deref(), Some("shown under the progress bar."));
        assert_eq!(
            errors("// Translators: lost\n\n\n\n\nlet x = tr!(\"far away\");"),
            ["a `Translators:` comment needs a message in the next three lines"]
        );
    }

    #[test]
    fn macros_that_the_catalogues_cannot_follow_are_errors() {
        let cases = [
            ("tr!(TEXT)", "must be a string literal"),
            ("tr!(concat!(\"a\", \"b\"))", "must be a string literal"),
            ("tr!(\"Image {index}\")", "cannot contain braces"),
            ("tr!(\"Image {index}\", count = 3)", "placeholders without a value [\"index\"]"),
            ("tr!(\"Image\", count = 3)", "values without a placeholder [\"count\"]"),
            ("tr!(\"{a}\", a = 1, a = 2)", "given twice"),
            ("tr!(\"{a}\", 1)", "name = value"),
            ("tr!(\"{a}\", a == 1)", "name = value"),
            ("tr!(\"{a:.1}\", a = 1)", "not a placeholder name"),
            ("tr!(\"{a\", a = 1)", "brace"),
            ("tr_n!(\"one\", \"many\", n)", "must show the count"),
            ("tr_n!(\"{n} a\", \"{n} b\")", "needs a count"),
            ("tr_n!(\"{n} a\", \"{n} b\", k, n = 3)", "cannot be called n"),
            ("tr_ctx!(CONTEXT, \"Window\")", "context of tr_ctx! must be a string literal"),
            ("tr!(\"\")", "empty text"),
        ];
        for (code, message) in cases {
            let errors = errors(code);
            assert!(errors.len() == 1 && errors[0].contains(message), "{code}: {errors:?}");
        }
        // The singular may leave out the count, the plural not.
        assert_eq!(messages("tr_n!(\"another archive\", \"{n} other archives\", n)").len(), 1);
    }
}
