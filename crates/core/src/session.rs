//! An open study index: selection, progressive background loading of the
//! displayed series with a memory budget (LRU eviction) and streamed
//! thumbnails.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;
use summrise_dicom::{Media, Series, Stacks, Study, StudyIndex, Thumbnail};

use crate::jobs::{Decoders, DicomFiles, ImageSource, SeriesLoader, Stream, Waker};
use crate::series::SeriesData;

mod overlay;
mod shown;
pub use overlay::{OverlayProblem, OverlayStatus};
pub use shown::OverBudget;

/// Identifies a series within a [`StudyIndex`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SeriesKey {
    pub study: usize,
    pub series: usize,
}

/// A series whose images are still arriving.
#[derive(Debug)]
pub struct LoadingSeries {
    /// What is loaded so far; shared with the loader, which fills it.
    pub data: Arc<SeriesData>,
    loader: SeriesLoader,
    /// Whether [`SessionEvent::SeriesOpened`] has been raised.
    announced: bool,
}

impl LoadingSeries {
    /// Loading waits because another series is displayed.
    pub fn is_paused(&self) -> bool {
        self.loader.is_paused()
    }
}

#[derive(Debug)]
pub enum SeriesState {
    /// Images arrive one by one, or loading is paused while another series
    /// is displayed. The images loaded so far can be viewed.
    Loading(LoadingSeries),
    /// Every image is loaded.
    Ready(Arc<SeriesData>),
    /// Not a single image could be decoded.
    Failed(String),
}

impl SeriesState {
    /// The images of the series, as far as they are loaded.
    pub fn data(&self) -> Option<&Arc<SeriesData>> {
        match self {
            Self::Loading(loading) => Some(&loading.data),
            Self::Ready(data) => Some(data),
            Self::Failed(_) => None,
        }
    }
}

/// What changed during [`Session::poll`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// The first image of a series is decoded: the series can be displayed
    /// while the remaining images arrive.
    SeriesOpened(SeriesKey),
    /// Every image of a series is loaded.
    SeriesReady(SeriesKey),
    SeriesFailed(SeriesKey, String),
    ThumbnailReady(SeriesKey),
}

/// Thumbnail edge length in pixels.
pub const THUMBNAIL_SIZE: u32 = 96;

pub struct Session {
    index: Arc<StudyIndex>,
    pub active_study: usize,
    pub active_series: Option<SeriesKey>,
    series: HashMap<SeriesKey, SeriesState>,
    recently_used: VecDeque<SeriesKey>,
    /// The image last displayed of each series; loading starts around it.
    displayed: HashMap<SeriesKey, usize>,
    thumbnails: HashMap<SeriesKey, Thumbnail>,
    thumbnail_stream: Option<Stream<(SeriesKey, Result<Thumbnail, String>)>>,
    budget_bytes: usize,
    source: Arc<dyn ImageSource>,
    /// Threads that decode the files the loaders read.
    decoders: Option<Arc<Decoders>>,
    waker: Option<Waker>,
    /// How each series splits into stacks, from its headers: `[study][series]`.
    stacks: Vec<Vec<Option<Stacks>>>,
    /// The series drawn over the displayed one (a PET over a CT or MR): it
    /// keeps loading and is never evicted while it is (see `overlay`).
    overlay: Option<SeriesKey>,
    /// Why the overlay is not loaded, when the headers already rule it out.
    overlay_problem: Option<OverlayProblem>,
    /// The series the viewports of a multi-series layout show (see `shown`).
    shown: Vec<SeriesKey>,
}

impl Session {
    pub fn new(index: StudyIndex, budget_bytes: usize, waker: Option<Waker>) -> Self {
        Self::with_source(index, budget_bytes, waker, Arc::new(DicomFiles))
    }

    /// Like [`Self::new`], loading images through `source` instead of
    /// straight from the indexed files.
    pub fn with_source(
        index: StudyIndex,
        budget_bytes: usize,
        waker: Option<Waker>,
        source: Arc<dyn ImageSource>,
    ) -> Self {
        let index = Arc::new(index);
        let thumbnail_stream = Some(Self::spawn_thumbnails(index.clone(), waker.clone()));
        let stacks =
            index.studies.iter().map(|s| s.series.iter().map(summrise_dicom::series_stacks).collect()).collect();
        let mut session = Self {
            index,
            active_study: 0,
            active_series: None,
            series: HashMap::new(),
            recently_used: VecDeque::new(),
            displayed: HashMap::new(),
            thumbnails: HashMap::new(),
            thumbnail_stream,
            budget_bytes,
            source,
            decoders: decoders(),
            waker,
            stacks,
            overlay: None,
            overlay_problem: None,
            shown: Vec::new(),
        };
        if let Some(key) = session.default_series(0) {
            session.select_series(key);
        }
        session
    }

