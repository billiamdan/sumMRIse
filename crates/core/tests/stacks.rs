//! Series split into stacks at the level of the session: the guarantees of
//! progressive loading (first image after one decode, nothing moves, final
//! state equal to loading without the background loader) hold for them, and
//! the session knows the stacks of every series before any is loaded.
//! Synthetic data only (`summrise_testkit::stacks`).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use summrise_core::dicom::{
    DecodedInstance, DicomError, InstanceHeader, LoadedSeries, Media, Progress, StackKind, StudyIndex, load_series,
    scan,
};
use summrise_core::{DicomFiles, ImageSlot, ImageSource, SeriesKey, Session, SessionEvent, ViewKind, ViewState};
use summrise_testkit::stacks::{self as phantoms, write_stacks_study};

fn stacks_study() -> (tempfile::TempDir, StudyIndex) {
    let dir = tempfile::tempdir().expect("tempdir");
    write_stacks_study(dir.path()).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    (dir, index)
}

fn key(series: usize) -> SeriesKey {
    SeriesKey { study: 0, series }
}

fn wait(session: &mut Session, events: &mut Vec<SessionEvent>, what: &str, done: impl Fn(&Session) -> bool) {
    let start = Instant::now();
    loop {
        events.extend(session.poll().into_iter().filter(|e| !matches!(e, SessionEvent::ThumbnailReady(_))));
        if done(session) {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Lets the reads of one series folder through only as far as the test
/// allows; every other file is read at once.
struct Gate {
    folder: String,
    allowed: AtomicUsize,
    started: AtomicUsize,
}

impl Gate {
    fn new(series_number: u32) -> Arc<Self> {
        Arc::new(Self {
            folder: format!("SE{series_number:06}"),
            allowed: AtomicUsize::new(0),
            started: AtomicUsize::new(0),
        })
    }

    fn allow(&self, n: usize) {
        self.allowed.fetch_add(n, Ordering::SeqCst);
    }

    fn allow_all(&self) {
        self.allowed.store(usize::MAX, Ordering::SeqCst);
    }

    fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }
}

impl ImageSource for Gate {
    fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError> {
        if header.path.parent().is_some_and(|folder| folder.ends_with(&self.folder)) {
            let turn = self.started.fetch_add(1, Ordering::SeqCst);
            while turn >= self.allowed.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        DicomFiles.read(media, header)
    }

    fn decode(&self, header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError> {
        DicomFiles.decode(header, bytes)
    }
}

#[test]
fn the_session_knows_the_stacks_of_every_series() {
    let (_dir, index) = stacks_study();
    let session = Session::new(index, 1 << 30, None);
    let kinds: Vec<Option<(StackKind, usize, usize)>> =
        (0..7).map(|i| session.stacks(key(i)).map(|s| (s.kind, s.count(), s.len))).collect();
    assert_eq!(
        kinds,
        [
            Some((StackKind::TimePoint, 3, 12)),
            Some((StackKind::Echo, 2, 12)),
            Some((StackKind::Diffusion, 2, 12)),
            Some((StackKind::Part, 3, 12)),
            None,
            None,
            Some((StackKind::Echo, 2, 6)),
        ]
    );
    assert_eq!(
        session.stacks(key(phantoms::DYNAMIC)).map(|s| s.summary()).as_deref(),
        Some("3 time points × 12 images")
    );
    assert!(session.stacks(SeriesKey { study: 3, series: 0 }).is_none());
}

/// Task 3.6 — with a slow source the first image of a split series is there
/// after one decode, at the initial position of the first stack, and the
/// order of the images and their stacks never change.
#[test]
fn a_split_series_opens_after_one_decode_and_nothing_moves() {
    let (_dir, index) = stacks_study();
    let gate = Gate::new(11);
    let mut session = Session::with_source(index, 1 << 30, None, gate.clone());
    let mut events = Vec::new();
    let dynamic = key(phantoms::DYNAMIC);
    session.select_series(dynamic);
    let data = session.data(dynamic).expect("planned on selection");
    assert_eq!((data.len(), data.stack_count(), data.stack_len(), data.loaded()), (36, 3, 12, 0));
    let positions = data.frames().to_vec();
    let shown = data.initial_index();
    assert_eq!(shown, 6, "image 7 / 12 of the first time point");

    gate.allow(1);
    wait(&mut session, &mut events, "the first image", |_| data.is_viewable() && gate.started() == 2);
    assert_eq!(data.loaded(), 1, "exactly one image was decoded");
    assert!(matches!(data.image(shown), ImageSlot::Ready(_)));
    assert!((0..36).filter(|&i| i != shown).all(|i| matches!(data.image(i), ImageSlot::Pending)));
    assert!(events.contains(&SessionEvent::SeriesOpened(dynamic)));
    let state = ViewState::new(&data);
    assert_eq!((state.stack(), state.slice_index(ViewKind::Stack, &data)), (0, 6));
    assert_eq!(state.slice_count(ViewKind::Stack, &data), 12);

    // The user goes to time point 3 at the same image: that image is read next.
    session.set_displayed(dynamic, 2 * 12 + 6);
    gate.allow(2);
    wait(&mut session, &mut events, "the displayed image", |_| data.loaded() == 3 && gate.started() == 4);
    assert!(data.frame(30).is_some(), "image 7 of time point 3 was loaded next");

    gate.allow_all();
    wait(&mut session, &mut events, "the whole series", |s| s.loaded(dynamic).is_some());
    assert_eq!(data.frames(), positions.as_slice(), "no image changed its place");
    assert_eq!(data.stacks(), session.stacks(dynamic));
    assert!((0..3).all(|stack| data.stack_volume(stack).is_some()));
}

/// Task 3.6 — the final state equals loading without the background loader
/// for every series of the stacks study: order, stacks, per-stack volumes,
/// value range and verdict.
#[test]
fn final_state_equals_non_progressive_loading_for_split_series() {
    let (_dir, index) = stacks_study();
    let reference: Vec<LoadedSeries> = index.studies[0]
        .series
        .iter()
        .map(|s| load_series(&index.media, s, &Progress::new()).expect("decode"))
        .collect();
    let mut session = Session::new(index, 1 << 30, None);
    let mut events = Vec::new();
    for (series, expected) in reference.iter().enumerate() {
        let key = key(series);
        session.select_series(key);
        wait(&mut session, &mut events, "the series", |s| s.loaded(key).is_some());
        let data = session.loaded(key).expect("complete");
        let title = session.series(key).expect("indexed").title();

        assert_eq!(data.frames(), expected.frames.as_slice(), "{title}");
        assert_eq!(data.stacks(), expected.stacks.as_ref(), "{title}");
        for index in 0..expected.len() {
            assert_eq!(data.frame(index), expected.frame(index), "{title}: image {index}");
        }
        for stack in 0..data.stack_count() {
            let (got, want) = (data.stack_volume(stack), expected.stack_volume(stack));
            assert_eq!(got.map(|v| v.geometry), want.map(|v| v.geometry), "{title}: stack {stack}");
            assert_eq!(got.map(|v| &v.slices), want.map(|v| &v.slices), "{title}: stack {stack}");
        }
        assert_eq!(data.volume_issue(), expected.volume_issue.as_ref(), "{title}");
        assert_eq!(data.value_range(), expected.value_range, "{title}");
        assert_eq!(data.presets(), expected.presets.as_slice(), "{title}");
        assert_eq!(data.warnings(), expected.warnings.as_slice(), "{title}");
        assert_eq!(data.byte_size(), expected.byte_size(), "{title}");
        if !expected.presets.is_empty() {
            assert_eq!(data.initial_window(), expected.initial_window, "{title}");
        }
    }
    // The study covers split series with and without volumes and the series that are not split.
    assert_eq!(reference.iter().filter(|r| r.stacks.is_some()).count(), 5);
    assert!(reference[phantoms::DYNAMIC].stack_volume(2).is_some());
    assert!(reference[phantoms::ENHANCED].stack_volume(1).is_some());
    assert!(reference[phantoms::INCOMPLETE].stacks.is_none() && reference[phantoms::INCOMPLETE].volume().is_none());
    assert!(reference[phantoms::CINE].stacks.is_none());
}
