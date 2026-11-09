//! RTF to plain text: a small converter, not a renderer. It follows the groups
//! and control words only as far as it takes to find the text and its code
//! page. Nothing embedded in the file (objects, pictures, fields) is processed.

use std::collections::HashMap;

use encoding_rs::{
    BIG5, EUC_KR, Encoding, GBK, IBM866, KOI8_R, KOI8_U, MACINTOSH, SHIFT_JIS, UTF_8, WINDOWS_874, WINDOWS_1250,
    WINDOWS_1251, WINDOWS_1252, WINDOWS_1253, WINDOWS_1254, WINDOWS_1255, WINDOWS_1256, WINDOWS_1257, WINDOWS_1258,
};
use summrise_i18n::tr;

use super::text::PlainText;

/// Deepest group followed. Word processors nest a handful of levels.
const MAX_GROUP_DEPTH: usize = 512;

/// Groups whose content is not text of the document.
const SKIPPED_DESTINATIONS: &[&str] = &[
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "pict",
    "object",
    "objdata",
    "listtable",
    "listoverridetable",
    "revtbl",
    "rsidtbl",
    "generator",
    "themedata",
    "colorschememapping",
    "datastore",
    "latentstyles",
    "xmlnstbl",
    "filetbl",
    "fldinst",
    "bkmkstart",
    "bkmkend",
    "nonshppict",
];

/// The encoding of a Windows code page (`\ansicpgN`).
fn code_page(number: i32) -> Option<&'static Encoding> {
    Some(match number {
        874 => WINDOWS_874,
        866 => IBM866,
        932 => SHIFT_JIS,
        936 => GBK,
        949 => EUC_KR,
        950 => BIG5,
        1250 => WINDOWS_1250,
        1251 => WINDOWS_1251,
        1252 => WINDOWS_1252,
        1253 => WINDOWS_1253,
        1254 => WINDOWS_1254,
        1255 => WINDOWS_1255,
        1256 => WINDOWS_1256,
        1257 => WINDOWS_1257,
        1258 => WINDOWS_1258,
        10000 => MACINTOSH,
        20866 => KOI8_R,
        21866 => KOI8_U,
        65001 => UTF_8,
        _ => return None,
    })
}

/// The encoding of a font's character set (`\fcharsetN`); `None` for those
/// that say nothing beyond the document's code page.
fn font_charset(number: i32) -> Option<&'static Encoding> {
    Some(match number {
        77 => MACINTOSH,
        128 => SHIFT_JIS,
        129 => EUC_KR,
        134 => GBK,
        136 => BIG5,
        161 => WINDOWS_1253,
        162 => WINDOWS_1254,
        163 => WINDOWS_1258,
        177 => WINDOWS_1255,
        178 => WINDOWS_1256,
        186 => WINDOWS_1257,
        204 => WINDOWS_1251,
        222 => WINDOWS_874,
        238 => WINDOWS_1250,
        255 => IBM866,
        _ => return None,
    })
}

#[derive(Clone, Copy)]
struct Group {
    /// Its content is not text.
    skip: bool,
    /// How many characters follow a `\u` as its fallback for old readers.
    fallback_chars: usize,
    /// The encoding of its 8-bit characters: that of its font, or `None` for
    /// the code page of the document.
    encoding: Option<&'static Encoding>,
    /// In the font table: the number of the font being described.
    font: Option<i32>,
    in_font_table: bool,
}

struct Converter<'a> {
    bytes: &'a [u8],
    at: usize,
    out: String,
    /// 8-bit characters not decoded yet: two of them can be one character.
    pending: Vec<u8>,
    document_encoding: &'static Encoding,
    fonts: HashMap<i32, &'static Encoding>,
    /// A high surrogate waiting for its other half.
    surrogate: Option<u16>,
    /// Fallback characters of a `\u` still to skip.
    to_skip: usize,
    group: Group,
    stack: Vec<Group>,
}

