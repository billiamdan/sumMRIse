//! Plain text: what a text file, an RTF and an HTML file become.

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};
use summrise_i18n::tr;

/// Most characters shown of one document. A report is a few thousand; the
/// limit bounds what a hostile file can make the text layout do.
pub(super) const MAX_TEXT_CHARS: usize = 1 << 20;

/// Text to show as it is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlainText {
    pub text: String,
    /// The document is longer than [`MAX_TEXT_CHARS`] and was cut off here.
    pub truncated: bool,
    /// The file does not say what its character set is, and it is not UTF-8:
    /// the character set it is read in, which is a guess.
    pub assumed_encoding: Option<&'static str>,
}

impl PlainText {
    /// Bounds and cleans `text`.
    pub(super) fn new(text: &str) -> Self {
        let (text, truncated) = bounded(text);
        Self { text: clean(text), truncated, assumed_encoding: None }
    }
}

fn bounded(text: &str) -> (&str, bool) {
    match text.char_indices().nth(MAX_TEXT_CHARS) {
        Some((end, _)) => (&text[..end], true),
        None => (text, false),
    }
}

/// `text` as it is written, up to [`MAX_TEXT_CHARS`], with line ends made
/// uniform and without control characters, which have no glyphs. Nothing else
/// changes: this is text a report's author wrote.
pub(super) fn clean(text: &str) -> String {
    let (text, _) = bounded(text);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The encoding that a byte-order mark at the start of `bytes` names, and the
/// bytes after it.
pub(super) fn split_bom(bytes: &[u8]) -> Option<(&'static Encoding, &[u8])> {
    Encoding::for_bom(bytes).map(|(encoding, length)| (encoding, &bytes[length..]))
}

/// Decodes a text file: by its byte-order mark, as UTF-8 when it is valid
/// UTF-8, else as Windows-1252 (which is then a guess, and says so). Bytes
/// that no text has make it no text file.
pub(super) fn decode_file(bytes: &[u8]) -> Result<PlainText, String> {
    if let Some((encoding, rest)) = split_bom(bytes) {
        let (text, _) = encoding.decode_without_bom_handling(rest);
        return Ok(PlainText::new(&text));
    }
    if bytes.contains(&0) {
        return Err(tr!("this file does not hold text").to_owned());
    }
    if let Some(text) = UTF_8.decode_without_bom_handling_and_without_replacement(bytes) {
        return Ok(PlainText::new(&text));
    }
    let (text, _) = WINDOWS_1252.decode_without_bom_handling(bytes);
    Ok(PlainText { assumed_encoding: Some(WINDOWS_1252.name()), ..PlainText::new(&text) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_files_are_decoded_by_their_mark_then_as_utf8_then_as_windows_1252() {
        // UTF-8 with and without its mark.
        assert_eq!(decode_file("Röntgen".as_bytes()).expect("text").text, "Röntgen");
        let marked = [b"\xEF\xBB\xBF".as_slice(), "Röntgen".as_bytes()].concat();
        assert_eq!(decode_file(&marked).expect("text").text, "Röntgen");
        // UTF-16 in both byte orders.
        let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain("Röntgen".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode_file(&le).expect("text").text, "Röntgen");
        let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain("Röntgen".encode_utf16().flat_map(u16::to_be_bytes)).collect();
        assert_eq!(decode_file(&be).expect("text").text, "Röntgen");
        // Neither: Windows-1252, and it says that this is what it assumed.
        let legacy = decode_file(b"R\xF6ntgen 25\x80").expect("text");
        assert_eq!(legacy.text, "Röntgen 25€");
        assert_eq!(legacy.assumed_encoding, Some("windows-1252"));
        assert_eq!(decode_file("Röntgen".as_bytes()).expect("text").assumed_encoding, None);
    }

    #[test]
    fn a_binary_file_named_txt_is_not_text() {
        assert!(decode_file(b"MZ\x90\x00\x03\x00\x00\x00").is_err());
    }

    #[test]
    fn text_is_changed_in_nothing_but_line_ends_and_control_characters() {
        let original = "  Two  spaces\tand a tab, ÄÖÜ, Заключение, 山田.\r\nNext line\rThird\u{7}\u{0}.";
        assert_eq!(
            PlainText::new(original).text,
            "  Two  spaces\tand a tab, ÄÖÜ, Заключение, 山田.\nNext line\nThird."
        );
    }

    #[test]
    fn long_text_is_cut_off_and_says_so() {
        let long = "x".repeat(MAX_TEXT_CHARS + 10);
        let text = PlainText::new(&long);
        assert!(text.truncated);
        assert_eq!(text.text.chars().count(), MAX_TEXT_CHARS);
        assert!(!PlainText::new("short").truncated);
    }
}