    fn spawn_thumbnails(
        index: Arc<StudyIndex>,
        waker: Option<Waker>,
    ) -> Stream<(SeriesKey, Result<Thumbnail, String>)> {
        Stream::spawn("thumbnails", waker, move |progress, emit| {
            let keys: Vec<SeriesKey> = index
                .studies
                .iter()
                .enumerate()
                .flat_map(|(s, study)| (0..study.series.len()).map(move |r| SeriesKey { study: s, series: r }))
                .collect();
            keys.par_iter().for_each(|key| {
                if progress.is_cancelled() {
                    return;
                }
                let series = &index.studies[key.study].series[key.series];
                let result = summrise_dicom::thumbnail(&index.media, series, THUMBNAIL_SIZE).map_err(|e| e.to_string());
                emit((*key, result));
            });
        })
    }

    pub fn index(&self) -> &StudyIndex {
        &self.index
    }

    pub fn root(&self) -> &Path {
        &self.index.root
    }

    /// Where the files of the open studies are read from: the folder, or the
    /// archive or disc image that was opened.
    pub fn media(&self) -> &Media {
        &self.index.media
    }

    pub fn studies(&self) -> &[Study] {
        &self.index.studies
    }

    pub fn study(&self) -> &Study {
        &self.index.studies[self.active_study.min(self.index.studies.len() - 1)]
    }

    pub fn series(&self, key: SeriesKey) -> Option<&Series> {
        self.index.studies.get(key.study)?.series.get(key.series)
    }

    pub fn active(&self) -> Option<(SeriesKey, &Series)> {
        let key = self.active_series?;
        Some((key, self.series(key)?))
    }

    /// How series `key` is split into stacks (from its headers, as the series
    /// will be planned), before any of its images is loaded; `None` when it
    /// is not split.
    pub fn stacks(&self, key: SeriesKey) -> Option<&Stacks> {
        self.stacks.get(key.study)?.get(key.series)?.as_ref()
    }

    /// The series shown when a study is opened: the largest cross-sectional
    /// grey-scale stack (skips localizers, reports and screenshots).
    pub fn default_series(&self, study: usize) -> Option<SeriesKey> {
        let s = self.index.studies.get(study)?;
        let score = |series: &Series| {
            let localizer = series.instances.iter().any(|i| i.image_type.iter().any(|t| t == "LOCALIZER"));
            let gray = series.instances.first().is_some_and(|i| i.samples_per_pixel == 1);
            let has_geometry = series.plane().is_some();
            (gray && has_geometry && !localizer, series.frame_count())
        };
        (0..s.series.len()).max_by_key(|&i| score(&s.series[i])).map(|series| SeriesKey { study, series })
    }

    pub fn select_study(&mut self, study: usize) {
        if study < self.index.studies.len() {
            self.active_study = study;
            if let Some(key) = self.default_series(study) {
                self.select_series(key);
            } else if let Some(previous) = self.active_series.take()
                && let Some(SeriesState::Loading(loading)) = self.series.get(&previous)
            {
                // The disc holds only reports of this study: no series is displayed.
                loading.loader.pause();
            }
        }
    }

    /// Makes `key` active. Loading of the series displayed before is paused
    /// (after the file in progress), unless it stays on screen in another
    /// viewport; loading of `key` starts, or resumes where it was paused with
    /// everything already loaded still in place.
    pub fn select_series(&mut self, key: SeriesKey) {
        if self.series(key).is_none() {
            return;
        }
        if let Some(previous) = self.active_series.filter(|previous| *previous != key && !self.shown.contains(previous))
            && let Some(SeriesState::Loading(loading)) = self.series.get(&previous)
        {
            loading.loader.pause();
        }
        self.active_study = key.study;
        self.active_series = Some(key);
        match self.series.get(&key) {
            Some(SeriesState::Loading(loading)) => loading.loader.resume(),
            Some(SeriesState::Ready(_)) => {}
            None | Some(SeriesState::Failed(_)) => self.start_loading(key),
        }
        self.touch(key);
    }

