//! Cine: the images of a view, or the time points of a series, played as a
//! moving loop, and the playback bar under the images.
//!
//! The state belongs to the viewer and is never saved. It advances on the UI
//! clock (`InputState::time`, which egui_kittest drives in tests): at most one
//! step per frame when a step is due, and the next repaint is asked for with
//! `request_repaint_after` — no thread. Playback waits at an acquired image
//! that is still loading, and stops when another series is shown
//! (`openspec/changes/multi-phase-cine/design.md`, D8 and D9).

use std::time::Duration;

use egui::{Button, DragValue, RichText, Ui};
use egui_material_icons::icons::{ICON_COMPARE_ARROWS, ICON_PAUSE, ICON_PLAY_ARROW, ICON_REPEAT};
use summrise_core::dicom::Series;
use summrise_core::dicom::format::number;
use summrise_core::{ImageSlot, SeriesData, SeriesKey, ViewKind, ViewState};
use summrise_i18n::tr;

use crate::theme;
use crate::widgets::{icon, keep_together};

/// Images per second when the images record no rate.
pub(crate) const DEFAULT_RATE: f64 = 10.0;
/// The speeds that can be chosen, in images per second.
pub(crate) const MIN_RATE: f64 = 1.0;
pub(crate) const MAX_RATE: f64 = 60.0;
/// How often to look again while playback waits for an image to load.
const WAIT_POLL: Duration = Duration::from_millis(30);

/// How playback goes on after the last image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Mode {
    /// … last, first, second …
    #[default]
    Loop,
    /// First to last, then back to the first, without showing the end images twice.
    BackAndForth,
}

/// What playback steps through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// The images of a view: the acquired images of the stack shown, or the
    /// slices of a reformat.
    Images(ViewKind),
    /// The stacks (time points, heart phases) at the current position, in every view.
    Stacks,
}

impl Target {
    /// What Play plays for `series` with the `view` active: the stacks of a
    /// series whose stacks are time points or heart phases, else the images
    /// of the view.
    pub(crate) fn of(series: &SeriesData, view: ViewKind) -> Self {
        if series.stacks().is_some_and(|s| s.kind.is_time_like()) { Self::Stacks } else { Self::Images(view) }
    }

    /// Number of steps: stacks, or images of the view.
    fn count(self, series: &SeriesData, state: &ViewState) -> usize {
        match self {
            Self::Stacks => series.stack_count(),
            Self::Images(kind) => state.slice_count(kind, series),
        }
    }

    fn current(self, series: &SeriesData, state: &ViewState) -> usize {
        match self {
            Self::Stacks => state.stack(),
            Self::Images(kind) => state.slice_index(kind, series),
        }
    }

    /// The acquired image (display index) that step `next` shows, if any:
    /// reformats exist only once every image is loaded.
    fn acquired_slot(self, next: usize, series: &SeriesData, state: &ViewState) -> Option<usize> {
        let image = state.slice_index(ViewKind::Stack, series);
        match self {
            Self::Stacks => Some(next * series.stack_len() + image),
            Self::Images(ViewKind::Stack) => Some(state.stack() * series.stack_len() + next),
            Self::Images(_) => None,
        }
    }

    fn show(self, next: usize, series: &SeriesData, state: &mut ViewState) {
        match self {
            Self::Stacks => state.set_stack(series, next),
            Self::Images(kind) => state.set_slice(kind, series, next),
        }
    }
}

/// The step after `current` of `count` (direction `forward`), and the
/// direction after it.
pub(crate) fn step(current: usize, count: usize, mode: Mode, forward: bool) -> (usize, bool) {
    if count < 2 {
        return (0, true);
    }
    let current = current.min(count - 1);
    match mode {
        Mode::Loop => ((current + 1) % count, true),
        Mode::BackAndForth if forward && current + 1 < count => (current + 1, true),
        Mode::BackAndForth if forward => (current - 1, false),
        Mode::BackAndForth if current > 0 => (current - 1, false),
        Mode::BackAndForth => (1, true),
    }
}

/// The speed the images record (Recommended Display Frame Rate, Cine Rate or
/// Frame Time), within the speeds that can be chosen.
pub(crate) fn recorded_rate(meta: &Series) -> Option<f64> {
    meta.acquisition.display_frame_rate().map(|rate| rate.clamp(MIN_RATE, MAX_RATE))
}

