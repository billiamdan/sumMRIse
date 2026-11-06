//! The series drawn over the displayed one: a PET over a CT or MR of the same
//! Frame of Reference (`crate::pet`).
//!
//! The session keeps one such series in addition to the displayed one. It
//! loads like any series — its own progressive loader, within the memory
//! budget — and while it is the overlay it is neither paused nor evicted.
//! The rules for the displayed series do not change: when another series is
//! selected, the one left is paused as before. Switched off, the overlay is
//! paused if it is still loading (unless it is the displayed series) and can
//! be evicted again. Before anything is loaded, the headers tell whether its
//! images form a volume, and an estimate whether it fits into the memory
//! budget together with the displayed series.

use std::sync::Arc;

use summrise_dicom::Series;
use summrise_dicom::format::number;
use summrise_i18n::tr;
use summrise_imaging::VolumeError;

use super::{SeriesKey, SeriesState, Session};
use crate::pet;
use crate::series::SeriesData;

/// Why the overlay cannot be drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayProblem {
    /// Its images do not form a volume.
    NoVolume(VolumeError),
    /// The displayed series and the overlay need more memory than the budget.
    TooLarge { needed_bytes: usize, budget_bytes: usize },
    /// None of its images could be decoded.
    Failed(String),
}

impl OverlayProblem {
    /// Why, in a few words, for a badge on the picture.
    pub fn summary(&self) -> &'static str {
        match self {
            Self::NoVolume(_) => tr!("no regular volume"),
            Self::TooLarge { .. } => tr!("not enough memory"),
            Self::Failed(_) => tr!("could not be loaded"),
        }
    }

    /// Why, as one sentence.
    pub fn reason(&self) -> String {
        match self {
            Self::NoVolume(issue) => tr!("The PET series does not form a volume: {issue}.", issue = issue),
            Self::TooLarge { needed_bytes, budget_bytes } => {
                let megabytes = |bytes: usize| number(bytes as f64 / f64::from(1u32 << 20), 1);
                tr!(
                    "Together with the displayed series it needs about {needed} MB; the memory budget is {budget} MB.",
                    needed = megabytes(*needed_bytes),
                    budget = megabytes(*budget_bytes)
                )
            }
            Self::Failed(reason) => tr!("It could not be loaded: {reason}", reason = reason),
        }
    }
}

/// How far the overlay is.
#[derive(Debug, Clone)]
pub enum OverlayStatus {
    /// Its images are being loaded: `loaded` of `total`.
    Loading { loaded: usize, total: usize },
    /// Complete, with a volume to draw.
    Ready(Arc<SeriesData>),
    /// It cannot be drawn.
    Unavailable(OverlayProblem),
}

impl Session {
    /// The PET series of the same study that can be drawn over `base`.
    pub fn overlay_candidates(&self, base: SeriesKey) -> Vec<SeriesKey> {
        self.partners(base, pet::can_fuse)
    }

    /// The CT and MR series of the same study that the PET `overlay` can be
    /// drawn over.
    pub fn underlay_candidates(&self, overlay: SeriesKey) -> Vec<SeriesKey> {
        self.partners(overlay, |pet_series, other| pet::can_fuse(other, pet_series))
    }

    fn partners(&self, key: SeriesKey, fits: impl Fn(&Series, &Series) -> bool) -> Vec<SeriesKey> {
        let (Some(series), Some(study)) = (self.series(key), self.index.studies.get(key.study)) else {
            return Vec::new();
        };
        (0..study.series.len())
            .map(|index| SeriesKey { study: key.study, series: index })
            .filter(|other| *other != key && fits(series, &study.series[other.series]))
            .collect()
    }

    /// The series drawn over the displayed one.
    pub fn overlay(&self) -> Option<SeriesKey> {
        self.overlay
    }

    /// Makes `key` the series drawn over the displayed one (`None`: no
    /// overlay). Cheap when nothing changes, so it can be called every frame.
    pub fn set_overlay(&mut self, key: Option<SeriesKey>) {
        let key = key.filter(|k| self.series(*k).is_some() && Some(*k) != self.active_series);
        if key == self.overlay {
            return;
        }
        if let Some(previous) = self.overlay.take()
            && Some(previous) != self.active_series
            && let Some(SeriesState::Loading(loading)) = self.series.get(&previous)
        {
            loading.loader.pause();
        }
        self.overlay = key;
        self.overlay_problem = None;
        let Some(key) = key else { return };
        self.overlay_problem = self.overlay_blocker(key);
        if self.overlay_problem.is_some() {
            return;
        }
        match self.series.get(&key) {
            Some(SeriesState::Loading(loading)) => loading.loader.resume(),
            Some(SeriesState::Ready(_)) => {}
            None | Some(SeriesState::Failed(_)) => self.start_loading(key),
        }
        self.touch(key);
    }

