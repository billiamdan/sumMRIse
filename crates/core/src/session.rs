//! An open study index: selection, background series loading with a memory
//! budget (LRU eviction) and streamed thumbnails.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;
use summrise_dicom::{DicomError, LoadedSeries, ProgressSnapshot, Series, Study, StudyIndex, Thumbnail};

use crate::jobs::{Job, Stream, Waker};

/// Identifies a series within a [`StudyIndex`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SeriesKey {
    pub study: usize,
    pub series: usize,
}

pub enum SeriesState {
    Loading(Job<Result<LoadedSeries, DicomError>>),
    Ready(Arc<LoadedSeries>),
    Failed(String),
}

impl std::fmt::Debug for SeriesState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Loading(_) => f.write_str("Loading"),
            Self::Ready(s) => write!(f, "Ready({} frames)", s.len()),
            Self::Failed(e) => write!(f, "Failed({e})"),
        }
    }
}

/// What changed during [`Session::poll`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
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
    thumbnails: HashMap<SeriesKey, Thumbnail>,
    thumbnail_stream: Option<Stream<(SeriesKey, Result<Thumbnail, String>)>>,
    budget_bytes: usize,
    waker: Option<Waker>,
}

impl Session {
    pub fn new(index: StudyIndex, budget_bytes: usize, waker: Option<Waker>) -> Self {
        let index = Arc::new(index);
        let thumbnail_stream = Some(Self::spawn_thumbnails(index.clone(), waker.clone()));
        let mut session = Self {
            index,
            active_study: 0,
            active_series: None,
            series: HashMap::new(),
            recently_used: VecDeque::new(),
            thumbnails: HashMap::new(),
            thumbnail_stream,
            budget_bytes,
            waker,
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
                let result = summrise_dicom::thumbnail(series, THUMBNAIL_SIZE).map_err(|e| e.to_string());
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
            }
        }
    }

    /// Makes `key` active and starts decoding it if needed.
    pub fn select_series(&mut self, key: SeriesKey) {
        if self.series(key).is_none() {
            return;
        }
        self.active_study = key.study;
        self.active_series = Some(key);
        self.touch(key);
        if matches!(self.series.get(&key), None | Some(SeriesState::Failed(_))) {
            let series = self.index.studies[key.study].series[key.series].clone();
            let job = Job::spawn("decode", self.waker.clone(), move |progress| {
                summrise_dicom::load_series(&series, progress)
            });
            self.series.insert(key, SeriesState::Loading(job));
        }
    }

    pub fn state(&self, key: SeriesKey) -> Option<&SeriesState> {
        self.series.get(&key)
    }

    pub fn loaded(&self, key: SeriesKey) -> Option<Arc<LoadedSeries>> {
        match self.series.get(&key)? {
            SeriesState::Ready(s) => Some(s.clone()),
            _ => None,
        }
    }

    pub fn loading_progress(&self, key: SeriesKey) -> Option<ProgressSnapshot> {
        match self.series.get(&key)? {
            SeriesState::Loading(job) => Some(job.progress()),
            _ => None,
        }
    }

    pub fn thumbnail(&self, key: SeriesKey) -> Option<&Thumbnail> {
        self.thumbnails.get(&key)
    }

    /// Whether any background work is still running.
    pub fn is_busy(&self) -> bool {
        self.series.values().any(|s| matches!(s, SeriesState::Loading(_)))
    }

    /// Collects finished background work. Call once per UI frame.
    pub fn poll(&mut self) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        for (key, state) in &mut self.series {
            if let SeriesState::Loading(job) = state
                && let Some(result) = job.poll()
            {
                *state = match result {
                    Ok(series) => {
                        log::info!(
                            "decoded series {}/{}: {} frames, {:.1} MB",
                            key.study,
                            key.series,
                            series.len(),
                            series.byte_size() as f64 / 1e6
                        );
                        events.push(SessionEvent::SeriesReady(*key));
                        SeriesState::Ready(Arc::new(series))
                    }
                    Err(e) => {
                        events.push(SessionEvent::SeriesFailed(*key, e.to_string()));
                        SeriesState::Failed(e.to_string())
                    }
                };
            }
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
        if events.iter().any(|e| matches!(e, SessionEvent::SeriesReady(_))) {
            self.enforce_budget();
        }
        events
    }

    fn touch(&mut self, key: SeriesKey) {
        self.recently_used.retain(|k| *k != key);
        self.recently_used.push_back(key);
    }

    /// Evicts least recently used decoded series beyond the memory budget.
    fn enforce_budget(&mut self) {
        let size = |s: &SeriesState| match s {
            SeriesState::Ready(s) => s.byte_size(),
            _ => 0,
        };
        let mut total: usize = self.series.values().map(size).sum();
        while total > self.budget_bytes {
            let Some(victim) =
                self.recently_used.iter().copied().find(|k| {
                    Some(*k) != self.active_series && matches!(self.series.get(k), Some(SeriesState::Ready(_)))
                })
            else {
                break;
            };
            if let Some(state) = self.series.remove(&victim) {
                total -= size(&state);
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

impl Drop for Session {
    fn drop(&mut self) {
        for state in self.series.values() {
            if let SeriesState::Loading(job) = state {
                job.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use summrise_dicom::Progress;
    use summrise_testkit::{PhantomOptions, write_phantom_study};

    #[test]
    fn opens_default_series_and_streams_thumbnails() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_phantom_study(dir.path(), &PhantomOptions::small()).expect("fixture");
        let index = summrise_dicom::scan(dir.path(), &Progress::new()).expect("scan");
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

    #[test]
    fn evicts_least_recently_used_series() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_phantom_study(dir.path(), &PhantomOptions::small()).expect("fixture");
        let index = summrise_dicom::scan(dir.path(), &Progress::new()).expect("scan");
        let mut session = Session::new(index, 1, None);
        let ct = session.active_series.expect("ct");
        let wait = |session: &mut Session, key| {
            let start = Instant::now();
            while session.loaded(key).is_none() && start.elapsed() < Duration::from_secs(20) {
                session.poll();
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&mut session, ct);
        let mr = SeriesKey { study: 0, series: 1 };
        session.select_series(mr);
        wait(&mut session, mr);
        assert!(session.loaded(mr).is_some());
        assert!(session.loaded(ct).is_none(), "CT evicted under a 1-byte budget");
    }
}