#[derive(Debug, Clone, Copy)]
struct Playing {
    series: SeriesKey,
    target: Target,
    /// UI time (s) of the last step, or of the start.
    last: f64,
    forward: bool,
    /// The acquired image (display index) playback waits for while it loads.
    waiting: Option<usize>,
}

/// The playback state of the viewer.
#[derive(Debug, Default)]
pub(crate) struct Cine {
    playing: Option<Playing>,
    /// The speed the user chose, and for which series.
    rate: Option<(SeriesKey, f64)>,
    pub(crate) mode: Mode,
}

impl Cine {
    pub(crate) fn is_playing(&self) -> bool {
        self.playing.is_some()
    }

    /// What is being played, if anything.
    pub(crate) fn target(&self) -> Option<Target> {
        self.playing.map(|p| p.target)
    }

    /// Whether playback waits for an image that is still loading.
    pub(crate) fn is_waiting(&self) -> bool {
        self.playing.is_some_and(|p| p.waiting.is_some())
    }

    /// The image (display index) playback of `series` waits for, so that the
    /// loader reads it next.
    pub(crate) fn awaited(&self, series: SeriesKey) -> Option<usize> {
        self.playing.filter(|p| p.series == series).and_then(|p| p.waiting)
    }

    /// Whether there is anything to play for `series` with `view` active.
    pub(crate) fn can_play(series: &SeriesData, state: &ViewState, view: ViewKind) -> bool {
        Target::of(series, view).count(series, state) > 1
    }

    /// Starts playing at UI time `now`, or stops (Space, the Play button).
    pub(crate) fn toggle(&mut self, key: SeriesKey, series: &SeriesData, state: &ViewState, view: ViewKind, now: f64) {
        if self.playing.is_some() {
            self.stop();
        } else if Self::can_play(series, state, view) {
            let target = Target::of(series, view);
            self.playing = Some(Playing { series: key, target, last: now, forward: true, waiting: None });
        }
    }

    pub(crate) fn stop(&mut self) {
        self.playing = None;
    }

    /// The speed for `key` in images per second: the one the user chose,
    /// else the one the images record, else [`DEFAULT_RATE`].
    pub(crate) fn rate(&self, key: SeriesKey, meta: &Series) -> f64 {
        match self.rate {
            Some((series, rate)) if series == key => rate,
            _ => recorded_rate(meta).unwrap_or(DEFAULT_RATE),
        }
    }

    pub(crate) fn set_rate(&mut self, key: SeriesKey, rate: f64) {
        self.rate = Some((key, rate.clamp(MIN_RATE, MAX_RATE)));
    }

    /// Advances playback at UI time `now` when the next step is due; `key`
    /// is the series shown and `views` the views on screen. Returns when to
    /// look again, or `None` when nothing is playing. Playback stops when
    /// another series is shown or the view it plays has left the screen; the
    /// speed chosen by the user is forgotten when another series is shown.
    pub(crate) fn tick(
        &mut self,
        now: f64,
        key: SeriesKey,
        views: &[ViewKind],
        series: &SeriesData,
        state: &mut ViewState,
        rate: f64,
    ) -> Option<Duration> {
        if self.rate.is_some_and(|(series, _)| series != key) {
            self.rate = None;
        }
        let mode = self.mode;
        let playing = self.playing.as_mut()?;
        let gone = matches!(playing.target, Target::Images(view) if !views.contains(&view));
        let count = playing.target.count(series, state);
        if playing.series != key || gone || count < 2 {
            self.playing = None;
            return None;
        }
        let period = 1.0 / rate.clamp(MIN_RATE, MAX_RATE);
        let due = playing.last + period;
        if now < due {
            return Some(Duration::from_secs_f64(due - now));
        }
        let current = playing.target.current(series, state);
        let (next, forward) = step(current, count, mode, playing.forward);
        // An acquired image that is still loading is waited for, never skipped.
        if let Some(slot) = playing.target.acquired_slot(next, series, state)
            && matches!(series.image(slot), ImageSlot::Pending)
        {
            playing.waiting = Some(slot);
            return Some(WAIT_POLL);
        }
        playing.waiting = None;
        playing.target.show(next, series, state);
        playing.forward = forward;
        // Keep the rhythm, but do not catch up after a wait or a slow frame.
        playing.last = if now - due < period { due } else { now };
        Some(Duration::from_secs_f64((playing.last + period - now).max(0.0)))
    }
}