    /// What the headers already tell against drawing `key`: its images do
    /// not form a volume, or it does not fit into the budget together with
    /// the displayed series.
    fn overlay_blocker(&self, key: SeriesKey) -> Option<OverlayProblem> {
        let series = self.series(key)?;
        if let Some(issue) = summrise_dicom::plan_series(series).volume_issue {
            return Some(OverlayProblem::NoVolume(issue));
        }
        // What is loaded completely has its true size; the rest is estimated from the headers.
        let size = |key: SeriesKey| match self.data(key).filter(|data| data.is_complete()) {
            Some(data) => data.byte_size(),
            None => self.series(key).map_or(0, pet::estimated_bytes),
        };
        let needed_bytes = size(key) + self.active_series.map_or(0, size);
        (needed_bytes > self.budget_bytes)
            .then_some(OverlayProblem::TooLarge { needed_bytes, budget_bytes: self.budget_bytes })
    }

    /// How far the overlay is, if there is one.
    pub fn overlay_status(&self) -> Option<OverlayStatus> {
        let key = self.overlay?;
        if let Some(problem) = &self.overlay_problem {
            return Some(OverlayStatus::Unavailable(problem.clone()));
        }
        let total = self.series(key).map_or(0, Series::frame_count);
        Some(match self.series.get(&key) {
            Some(SeriesState::Loading(loading)) => match loading.data.volume_issue() {
                Some(issue) => OverlayStatus::Unavailable(OverlayProblem::NoVolume(issue.clone())),
                None => OverlayStatus::Loading { loaded: loading.data.loaded(), total: loading.data.len() },
            },
            Some(SeriesState::Ready(data)) => match (data.volume(), data.volume_issue()) {
                (Some(_), _) => OverlayStatus::Ready(data.clone()),
                (None, issue) => OverlayStatus::Unavailable(OverlayProblem::NoVolume(
                    issue.cloned().unwrap_or(VolumeError::InconsistentGrid),
                )),
            },
            Some(SeriesState::Failed(reason)) => OverlayStatus::Unavailable(OverlayProblem::Failed(reason.clone())),
            None => OverlayStatus::Loading { loaded: 0, total },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use summrise_dicom::{Progress, StudyIndex, scan};
    use summrise_testkit::pet::{self as phantom, PetStudyOptions, write_pet_study};

    use super::*;
    use crate::session::SessionEvent;

    const CT: SeriesKey = SeriesKey { study: 0, series: phantom::CT };
    const PET: SeriesKey = SeriesKey { study: 0, series: phantom::PET };
    const NAC: SeriesKey = SeriesKey { study: 0, series: phantom::PET_NAC };

    fn pet_study(options: &PetStudyOptions) -> (tempfile::TempDir, StudyIndex) {
        let dir = tempfile::tempdir().expect("tempdir");
        write_pet_study(dir.path(), options).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        (dir, index)
    }

    /// Polls the session until `done`.
    fn wait(session: &mut Session, what: &str, done: impl Fn(&Session) -> bool) -> Vec<SessionEvent> {
        let start = Instant::now();
        let mut events = Vec::new();
        loop {
            events.extend(session.poll().into_iter().filter(|e| !matches!(e, SessionEvent::ThumbnailReady(_))));
            if done(session) {
                return events;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn ready(session: &Session) -> bool {
        matches!(session.overlay_status(), Some(OverlayStatus::Ready(_)))
    }

    /// The session on the PET study with the CT displayed (the PET series,
    /// which have more images, would open by default).
    fn on_the_ct(index: StudyIndex, budget_bytes: usize) -> Session {
        let mut session = Session::new(index, budget_bytes, None);
        session.select_series(CT);
        session
    }

    #[test]
    fn candidates_share_the_frame_of_reference() {
        let (_dir, index) =
            pet_study(&PetStudyOptions { other_frame_of_reference: true, ..PetStudyOptions::default() });
        let session = on_the_ct(index, 1 << 30);
        // Both PET series of the CT's Frame of Reference, not the one elsewhere.
        assert_eq!(session.overlay_candidates(CT), [PET, NAC]);
        assert_eq!(session.underlay_candidates(PET), [CT]);
        assert_eq!(session.underlay_candidates(SeriesKey { study: 0, series: phantom::PET_ELSEWHERE }), []);
        assert_eq!(session.overlay_candidates(PET), [], "nothing is drawn over a PET");
        assert_eq!(session.overlay_candidates(SeriesKey { study: 3, series: 0 }), []);
    }

    #[test]
    fn the_overlay_loads_and_is_not_evicted_while_it_is_in_use() {
        let (_dir, index) = pet_study(&PetStudyOptions::default());
        let mut session = on_the_ct(index, 1 << 30);
        wait(&mut session, "the CT", |s| s.loaded(CT).is_some());
        assert!(session.overlay_status().is_none());

        session.set_overlay(Some(PET));
        assert_eq!(session.overlay(), Some(PET));
        assert!(matches!(session.overlay_status(), Some(OverlayStatus::Loading { total: 50, .. })));
        let events = wait(&mut session, "the PET", ready);
        assert!(events.contains(&SessionEvent::SeriesReady(PET)));
        let Some(OverlayStatus::Ready(pet)) = session.overlay_status() else { panic!("ready") };
        assert_eq!(pet.units(), Some("SUV"));
        assert_eq!(session.active_series, Some(CT), "the CT is still the displayed series");

        // The budget shrinks to nothing: neither the displayed series nor the overlay goes.
        session.set_budget(1);
        session.poll();
        assert!(session.loaded(CT).is_some() && session.loaded(PET).is_some());
        // Calling it again (every frame) changes nothing.
        session.set_overlay(Some(PET));
        assert!(ready(&session));

        // Switched off, the PET is evicted like any series that is not displayed.
        session.set_overlay(None);
        assert!(session.overlay_status().is_none());
        session.poll();
        assert!(session.state(PET).is_none(), "released and evicted");
        assert!(session.loaded(CT).is_some());
    }

    #[test]
    fn switching_the_overlay_pauses_the_one_left_and_selecting_keeps_the_rules() {
        let (_dir, index) = pet_study(&PetStudyOptions::default());
        let mut session = on_the_ct(index, 1 << 30);
        session.set_overlay(Some(PET));
        assert!(matches!(session.state(PET), Some(SeriesState::Loading(l)) if !l.is_paused()));
        // Another PET: the first one is paused where it was.
        session.set_overlay(Some(NAC));
        if let Some(SeriesState::Loading(loading)) = session.state(PET) {
            assert!(loading.is_paused(), "the overlay left is paused");
        }
        // The overlay cannot be the displayed series itself.
        session.select_series(NAC);
        assert_eq!(session.active_series, Some(NAC));
        session.set_overlay(Some(NAC));
        assert_eq!(session.overlay(), None);
        // The displayed series is not paused when it stops being the overlay.
        session.set_overlay(None);
        if let Some(SeriesState::Loading(loading)) = session.state(NAC) {
            assert!(!loading.is_paused());
        }
        wait(&mut session, "the NAC series", |s| s.loaded(NAC).is_some());
    }

    #[test]
    fn an_overlay_without_a_volume_or_beyond_the_budget_is_unavailable_and_not_loaded() {
        let (_dir, index) = pet_study(&PetStudyOptions { missing_slice: true, ..PetStudyOptions::default() });
        let mut session = on_the_ct(index, 1 << 30);
        session.set_overlay(Some(PET));
        let Some(OverlayStatus::Unavailable(problem)) = session.overlay_status() else { panic!("unavailable") };
        assert!(matches!(problem, OverlayProblem::NoVolume(VolumeError::IrregularSpacing { .. })), "{problem:?}");
        assert!(problem.reason().starts_with("The PET series does not form a volume: slices are not evenly spaced"));
        assert_eq!(problem.summary(), "no regular volume");
        assert!(session.state(PET).is_none(), "nothing is loaded");

        // The CT (110.6 kB) and the PET (720 kB) do not fit into 512 kB.
        let (_dir, index) = pet_study(&PetStudyOptions::default());
        let mut session = on_the_ct(index, 512 << 10);
        session.set_overlay(Some(PET));
        let Some(OverlayStatus::Unavailable(problem)) = session.overlay_status() else { panic!("unavailable") };
        assert_eq!(
            problem,
            OverlayProblem::TooLarge { needed_bytes: 48 * 48 * 24 * 2 + 60 * 60 * 50 * 4, budget_bytes: 512 << 10 }
        );
        assert_eq!(
            problem.reason(),
            "Together with the displayed series it needs about 0.8 MB; the memory budget is 0.5 MB."
        );
        assert_eq!(problem.summary(), "not enough memory");
        assert!(session.state(PET).is_none());
        // Switched off and on with a larger budget, it loads.
        session.set_overlay(None);
        session.set_budget(1 << 30);
        session.set_overlay(Some(PET));
        wait(&mut session, "the PET", ready);
    }
}