impl Converter<'_> {
    fn flush(&mut self, encoding: Option<&'static Encoding>) {
        if !self.pending.is_empty() {
            let encoding = encoding.unwrap_or(self.document_encoding);
            let (text, _) = encoding.decode_without_bom_handling(&self.pending);
            self.out.push_str(&text);
            self.pending.clear();
        }
    }

    fn push_text(&mut self, text: &str) {
        let encoding = self.group.encoding;
        self.flush(encoding);
        if !self.group.skip {
            self.out.push_str(text);
        }
    }

    /// A character of the document in an 8-bit code page.
    fn push_byte(&mut self, byte: u8) {
        if self.to_skip > 0 {
            self.to_skip -= 1;
        } else if !self.group.skip {
            self.pending.push(byte);
        }
    }

    fn push_unicode(&mut self, value: i32) {
        // RTF writes values above 32767 as negative numbers.
        let unit = if value < 0 { (value + 65536) as u32 } else { value as u32 };
        let encoding = self.group.encoding;
        self.flush(encoding);
        if !self.group.skip {
            let Ok(unit) = u16::try_from(unit) else { return };
            match (self.surrogate.take(), unit) {
                (None, 0xD800..=0xDBFF) => self.surrogate = Some(unit),
                (high, unit) => {
                    let units: Vec<u16> = high.into_iter().chain([unit]).collect();
                    self.out.extend(char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)));
                }
            }
        }
        self.to_skip = self.group.fallback_chars;
    }

    fn control_word(&mut self, word: &str, parameter: Option<i32>) {
        // A control word is one character of the fallback of a `\u` (which never is a destination).
        if self.to_skip > 0 {
            self.to_skip -= 1;
            return;
        }
        match (word, parameter) {
            ("ansicpg", Some(number)) => {
                if let Some(encoding) = code_page(number) {
                    self.document_encoding = encoding;
                }
            }
            ("mac", _) => self.document_encoding = MACINTOSH,
            ("uc", Some(count)) => self.group.fallback_chars = usize::try_from(count).unwrap_or(1),
            ("u", Some(value)) => self.push_unicode(value),
            ("fonttbl", _) => {
                self.group.skip = true;
                self.group.in_font_table = true;
            }
            ("f", Some(font)) => {
                if self.group.in_font_table {
                    self.group.font = Some(font);
                } else {
                    let encoding = self.group.encoding;
                    self.flush(encoding);
                    self.group.encoding = self.fonts.get(&font).copied();
                }
            }
            ("fcharset", Some(charset)) => {
                if let (Some(font), Some(encoding)) = (self.group.font, font_charset(charset)) {
                    self.fonts.insert(font, encoding);
                }
            }
            ("par" | "line" | "sect" | "page" | "row", _) => self.push_text("\n"),
            ("tab" | "cell", _) => self.push_text("\t"),
            ("emdash", _) => self.push_text("—"),
            ("endash", _) => self.push_text("–"),
            ("bullet", _) => self.push_text("•"),
            ("lquote", _) => self.push_text("‘"),
            ("rquote", _) => self.push_text("’"),
            ("ldblquote", _) => self.push_text("“"),
            ("rdblquote", _) => self.push_text("”"),
            (word, _) if SKIPPED_DESTINATIONS.contains(&word) => self.group.skip = true,
            _ => {}
        }
    }

    /// Reads what follows a backslash.
    fn control(&mut self) {
        let Some(&next) = self.bytes.get(self.at) else { return };
        match next {
            b'\'' => {
                let hex = self.bytes.get(self.at + 1..self.at + 3);
                let byte =
                    hex.and_then(|hex| std::str::from_utf8(hex).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
                self.at += 3;
                if let Some(byte) = byte {
                    self.push_byte(byte);
                }
            }
            b'*' => {
                // "Ignore this group if you do not know the destination", and none is known here.
                self.at += 1;
                self.group.skip = true;
            }
            b'a'..=b'z' | b'A'..=b'Z' => {
                let start = self.at;
                while self.bytes.get(self.at).is_some_and(u8::is_ascii_alphabetic) {
                    self.at += 1;
                }
                let word = String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned();
                let digits = self.at;
                if self.bytes.get(self.at) == Some(&b'-') {
                    self.at += 1;
                }
                while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
                let parameter = std::str::from_utf8(&self.bytes[digits..self.at]).ok().and_then(|p| p.parse().ok());
                // One space ends the control word and belongs to it.
                if self.bytes.get(self.at) == Some(&b' ') {
                    self.at += 1;
                }
                self.control_word(&word, parameter);
            }
            b'\\' | b'{' | b'}' => {
                self.at += 1;
                self.push_byte(next);
            }
            b'~' => {
                self.at += 1;
                self.push_text("\u{A0}");
            }
            b'_' => {
                self.at += 1;
                self.push_text("-");
            }
            // The end of a line after a backslash is a paragraph; an optional hyphen is nothing.
            b'\n' | b'\r' => {
                self.at += 1;
                self.push_text("\n");
            }
            _ => self.at += 1,
        }
    }

    fn run(&mut self) -> Result<(), String> {
        while let Some(&byte) = self.bytes.get(self.at) {
            match byte {
                b'{' => {
                    self.at += 1;
                    self.to_skip = 0;
                    if self.stack.len() >= MAX_GROUP_DEPTH {
                        return Err(tr!("this file is nested too deeply").to_owned());
                    }
                    self.stack.push(self.group);
                }
                b'}' => {
                    self.at += 1;
                    self.to_skip = 0;
                    let encoding = self.group.encoding;
                    self.flush(encoding);
                    if let Some(group) = self.stack.pop() {
                        self.group = group;
                    }
                }
                b'\\' => {
                    self.at += 1;
                    self.control();
                }
                b'\r' | b'\n' => self.at += 1,
                byte => {
                    self.at += 1;
                    self.push_byte(byte);
                }
            }
        }
        let encoding = self.group.encoding;
        self.flush(encoding);
        Ok(())
    }
}