/// The playback bar under the images: the stack control of a series split
/// into stacks, Play/Pause, the speed and how to go on after the last image.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bar(
    ui: &mut Ui,
    cine: &mut Cine,
    key: SeriesKey,
    meta: &Series,
    series: &SeriesData,
    state: &mut ViewState,
    view: ViewKind,
    now: f64,
) {
    let p = theme::palette(ui);
    // Wraps onto another row in a narrow view, each label with its control.
    ui.horizontal_wrapped(|ui| {
        if series.stack_count() > 1 {
            keep_together(ui, "image set control", |ui| crate::stacks::control(ui, series, state));
            ui.add_space(8.0);
        }
        let target = cine.target().unwrap_or_else(|| Target::of(series, view));
        let playable = Cine::can_play(series, state, view) || cine.is_playing();
        let (glyph, name) = if cine.is_playing() { (ICON_PAUSE, tr!("Pause")) } else { (ICON_PLAY_ARROW, tr!("Play")) };
        let hint = match (cine.is_playing(), target, series.stacks().map(|s| s.kind)) {
            (true, ..) => tr!("Stop playing (Space)"),
            (false, Target::Stacks, Some(summrise_core::dicom::StackKind::HeartPhase)) => {
                tr!("Play the heart phases at this position as a loop (Space)")
            }
            (false, Target::Stacks, _) => tr!("Play the time points at this position as a loop (Space)"),
            (false, Target::Images(_), _) => tr!("Play the images of the active view as a loop (Space)"),
        };
        let play = ui
            .add_enabled(playable, Button::new(format!("{} {name}", icon(glyph))).selected(cine.is_playing()))
            .accessible_name(name)
            .on_hover_text(hint)
            .on_disabled_hover_text(tr!("This view shows a single image"));
        if play.clicked() {
            cine.toggle(key, series, state, view, now);
        }

        keep_together(ui, "playback speed", |ui| {
            // Translators: the speed of playback, in images per second (" / s" follows the number).
            let speed = ui.label(RichText::new(tr!("Speed")).small());
            let recorded = recorded_rate(meta);
            let speed_hint = match recorded {
                Some(rate) => {
                    tr!("Images per second, from 1 to 60. The images record {rate} per second.", rate = number(rate, 1))
                }
                None => tr!(
                    "Images per second, from 1 to 60. The images record no speed; {rate} per second is used.",
                    rate = number(DEFAULT_RATE, 0)
                ),
            };
            let mut rate = cine.rate(key, meta);
            let field = DragValue::new(&mut rate)
                .range(MIN_RATE..=MAX_RATE)
                .speed(0.2)
                .custom_formatter(|value, _| number(value, 1))
                .suffix(tr!(" / s"));
            if ui.add(field).labelled_by(speed.id).on_hover_text(speed_hint).changed() {
                cine.set_rate(key, rate);
            }
        });

        // Icons, as on media players: the bar has to fit beside the browser and the inspector.
        let modes = [
            // Translators: the ways of playback: again from the first image, or back and forth.
            (Mode::Loop, ICON_REPEAT, tr!("Loop"), tr!("Loop: after the last image, start again at the first")),
            (
                Mode::BackAndForth,
                ICON_COMPARE_ARROWS,
                tr!("Back and forth"),
                tr!("Back and forth: play to the last image, then back to the first"),
            ),
        ];
        for (mode, glyph, name, hint) in modes {
            if ui.selectable_label(cine.mode == mode, icon(glyph)).accessible_name(name).on_hover_text(hint).clicked() {
                cine.mode = mode;
            }
        }
        if cine.is_waiting() {
            ui.add(egui::Spinner::new().size(12.0));
            let waiting = RichText::new(tr!("Waiting for images to load")).small().color(p.text_weak);
            ui.add(egui::Label::new(waiting).truncate());
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    use summrise_core::dicom::{DecodedInstance, DicomError, InstanceHeader, Media, Progress, scan};
    use summrise_core::imaging::MprAxis;
    use summrise_core::{DicomFiles, ImageSource, Session};
    use summrise_testkit::stacks::{self as phantoms, write_stacks_study};

    use super::*;

    /// Holds back the images after the first time point (instance numbers
    /// above 12) until it is opened; the other files are read at once.
    #[derive(Default)]
    struct FirstTimePointOnly {
        open: AtomicBool,
    }

    impl ImageSource for FirstTimePointOnly {
        fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError> {
            while header.instance_number.is_some_and(|n| n > 12) && !self.open.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            DicomFiles.read(media, header)
        }

        fn decode(&self, header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError> {
            DicomFiles.decode(header, bytes)
        }
    }

    /// Series `which` of the stacks study, loaded through `source` as far as
    /// `ready` says.
    struct Opened {
        session: Session,
        key: SeriesKey,
        data: Arc<SeriesData>,
        _dir: tempfile::TempDir,
    }

    fn open(which: usize, source: Arc<dyn ImageSource>, ready: impl Fn(&SeriesData) -> bool) -> Opened {
        let dir = tempfile::tempdir().expect("tempdir");
        write_stacks_study(dir.path()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        let mut session = Session::with_source(index, 1 << 30, None, source);
        let key = SeriesKey { study: 0, series: which };
        session.select_series(key);
        let start = Instant::now();
        loop {
            session.poll();
            if let Some(data) = session.data(key).filter(|data| ready(data)) {
                return Opened { session, key, data, _dir: dir };
            }
            assert!(start.elapsed() < Duration::from_secs(30), "timed out loading series {which}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn complete(which: usize) -> Opened {
        open(which, Arc::new(DicomFiles), SeriesData::is_complete)
    }

    impl Opened {
        fn meta(&self) -> &Series {
            self.session.series(self.key).expect("indexed")
        }
    }

    /// Ticks every `dt` seconds of UI time until `seconds` (and 10 ms more),
    /// collecting the stack or image shown after every step.
    fn run(cine: &mut Cine, o: &Opened, state: &mut ViewState, rate: f64, dt: f64, seconds: f64) -> Vec<usize> {
        let mut shown = Vec::new();
        let ticks = ((seconds + 0.01) / dt).floor() as u32;
        for i in 1..=ticks {
            let before = (state.stack(), state.slice);
            cine.tick(f64::from(i) * dt, o.key, &[ViewKind::Stack], &o.data, state, rate);
            if (state.stack(), state.slice) != before {
                shown.push(if cine.target() == Some(Target::Stacks) { state.stack() } else { state.slice });
            }
        }
        shown
    }

    #[test]
    fn steps_loop_or_go_back_and_forth() {
        let walk = |mode| {
            let (mut at, mut forward, mut out) = (0, true, vec![1]);
            for _ in 0..7 {
                (at, forward) = step(at, 4, mode, forward);
                out.push(at + 1);
            }
            out
        };
        assert_eq!(walk(Mode::Loop), [1, 2, 3, 4, 1, 2, 3, 4]);
        assert_eq!(walk(Mode::BackAndForth), [1, 2, 3, 4, 3, 2, 1, 2], "the end images are not shown twice");
        assert_eq!(step(5, 4, Mode::Loop, true), (0, true), "a position beyond the end counts as the last");
        assert_eq!(step(0, 1, Mode::BackAndForth, true), (0, true), "nothing to play");
    }

    /// Task 4.3 — 25 images per second for Frame Time 40 ms, with the injected clock.
    #[test]
    fn frame_time_of_40_ms_plays_25_images_per_second() {
        let o = complete(phantoms::CINE);
        assert_eq!(recorded_rate(o.meta()), Some(25.0));
        let mut state = ViewState::new(&o.data);
        state.set_slice(ViewKind::Stack, &o.data, 0);
        let mut cine = Cine::default();
        let rate = cine.rate(o.key, o.meta());
        assert_eq!(rate, 25.0);
        cine.toggle(o.key, &o.data, &state, ViewKind::Stack, 0.0);
        assert_eq!(cine.target(), Some(Target::Images(ViewKind::Stack)), "a cine of one slice plays its images");
        // Nothing before 40 ms.
        assert!(cine.tick(0.039, o.key, &[ViewKind::Stack], &o.data, &mut state, rate).is_some());
        assert_eq!(state.slice, 0);
        let shown = run(&mut cine, &o, &mut state, rate, 0.001, 1.0);
        assert_eq!(shown.len(), 25, "one second at 25 per second");
        assert_eq!(shown[..3], [1, 2, 3]);
        assert_eq!(shown[18..21], [19, 0, 1], "image 20 is followed by image 1");
        // The speed the user sets is kept for the series, within 1 to 60.
        let other = SeriesKey { study: 0, series: phantoms::DYNAMIC };
        cine.set_rate(o.key, 100.0);
        assert_eq!(cine.rate(o.key, o.meta()), 60.0);
        cine.set_rate(o.key, 0.0);
        assert_eq!(cine.rate(o.key, o.meta()), 1.0);
        assert_eq!(cine.rate(other, o.meta()), 25.0, "not for another series");
        // A late frame is not caught up: one step only.
        let mut late = Cine::default();
        late.toggle(o.key, &o.data, &state, ViewKind::Stack, 0.0);
        let before = state.slice;
        late.tick(1.0, o.key, &[ViewKind::Stack], &o.data, &mut state, 25.0);
        assert_eq!(state.slice, (before + 1) % 20);
    }

    /// Task 4.3 — through the time points at one position, and back and forth.
    #[test]
    fn time_points_are_played_at_the_current_position() {
        let o = complete(phantoms::DYNAMIC);
        let mut state = ViewState::new(&o.data);
        let mut cine = Cine::default();
        cine.toggle(o.key, &o.data, &state, ViewKind::Mpr(MprAxis::Axial), 0.0);
        assert_eq!(cine.target(), Some(Target::Stacks), "time-like stacks are played whatever the view");
        let shown = run(&mut cine, &o, &mut state, 10.0, 0.01, 0.4);
        assert_eq!(shown, [1, 2, 0, 1], "time points 2, 3, 1, 2");
        assert_eq!(state.slice, 6, "at the same image");
        cine.stop();
        state.set_stack(&o.data, 0);
        cine.mode = Mode::BackAndForth;
        cine.toggle(o.key, &o.data, &state, ViewKind::Stack, 0.0);
        assert_eq!(run(&mut cine, &o, &mut state, 10.0, 0.01, 0.6), [1, 2, 1, 0, 1, 2]);
    }

    /// Task 4.3 — playback waits at an image that is still loading, and
    /// names the image it waits for.
    #[test]
    fn playback_waits_for_an_image_that_is_loading() {
        let gate = Arc::new(FirstTimePointOnly::default());
        let mut o = open(phantoms::DYNAMIC, gate.clone(), |data| data.frame(6).is_some() && data.frame(7).is_some());
        let mut state = ViewState::new(&o.data);
        let mut cine = Cine::default();
        cine.toggle(o.key, &o.data, &state, ViewKind::Stack, 0.0);
        for t in 1..20 {
            cine.tick(f64::from(t) * 0.05, o.key, &[ViewKind::Stack], &o.data, &mut state, 10.0);
        }
        assert_eq!(state.stack(), 0, "image 7 of time point 2 is not loaded");
        assert!(cine.is_waiting());
        assert_eq!(cine.awaited(o.key), Some(12 + 6));
        assert_eq!(cine.awaited(SeriesKey { study: 0, series: 9 }), None);
        // The image arrives: playback goes on with it.
        gate.open.store(true, Ordering::SeqCst);
        let start = Instant::now();
        while o.data.frame(18).is_none() {
            o.session.poll();
            assert!(start.elapsed() < Duration::from_secs(30), "timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        cine.tick(1.0, o.key, &[ViewKind::Stack], &o.data, &mut state, 10.0);
        assert_eq!(state.stack(), 1);
        assert!(!cine.is_waiting());
    }

    /// Task 4.3 — playback stops when another series is shown or the view it
    /// plays leaves the screen; the speed chosen is forgotten with the series.
    #[test]
    fn playback_stops_when_the_series_or_the_view_goes() {
        let o = complete(phantoms::DUAL_ECHO);
        let mut state = ViewState::new(&o.data);
        let mut cine = Cine::default();
        cine.toggle(o.key, &o.data, &state, ViewKind::Stack, 0.0);
        assert_eq!(cine.target(), Some(Target::Images(ViewKind::Stack)), "echoes: the images of the view");
        cine.set_rate(o.key, 5.0);
        let other = SeriesKey { study: 0, series: phantoms::DYNAMIC };
        assert_eq!(cine.tick(0.1, other, &[ViewKind::Stack], &o.data, &mut state, 5.0), None);
        assert!(!cine.is_playing());
        assert_eq!(cine.rate(o.key, o.meta()), DEFAULT_RATE, "the speed chosen is forgotten");

        let axial = ViewKind::Mpr(MprAxis::Axial);
        cine.toggle(o.key, &o.data, &state, axial, 0.0);
        assert!(cine.is_playing());
        cine.tick(0.05, o.key, &[ViewKind::Stack], &o.data, &mut state, 5.0);
        assert!(!cine.is_playing(), "the axial view is no longer shown");
        // Toggling twice starts and stops.
        cine.toggle(o.key, &o.data, &state, ViewKind::Stack, 0.0);
        cine.toggle(o.key, &o.data, &state, ViewKind::Stack, 0.0);
        assert!(!cine.is_playing());
    }
}
