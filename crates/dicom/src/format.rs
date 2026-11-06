//! Human-readable formatting of DICOM values (PS3.5 value representations
//! and common coded values).

use dicom_core::dictionary::UidDictionary;
use dicom_dictionary_std::StandardSopClassDictionary;
use dicom_encoding::TransferSyntaxIndex;
use dicom_transfer_syntax_registry::TransferSyntaxRegistry;
use summrise_i18n::{tr, tr_ctx, tr_n};

/// Person Name (PN): `Family^Given^Middle^Prefix^Suffix` → `Prefix Given Middle Family, Suffix`.
/// Only the alphabetic component group (before any `=`) is used.
pub fn person_name(pn: &str) -> String {
    let alphabetic = pn.split('=').next().unwrap_or_default();
    let parts: Vec<&str> = alphabetic.split('^').map(str::trim).collect();
    let get = |i: usize| parts.get(i).copied().filter(|s| !s.is_empty());
    let mut name: Vec<&str> = Vec::new();
    name.extend(get(3));
    name.extend(get(1));
    name.extend(get(2));
    name.extend(get(0));
    let mut out = name.join(" ");
    if let Some(suffix) = get(4) {
        out.push_str(", ");
        out.push_str(suffix);
    }
    if out.is_empty() { pn.trim().to_owned() } else { out }
}

/// Date (DA): `YYYYMMDD` (or legacy `YYYY.MM.DD`) → ISO `YYYY-MM-DD`.
pub fn date(da: &str) -> Option<String> {
    let digits: String = da.chars().filter(char::is_ascii_digit).collect();
    (digits.len() == 8).then(|| format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8]))
}

/// Time (TM): `HHMMSS.FFFFFF` (or legacy `HH:MM:SS`) → `HH:MM:SS`.
pub fn time(tm: &str) -> Option<String> {
    let main = tm.split('.').next().unwrap_or_default();
    let digits: String = main.chars().filter(char::is_ascii_digit).collect();
    match digits.len() {
        6 => Some(format!("{}:{}:{}", &digits[0..2], &digits[2..4], &digits[4..6])),
        4 => Some(format!("{}:{}", &digits[0..2], &digits[2..4])),
        2 => Some(format!("{}:00", &digits[0..2])),
        _ => None,
    }
}

/// Age String (AS): `056Y` → `56 years`.
pub fn age(value: &str) -> Option<String> {
    let value = value.trim();
    let (num, unit) = value.split_at(value.len().checked_sub(1)?);
    let n: u32 = num.parse().ok()?;
    // Translators: the age of the patient when the images were made.
    Some(match unit {
        "Y" => tr_n!("{n} year", "{n} years", n),
        "M" => tr_n!("{n} month", "{n} months", n),
        "W" => tr_n!("{n} week", "{n} weeks", n),
        "D" => tr_n!("{n} day", "{n} days", n),
        _ => return None,
    })
}

pub fn sex(code: &str) -> String {
    match code.trim() {
        // Translators: the sex of the patient as recorded in the images.
        "M" => tr_ctx!("sex", "Male").into(),
        "F" => tr_ctx!("sex", "Female").into(),
        "O" => tr_ctx!("sex", "Other").into(),
        other => other.into(),
    }
}

/// Modality (0008,0060) code → name.
pub fn modality_name(code: &str) -> Option<&'static str> {
    Some(match code.trim() {
        // Translators: the names of the DICOM modalities (kinds of examination), shown next to their code,
        // as in "Computed Tomography (CT)".
        "CT" => tr!("Computed Tomography"),
        "MR" => tr!("Magnetic Resonance"),
        "PT" => tr!("Positron Emission Tomography"),
        "NM" => tr!("Nuclear Medicine"),
        "US" => tr!("Ultrasound"),
        "CR" => tr!("Computed Radiography"),
        "DX" => tr!("Digital Radiography"),
        "MG" => tr!("Mammography"),
        "XA" => tr!("X-Ray Angiography"),
        "RF" => tr!("Radio Fluoroscopy"),
        "OT" => tr_ctx!("modality", "Other"),
        "SR" => tr!("Structured Report"),
        "SC" => tr!("Secondary Capture"),
        "PR" => tr!("Presentation State"),
        "KO" => tr!("Key Object Selection"),
        "DOC" => tr!("Document"),
        "SEG" => tr!("Segmentation"),
        "RTSTRUCT" => tr!("Radiotherapy Structure Set"),
        "RTDOSE" => tr!("Radiotherapy Dose"),
        "RTPLAN" => tr!("Radiotherapy Plan"),
        "OPT" => tr!("Ophthalmic Tomography"),
        "XC" => tr!("External-camera Photography"),
        _ => return None,
    })
}

