//! Which studies on a medium belong to one patient, and which of their series
//! match (`openspec/changes/series-comparison/design.md`, D6). The headers
//! decide; nothing is registered or compared in the images.

use summrise_dicom::{Patient, Series, Study};

use crate::session::SeriesKey;

/// Whether two studies are of the same patient: the Patient ID matches, or,
/// when one of them has no ID, the name and the birth date do.
pub fn same_patient(a: &Patient, b: &Patient) -> bool {
    let given =
        |value: &Option<String>| value.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned);
    match (given(&a.id), given(&b.id)) {
        (Some(a), Some(b)) => a == b,
        _ => {
            let (name, birth) = (given(&a.name), given(&a.birth_date));
            name.is_some() && birth.is_some() && name == given(&b.name) && birth == given(&b.birth_date)
        }
    }
}

/// The pairs of studies of one patient (indices into `studies`), the earlier
/// study of each pair first; a study without a date counts as the later one.
pub fn comparable_studies(studies: &[Study]) -> Vec<(usize, usize)> {
    let when = |study: &Study| (study.date.is_none(), study.date.clone(), study.time.clone());
    let mut pairs = Vec::new();
    for a in 0..studies.len() {
        for b in a + 1..studies.len() {
            if same_patient(&studies[a].patient, &studies[b].patient) {
                pairs.push(if when(&studies[b]) < when(&studies[a]) { (b, a) } else { (a, b) });
            }
        }
    }
    pairs
}

fn is_localizer(series: &Series) -> bool {
    series.instances.iter().any(|i| i.image_type.iter().any(|t| t == "LOCALIZER"))
}

/// The words of a series description, in lower case.
fn words(description: &str) -> Vec<String> {
    let mut words: Vec<String> = description
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    words.sort_unstable();
    words.dedup();
    words
}

/// How well two descriptions agree: whether they are the same, then the share
/// of their words they have in common (in thousandths).
fn agreement(a: &Series, b: &Series) -> (bool, usize) {
    let (a, b) = (a.description.as_deref().unwrap_or_default(), b.description.as_deref().unwrap_or_default());
    let exact = !a.trim().is_empty() && a.trim().eq_ignore_ascii_case(b.trim());
    let (first, second) = (words(a), words(b));
    let shared = first.iter().filter(|word| second.contains(word)).count();
    let all = first.len() + second.len() - shared;
    (exact, (shared * 1000).checked_div(all).unwrap_or(0))
}

/// The series of two studies to show side by side: the pair of the same
/// modality and anatomical plane whose descriptions agree best, an exact match
/// first, then by the words they share. Of pairs that agree equally, the one
/// with the series the user is looking at, then the one with the most images.
/// `None` when no such pair exists.
pub fn matching_series(
    studies: &[Study],
    earlier: usize,
    later: usize,
    looking_at: Option<SeriesKey>,
) -> Option<(SeriesKey, SeriesKey)> {
    let series_of = |study: usize| {
        let series = studies.get(study).map_or(&[][..], |s| s.series.as_slice());
        series.iter().enumerate().filter(|(_, series)| !is_localizer(series) && series.plane().is_some())
    };
    let mut best = None;
    let mut best_score = None;
    for (i, a) in series_of(earlier) {
        for (j, b) in series_of(later) {
            if a.modality.trim() != b.modality.trim() || a.plane() != b.plane() {
                continue;
            }
            let (left, right) = (SeriesKey { study: earlier, series: i }, SeriesKey { study: later, series: j });
            let (exact, share) = agreement(a, b);
            let looked_at = looking_at == Some(left) || looking_at == Some(right);
            let score = (exact, share, looked_at, a.frame_count().min(b.frame_count()));
            if best_score.as_ref().is_none_or(|known| score > *known) {
                best_score = Some(score);
                best = Some((left, right));
            }
        }
    }
    best
}
