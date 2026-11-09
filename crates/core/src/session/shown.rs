//! The series on screen in the viewports of a multi-series layout, besides the
//! active one: they stay loaded whatever the memory budget says, and the
//! session says when that exceeds it. Each loads on its own, and the active
//! viewport's series comes first: while it is still loading, the others read
//! only the image on screen (`openspec/changes/series-comparison/design.md`, D7).

use super::{SeriesKey, SeriesState, Session};
use crate::pet;

/// The series on screen need more memory than the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverBudget {
    pub needed_bytes: usize,
    pub budget_bytes: usize,
}

impl Session {
    /// Tells the session which series the viewports show (none for a layout of
    /// one series). Those not loaded yet start loading; those no longer on
    /// screen may be paused and evicted again. Cheap when nothing changes, so
    /// it can be called every frame.
    pub fn set_shown(&mut self, keys: &[SeriesKey]) {
        if keys != self.shown {
            let left: Vec<SeriesKey> = self.shown.iter().copied().filter(|key| !keys.contains(key)).collect();
            self.shown = keys.iter().copied().filter(|key| self.series(*key).is_some()).collect();
            for key in left {
                if Some(key) != self.active_series
                    && Some(key) != self.overlay
                    && let Some(SeriesState::Loading(loading)) = self.series.get(&key)
                {
                    loading.loader.pause();
                }
            }
            for key in self.shown.clone() {
                match self.series.get(&key) {
                    Some(SeriesState::Loading(loading)) => loading.loader.resume(),
                    Some(SeriesState::Ready(_) | SeriesState::Failed(_)) => {}
                    None => self.start_loading(key),
                }
                self.touch(key);
            }
        }
        self.give_priority();
    }

    /// While the active series is loading, the others on screen read only
    /// the image on screen.
    pub(super) fn give_priority(&self) {
        let active_loads =
            matches!(self.active_series.and_then(|key| self.series.get(&key)), Some(SeriesState::Loading(_)));
        for key in &self.shown {
            if let Some(SeriesState::Loading(loading)) = self.series.get(key) {
                loading.loader.set_background(active_loads && Some(*key) != self.active_series);
            }
        }
    }

    /// The series the viewports of a multi-series layout show.
    pub fn shown(&self) -> &[SeriesKey] {
        &self.shown
    }

    /// No series is active: the active viewport waits for one.
    pub fn deselect(&mut self) {
        if let Some(previous) = self.active_series.take()
            && !self.shown.contains(&previous)
            && Some(previous) != self.overlay
            && let Some(SeriesState::Loading(loading)) = self.series.get(&previous)
        {
            loading.loader.pause();
        }
    }

