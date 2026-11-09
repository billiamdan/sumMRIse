//! HTML to plain text: tags are removed and entities decoded, and that is all.
//! HTML is text here. Scripts never run, because there is nothing to run them;
//! style sheets, images, frames and objects are not read; links are not
//! followed; nothing is fetched.

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};

use super::text::{PlainText, split_bom};

/// Elements whose content is not text of the page, all with a closing tag that
/// HTML requires: an element without one, such as `<embed>` or a `<head>` that
/// is left open, must not take the rest of the page with it.
const NOT_TEXT: &[&str] =
    &["script", "style", "title", "template", "iframe", "object", "applet", "svg", "math", "audio", "video"];

/// Elements that begin a new line.
const BLOCKS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "br",
    "center",
    "dd",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "tr",
    "ul",
];

/// The named characters of Latin-1, in order from U+00A0.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf", "laquo", "not", "shy",
    "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro", "para", "middot", "cedil", "sup1", "ordm",
    "raquo", "frac14", "frac12", "frac34", "iquest", "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig",
    "Ccedil", "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve",
    "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute", "THORN",
    "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc",
    "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml",
    "divide", "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml",
];

/// Other named characters that reports use.
const OTHER_ENTITIES: &[(&str, char)] = &[
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("ndash", '–'),
    ("mdash", '—'),
    ("hellip", '…'),
    ("lsquo", '‘'),
    ("rsquo", '’'),
    ("ldquo", '“'),
    ("rdquo", '”'),
    ("bull", '•'),
    ("euro", '€'),
    ("trade", '™'),
    ("le", '≤'),
    ("ge", '≥'),
    ("ne", '≠'),
    ("asymp", '≈'),
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("mu", 'μ'),
];

fn named_entity(name: &str) -> Option<char> {
    OTHER_ENTITIES.iter().find(|(known, _)| *known == name).map(|(_, c)| *c).or_else(|| {
        let index = LATIN1.iter().position(|known| *known == name)?;
        char::from_u32(0xA0 + index as u32)
    })
}

/// The character that the entity at the start of `after_ampersand` stands
/// for, and the length of its name with the semicolon.
fn entity(after_ampersand: &str) -> Option<(char, usize)> {
    let end = after_ampersand.char_indices().take(32).find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '#')?;
    let (end, terminator) = end;
    if terminator != ';' || end == 0 {
        return None;
    }
    let name = &after_ampersand[..end];
    let character = match name.strip_prefix('#') {
        Some(number) => {
            let value = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(value)?
        }
        None => named_entity(name)?,
    };
    Some((character, end + 1))
}

/// Where the tag that starts at `text` (after its `<`) ends: after its `>`,
/// not counting one inside quotes.
fn tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (None, '>') => return Some(i + 1),
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), c) if c == open => quote = None,
            _ => {}
        }
    }
    None
}

/// The encoding an HTML file declares in its first kilobyte.
fn declared_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    let head = &bytes[..bytes.len().min(1024)];
    let lower = head.to_ascii_lowercase();
    let at = lower.windows(8).position(|w| w == b"charset=")?;
    let rest = &lower[at + 8..];
    let rest = rest.strip_prefix(b"\"").or_else(|| rest.strip_prefix(b"'")).unwrap_or(rest);
    let end = rest.iter().position(|b| !(b.is_ascii_alphanumeric() || b"-_:.".contains(b))).unwrap_or(rest.len());
    Encoding::for_label(&rest[..end])
}

/// Decodes an HTML file: by its byte-order mark or its declaration, as UTF-8
/// when it is valid UTF-8, else as Windows-1252 (a guess, which it says).
fn decode(bytes: &[u8]) -> (String, Option<&'static str>) {
    if let Some((encoding, rest)) = split_bom(bytes) {
        return (encoding.decode_without_bom_handling(rest).0.into_owned(), None);
    }
    if let Some(encoding) = declared_encoding(bytes) {
        return (encoding.decode_without_bom_handling(bytes).0.into_owned(), None);
    }
    match UTF_8.decode_without_bom_handling_and_without_replacement(bytes) {
        Some(text) => (text.into_owned(), None),
        None => (WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned(), Some(WINDOWS_1252.name())),
    }
}

