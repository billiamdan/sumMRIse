//! Series that hold several sets of images at the same slice positions — time
//! points, heart phases, echoes, diffusion weightings, image types … — and how
//! such a series is split into stacks of equal length from its headers, before
//! any pixel data is read.
//!
//! The rule (documented with its reasons in
//! `openspec/changes/multi-phase-cine/design.md`, D2): with P ≥ 2 distinct slice
//! positions that each hold exactly k ≥ 2 frames, the first discriminator of
//! [`StackKind::ORDER`] whose values partition the frames into k groups that
//! each hold every position exactly once decides; failing all, the order in
//! which the frames occur at each position does ([`StackKind::Part`]). The
//! stacks share one geometry; image n of every stack lies at the same position.

use dicom_core::Tag;
use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use summrise_i18n::{tr, tr_n};
use summrise_imaging::{ImagePlane, VolumeError, VolumeGeometry};

use crate::attrs;
use crate::format::number;

/// Frames closer than this along the slice normal share a position, and the
/// frames of one position in different stacks must lie this close together
/// (whole Image Position (Patient)), in mm.
const SAME_POSITION_MM: f64 = 0.01;

/// What the headers record about one frame that can tell it apart from other
/// frames at the same slice position. Every value is optional.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameValues {
    /// Temporal Position Identifier (0020,0100), or the Temporal Position
    /// Index (0020,9128) of an Enhanced frame.
    pub temporal_position: Option<i64>,
    /// Trigger Time (0018,1060), or the Nominal Cardiac Trigger Delay Time
    /// (0020,9153) of an Enhanced frame, in ms.
    pub trigger_time_ms: Option<f64>,
    /// Echo Number(s) (0018,0086).
    pub echo_number: Option<i64>,
    /// Echo Time (0018,0081), or the Effective Echo Time (0018,9082) of an
    /// Enhanced frame, in ms.
    pub echo_time_ms: Option<f64>,
    /// Diffusion b-value (0018,9087), in s/mm².
    pub b_value: Option<f64>,
    /// Image Type (0008,0008), or the Frame Type (0008,9007) of an Enhanced
    /// frame; empty values keep their place.
    pub image_type: Vec<String>,
    /// Acquisition Number (0020,0012), or the Frame Acquisition Number
    /// (0020,9156) of an Enhanced frame.
    pub acquisition_number: Option<i64>,
    /// Sequence Name (0018,0024), or Pulse Sequence Name (0018,9005).
    pub sequence_name: Option<String>,
}

/// Nothing recorded.
static NO_VALUES: FrameValues = FrameValues {
    temporal_position: None,
    trigger_time_ms: None,
    echo_number: None,
    echo_time_ms: None,
    b_value: None,
    image_type: Vec::new(),
    acquisition_number: None,
    sequence_name: None,
};

fn integer(obj: &InMemDicomObject, tag: Tag) -> Option<i64> {
    obj.get(tag)?.to_int::<i64>().ok()
}

impl FrameValues {
    /// The attributes of a classic object, which hold for all its frames.
    fn classic(obj: &InMemDicomObject) -> Self {
        Self {
            temporal_position: integer(obj, tags::TEMPORAL_POSITION_IDENTIFIER),
            trigger_time_ms: attrs::float(obj, tags::TRIGGER_TIME),
            echo_number: integer(obj, tags::ECHO_NUMBERS),
            echo_time_ms: attrs::float(obj, tags::ECHO_TIME),
            b_value: attrs::float(obj, tags::DIFFUSION_B_VALUE),
            image_type: attrs::texts(obj, tags::IMAGE_TYPE),
            acquisition_number: integer(obj, tags::ACQUISITION_NUMBER),
            sequence_name: attrs::text(obj, tags::SEQUENCE_NAME)
                .or_else(|| attrs::text(obj, tags::PULSE_SEQUENCE_NAME)),
        }
    }

