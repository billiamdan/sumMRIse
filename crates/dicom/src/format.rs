//! Human-readable formatting of DICOM values (PS3.5 value representations
//! and common coded values).

use dicom_core::dictionary::UidDictionary;
use dicom_dictionary_std::StandardSopClassDictionary;
use dicom_encoding::TransferSyntaxIndex;
use dicom_transfer_syntax_registry::TransferSyntaxRegistry;

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
    let unit = match (unit, n) {
        ("Y", 1) => "year",
        ("Y", _) => "years",
        ("M", 1) => "month",
        ("M", _) => "months",
        ("W", 1) => "week",
        ("W", _) => "weeks",
        ("D", 1) => "day",
        ("D", _) => "days",
        _ => return None,
    };
    Some(format!("{n} {unit}"))
}

pub fn sex(code: &str) -> String {
    match code.trim() {
        "M" => "Male".into(),
        "F" => "Female".into(),
        "O" => "Other".into(),
        other => other.into(),
    }
}

/// Modality (0008,0060) code → name.
pub fn modality_name(code: &str) -> Option<&'static str> {
    Some(match code.trim() {
        "CT" => "Computed Tomography",
        "MR" => "Magnetic Resonance",
        "PT" => "Positron Emission Tomography",
        "NM" => "Nuclear Medicine",
        "US" => "Ultrasound",
        "CR" => "Computed Radiography",
        "DX" => "Digital Radiography",
        "MG" => "Mammography",
        "XA" => "X-Ray Angiography",
        "RF" => "Radio Fluoroscopy",
        "OT" => "Other",
        "SR" => "Structured Report",
        "SC" => "Secondary Capture",
        "PR" => "Presentation State",
        "KO" => "Key Object Selection",
        "DOC" => "Document",
        "SEG" => "Segmentation",
        "RTSTRUCT" => "Radiotherapy Structure Set",
        "RTDOSE" => "Radiotherapy Dose",
        "RTPLAN" => "Radiotherapy Plan",
        "OPT" => "Ophthalmic Tomography",
        "XC" => "External-camera Photography",
        _ => return None,
    })
}

/// Patient Position (0018,5100) code → description.
pub fn patient_position(code: &str) -> Option<&'static str> {
    Some(match code.trim() {
        "HFS" => "Head first, supine",
        "HFP" => "Head first, prone",
        "HFDR" => "Head first, decubitus right",
        "HFDL" => "Head first, decubitus left",
        "FFS" => "Feet first, supine",
        "FFP" => "Feet first, prone",
        "FFDR" => "Feet first, decubitus right",
        "FFDL" => "Feet first, decubitus left",
        _ => return None,
    })
}

/// Scanning Sequence (0018,0020) codes → description.
pub fn scanning_sequence(values: &[String]) -> String {
    values
        .iter()
        .map(|code| match code.as_str() {
            "SE" => "Spin echo",
            "IR" => "Inversion recovery",
            "GR" => "Gradient recalled",
            "EP" => "Echo planar",
            "RM" => "Research mode",
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
    fn splits_keywords() {
        assert_eq!(keyword_to_words("RepetitionTime"), "Repetition Time");
        assert_eq!(keyword_to_words("KVP"), "KVP");
        assert_eq!(keyword_to_words("SOPInstanceUID"), "SOP Instance UID");
        assert_eq!(keyword_to_words("XRayTubeCurrent"), "X Ray Tube Current");
        assert_eq!(number(2.50, 3), "2.5");
        assert_eq!(number(120.0, 2), "120");
    }
}