/// Removes the element name of a tag from the text after its `<`: its name
/// in lower case and whether it is a closing tag.
fn tag_name(tag: &str) -> (String, bool) {
    let (tag, closing) = match tag.strip_prefix('/') {
        Some(rest) => (rest, true),
        None => (tag, false),
    };
    let name: String = tag.chars().take_while(char::is_ascii_alphanumeric).collect();
    (name.to_ascii_lowercase(), closing)
}

/// Finds the end of the element `name` whose content starts `rest`: the
/// position after its closing tag, or the end of the text.
fn skip_element(rest: &str, name: &str) -> usize {
    let mut from = 0;
    let lower = rest.to_ascii_lowercase();
    let closing = format!("</{name}");
    while let Some(found) = lower[from..].find(&closing) {
        let at = from + found + closing.len();
        // "</scriptx" closes nothing.
        if !lower[at..].starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return tag_end(&rest[at..]).map_or(rest.len(), |end| at + end);
        }
        from = at;
    }
    rest.len()
}

/// What comes out, with white space collapsed as a browser does it.
#[derive(Default)]
struct Output {
    text: String,
    /// Inside `<pre>`, white space stays as it is.
    preformatted: usize,
}

impl Output {
    fn push(&mut self, c: char) {
        if self.preformatted == 0 && c.is_whitespace() && c != '\u{A0}' {
            if !self.text.is_empty() && !self.text.ends_with([' ', '\n', '\t']) {
                self.text.push(' ');
            }
        } else {
            self.text.push(c);
        }
    }

    /// A new line, unless the text already is at the start of one.
    fn line(&mut self) {
        let trimmed = self.text.trim_end_matches(' ').len();
        self.text.truncate(trimmed);
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
    }
}

/// The text of an HTML file.
pub(super) fn to_text(bytes: &[u8]) -> PlainText {
    let (html, assumed_encoding) = decode(bytes);
    let mut out = Output::default();
    let mut rest = html.as_str();
    while let Some(c) = rest.chars().next() {
        match c {
            '<' => rest = markup(rest, &mut out),
            '&' => match entity(&rest[1..]) {
                Some((character, length)) => {
                    out.text.push(character);
                    rest = &rest[1 + length..];
                }
                None => {
                    out.push('&');
                    rest = &rest[1..];
                }
            },
            c => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    PlainText { assumed_encoding, ..PlainText::new(out.text.trim_end()) }
}

/// Handles the markup at the start of `rest` (which starts with `<`) and
/// returns what follows it.
fn markup<'a>(rest: &'a str, out: &mut Output) -> &'a str {
    let after = &rest[1..];
    if let Some(comment) = after.strip_prefix("!--") {
        return comment.find("-->").map_or("", |end| &comment[end + 3..]);
    }
    if after.starts_with(['!', '?']) {
        return tag_end(after).map_or("", |end| &after[end..]);
    }
    if !after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/') {
        // A lone "<" in text, as in "size < 5 mm".
        out.push('<');
        return after;
    }
    let Some(end) = tag_end(after) else { return "" };
    let (name, closing) = tag_name(&after[..end]);
    let following = &after[end..];
    if !closing && NOT_TEXT.contains(&name.as_str()) {
        // "<title/>" and the like have no content to skip.
        if after[..end].trim_end_matches('>').ends_with('/') {
            return following;
        }
        return &following[skip_element(following, &name)..];
    }
    if name == "pre" {
        out.preformatted = if closing { out.preformatted.saturating_sub(1) } else { out.preformatted + 1 };
    }
    if BLOCKS.contains(&name.as_str()) {
        out.line();
    } else if !closing && matches!(name.as_str(), "td" | "th") && !out.text.is_empty() && !out.text.ends_with('\n') {
        out.text.push('\t');
    }
    if !closing && name == "li" {
        out.text.push_str("- ");
    }
    following
}

#[cfg(test)]
mod tests {
    use summrise_testkit::reports::HTML_REPORT;

    use super::*;

    fn text(html: &str) -> String {
        to_text(html.as_bytes()).text
    }