    /// The functional group macros of an Enhanced frame (`frame`), else of the
    /// shared group, else values stored flat in the shared group; what they do
    /// not record is taken from `classic`.
    fn functional(frame: Option<&InMemDicomObject>, shared: Option<&InMemDicomObject>, classic: &Self) -> Self {
        fn lookup<T>(
            frame: Option<&InMemDicomObject>,
            shared: Option<&InMemDicomObject>,
            macro_seq: Tag,
            read: impl Fn(&InMemDicomObject) -> Option<T>,
        ) -> Option<T> {
            frame
                .and_then(|group| attrs::functional(group, macro_seq))
                .and_then(&read)
                .or_else(|| shared.and_then(|group| attrs::functional(group, macro_seq)).and_then(&read))
                .or_else(|| shared.and_then(&read))
        }
        let int = |macro_seq: Tag, tag: Tag| lookup(frame, shared, macro_seq, |o| integer(o, tag));
        let float = |macro_seq: Tag, tag: Tag| lookup(frame, shared, macro_seq, |o| attrs::float(o, tag));
        let frame_type = [tags::MR_IMAGE_FRAME_TYPE_SEQUENCE, tags::CT_IMAGE_FRAME_TYPE_SEQUENCE].into_iter().find_map(
            |macro_seq| {
                lookup(frame, shared, macro_seq, |o| {
                    Some(attrs::texts(o, tags::FRAME_TYPE)).filter(|values| values.iter().any(|v| !v.is_empty()))
                })
            },
        );
        Self {
            temporal_position: int(tags::FRAME_CONTENT_SEQUENCE, tags::TEMPORAL_POSITION_INDEX)
                .or(classic.temporal_position),
            trigger_time_ms: float(tags::CARDIAC_SYNCHRONIZATION_SEQUENCE, tags::NOMINAL_CARDIAC_TRIGGER_DELAY_TIME)
                .or(classic.trigger_time_ms),
            echo_number: classic.echo_number,
            echo_time_ms: float(tags::MR_ECHO_SEQUENCE, tags::EFFECTIVE_ECHO_TIME).or(classic.echo_time_ms),
            b_value: float(tags::MR_DIFFUSION_SEQUENCE, tags::DIFFUSION_B_VALUE).or(classic.b_value),
            image_type: frame_type.unwrap_or_else(|| classic.image_type.clone()),
            acquisition_number: int(tags::FRAME_CONTENT_SEQUENCE, tags::FRAME_ACQUISITION_NUMBER)
                .or(classic.acquisition_number),
            sequence_name: classic.sequence_name.clone(),
        }
    }
}

/// The values of the frames of one file: one entry per frame for an Enhanced
/// object with per-frame functional groups, else one entry for all its frames.
pub(crate) fn read_frame_values(obj: &InMemDicomObject, frames: u32) -> Vec<FrameValues> {
    let classic = FrameValues::classic(obj);
    let shared = attrs::shared_group(obj);
    let per_frame = attrs::per_frame_groups(obj);
    if per_frame.len() == frames as usize && frames > 1 {
        per_frame.iter().map(|frame| FrameValues::functional(Some(frame), shared, &classic)).collect()
    } else if shared.is_some() || !per_frame.is_empty() {
        let frame = (per_frame.len() == frames as usize).then(|| per_frame.first()).flatten();
        vec![FrameValues::functional(frame, shared, &classic)]
    } else {
        vec![classic]
    }
}

/// The values of frame `frame` of a file whose values are `values` (see
/// [`read_frame_values`]; empty: nothing recorded).
pub(crate) fn of_frame(values: &[FrameValues], frame: u32) -> &FrameValues {
    match values {
        [all] => all,
        each => each.get(frame as usize).unwrap_or(&NO_VALUES),
    }
}

/// What tells the stacks of a series apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackKind {
    /// Time points of a dynamic scan (temporal position).
    TimePoint,
    /// Phases of the heart cycle (trigger time after the R wave).
    HeartPhase,
    /// Echoes (echo number and echo time).
    Echo,
    /// Diffusion weightings (b-value).
    Diffusion,
    /// Kinds of image, e.g. magnitude and phase, or a derived map (image type).
    ImageType,
    /// Repeated acquisitions (acquisition number).
    Acquisition,
    /// Images made with different sequences (sequence name).
    Sequence,
    /// Nothing recorded tells the sets apart: the order in which the images
    /// occur at each position.
    Part,
}

impl StackKind {
    /// The discriminators in the order they are tried ([`Self::Part`] is the
    /// last resort and always applies).
    pub const ORDER: [Self; 7] = [
        Self::TimePoint,
        Self::HeartPhase,
        Self::Echo,
        Self::Diffusion,
        Self::ImageType,
        Self::Acquisition,
        Self::Sequence,
    ];

    /// Time points and heart phases: one acquisition seen at different times.
    /// They share one contrast window and can be played as a loop.
    pub fn is_time_like(self) -> bool {
        matches!(self, Self::TimePoint | Self::HeartPhase)
    }

    /// Stacks ordered by the value that tells them apart; the others keep the
    /// order in which they first occur.
    fn ordered_by_value(self) -> bool {
        !matches!(self, Self::ImageType | Self::Sequence | Self::Part)
    }
}

/// What distinguishes one stack, as recorded in its images.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StackValue {
    /// Trigger time (heart phases) or echo time in ms, b-value in s/mm².
    pub value: Option<f64>,
    /// The values of Image Type that differ between the stacks, or the
    /// sequence name.
    pub text: Option<String>,
}

/// How a series is split into stacks. Display index `s × len + i` is image `i`
/// of stack `s`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stacks {
    pub kind: StackKind,
    /// Images in every stack: one per slice position.
    pub len: usize,
    /// One entry per stack, in display order.
    pub values: Vec<StackValue>,
}