    /// Whether the series on screen in a multi-series layout together need
    /// more memory than the budget: what is loaded completely with its true
    /// size, the rest as estimated from the headers. They stay loaded anyway.
    pub fn over_budget(&self) -> Option<OverBudget> {
        if self.shown.len() < 2 {
            return None;
        }
        let size = |key: &SeriesKey| match self.data(*key).filter(|data| data.is_complete()) {
            Some(data) => data.byte_size(),
            None => {
                let loaded = self.data(*key).map_or(0, |data| data.byte_size());
                self.series(*key).map_or(0, pet::estimated_bytes).max(loaded)
            }
        };
        let extra = self.overlay.filter(|key| !self.shown.contains(key));
        let needed_bytes = self.shown.iter().chain(extra.iter()).map(size).sum();
        (needed_bytes > self.budget_bytes).then_some(OverBudget { needed_bytes, budget_bytes: self.budget_bytes })
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use summrise_dicom::{Progress, StudyIndex, scan};
    use summrise_testkit::comparison::{self as phantom, ComparisonOptions, write_comparison_studies};

    use super::*;
    use crate::jobs::testing::SlowSource;

    const AXIAL: SeriesKey = SeriesKey { study: phantom::EARLIER, series: phantom::AXIAL_5MM };
    const FINE: SeriesKey = SeriesKey { study: phantom::EARLIER, series: phantom::AXIAL_2_5MM };
    const SAGITTAL: SeriesKey = SeriesKey { study: phantom::EARLIER, series: phantom::SAGITTAL };
    /// Folders of the three series in the earlier study.
    const AXIAL_FOLDER: &str = "SE000001";
    const FINE_FOLDER: &str = "SE000002";

    fn studies() -> (tempfile::TempDir, StudyIndex) {
        let dir = tempfile::tempdir().expect("tempdir");
        write_comparison_studies(dir.path(), &ComparisonOptions::default()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        (dir, index)
    }

    fn wait(session: &mut Session, what: &str, done: impl Fn(&Session) -> bool) {
        let start = Instant::now();
        while !done(session) {
            session.poll();
            assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Task 1.2 and 6.4 — displayed series are not evicted; the session says
    /// the budget is exceeded, with the numbers the notice needs.
    #[test]
    fn displayed_series_stay_loaded_over_the_budget_and_it_is_said() {
        let (_dir, index) = studies();
        let mut session = Session::new(index, 1 << 30, None);
        session.select_series(AXIAL);
        session.set_shown(&[AXIAL, FINE]);
        wait(&mut session, "both series", |s| s.loaded(AXIAL).is_some() && s.loaded(FINE).is_some());
        assert_eq!(session.over_budget(), None, "they fit");
        let axial = session.loaded(AXIAL).expect("loaded").byte_size();
        let fine = session.loaded(FINE).expect("loaded").byte_size();
        assert_eq!((axial, fine), (25 * 64 * 64 * 2, 49 * 64 * 64 * 2));

        // The limit is below what the two need together: both stay, and it is said.
        session.set_budget(axial + fine - 1);
        session.poll();
        assert!(session.loaded(AXIAL).is_some() && session.loaded(FINE).is_some());
        let over = session.over_budget().expect("over the budget");
        assert_eq!(over, OverBudget { needed_bytes: axial + fine, budget_bytes: axial + fine - 1 });

        // Taken off the screen, the other series can be evicted, and the notice ends.
        session.set_shown(&[AXIAL]);
        session.poll();
        assert!(session.state(FINE).is_none(), "no longer displayed: evicted");
        assert!(session.loaded(AXIAL).is_some(), "the active series stays");
        assert_eq!(session.over_budget(), None);

        // Without a multi-series layout, as before: only the active series is protected.
        session.set_shown(&[]);
        session.set_budget(1);
        session.select_series(SAGITTAL);
        wait(&mut session, "the sagittal", |s| s.loaded(SAGITTAL).is_some());
        assert!(session.state(AXIAL).is_none());
    }

    /// Task 1.2 — switching the active viewport does not pause a series that
    /// stays on screen, and the active one is loaded first.
    #[test]
    fn the_active_series_loads_first_and_a_shown_series_is_not_paused() {
        let (_dir, index) = studies();
        let slow = SlowSource::new();
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        session.select_series(AXIAL);
        session.set_shown(&[AXIAL, FINE]);
        let fine = session.data(FINE).expect("planned at once");
        assert_eq!(fine.len(), 49);

        // Everything is let through: while the active series loads, the other one reads
        // only the image on screen.
        slow.allow(6);
        wait(&mut session, "five axial images and one fine", |s| {
            s.data(AXIAL).is_some_and(|data| data.loaded() >= 4) && fine.loaded() >= 1
        });
        let reads_before = slow.reads_of(FINE_FOLDER).len();
        assert_eq!(reads_before, 1, "the image on screen of the series beside it: {:?}", slow.reads_of(FINE_FOLDER));

        // It follows to another image: that image is read next, also in the background.
        let started = slow.started();
        session.set_displayed(FINE, 40);
        wait(&mut session, "the read of that image", |_| slow.started() > started);
        slow.allow(3);
        wait(&mut session, "the image the other viewport went to", |_| fine.frame(40).is_some());
        assert_eq!(slow.reads_of(FINE_FOLDER).len(), 2, "only the image on screen");

        // The active series is complete: the other one goes on by itself.
        slow.allow_all();
        wait(&mut session, "the active series", |s| s.loaded(AXIAL).is_some());
        wait(&mut session, "the series beside it", |s| s.loaded(FINE).is_some());
        assert_eq!(slow.reads_of(FINE_FOLDER).len(), 49);
        assert_eq!(slow.reads_of(AXIAL_FOLDER).len(), 25);
    }

    /// Switching the active viewport hands the priority over, without pausing.
    #[test]
    fn switching_the_active_viewport_keeps_both_loading() {
        let (_dir, index) = studies();
        let slow = SlowSource::new();
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        session.select_series(AXIAL);
        session.set_shown(&[AXIAL, FINE]);
        slow.allow(2);
        wait(&mut session, "a first image", |s| s.data(AXIAL).is_some_and(|data| data.loaded() >= 1));

        session.select_series(FINE);
        session.poll();
        let paused = |key| matches!(session.state(key), Some(SeriesState::Loading(l)) if l.is_paused());
        assert!(!paused(AXIAL), "it stays on screen");
        assert!(!paused(FINE));
        assert!(session.is_busy());
        slow.allow_all();
        wait(&mut session, "the active series", |s| s.loaded(FINE).is_some());
        wait(&mut session, "the other series", |s| s.loaded(AXIAL).is_some());
        assert_eq!(slow.reads_of(AXIAL_FOLDER).len(), 25);
    }

    /// Deselecting (an empty viewport is active) leaves shown series loading,
    /// and nothing else.
    #[test]
    fn an_empty_active_viewport_leaves_the_shown_series_loading() {
        let (_dir, index) = studies();
        let slow = SlowSource::new();
        let mut session = Session::with_source(index, 1 << 30, None, slow.clone());
        session.select_series(AXIAL);
        session.set_shown(&[AXIAL]);
        session.deselect();
        assert_eq!(session.active_series, None);
        assert!(matches!(session.state(AXIAL), Some(SeriesState::Loading(l)) if !l.is_paused()));
        session.set_shown(&[]);
        assert!(matches!(session.state(AXIAL), Some(SeriesState::Loading(l)) if l.is_paused()));
        slow.allow_all();
    }
}