    /// Plans series `key` and starts loading it, around the image last
    /// displayed (for the displayed series and for the overlay).
    fn start_loading(&mut self, key: SeriesKey) {
        let Some(series) = self.series(key) else { return };
        let series = Arc::new(series.clone());
        // Order, positions and total are fixed here, before any pixel data is read.
        let data = Arc::new(SeriesData::new(summrise_dicom::plan_series(&series)));
        let displayed = self
            .displayed
            .get(&key)
            .copied()
            .filter(|&index| index < data.len())
            .unwrap_or_else(|| data.initial_index());
        let loader = SeriesLoader::spawn(
            series,
            self.index.media.clone(),
            data.clone(),
            self.source.clone(),
            self.decoders.clone(),
            displayed,
            self.waker.clone(),
        );
        self.series.insert(key, SeriesState::Loading(LoadingSeries { data, loader, announced: false }));
    }

    pub fn state(&self, key: SeriesKey) -> Option<&SeriesState> {
        self.series.get(&key)
    }

    /// The images of a series, as far as they are loaded.
    pub fn data(&self, key: SeriesKey) -> Option<Arc<SeriesData>> {
        self.series.get(&key)?.data().cloned()
    }

    /// The series once every image of it is loaded.
    pub fn loaded(&self, key: SeriesKey) -> Option<Arc<SeriesData>> {
        match self.series.get(&key)? {
            SeriesState::Ready(data) => Some(data.clone()),
            _ => None,
        }
    }

    /// Tells the session which image of `key` is on screen: loading favours
    /// the images around it and turns to it at once if it is not loaded yet.
    pub fn set_displayed(&mut self, key: SeriesKey, index: usize) {
        self.displayed.insert(key, index);
        if let Some(SeriesState::Loading(loading)) = self.series.get(&key) {
            loading.loader.set_displayed(index);
        }
    }

    pub fn thumbnail(&self, key: SeriesKey) -> Option<&Thumbnail> {
        self.thumbnails.get(&key)
    }

    /// Whether images are being loaded right now (paused series do not count).
    pub fn is_busy(&self) -> bool {
        self.series.values().any(|s| matches!(s, SeriesState::Loading(loading) if !loading.is_paused()))
    }

    /// Collects what the background work has achieved. Call once per UI frame.
    pub fn poll(&mut self) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        for (key, state) in &mut self.series {
            let SeriesState::Loading(loading) = state else { continue };
            // Read first: once this is true, everything else has settled.
            let complete = loading.data.is_complete();
            if !loading.announced && loading.data.is_viewable() {
                loading.announced = true;
                events.push(SessionEvent::SeriesOpened(*key));
            }
            if !complete {
                continue;
            }
            let data = loading.data.clone();
            *state = match data.failure() {
                Some(reason) => {
                    events.push(SessionEvent::SeriesFailed(*key, reason.to_owned()));
                    SeriesState::Failed(reason.to_owned())
                }
                None => {
                    log::info!(
                        "loaded series {}/{}: {} images, {:.1} MB",
                        key.study,
                        key.series,
                        data.len(),
                        data.byte_size() as f64 / 1e6
                    );
                    events.push(SessionEvent::SeriesReady(*key));
                    SeriesState::Ready(data)
                }
            };
        }
        if let Some(stream) = &self.thumbnail_stream {
            for (key, result) in stream.drain() {
                match result {
                    Ok(t) => {
                        self.thumbnails.insert(key, t);
                        events.push(SessionEvent::ThumbnailReady(key));
                    }
                    Err(e) => log::debug!("thumbnail {}/{} failed: {e}", key.study, key.series),
                }
            }
        }
        // Memory is accounted per image, so the budget can be exceeded while a series loads.
        self.enforce_budget();
        self.give_priority();
        events
    }

    fn touch(&mut self, key: SeriesKey) {
        self.recently_used.retain(|k| *k != key);
        self.recently_used.push_back(key);
    }

    /// Evicts the least recently used series beyond the memory budget —
    /// complete or partly loaded, but always as a whole and never one
    /// displayed, nor the one drawn over it. An evicted series is loaded again
    /// when it is re-selected.
    fn enforce_budget(&mut self) {
        let size = |s: &SeriesState| s.data().map_or(0, |data| data.byte_size());
        let mut total: usize = self.series.values().map(size).sum();
        while total > self.budget_bytes {
            let in_use =
                |k: &SeriesKey| Some(*k) == self.active_series || Some(*k) == self.overlay || self.shown.contains(k);
            let Some(victim) = self
                .recently_used
                .iter()
                .copied()
                .find(|k| !in_use(k) && self.series.get(k).is_some_and(|s| s.data().is_some()))
            else {
                break;
            };
            if let Some(state) = self.series.remove(&victim) {
                total = total.saturating_sub(size(&state));
                log::info!("evicted series {}/{} to stay within the memory budget", victim.study, victim.series);
            }
            self.recently_used.retain(|k| *k != victim);
        }
    }

    pub fn set_budget(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
        self.enforce_budget();
    }

    /// Path of the folder the user opened.
    pub fn display_root(&self) -> PathBuf {
        self.index.root.clone()
    }
}