impl Stacks {
    /// Number of stacks (at least 2).
    pub fn count(&self) -> usize {
        self.values.len()
    }

    /// What stack `stack` (from 0) is, e.g. "Time point 3 / 20",
    /// "Echo 2 / 2 · TE 90 ms" or "b = 1000 s/mm²".
    pub fn label(&self, stack: usize) -> String {
        let (index, count) = (stack + 1, self.count());
        let value = self.values.get(stack).and_then(|v| v.value);
        let text = self.values.get(stack).and_then(|v| v.text.as_deref());
        match (self.kind, value, text) {
            (StackKind::TimePoint, ..) => tr!("Time point {index} / {count}", index = index, count = count),
            (StackKind::HeartPhase, Some(ms), _) => {
                tr!("Heart phase {index} / {count} · {time} ms", index = index, count = count, time = number(ms, 1))
            }
            (StackKind::HeartPhase, None, _) => tr!("Heart phase {index} / {count}", index = index, count = count),
            (StackKind::Echo, Some(ms), _) => {
                tr!("Echo {index} / {count} · TE {time} ms", index = index, count = count, time = number(ms, 2))
            }
            (StackKind::Echo, None, _) => tr!("Echo {index} / {count}", index = index, count = count),
            (StackKind::Diffusion, Some(b), _) => tr!("b = {b} s/mm²", b = number(b, 1)),
            (StackKind::Diffusion, None, _) => {
                tr!("Diffusion weighting {index} / {count}", index = index, count = count)
            }
            (StackKind::ImageType, _, Some(value)) => {
                tr!("Image type {index} / {count} · {value}", index = index, count = count, value = value)
            }
            (StackKind::ImageType, _, None) => tr!("Image type {index} / {count}", index = index, count = count),
            (StackKind::Acquisition, ..) => tr!("Acquisition {index} / {count}", index = index, count = count),
            (StackKind::Sequence, _, Some(name)) => {
                tr!("Sequence {index} / {count} · {name}", index = index, count = count, name = name)
            }
            (StackKind::Sequence, _, None) => tr!("Sequence {index} / {count}", index = index, count = count),
            (StackKind::Part, ..) => tr!("Part {index} / {count}", index = index, count = count),
        }
    }

    /// How many stacks of how many images, e.g. "3 time points × 12 images".
    pub fn summary(&self) -> String {
        let n = self.count();
        let stacks = match self.kind {
            StackKind::TimePoint => tr_n!("{n} time point", "{n} time points", n),
            StackKind::HeartPhase => tr_n!("{n} heart phase", "{n} heart phases", n),
            StackKind::Echo => tr_n!("{n} echo", "{n} echoes", n),
            StackKind::Diffusion => tr_n!("{n} b-value", "{n} b-values", n),
            StackKind::ImageType => tr_n!("{n} image type", "{n} image types", n),
            StackKind::Acquisition => tr_n!("{n} acquisition", "{n} acquisitions", n),
            StackKind::Sequence => tr_n!("{n} sequence", "{n} sequences", n),
            StackKind::Part => tr_n!("{n} part", "{n} parts", n),
        };
        let images = tr_n!("{n} image", "{n} images", self.len);
        tr!("{stacks} × {images}", stacks = stacks, images = images)
    }
}

/// A series split into stacks: the display order (indices into the frames
/// given), the verdict about the volume every stack forms, and the stacks.
pub(crate) struct Split {
    pub order: Vec<usize>,
    pub verdict: Result<VolumeGeometry, VolumeError>,
    pub stacks: Stacks,
}

/// The position of every frame (0 is the lowest along the normal of the first
/// plane) and the number of positions.
fn positions(planes: &[ImagePlane]) -> (Vec<usize>, usize) {
    let Some(first) = planes.first() else { return (Vec::new(), 0) };
    let normal = first.normal();
    let distance: Vec<f64> = planes.iter().map(|p| p.distance_along(normal)).collect();
    let mut order: Vec<usize> = (0..planes.len()).collect();
    order.sort_by(|&a, &b| distance[a].total_cmp(&distance[b]));
    let mut position_of = vec![0; planes.len()];
    let (mut count, mut start) = (0, f64::NAN);
    for i in order {
        if count == 0 || distance[i] - start > SAME_POSITION_MM {
            count += 1;
            start = distance[i];
        }
        position_of[i] = count - 1;
    }
    (position_of, count)
}

/// Whether all frames lie at one position (a cine of one slice).
pub(crate) fn at_one_position(planes: &[ImagePlane]) -> bool {
    positions(planes).1 == 1
}

