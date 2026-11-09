//! A limit on how deeply the sequences of a DICOM file nest.
//!
//! The object parser builds nested sequences by recursion, one set of stack
//! frames per level, and does not limit the depth. About 1500 levels, which fit
//! in 30 kB of a file, overflow the stack of a worker thread, and a stack
//! overflow ends the whole application: it is not a panic that can be caught.
//! So every file goes through the parser's own token stream first, which is not
//! recursive and yields the same structure the object parser would build on.
//! Real files nest a few levels, deep structured reports a few dozen.

use std::io::{BufReader, Read};

use dicom_core::Tag;
use dicom_encoding::Codec;
use dicom_encoding::TransferSyntaxIndex;
use dicom_object::FileMetaTable;
use dicom_parser::dataset::DataToken;
use dicom_parser::dataset::read::DataSetReader;
use dicom_transfer_syntax_registry::TransferSyntaxRegistry;
use summrise_i18n::tr;

/// Deepest nesting of sequences accepted: far beyond real files and far below
/// what overflows the stack of any thread that parses.
pub(crate) const MAX_SEQUENCE_DEPTH: usize = 128;

/// Reads the tokens of the data set up to (not including) the attribute
/// `read_until` and says whether its sequences nest too deeply.
fn check_tokens<E>(tokens: impl Iterator<Item = Result<DataToken, E>>, read_until: Option<Tag>) -> Result<(), String> {
    let past_the_end = |tag: Tag| read_until.is_some_and(|until| tag >= until);
    let mut depth = 0usize;
    for token in tokens {
        // What cannot be read is for the object parser to report.
        let Ok(token) = token else { return Ok(()) };
        match token {
            DataToken::ElementHeader(header) if depth == 0 && past_the_end(header.tag) => return Ok(()),
            DataToken::SequenceStart { tag, .. } if depth == 0 && past_the_end(tag) => return Ok(()),
            DataToken::SequenceStart { .. } | DataToken::PixelSequenceStart => {
                depth += 1;
                if depth > MAX_SEQUENCE_DEPTH {
                    return Err(tr!("its data is nested too deeply").to_owned());
                }
            }
            DataToken::SequenceEnd => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// Checks the DICOM file whose content starts with the `DICM` code in
/// `source`. A file that is no DICOM file passes: the parser says what is wrong
/// with it.
pub(crate) fn check(source: impl Read, read_until: Option<Tag>) -> Result<(), String> {
    let mut source = BufReader::new(source);
    let Ok(meta) = FileMetaTable::from_reader(&mut source) else { return Ok(()) };
    let Some(syntax) = TransferSyntaxRegistry.get(meta.transfer_syntax()) else { return Ok(()) };
    match syntax.codec() {
        Codec::Dataset(Some(adapter)) => {
            let source = adapter.adapt_reader(Box::new(source));
            match DataSetReader::new_with_ts(source, syntax) {
                Ok(tokens) => check_tokens(tokens, read_until),
                Err(_) => Ok(()),
            }
        }
        Codec::Dataset(None) => Ok(()),
        Codec::None | Codec::EncapsulatedPixelData(..) => match DataSetReader::new_with_ts(source, syntax) {
            Ok(tokens) => check_tokens(tokens, read_until),
            Err(_) => Ok(()),
        },
    }
}

#[cfg(test)]
mod tests {
    use dicom_dictionary_std::tags;
    use summrise_testkit::reports::deeply_nested_report;

    use super::*;

    fn after_preamble(bytes: &[u8]) -> &[u8] {
        &bytes[128..]
    }

    #[test]
    fn nesting_within_the_limit_passes_and_beyond_it_does_not() {
        // The report's own content sequence and the sequence of the text item are two levels more.
        let fits = deeply_nested_report(MAX_SEQUENCE_DEPTH - 1, "x").expect("bytes");
        assert_eq!(check(after_preamble(&fits), None), Ok(()));
        let too_deep = deeply_nested_report(MAX_SEQUENCE_DEPTH, "x").expect("bytes");
        assert_eq!(check(after_preamble(&too_deep), None), Err("its data is nested too deeply".to_owned()));
    }

    /// Far more levels than overflow the stack of this thread when the object is built.
    #[test]
    fn the_check_itself_does_not_recurse() {
        let bytes = deeply_nested_report(200_000, "x").expect("bytes");
        assert!(check(after_preamble(&bytes), None).is_err());
    }

    #[test]
    fn the_check_stops_where_the_parser_stops() {
        let bytes = deeply_nested_report(MAX_SEQUENCE_DEPTH + 5, "x").expect("bytes");
        // The nesting lies beyond the attribute the parser would stop at.
        assert_eq!(check(after_preamble(&bytes), Some(tags::SPECIFIC_CHARACTER_SET)), Ok(()));
        assert!(check(after_preamble(&bytes), Some(tags::PIXEL_DATA)).is_err());
    }

    #[test]
    fn what_is_no_dicom_file_is_left_to_the_parser() {
        assert_eq!(check(b"".as_slice(), None), Ok(()));
        assert_eq!(check(b"DICM".as_slice(), None), Ok(()));
        assert_eq!(check(b"not a DICOM file at all".as_slice(), None), Ok(()));
        // A file cut off inside the data set.
        let bytes = deeply_nested_report(8, "x").expect("bytes");
        assert_eq!(check(&after_preamble(&bytes)[..bytes.len() - 200], None), Ok(()));
    }
}