/// Patient Position (0018,5100) code → description.
pub fn patient_position(code: &str) -> Option<&'static str> {
    Some(match code.trim() {
        // Translators: how the patient lay in the scanner: entering head or feet first, on the back (supine),
        // on the front (prone) or on the right or left side (decubitus).
        "HFS" => tr!("Head first, supine"),
        "HFP" => tr!("Head first, prone"),
        "HFDR" => tr!("Head first, decubitus right"),
        "HFDL" => tr!("Head first, decubitus left"),
        "FFS" => tr!("Feet first, supine"),
        "FFP" => tr!("Feet first, prone"),
        "FFDR" => tr!("Feet first, decubitus right"),
        "FFDL" => tr!("Feet first, decubitus left"),
        _ => return None,
    })
}

/// Scanning Sequence (0018,0020) codes → description.
pub fn scanning_sequence(values: &[String]) -> String {
    values
        .iter()
        .map(|code| match code.as_str() {
            // Translators: the kinds of MR sequence (DICOM Scanning Sequence); established English terms.
            "SE" => tr!("Spin echo"),
            "IR" => tr!("Inversion recovery"),
            "GR" => tr!("Gradient recalled"),
            "EP" => tr!("Echo planar"),
            "RM" => tr!("Research mode"),
            other => other,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Transfer syntax UID → name (falls back to the UID).
pub fn transfer_syntax_name(uid: &str) -> String {
    let uid = uid.trim_end_matches('\0').trim();
    TransferSyntaxRegistry.get(uid).map_or_else(|| uid.to_owned(), |ts| ts.name().to_owned())
}

/// Whether pixel data in this transfer syntax can be decoded by this build.
pub fn transfer_syntax_supported(uid: &str) -> bool {
    let uid = uid.trim_end_matches('\0').trim();
    TransferSyntaxRegistry.get(uid).is_some_and(|ts| ts.can_decode_all())
}

/// The family of a transfer syntax whose compression always discards detail
/// ("JPEG", "JPEG-LS", "MPEG-2", "H.264", "HEVC", "JPEG XL"); `None` for
/// transfer syntaxes that are lossless or can be either (JPEG 2000, HTJ2K,
/// JPEG XL): for those only Lossy Image Compression tells.
pub fn lossy_family(uid: &str) -> Option<&'static str> {
    let uid = uid.trim_end_matches('\0').trim();
    let process = uid.strip_prefix("1.2.840.10008.1.2.4.")?;
    Some(match process {
        // Baseline, Extended and the retired lossy JPEG processes (not 57, 58, 65, 66, 70: lossless).
        "50" | "51" | "52" | "53" | "54" | "55" | "56" | "59" | "60" | "61" | "62" | "63" | "64" => "JPEG",
        "81" => "JPEG-LS",
        "100" | "100.1" | "101" | "101.1" => "MPEG-2",
        "102" | "102.1" | "103" | "103.1" | "104" | "104.1" | "105" | "105.1" | "106" | "106.1" => "H.264",
        "107" | "108" => "HEVC",
        // A JPEG image recompressed without further loss: the loss of the JPEG remains.
        "111" => "JPEG XL",
        _ => return None,
    })
}

/// Whether pixel data in this transfer syntax has always lost detail.
pub fn transfer_syntax_is_lossy(uid: &str) -> bool {
    lossy_family(uid).is_some()
}

/// SOP Class UID → name (falls back to the UID).
pub fn sop_class_name(uid: &str) -> String {
    let uid = uid.trim_end_matches('\0').trim();
    StandardSopClassDictionary.by_uid(uid).map_or_else(|| uid.to_owned(), |e| e.name.to_owned())
}

/// Splits an attribute keyword into words: `RepetitionTime` → `Repetition Time`,
/// `KVP` stays `KVP`, `XRayTubeCurrent` → `X Ray Tube Current`.
pub fn keyword_to_words(keyword: &str) -> String {
    let chars: Vec<char> = keyword.chars().collect();
    let mut out = String::with_capacity(keyword.len() + 8);
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                out.push(' ');
            }
        }
        out.push(c);
    }
    out
}