/// The frames of a series whose images all lie at one position, in time
/// order: by temporal position, else by trigger time, when every frame
/// records it; else `natural` as it is.
pub(crate) fn in_time_order(natural: &[usize], values: &[&FrameValues]) -> Vec<usize> {
    let mut order = natural.to_vec();
    if natural.iter().all(|&i| values[i].temporal_position.is_some()) {
        order.sort_by_key(|&i| values[i].temporal_position);
    } else if natural.iter().all(|&i| values[i].trigger_time_ms.is_some()) {
        let time = |i: usize| values[i].trigger_time_ms.unwrap_or_default();
        order.sort_by(|&a, &b| time(a).total_cmp(&time(b)));
    }
    order
}

/// A value of a discriminator, comparable and ordered. Numbers are kept to a
/// thousandth of their unit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Number(i64),
    Echo(Option<i64>, Option<i64>),
    Text(String),
}

fn fixed(value: f64) -> i64 {
    (value * 1000.0).round() as i64
}

fn key(kind: StackKind, v: &FrameValues) -> Option<Key> {
    match kind {
        StackKind::TimePoint => v.temporal_position.map(Key::Number),
        StackKind::HeartPhase => v.trigger_time_ms.map(|t| Key::Number(fixed(t))),
        StackKind::Echo => (v.echo_number.is_some() || v.echo_time_ms.is_some())
            .then(|| Key::Echo(v.echo_number, v.echo_time_ms.map(fixed))),
        StackKind::Diffusion => v.b_value.map(|b| Key::Number(fixed(b))),
        StackKind::ImageType => v.image_type.iter().any(|t| !t.is_empty()).then(|| Key::Text(v.image_type.join("\\"))),
        StackKind::Acquisition => v.acquisition_number.map(Key::Number),
        StackKind::Sequence => v.sequence_name.clone().map(Key::Text),
        StackKind::Part => None,
    }
}

/// The frames of each stack if `kind` splits them into `count` groups that
/// each hold every one of the `positions` exactly once; stacks in display order.
fn partition(
    kind: StackKind,
    natural: &[usize],
    values: &[&FrameValues],
    position_of: &[usize],
    positions: usize,
    count: usize,
) -> Option<Vec<Vec<usize>>> {
    let mut keys: Vec<Key> = Vec::with_capacity(count);
    let mut groups: Vec<Vec<usize>> = Vec::with_capacity(count);
    for &frame in natural {
        let key = key(kind, values[frame])?;
        let group = match keys.iter().position(|k| *k == key) {
            Some(group) => group,
            None if keys.len() < count => {
                keys.push(key);
                groups.push(Vec::with_capacity(positions));
                keys.len() - 1
            }
            None => return None,
        };
        groups[group].push(frame);
    }
    if keys.len() != count {
        return None;
    }
    let mut seen = vec![false; count * positions];
    for (group, frames) in groups.iter().enumerate() {
        for &frame in frames {
            if std::mem::replace(&mut seen[group * positions + position_of[frame]], true) {
                return None;
            }
        }
    }
    let mut order: Vec<usize> = (0..count).collect();
    if kind.ordered_by_value() {
        order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
    }
    Some(order.into_iter().map(|group| std::mem::take(&mut groups[group])).collect())
}

/// The last resort: stack `j` holds the `j`-th frame met at each position.
fn by_occurrence(natural: &[usize], position_of: &[usize], positions: usize, count: usize) -> Vec<Vec<usize>> {
    let mut met = vec![0; positions];
    let mut groups = vec![Vec::with_capacity(positions); count];
    for &frame in natural {
        let position = position_of[frame];
        if let Some(group) = groups.get_mut(met[position]) {
            group.push(frame);
        }
        met[position] += 1;
    }
    groups
}

/// What each stack's label shows, from the first frame of each stack.
fn stack_values(kind: StackKind, groups: &[Vec<usize>], values: &[&FrameValues]) -> Vec<StackValue> {
    let first = |group: &Vec<usize>| group.first().map_or(&NO_VALUES, |&frame| values[frame]);
    let image_types: Vec<&[String]> = groups.iter().map(|g| first(g).image_type.as_slice()).collect();
    let longest = image_types.iter().map(|t| t.len()).max().unwrap_or(0);
    // Only the values of Image Type that differ between the stacks tell them apart.
    let reference = image_types.first().copied().unwrap_or_default();
    let differ: Vec<usize> =
        (0..longest).filter(|&i| image_types.iter().any(|t| t.get(i) != reference.get(i))).collect();
    groups
        .iter()
        .zip(&image_types)
        .map(|(group, image_type)| {
            let v = first(group);
            match kind {
                StackKind::HeartPhase => StackValue { value: v.trigger_time_ms, text: None },
                StackKind::Echo => StackValue { value: v.echo_time_ms, text: None },
                StackKind::Diffusion => StackValue { value: v.b_value, text: None },
                StackKind::ImageType => {
                    let parts: Vec<&str> = differ
                        .iter()
                        .filter_map(|&i| image_type.get(i))
                        .map(String::as_str)
                        .filter(|s| !s.is_empty())
                        .collect();
                    let text = if parts.is_empty() { image_type.join("\\") } else { parts.join("\\") };
                    StackValue { value: None, text: Some(text).filter(|t| !t.is_empty()) }
                }
                StackKind::Sequence => StackValue { value: None, text: v.sequence_name.clone() },
                StackKind::TimePoint | StackKind::Acquisition | StackKind::Part => StackValue::default(),
            }
        })
        .collect()
}