/// The threads that decode images for the loaders: one per core but one,
/// which is left to the UI so that scrolling stays smooth while a series
/// loads. Without any, each loader decodes on its own thread.
fn decoders() -> Option<Arc<Decoders>> {
    let cores = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    Decoders::new(cores.saturating_sub(1).max(1)).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use summrise_dicom::{FrameRef, LoadedSeries, Progress, load_series};
    use summrise_imaging::{VolumeError, VoxelBuffer};
    use summrise_testkit::PhantomOptions;

    use super::*;
    use crate::jobs::testing::{SlowSource, folder};
    use crate::series::ImageSlot;
    use crate::series::testing::phantom;
    use crate::view::{ViewKind, ViewState};

    const CT: SeriesKey = SeriesKey { study: 0, series: 0 };
    const MR: SeriesKey = SeriesKey { study: 0, series: 1 };
    /// Folder of the CT files on the synthetic CD.
    const CT_FOLDER: &str = "SE000001";

    /// Polls the session until `done`, collecting what happened to series.
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

    /// Indices of the images that are decoded.
    fn decoded(data: &SeriesData) -> Vec<usize> {
        (0..data.len()).filter(|&i| data.frame(i).is_some()).collect()
    }

    fn pixels(data: &SeriesData, index: usize) -> *const VoxelBuffer {
        match data.frame(index) {
            Some(FrameRef::Gray { data, .. }) => std::ptr::from_ref(data),
            _ => panic!("image {index} is not a loaded grey-scale image"),
        }
    }

    #[test]
    fn opens_default_series_and_streams_thumbnails() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let mut session = Session::new(index, 1 << 30, None);
        // The CT (largest axial grey-scale stack) is selected by default.
        let (key, series) = session.active().expect("active series");
        assert_eq!(series.modality, "CT");
        let start = Instant::now();
        while (session.loaded(key).is_none() || session.thumbnails.len() < 4)
            && start.elapsed() < Duration::from_secs(20)
        {
            session.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(session.loaded(key).is_some(), "CT decoded");
        assert_eq!(session.thumbnails.len(), 4, "all thumbnails");
        assert!(!session.is_busy());
    }

    /// Task 4.1 — with a slow source the first image is there after one
    /// decode, and the counter and the positions never change.
    #[test]
    fn first_image_shows_after_one_decode_and_nothing_moves() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let slow = SlowSource::new();
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        let mut events = Vec::new();
        assert_eq!(session.active_series, Some(CT));

        // Selected but nothing read yet: the total and every position are known already.
        let data = session.data(CT).expect("planned on selection");
        assert_eq!((data.len(), data.loaded(), data.byte_size()), (24, 0, 0));
        assert!(!data.is_viewable());
        let positions = data.frames().to_vec();
        let shown = data.initial_index();

        // One file is read and decoded: the series is on screen.
        slow.allow(1);
        wait(&mut session, &mut events, "the first image", |_| data.is_viewable() && slow.started() == 2);
        assert_eq!(data.loaded(), 1, "exactly one image was decoded");
        assert!(matches!(data.image(shown), ImageSlot::Ready(_)), "the image at the initial position");
        assert!((0..24).filter(|&i| i != shown).all(|i| matches!(data.image(i), ImageSlot::Pending)));
        assert_eq!(events, [SessionEvent::SeriesOpened(CT)]);
        assert!(session.loaded(CT).is_none() && session.is_busy());
        assert_eq!(data.byte_size(), 48 * 48 * 2, "memory is accounted per image");
        // The counter shows the final total, the image its final number and position.
        let state = ViewState::new(&data);
        assert_eq!(state.slice_index(ViewKind::Stack, &data), shown);
        assert_eq!(state.slice_count(ViewKind::Stack, &data), 24);
        assert_eq!(data.frames(), positions.as_slice());
        let first = pixels(&data, shown);

        // The remaining 23 images arrive.
        slow.allow_all();
        wait(&mut session, &mut events, "the whole series", |s| s.loaded(CT).is_some());
        assert_eq!(events, [SessionEvent::SeriesOpened(CT), SessionEvent::SeriesReady(CT)]);
        assert!(Arc::ptr_eq(&data, &session.loaded(CT).expect("complete")), "the same store, now complete");
        assert_eq!((data.len(), data.loaded()), (24, 24));
        assert_eq!(data.frames(), positions.as_slice(), "no image changed its number or its position");
        assert!(std::ptr::eq(first, pixels(&data, shown)), "the displayed image was not replaced");
        assert_eq!(slow.reads_of(CT_FOLDER).len(), 24);
        assert!(!session.is_busy());
    }

    /// Task 4.2 — after a jump, the target image is decoded before the gap.
    #[test]
    fn jump_loads_the_target_and_its_neighbours_before_the_gap() {
        let opts = PhantomOptions { ct_size: 16, ct_slices: 60, ..PhantomOptions::small() };
        let (_dir, index) = phantom(&opts);
        let slow = SlowSource::new();
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        let mut events = Vec::new();
        let data = session.data(CT).expect("planned");
        assert_eq!((data.len(), data.initial_index()), (60, 30));

        // Images 31 to 33 are loaded and the loader is reading the next one of its run.
        slow.allow(3);
        wait(&mut session, &mut events, "three images", |_| data.loaded() == 3 && slow.started() == 4);
        assert_eq!(decoded(&data), [30, 31, 32]);

        // The user jumps to image 56, far from what is loaded. The read in
        // progress finishes; the very next image is the one displayed.
        session.set_displayed(CT, 55);
        slow.allow(2);
        wait(&mut session, &mut events, "the target", |_| data.loaded() == 5 && slow.started() == 6);
        assert_eq!(decoded(&data), [30, 31, 32, 33, 55]);

        // Then its neighbours on both sides, while the gap (images 35 to 47) still waits.
        slow.allow(12);
        wait(&mut session, &mut events, "the neighbours", |_| data.loaded() == 17 && slow.started() == 18);
        assert_eq!(decoded(&data), (30..=33).chain(47..=59).collect::<Vec<_>>());

        slow.allow_all();
        wait(&mut session, &mut events, "the whole series", |s| s.loaded(CT).is_some());
        assert_eq!(decoded(&data).len(), 60);
        // Read order: the gap was filled last of the images near the viewer, from the viewer's side.
        let order: Vec<usize> = slow.reads_of(CT_FOLDER).iter().map(|n| *n as usize - 1).collect();
        assert_eq!(order[..18], [30, 31, 32, 33, 55, 56, 57, 58, 59, 54, 53, 52, 51, 50, 49, 48, 47, 46]);
    }

    /// Task 4.3 — pause on series switch, resume on return, and a window set
    /// during loading (W 1500 / L −600) is unchanged at completion.
    #[test]
    fn switching_series_pauses_and_returning_resumes() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let slow = SlowSource::new();
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        let mut events = Vec::new();
        let ct = session.data(CT).expect("planned");

        // Five CT images are loaded and a sixth is being read.
        slow.allow(5);
        wait(&mut session, &mut events, "five images", |_| ct.loaded() == 5 && slow.started() == 6);
        // The user picks the lung window while the series loads.
        let mut state = ViewState::new(&ct);
        let opened_with = state.window;
        let lung = ViewState::presets(&ct).into_iter().find(|p| p.name == "Lung").expect("stored window");
        state.apply_preset(&lung);
        assert_eq!((state.window.width, state.window.center), (1500.0, -600.0));

        // Another series is selected: the CT stops after the image in progress …
        session.select_series(MR);
        assert!(matches!(session.state(CT), Some(SeriesState::Loading(loading)) if loading.is_paused()));
        slow.allow_all();
        wait(&mut session, &mut events, "the MR", |s| s.loaded(MR).is_some() && ct.loaded() == 6);
        // … and stays there, however long the other series is displayed.
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(ct.loaded(), 6, "paused: no further CT image is read");
        assert_eq!(slow.reads_of(CT_FOLDER).len(), 6);
        assert!(!session.is_busy(), "a paused series is not being loaded");
        let kept = decoded(&ct);
        assert_eq!(kept.len(), 6);

        // Back to the CT: what was loaded is there at once, the rest follows.
        session.select_series(CT);
        let resumed = session.data(CT).expect("kept while paused");
        assert!(Arc::ptr_eq(&ct, &resumed), "the partly loaded series is the one resumed");
        assert!(kept.iter().all(|&i| resumed.frame(i).is_some()), "its images are available immediately");
        wait(&mut session, &mut events, "the CT", |s| s.loaded(CT).is_some());
        assert_eq!(ct.loaded(), 24);
        // Every file was read exactly once: loaded images were not decoded again.
        let mut reads = slow.reads_of(CT_FOLDER);
        reads.sort_unstable();
        assert_eq!(reads, (1..=24).collect::<Vec<_>>());
        assert_eq!(
            events,
            [
                SessionEvent::SeriesOpened(CT),
                SessionEvent::SeriesOpened(MR),
                SessionEvent::SeriesReady(MR),
                SessionEvent::SeriesReady(CT)
            ]
        );

        // The window set during loading is unchanged at completion, and so
        // is the window the series itself opens with.
        assert_eq!((state.window.width, state.window.center), (1500.0, -600.0));
        assert_eq!(state.preset.as_deref(), Some("Lung"));
        assert_eq!(ViewState::new(&ct).window, opened_with);
    }

    /// Task 4.4 — the final state equals non-progressive loading for every
    /// synthetic series: order, volume, value range (and the pixels).
    #[test]
    fn final_state_equals_non_progressive_loading_for_every_synthetic_series() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let reference: Vec<LoadedSeries> = index.studies[0]
            .series
            .iter()
            .map(|s| load_series(&index.media, s, &Progress::new()).expect("decode"))
            .collect();
        assert_eq!(reference.len(), 4, "CT, MR, localizer, colour");
        let mut session = Session::new(index, 1 << 30, None);
        let mut events = Vec::new();
        for (series, expected) in reference.iter().enumerate() {
            let key = SeriesKey { study: 0, series };
            session.select_series(key);
            wait(&mut session, &mut events, "the series", |s| s.loaded(key).is_some());
            let data = session.loaded(key).expect("complete");
            let title = session.series(key).expect("indexed").title();

            // Order: the same images at the same numbers and positions.
            assert_eq!(data.frames(), expected.frames.as_slice(), "{title}");
            for index in 0..expected.len() {
                assert_eq!(data.frame(index), expected.frame(index), "{title}: image {index}");
            }
            // Volume: the same geometry and voxels, or the same reason for having none.
            assert_eq!(data.volume().map(|v| v.geometry), expected.volume().map(|v| v.geometry), "{title}");
            assert_eq!(data.volume().map(|v| &v.slices), expected.volume().map(|v| &v.slices), "{title}");
            assert_eq!(data.volume_issue(), expected.volume_issue.as_ref(), "{title}");
            // Value range.
            assert_eq!(data.value_range(), expected.value_range, "{title}");
            // And everything else that does not depend on which image was decoded first.
            assert_eq!(data.is_color(), expected.is_color, "{title}");
            assert_eq!(data.monochrome1(), expected.monochrome1, "{title}");
            assert_eq!(data.units(), expected.units.as_deref(), "{title}");
            assert_eq!(data.modality(), expected.modality, "{title}");
            assert_eq!(data.presets(), expected.presets.as_slice(), "{title}");
            assert_eq!(data.warnings(), expected.warnings.as_slice(), "{title}");
            assert_eq!(data.byte_size(), expected.byte_size(), "{title}");
            if !expected.presets.is_empty() || expected.is_color {
                assert_eq!(data.initial_window(), expected.initial_window, "{title}");
            }
        }
        // The fixtures cover both outcomes.
        assert!(reference[0].volume().is_some() && reference[1].volume().is_some());
        assert_eq!(reference[2].volume_issue, Some(VolumeError::InconsistentGrid));
        assert_eq!(reference[3].volume_issue, Some(VolumeError::NotGrayscale));
    }

    #[test]
    fn undecodable_image_is_reported_at_its_position() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        // Image 17 of the CT cannot be decoded.
        let slow = SlowSource::with_corrupt_images(|h| folder(h) == CT_FOLDER && h.instance_number == Some(17));
        slow.allow_all();
        let mut session = Session::with_source(index, 1 << 30, None, slow);
        let mut events = Vec::new();
        wait(&mut session, &mut events, "the CT", |s| s.loaded(CT).is_some());
        let data = session.loaded(CT).expect("complete");
        assert!(matches!(data.image(16), ImageSlot::Failed("corrupt pixel data")));
        assert_eq!(decoded(&data), (0..24).filter(|&i| i != 16).collect::<Vec<_>>(), "the others display normally");
        assert_eq!(data.warnings(), ["1 image could not be decoded: corrupt pixel data"]);
        assert!(data.volume().is_none());
        assert!(matches!(data.volume_issue(), Some(VolumeError::IrregularSpacing { .. })));
        assert_eq!(events, [SessionEvent::SeriesOpened(CT), SessionEvent::SeriesReady(CT)]);
    }

    #[test]
    fn series_opens_on_the_next_image_when_the_displayed_one_fails() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        // The image at the initial position (image 13) cannot be decoded.
        let slow = SlowSource::with_corrupt_images(|h| folder(h) == CT_FOLDER && h.instance_number == Some(13));
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        let mut events = Vec::new();
        let data = session.data(CT).expect("planned");
        slow.allow(1);
        wait(&mut session, &mut events, "the failed image", |_| data.loaded() == 1 && slow.started() == 2);
        assert!(matches!(data.image(12), ImageSlot::Failed(_)) && !data.is_viewable());
        assert_eq!(events, []);
        slow.allow(1);
        wait(&mut session, &mut events, "the next image", |_| data.is_viewable());
        assert_eq!(decoded(&data), [13]);
        assert_eq!(events, [SessionEvent::SeriesOpened(CT)]);
        slow.allow_all();
        wait(&mut session, &mut events, "the CT", |s| s.loaded(CT).is_some());
    }

    #[test]
    fn series_without_a_decodable_image_fails_and_can_be_retried() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let slow = SlowSource::with_corrupt_images(|h| folder(h) == CT_FOLDER);
        slow.allow_all();
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        let mut events = Vec::new();
        wait(&mut session, &mut events, "the failure", |s| matches!(s.state(CT), Some(SeriesState::Failed(_))));
        let reason = "none of the images in this series could be decoded: corrupt pixel data";
        assert_eq!(events, [SessionEvent::SeriesFailed(CT, reason.to_owned())]);
        assert!(session.data(CT).is_none() && !session.is_busy());
        // Selecting the series again tries again.
        session.select_series(CT);
        assert!(matches!(session.state(CT), Some(SeriesState::Loading(_))));
        wait(&mut session, &mut events, "the second failure", |s| matches!(s.state(CT), Some(SeriesState::Failed(_))));
        assert_eq!(slow.reads_of(CT_FOLDER).len(), 48);
    }

    #[test]
    fn series_left_during_its_only_image_is_still_finished() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let slow = SlowSource::new();
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        let mut events = Vec::new();
        wait(&mut session, &mut events, "the first read of the CT", |_| slow.started() == 1);
        // The user opens the one-image dose report and goes back to the CT before the image is there.
        let report = SeriesKey { study: 0, series: 3 };
        session.select_series(report);
        wait(&mut session, &mut events, "the read of the report", |_| slow.started() == 2);
        session.select_series(CT);
        slow.allow_all();
        wait(&mut session, &mut events, "both series", |s| s.loaded(CT).is_some() && s.loaded(report).is_some());
        // The report was not displayed any more, but nothing was left to hold back: it is
        // complete, not "paused at 1 / 1".
        assert!(matches!(session.state(report), Some(SeriesState::Ready(_))));
        let of_report: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SessionEvent::SeriesOpened(k) | SessionEvent::SeriesReady(k) if *k == report))
            .collect();
        assert_eq!(of_report, [&SessionEvent::SeriesOpened(report), &SessionEvent::SeriesReady(report)]);
    }

    /// A study of which the disc holds only reports: no series is displayed.
    #[test]
    fn a_study_without_images_displays_no_series_and_stops_the_loading() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = summrise_testkit::reports::ReportSpec {
            study_uid: "2.25.42".to_owned(),
            ..summrise_testkit::reports::ReportSpec::new("1.2.840.10008.5.1.4.1.1.88.11", "Report", Vec::new())
        };
        summrise_testkit::write_phantom_study(
            dir.path(),
            &PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() },
        )
        .expect("fixture");
        summrise_testkit::reports::write_structured_report(&dir.path().join("SR1"), &spec).expect("report");
        let index = summrise_dicom::scan(dir.path(), &Progress::new()).expect("scan");
        let lonely = index.studies.iter().position(|s| s.series.is_empty()).expect("the study without images");
        let images = 1 - lonely;
        let slow = SlowSource::new();
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        session.select_study(images);
        let shown = session.active_series.expect("a series of the study with images");
        session.select_study(lonely);
        assert_eq!(session.active_series, None);
        assert_eq!(session.active_study, lonely);
        assert!(matches!(session.state(shown), Some(SeriesState::Loading(loading)) if loading.is_paused()));
        // And back.
        session.select_study(images);
        assert_eq!(session.active_series, Some(shown));
        assert!(matches!(session.state(shown), Some(SeriesState::Loading(loading)) if !loading.is_paused()));
        slow.allow_all();
    }

    #[test]
    fn evicts_least_recently_used_series() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let mut session = Session::new(index, 1, None);
        let mut events = Vec::new();
        wait(&mut session, &mut events, "the CT", |s| s.loaded(CT).is_some());
        session.select_series(MR);
        wait(&mut session, &mut events, "the MR", |s| s.loaded(MR).is_some());
        assert!(session.loaded(CT).is_none(), "CT evicted under a 1-byte budget");
        assert!(session.state(CT).is_none());
    }

    #[test]
    fn partly_loaded_series_is_evicted_as_a_whole() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let slow = SlowSource::new();
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        let mut events = Vec::new();
        let ct = session.data(CT).expect("planned");
        slow.allow(5);
        wait(&mut session, &mut events, "five images", |_| ct.loaded() == 5 && slow.started() == 6);
        session.select_series(MR);
        slow.allow_all();
        wait(&mut session, &mut events, "the MR", |s| s.loaded(MR).is_some() && ct.loaded() == 6);
        // Both fit into the budget: the paused CT keeps its six images.
        assert_eq!(session.data(CT).map(|d| d.loaded()), Some(6));

        // The budget shrinks: the paused series goes, all of it; the displayed one stays.
        session.set_budget(1);
        assert!(session.state(CT).is_none(), "evicted as a whole, not image by image");
        assert!(session.loaded(MR).is_some());

        // Selected again, it is loaded from the start.
        session.select_series(CT);
        let again = session.data(CT).expect("loading again");
        assert!(!Arc::ptr_eq(&ct, &again));
        wait(&mut session, &mut events, "the CT", |s| s.loaded(CT).is_some());
        assert_eq!(slow.reads_of(CT_FOLDER).len(), 6 + 24);
        assert!(session.state(MR).is_none(), "now the MR is the one not displayed");
    }

    #[test]
    fn loading_starts_at_the_image_last_displayed() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let slow = SlowSource::new();
        let mut session = Session::with_source(index, 1, None, slow.clone());
        let mut events = Vec::new();
        slow.allow_all();
        wait(&mut session, &mut events, "the CT", |s| s.loaded(CT).is_some());
        // The user leaves the CT on image 3; under a 1-byte budget it is evicted.
        session.set_displayed(CT, 2);
        session.select_series(MR);
        wait(&mut session, &mut events, "the MR", |s| s.loaded(MR).is_some());
        assert!(session.state(CT).is_none());
        // Coming back, image 3 is the first to be read again.
        session.select_series(CT);
        wait(&mut session, &mut events, "the CT again", |s| s.loaded(CT).is_some());
        assert_eq!(slow.reads_of(CT_FOLDER)[24], 3);
    }
}