    /// The scenario of the specification: text is shown, nothing else is touched.
    #[test]
    fn html_report_with_a_script_and_a_remote_image() {
        let shown = text(HTML_REPORT);
        assert_eq!(shown, "Radiology report\nThe lungs are clear & the heart is normal.\nSee the archive for more.");
        for absent in ["SCRIPT RAN", "fetch", "example.invalid", "tracker", "Hidden title", "color: red", "comment"] {
            assert!(!shown.contains(absent), "{absent:?} in {shown:?}");
        }
    }

    #[test]
    fn scripts_and_their_tricks_stay_out() {
        for html in [
            "<script>alert(1)</script>Text",
            "<SCRIPT type='x'>alert(1)</ScRiPt >Text",
            "<script>if (a < b) { document.write('</p>'); }</script>Text",
            "<style>p { content: '<b>'; }</style>Text",
            "<iframe src='http://example.invalid'>inner</iframe>Text",
            "<object data='x.exe'>inner</object>Text",
            "<svg><script>alert(1)</script></svg>Text",
            "<img src=x onerror=alert(1)>Text",
            r#"<img alt="a>b" src="http://example.invalid/x.png">Text"#,
        ] {
            assert_eq!(text(html), "Text", "{html}");
        }
        // Elements whose closing tag may be missing take nothing with them.
        assert_eq!(text("<html><head><title>Hidden</title><body>Text"), "Text");
        assert_eq!(text("<embed src='x.exe'>Text"), "Text");
        // Nothing after a script that is never closed is text.
        assert_eq!(text("Before<script>alert(1)"), "Before");
        // "</scriptx" does not close a script.
        assert_eq!(text("<script>a</scriptx>b</script>Text"), "Text");
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(text("Gr&ouml;&szlig;e 5&nbsp;mm &lt; 7 &amp; &#1047;&#x430;"), "Größe 5\u{A0}mm < 7 & За");
        // What is not an entity stays as it is written.
        assert_eq!(text("A&B &unknown; &#xZZ; &"), "A&B &unknown; &#xZZ; &");
        assert_eq!(text("&#1114112;"), "&#1114112;", "beyond Unicode");
    }

    #[test]
    fn structure_becomes_lines() {
        assert_eq!(text("<p>One</p><p>Two<br>Three</p>"), "One\nTwo\nThree");
        assert_eq!(text("<ul><li>One</li><li>Two</li></ul>"), "- One\n- Two");
        assert_eq!(text("<table><tr><td>A</td><td>B</td></tr><tr><td>C</td></tr></table>"), "A\tB\nC");
        assert_eq!(text("One   two\n  three"), "One two three");
        assert_eq!(text("<pre>One   two\n  three</pre>"), "One   two\n  three");
        assert_eq!(text("Size < 5 mm"), "Size < 5 mm");
    }

    #[test]
    fn the_character_set_comes_from_the_mark_or_the_declaration() {
        // Declared.
        let declared = b"<meta charset=\"windows-1251\"><p>\xC7\xE0</p>";
        assert_eq!(to_text(declared).text, "За");
        let http_equiv = b"<meta http-equiv=Content-Type content='text/html; charset=iso-8859-1'>Gr\xF6\xDFe";
        assert_eq!(to_text(http_equiv).text, "Größe");
        // A mark outranks a declaration.
        let marked = [b"\xEF\xBB\xBF<meta charset=windows-1251>".as_slice(), "За".as_bytes()].concat();
        assert_eq!(to_text(&marked).text, "За");
        // Neither: UTF-8 if it is, else a guess that says so.
        assert_eq!(to_text("За".as_bytes()).assumed_encoding, None);
        let guess = to_text(b"Gr\xF6\xDFe");
        assert_eq!(guess.text, "Größe");
        assert_eq!(guess.assumed_encoding, Some("windows-1252"));
    }

    #[test]
    fn malformed_html_is_never_a_panic() {
        for broken in ["<", "<a", "</", "<!", "<!--", "<?", "&", "&#", "<a href='", "<script", "<pre>", "</pre></pre>x"]
        {
            let _ = text(broken);
        }
        assert_eq!(text("</pre></pre>x"), "x");
        // Multi-byte characters around everything the tokenizer looks at.
        assert_eq!(text("<p>Зa</p>Зb&amp;Зc"), "Зa\nЗb&Зc");
        // A flood of markup takes linear time.
        let flood = "<a>".repeat(200_000);
        assert_eq!(text(&flood), "");
    }
}