/// Splits frames that share positions into stacks, or `None` when the rule
/// does not apply. The frames must be greyscale, all with geometry, one
/// orientation, size and spacing (`planes[i]` and `values[i]` belong to frame
/// `i`). `natural` is every frame in instance order; `instance_key` gives the
/// instance number and frame number that decide the direction of the stacks.
pub(crate) fn split(
    planes: &[ImagePlane],
    natural: &[usize],
    values: &[&FrameValues],
    instance_key: impl Fn(usize) -> (i32, u32),
) -> Option<Split> {
    let (position_of, positions) = positions(planes);
    let frames = planes.len();
    if positions < 2 || !frames.is_multiple_of(positions) || frames / positions < 2 || values.len() != frames {
        return None;
    }
    let count = frames / positions;
    let mut per_position = vec![0; positions];
    for &position in &position_of {
        per_position[position] += 1;
    }
    if per_position.iter().any(|&n| n != count) {
        return None;
    }
    let (kind, groups) = StackKind::ORDER
        .into_iter()
        .find_map(|kind| partition(kind, natural, values, &position_of, positions, count).map(|groups| (kind, groups)))
        .unwrap_or_else(|| (StackKind::Part, by_occurrence(natural, &position_of, positions, count)));

    // The frame of every stack at every position.
    let mut at = vec![0; count * positions];
    for (stack, group) in groups.iter().enumerate() {
        for &frame in group {
            at[stack * positions + position_of[frame]] = frame;
        }
    }
    // The direction in which the first stack has its lowest instance number
    // first, for every stack: image n of every stack is the same slice.
    let ascending = instance_key(at[0]) <= instance_key(at[positions - 1]);
    let order: Vec<usize> = (0..count)
        .flat_map(|stack| {
            let at = &at;
            (0..positions).map(move |i| at[stack * positions + if ascending { i } else { positions - 1 - i }])
        })
        .collect();
    // One geometry for all stacks: the images of a position lie at one place.
    let (first, rest) = order.split_at(positions);
    let shared = rest.chunks(positions).all(|stack| {
        stack.iter().zip(first).all(|(&a, &b)| planes[a].position.distance(planes[b].position) <= SAME_POSITION_MM)
    });
    if !shared {
        return None;
    }
    let sorted: Vec<ImagePlane> = first.iter().map(|&frame| planes[frame]).collect();
    let verdict = VolumeGeometry::from_sorted_planes(&sorted);
    let stacks = Stacks { kind, len: positions, values: stack_values(kind, &groups, values) };
    Some(Split { order, verdict, stacks })
}

#[cfg(test)]
mod tests {
    use dicom_core::value::DataSetSequence;
    use dicom_core::{DataElement, Length, PrimitiveValue, VR};

    use super::*;

    type Element = DataElement<InMemDicomObject>;

    fn text(tag: Tag, vr: VR, value: &str) -> Element {
        DataElement::new(tag, vr, PrimitiveValue::from(value))
    }

    fn double(tag: Tag, value: f64) -> Element {
        DataElement::new(tag, VR::FD, PrimitiveValue::from(value))
    }

    fn sequence(tag: Tag, items: Vec<InMemDicomObject>) -> Element {
        DataElement::new(tag, VR::SQ, DataSetSequence::new(items, Length::UNDEFINED))
    }

    fn object(elements: impl IntoIterator<Item = Element>) -> InMemDicomObject {
        InMemDicomObject::from_element_iter(elements)
    }

    fn codes(tag: Tag, values: &[&str]) -> Element {
        DataElement::new(tag, VR::CS, PrimitiveValue::Strs(values.iter().map(|v| (*v).to_owned()).collect()))
    }

    /// An axial plane at height `z` (mm), 4 × 4 pixels of 1 mm.
    fn plane(z: f64) -> ImagePlane {
        ImagePlane::from_dicom([0.0, 0.0, z], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [1.0, 1.0], 4, 4).expect("valid plane")
    }

    /// Splits frames at the heights `z` with `values`, in the order given
    /// (which is also their instance order, numbered from 1).
    fn split_of(z: &[f64], values: &[FrameValues]) -> Option<Split> {
        let planes: Vec<ImagePlane> = z.iter().map(|&z| plane(z)).collect();
        let natural: Vec<usize> = (0..z.len()).collect();
        let values: Vec<&FrameValues> = values.iter().collect();
        split(&planes, &natural, &values, |i| (i as i32 + 1, 0))
    }

