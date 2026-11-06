//! Typed, forgiving accessors for DICOM attributes.

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;

/// Trimmed, non-empty text value (multiple values joined with `\`).
pub(crate) fn text(obj: &InMemDicomObject, tag: Tag) -> Option<String> {
    let value = obj.get(tag)?.to_str().ok()?;
    let trimmed = value.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// All text values, trimmed (empty values are kept to preserve positions).
pub(crate) fn texts(obj: &InMemDicomObject, tag: Tag) -> Vec<String> {
    obj.get(tag)
        .and_then(|e| e.to_multi_str().ok())
        .map(|v| v.iter().map(|s| s.trim_matches(|c: char| c.is_whitespace() || c == '\0').to_owned()).collect())
        .unwrap_or_default()
}

pub(crate) fn float(obj: &InMemDicomObject, tag: Tag) -> Option<f64> {
    obj.get(tag)?.to_float64().ok().filter(|v| v.is_finite())
}

pub(crate) fn floats(obj: &InMemDicomObject, tag: Tag) -> Vec<f64> {
    obj.get(tag).and_then(|e| e.to_multi_float64().ok()).unwrap_or_default()
}

pub(crate) fn int(obj: &InMemDicomObject, tag: Tag) -> Option<i32> {
    obj.get(tag)?.to_int::<i32>().ok()
}

pub(crate) fn uint(obj: &InMemDicomObject, tag: Tag) -> Option<u32> {
    obj.get(tag)?.to_int::<u32>().ok()
}

/// First item of a sequence attribute.
pub(crate) fn first_item(obj: &InMemDicomObject, tag: Tag) -> Option<&InMemDicomObject> {
    obj.get(tag)?.items()?.first()
}

/// Looks up `tag` inside `macro_seq` of an Enhanced (multi-frame) functional
/// group item, e.g. `PlanePositionSequence > ImagePositionPatient`.
pub(crate) fn functional(group: &InMemDicomObject, macro_seq: Tag) -> Option<&InMemDicomObject> {
    first_item(group, macro_seq)
}

pub(crate) fn shared_group(obj: &InMemDicomObject) -> Option<&InMemDicomObject> {
    first_item(obj, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
}

pub(crate) fn per_frame_groups(obj: &InMemDicomObject) -> &[InMemDicomObject] {
    obj.get(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE).and_then(|e| e.items()).unwrap_or(&[])
}

/// Where an attribute that classic objects hold at the top level may be
/// found, in the order to try: the data set itself, then for Enhanced
/// multi-frame objects the functional group macro `macro_seq` of the shared
/// group, the shared group itself (some files store the values flat instead
/// of in their own sequence) and the macro of the first frame.
pub(crate) fn classic_or_functional(obj: &InMemDicomObject, macro_seq: Tag) -> impl Iterator<Item = &InMemDicomObject> {
    let shared = shared_group(obj);
    [
        Some(obj),
        shared.and_then(|group| functional(group, macro_seq)),
        shared,
        per_frame_groups(obj).first().and_then(|group| functional(group, macro_seq)),
    ]
    .into_iter()
    .flatten()
}

/// Fixed-length float array, e.g. Image Position (Patient).
pub(crate) fn float_array<const N: usize>(obj: &InMemDicomObject, tag: Tag) -> Option<[f64; N]> {
    let v = floats(obj, tag);
    (v.len() >= N && v.iter().take(N).all(|x| x.is_finite())).then(|| {
        let mut out = [0.0; N];
        out.copy_from_slice(&v[..N]);
        out
    })
}