/// The text of an RTF file.
pub(super) fn to_text(bytes: &[u8]) -> Result<PlainText, String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(0);
    if !bytes[start..].starts_with(b"{\\rtf") {
        return Err(tr!("this file is not an RTF document").to_owned());
    }
    let group = Group { skip: false, fallback_chars: 1, encoding: None, font: None, in_font_table: false };
    let mut converter = Converter {
        bytes: &bytes[start..],
        at: 0,
        out: String::new(),
        pending: Vec::new(),
        document_encoding: WINDOWS_1252,
        fonts: HashMap::new(),
        surrogate: None,
        to_skip: 0,
        group,
        stack: Vec::new(),
    };
    converter.run()?;
    let text = converter.out.trim_end().to_owned();
    Ok(PlainText::new(&text))
}

#[cfg(test)]
mod tests {
    use summrise_testkit::reports::{RTF_CYRILLIC_TEXT, rtf_cyrillic};

    use super::*;

    fn text(rtf: &[u8]) -> String {
        to_text(rtf).expect("an RTF file").text
    }

    #[test]
    fn windows_1251_cyrillic_is_displayed_correctly() {
        let shown = text(&rtf_cyrillic());
        assert!(shown.contains(RTF_CYRILLIC_TEXT), "{shown}");
        // What is no text of the document is not shown.
        for hidden in ["Times New Roman", "Hidden author", "Synthetic 1.0", "0123456789abcdef", "Normal"] {
            assert!(!shown.contains(hidden), "{hidden:?} in {shown:?}");
        }
    }

    #[test]
    fn code_page_comes_from_the_font_when_the_font_has_one() {
        // A document in code page 1252 whose font is Cyrillic: common with old word processors.
        let rtf = br"{\rtf1\ansi\ansicpg1252{\fonttbl{\f0\fnil\fcharset0 Arial;}{\f1\fnil\fcharset204 Arial;}}\f1 \'c4\'e0 \f0 caf\'e9}";
        assert_eq!(text(rtf), "Да café");
        // No font says anything: the code page of the document decides.
        assert_eq!(text(br"{\rtf1\ansi\ansicpg1251 \'c7\'e0}"), "За");
        assert_eq!(text(br"{\rtf1\ansi\ansicpg1252 \'c7\'e0}"), "Çà");
        // The Greek code page.
        assert_eq!(text(br"{\rtf1\ansi\ansicpg1253 \'e1\'e2\'e3}"), "αβγ");
        // Two bytes that are one character in a double-byte code page.
        assert_eq!(text(br"{\rtf1\ansi\ansicpg932 \'93\'fa\'96\'7b}"), "日本");
    }

    #[test]
    fn unicode_escapes_skip_their_fallback() {
        // \u with its negative form and the fallback character for old readers.
        let u = |value: i32| format!("\\u{value}");
        let rtf = |body: String| format!("{{\\rtf1\\ansi{body}}}").into_bytes();
        assert_eq!(text(&rtf(format!(" {}?{}?", u(1047), u(1072)))), "За");
        assert_eq!(text(&rtf(format!("\\uc2 {}\\'3f\\'3fend", u(1047)))), "Зend");
        assert_eq!(text(&rtf(format!("\\uc0 {} end", u(1047)))), "Зend");
        // A pair of surrogates makes one character.
        assert_eq!(text(br"{\rtf1\ansi \u-10179?\u-8704?}"), "\u{1F600}");
    }

    #[test]
    fn structure_becomes_lines_and_literals_stay() {
        let rtf = br"{\rtf1\ansi First\par Second\line Third\tab cell \{braces\} back\\slash\~\_}";
        assert_eq!(text(rtf), "First\nSecond\nThird\tcell {braces} back\\slash\u{A0}-");
        // Line breaks in the file are not text.
        assert_eq!(text(b"{\\rtf1\\ansi one\r\ntwo}"), "onetwo");
        // A field shows its result, not its instruction.
        let field = br#"{\rtf1\ansi {\field{\*\fldinst HYPERLINK "http://example.invalid"}{\fldrslt the archive}}}"#;
        assert_eq!(text(field), "the archive");
    }

    #[test]
    fn malformed_files_are_errors_or_text_but_never_a_panic() {
        assert!(to_text(b"plain text").is_err());
        assert!(to_text(b"").is_err());
        for broken in [
            b"{\\rtf1".as_slice(),
            b"{\\rtf1 text\\",
            b"{\\rtf1 text\\'",
            b"{\\rtf1 text\\'4",
            b"{\\rtf1 text\\'zz}",
            b"{\\rtf1}}}}} after",
            b"{\\rtf1 \\u",
            b"{\\rtf1 \\u99999999999999999999 x}",
            b"{\\rtf1 \\uc-1 \\u65 x}",
            b"{\\rtf1 \\f99999999999999999999 x}",
        ] {
            let _ = to_text(broken);
        }
        // A deep nest is refused, not followed.
        let deep = [b"{\\rtf1 ".as_slice(), "{".repeat(100_000).as_bytes()].concat();
        assert!(to_text(&deep).is_err());
        // Text survives a group left open.
        assert_eq!(text(br"{\rtf1\ansi left open"), "left open");
    }
}