    fn heights(split: &Split, z: &[f64]) -> Vec<f64> {
        split.order.iter().map(|&i| z[i]).collect()
    }

    #[test]
    fn classic_values_are_read_from_the_top_level() {
        let obj = object([
            text(tags::TEMPORAL_POSITION_IDENTIFIER, VR::IS, "3"),
            text(tags::TRIGGER_TIME, VR::DS, "412.5"),
            text(tags::ECHO_NUMBERS, VR::IS, "2"),
            text(tags::ECHO_TIME, VR::DS, "90"),
            double(tags::DIFFUSION_B_VALUE, 1000.0),
            codes(tags::IMAGE_TYPE, &["ORIGINAL", "PRIMARY", "P"]),
            text(tags::ACQUISITION_NUMBER, VR::IS, "7"),
            text(tags::SEQUENCE_NAME, VR::SH, "*ep_b1000t"),
        ]);
        let values = read_frame_values(&obj, 1);
        assert_eq!(
            values,
            [FrameValues {
                temporal_position: Some(3),
                trigger_time_ms: Some(412.5),
                echo_number: Some(2),
                echo_time_ms: Some(90.0),
                b_value: Some(1000.0),
                image_type: ["ORIGINAL", "PRIMARY", "P"].map(String::from).into(),
                acquisition_number: Some(7),
                sequence_name: Some("*ep_b1000t".to_owned()),
            }]
        );
        // A classic multi-frame object records them once for all its frames.
        let values = read_frame_values(&obj, 5);
        assert_eq!(values.len(), 1);
        assert_eq!(of_frame(&values, 4).temporal_position, Some(3));
        assert_eq!(read_frame_values(&InMemDicomObject::new_empty(), 1), [FrameValues::default()]);
        assert_eq!(of_frame(&[], 0), &FrameValues::default(), "nothing recorded");
    }

    #[test]
    fn enhanced_values_are_read_per_frame_then_shared_then_classic() {
        let frame = |index: u32, echo: f64, trigger: f64| {
            object([
                sequence(
                    tags::FRAME_CONTENT_SEQUENCE,
                    vec![object([
                        DataElement::new(tags::TEMPORAL_POSITION_INDEX, VR::UL, PrimitiveValue::from(index)),
                        DataElement::new(tags::FRAME_ACQUISITION_NUMBER, VR::US, PrimitiveValue::from(index as u16)),
                    ])],
                ),
                sequence(tags::MR_ECHO_SEQUENCE, vec![object([double(tags::EFFECTIVE_ECHO_TIME, echo)])]),
                sequence(
                    tags::CARDIAC_SYNCHRONIZATION_SEQUENCE,
                    vec![object([double(tags::NOMINAL_CARDIAC_TRIGGER_DELAY_TIME, trigger)])],
                ),
                sequence(
                    tags::MR_IMAGE_FRAME_TYPE_SEQUENCE,
                    vec![object([codes(
                        tags::FRAME_TYPE,
                        &["ORIGINAL", "PRIMARY", if index == 1 { "M" } else { "P" }],
                    )])],
                ),
            ])
        };
        let shared =
            object([sequence(tags::MR_DIFFUSION_SEQUENCE, vec![object([double(tags::DIFFUSION_B_VALUE, 800.0)])])]);
        let obj = object([
            text(tags::PULSE_SEQUENCE_NAME, VR::SH, "DWI_EPI"),
            text(tags::IMAGE_TYPE, VR::CS, "ORIGINAL"),
            sequence(tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE, vec![shared]),
            sequence(tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE, vec![frame(1, 30.0, 0.0), frame(2, 100.0, 40.0)]),
        ]);
        let values = read_frame_values(&obj, 2);
        assert_eq!(values.len(), 2);
        assert_eq!(values.iter().map(|v| v.temporal_position).collect::<Vec<_>>(), [Some(1), Some(2)]);
        assert_eq!(values.iter().map(|v| v.echo_time_ms).collect::<Vec<_>>(), [Some(30.0), Some(100.0)]);
        assert_eq!(values.iter().map(|v| v.trigger_time_ms).collect::<Vec<_>>(), [Some(0.0), Some(40.0)]);
        assert_eq!(values.iter().map(|v| v.acquisition_number).collect::<Vec<_>>(), [Some(1), Some(2)]);
        assert_eq!(values[1].image_type, ["ORIGINAL", "PRIMARY", "P"], "the Frame Type of the frame");
        // The shared group and the top level fill in what the frame does not record.
        assert!(values.iter().all(|v| v.b_value == Some(800.0)));
        assert!(values.iter().all(|v| v.sequence_name.as_deref() == Some("DWI_EPI")));
        assert_eq!(of_frame(&values, 1).echo_time_ms, Some(100.0));
        assert_eq!(of_frame(&values, 7), &FrameValues::default(), "a frame the file does not describe");
    }