/// Compact number formatting without needless trailing zeros.
pub fn number(v: f64, max_decimals: usize) -> String {
    let s = format!("{v:.max_decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_person_names() {
        assert_eq!(person_name("DOE^JANE"), "JANE DOE");
        assert_eq!(person_name("Smith^John^Q^Dr^Jr"), "Dr John Q Smith, Jr");
        assert_eq!(person_name("Yamada^Tarou=山田^太郎"), "Tarou Yamada");
        assert_eq!(person_name("ANONYMOUS"), "ANONYMOUS");
    }

    #[test]
    fn formats_dates_times_ages() {
        assert_eq!(date("20260115").as_deref(), Some("2026-01-15"));
        assert_eq!(date("2026.01.15").as_deref(), Some("2026-01-15"));
        assert_eq!(date("2026"), None);
        assert_eq!(time("093000.123").as_deref(), Some("09:30:00"));
        assert_eq!(time("0930").as_deref(), Some("09:30"));
        assert_eq!(age("056Y").as_deref(), Some("56 years"));
        assert_eq!(age("001M").as_deref(), Some("1 month"));
        assert_eq!(age("X"), None);
    }

    #[test]
    fn expands_codes() {
        assert_eq!(modality_name("MR"), Some("Magnetic Resonance"));
        assert_eq!(patient_position("HFS"), Some("Head first, supine"));
        assert_eq!(scanning_sequence(&["SE".into(), "IR".into()]), "Spin echo, Inversion recovery");
        assert_eq!(transfer_syntax_name("1.2.840.10008.1.2.1"), "Explicit VR Little Endian");
        assert!(transfer_syntax_supported("1.2.840.10008.1.2.4.50"));
        assert_eq!(sop_class_name("1.2.840.10008.5.1.4.1.1.2"), "CT Image Storage");
    }

    #[test]
    fn knows_which_transfer_syntaxes_are_always_lossy() {
        assert_eq!(lossy_family("1.2.840.10008.1.2.4.50\0"), Some("JPEG"));
        assert_eq!(lossy_family("1.2.840.10008.1.2.4.81"), Some("JPEG-LS"));
        assert_eq!(lossy_family("1.2.840.10008.1.2.4.102.1"), Some("H.264"));
        for lossless_or_either in [
            "1.2.840.10008.1.2.1",
            "1.2.840.10008.1.2.4.57",
            "1.2.840.10008.1.2.4.70",
            "1.2.840.10008.1.2.4.80",
            "1.2.840.10008.1.2.4.90",
            "1.2.840.10008.1.2.4.91",
            "1.2.840.10008.1.2.4.203",
            "1.2.840.10008.1.2.4.112",
            "1.2.840.10008.1.2.5",
        ] {
            assert!(!transfer_syntax_is_lossy(lossless_or_either), "{lossless_or_either}");
        }
    }

    #[test]
    fn splits_keywords() {
        assert_eq!(keyword_to_words("RepetitionTime"), "Repetition Time");
        assert_eq!(keyword_to_words("KVP"), "KVP");
        assert_eq!(keyword_to_words("SOPInstanceUID"), "SOP Instance UID");
        assert_eq!(keyword_to_words("XRayTubeCurrent"), "X Ray Tube Current");
        assert_eq!(number(2.50, 3), "2.5");
        assert_eq!(number(120.0, 2), "120");
    }
}