    #[test]
    fn time_points_are_split_and_ordered_by_their_value() {
        // Time point 2 is stored before time point 1; two slices each, at 0 and 5 mm.
        let z = [5.0, 0.0, 0.0, 5.0, 0.0, 5.0];
        let tp = |t| FrameValues { temporal_position: Some(t), ..FrameValues::default() };
        let split = split_of(&z, &[tp(2), tp(2), tp(1), tp(1), tp(3), tp(3)]).expect("split");
        assert_eq!(split.stacks.kind, StackKind::TimePoint);
        assert_eq!((split.stacks.count(), split.stacks.len), (3, 2));
        assert_eq!(split.order, [2, 3, 1, 0, 4, 5], "time point 1, then 2, then 3; each from 0 to 5 mm");
        assert_eq!(heights(&split, &z), [0.0, 5.0, 0.0, 5.0, 0.0, 5.0]);
        let geometry = split.verdict.expect("a volume");
        assert_eq!(geometry.dims, [4, 4, 2]);
        assert_eq!(split.stacks.label(0), "Time point 1 / 3");
        assert_eq!(split.stacks.summary(), "3 time points × 2 images");
    }

    #[test]
    fn the_first_discriminator_that_partitions_wins() {
        let z = [0.0, 5.0, 0.0, 5.0];
        // The same trigger time everywhere: one group, not two. The echoes split.
        let echo = |n, te| FrameValues {
            trigger_time_ms: Some(10.0),
            echo_number: Some(n),
            echo_time_ms: Some(te),
            ..FrameValues::default()
        };
        let split = split_of(&z, &[echo(2, 90.0), echo(2, 90.0), echo(1, 15.0), echo(1, 15.0)]).expect("split");
        assert_eq!(split.stacks.kind, StackKind::Echo);
        assert_eq!(split.order, [2, 3, 0, 1], "echo 1 first");
        assert_eq!(split.stacks.label(0), "Echo 1 / 2 · TE 15 ms");
        assert_eq!(split.stacks.label(1), "Echo 2 / 2 · TE 90 ms");
        assert!(!split.stacks.kind.is_time_like());

        // Temporal positions that put both images of a slice into one group are no
        // partition: the next discriminator (the b-value) decides.
        let mixed = |t, b| FrameValues { temporal_position: Some(t), b_value: Some(b), ..FrameValues::default() };
        let split = split_of(&z, &[mixed(1, 0.0), mixed(2, 1000.0), mixed(1, 1000.0), mixed(2, 0.0)]).expect("split");
        assert_eq!(split.stacks.kind, StackKind::Diffusion);
        assert_eq!(split.order, [0, 3, 2, 1], "b = 0 at 0 and 5 mm, then b = 1000");
        assert_eq!(
            (split.stacks.label(0), split.stacks.label(1)),
            ("b = 0 s/mm²".to_owned(), "b = 1000 s/mm²".to_owned())
        );

        // A value missing from one frame rules a discriminator out.
        let mut partly = vec![FrameValues { temporal_position: Some(1), ..FrameValues::default() }; 4];
        partly[3].temporal_position = None;
        assert_eq!(split_of(&z, &partly).expect("split").stacks.kind, StackKind::Part);
    }

    #[test]
    fn image_type_labels_show_what_differs() {
        let z = [0.0, 5.0, 0.0, 5.0];
        let typed = |t: &[&str]| FrameValues {
            image_type: t.iter().map(|s| (*s).to_owned()).collect(),
            ..FrameValues::default()
        };
        let magnitude = typed(&["ORIGINAL", "PRIMARY", "M", "ND"]);
        let phase = typed(&["ORIGINAL", "PRIMARY", "P", "ND"]);
        let split = split_of(&z, &[phase.clone(), magnitude.clone(), magnitude, phase]).expect("split");
        assert_eq!(split.stacks.kind, StackKind::ImageType);
        // In the order the types first occur: phase first.
        assert_eq!(split.stacks.label(0), "Image type 1 / 2 · P");
        assert_eq!(split.stacks.label(1), "Image type 2 / 2 · M");
        assert_eq!(split.order, [0, 3, 2, 1]);
    }

    #[test]
    fn nothing_recorded_splits_by_occurrence() {
        let z = [0.0, 5.0, 10.0, 0.0, 5.0, 10.0];
        let split = split_of(&z, &vec![FrameValues::default(); 6]).expect("split");
        assert_eq!(split.stacks.kind, StackKind::Part);
        assert_eq!(split.order, [0, 1, 2, 3, 4, 5]);
        assert_eq!(split.stacks.label(1), "Part 2 / 2");
        assert_eq!(split.stacks.summary(), "2 parts × 3 images");
    }

    #[test]
    fn stacks_run_in_the_direction_of_the_first_stack() {
        // Instance 1 is the top slice of the first stack.
        let z = [10.0, 5.0, 0.0, 10.0, 5.0, 0.0];
        let tp = |t| FrameValues { temporal_position: Some(t), ..FrameValues::default() };
        let split = split_of(&z, &[tp(1), tp(1), tp(1), tp(2), tp(2), tp(2)]).expect("split");
        assert_eq!(heights(&split, &z), [10.0, 5.0, 0.0, 10.0, 5.0, 0.0]);
        let geometry = split.verdict.expect("a volume");
        assert_eq!(geometry.origin.z, 10.0);
        assert_eq!(geometry.axes[2].z, -5.0);
    }

    #[test]
    fn uneven_or_inconsistent_repetition_is_not_split() {
        let none = FrameValues::default();
        // One image missing: 5 frames at 3 positions.
        assert!(split_of(&[0.0, 5.0, 10.0, 0.0, 5.0], &vec![none.clone(); 5]).is_none());
        // Six frames at three positions, but one position holds three of them.
        assert!(split_of(&[0.0, 0.0, 0.0, 5.0, 10.0, 10.0], &vec![none.clone(); 6]).is_none());
        // Every frame at its own position, or all at one position: nothing to split.
        assert!(split_of(&[0.0, 5.0, 10.0], &vec![none.clone(); 3]).is_none());
        assert!(split_of(&[0.0, 0.0, 0.0], &vec![none.clone(); 3]).is_none());
        assert!(at_one_position(&[plane(0.0), plane(0.004), plane(0.0)]));
        // The second set lies 2 mm to the side: the stacks would not share one geometry.
        let planes = vec![plane(0.0), plane(5.0), plane(0.0), plane(5.0)];
        let mut shifted = planes.clone();
        shifted[2].position.x += 2.0;
        shifted[3].position.x += 2.0;
        let values = vec![&none; 4];
        assert!(split(&planes, &[0, 1, 2, 3], &values, |i| (i as i32, 0)).is_some());
        assert!(split(&shifted, &[0, 1, 2, 3], &values, |i| (i as i32, 0)).is_none());
    }

    #[test]
    fn a_missing_position_gives_stacks_without_a_volume() {
        // Positions 0, 5 and 15 mm in both stacks: split, but not evenly spaced.
        let z = [0.0, 5.0, 15.0, 0.0, 5.0, 15.0];
        let split = split_of(&z, &vec![FrameValues::default(); 6]).expect("split");
        assert_eq!(split.stacks.count(), 2);
        assert!(matches!(split.verdict, Err(VolumeError::IrregularSpacing { .. })), "{:?}", split.verdict.err());
    }

    #[test]
    fn one_position_is_played_in_time_order() {
        let tp = |t: Option<i64>, ms: f64| FrameValues {
            temporal_position: t,
            trigger_time_ms: Some(ms),
            ..FrameValues::default()
        };
        let values = [tp(None, 80.0), tp(None, 0.0), tp(None, 40.0)];
        let refs: Vec<&FrameValues> = values.iter().collect();
        assert_eq!(in_time_order(&[0, 1, 2], &refs), [1, 2, 0], "by trigger time");
        let values = [tp(Some(2), 0.0), tp(Some(1), 40.0), tp(Some(3), 80.0)];
        let refs: Vec<&FrameValues> = values.iter().collect();
        assert_eq!(in_time_order(&[0, 1, 2], &refs), [1, 0, 2], "temporal position first");
        let values = [FrameValues::default(), tp(None, 40.0)];
        let refs: Vec<&FrameValues> = values.iter().collect();
        assert_eq!(in_time_order(&[1, 0], &refs), [1, 0], "not recorded by every frame: as given");
    }

    #[test]
    fn labels_name_what_was_recorded() {
        let stacks = |kind, values: Vec<StackValue>| Stacks { kind, len: 12, values };
        let value = |v: f64| StackValue { value: Some(v), text: None };
        let phases = stacks(StackKind::HeartPhase, vec![value(0.0), value(42.5)]);
        assert_eq!(phases.label(1), "Heart phase 2 / 2 · 42.5 ms");
        assert_eq!(phases.summary(), "2 heart phases × 12 images");
        assert!(phases.kind.is_time_like());
        let sequences =
            stacks(StackKind::Sequence, vec![StackValue { value: None, text: Some("*ep_b0".to_owned()) }; 2]);
        assert_eq!(sequences.label(0), "Sequence 1 / 2 · *ep_b0");
        let acquisitions = stacks(StackKind::Acquisition, vec![StackValue::default(); 3]);
        assert_eq!(acquisitions.label(2), "Acquisition 3 / 3");
        assert_eq!(stacks(StackKind::Echo, vec![StackValue::default(); 2]).label(1), "Echo 2 / 2");
        assert_eq!(stacks(StackKind::Diffusion, vec![StackValue::default(); 2]).label(0), "Diffusion weighting 1 / 2");
    }
}
