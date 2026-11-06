//! The pictures of the 3D view: a quick preview whenever what is to be shown
//! changes, the full picture from a background job as soon as it stays the
//! same for a frame. The latest request wins: a job that is no longer wanted
//! is cancelled, and its picture is never shown.
//!
//! [`Progressive`] is the decision logic alone, without threads or clocks, so
//! that it can be tested step by step. [`VolumePictures`] runs it: previews
//! on the calling thread (they are small and quick), full pictures on a
//! [`Job`]. The calling thread never waits for a full picture.

use std::sync::Arc;
use std::time::{Duration, Instant};

use summrise_imaging::Volume;
use summrise_imaging::render3d::{Camera, Quality, RenderVolume, Rendering, Style, render};

use crate::jobs::{Job, Waker};

/// Longest side of a full picture, in pixels: larger viewports show it
/// magnified.
pub const FULL_LONGEST_SIDE: usize = 1024;

/// Longest side of a preview: it starts here and adapts to the time previews
/// take on this computer (see [`PreviewSize`]).
const PREVIEW_START: usize = 256;
const PREVIEW_SMALLEST: usize = 96;
const PREVIEW_LARGEST: usize = 384;
/// Previews should take about this long, so that turning stays smooth.
const PREVIEW_BUDGET: Duration = Duration::from_millis(12);

/// What a picture shows: everything that changes its pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct PictureRequest {
    pub camera: Camera,
    pub style: Style,
    /// Size of the full picture (columns, rows); a preview is smaller.
    pub size: [usize; 2],
}

impl PictureRequest {
    /// A request for a viewport of `pixels` (width, height) physical pixels:
    /// the full picture has the viewport's size, its longest side capped at
    /// [`FULL_LONGEST_SIDE`].
    pub fn new(camera: Camera, style: Style, pixels: [f32; 2]) -> Self {
        Self { camera, style, size: scaled(pixels, FULL_LONGEST_SIDE) }
    }
}

/// `pixels` scaled down (never up) so that its longest side is at most
/// `longest`, with the aspect kept; at least one pixel each way.
fn scaled(pixels: [f32; 2], longest: usize) -> [usize; 2] {
    let [w, h] = pixels.map(|p| if p.is_finite() { p.max(1.0) } else { 1.0 });
    let factor = (longest as f32 / w.max(h)).min(1.0);
    [(w * factor).round().max(1.0) as usize, (h * factor).round().max(1.0) as usize]
}

/// The number of a picture asked for; later requests have larger numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticket(u64);

/// A picture on screen, and what it shows.
#[derive(Debug, Clone)]
pub struct Shown<P> {
    pub ticket: Ticket,
    pub request: PictureRequest,
    pub quality: Quality,
    pub picture: P,
}

/// What to do in a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing: the full picture is shown or on its way.
    Wait,
    /// Render a preview now and deliver it with this ticket.
    Preview(Ticket),
    /// Start the full picture in the background and deliver it with this
    /// ticket when it is done.
    Full(Ticket),
}

/// Decides, frame by frame, which picture to render and which to show.
///
/// - A changed request gets a preview at once; a full picture in progress is
///   no longer wanted.
/// - A request that stays the same for a frame gets the full picture, once.
/// - A delivered picture replaces the one shown only if it was asked for the
///   current request and is newer than the one shown; a full picture only if
///   it is the one in progress. So a stale full picture never replaces a
///   newer preview.
#[derive(Debug)]
pub struct Progressive<P> {
    request: Option<PictureRequest>,
    /// The first ticket issued for `request`.
    since: Ticket,
    /// The full picture in progress, for `request`.
    pending: Option<Ticket>,
    shown: Option<Shown<P>>,
    next: u64,
}

impl<P> Default for Progressive<P> {
    fn default() -> Self {
        Self { request: None, since: Ticket(0), pending: None, shown: None, next: 0 }
    }
}

impl<P> Progressive<P> {
    /// Tells what the view needs this frame and returns what to do.
    pub fn update(&mut self, request: &PictureRequest) -> Step {
        if self.request.as_ref() != Some(request) {
            self.request = Some(request.clone());
            self.pending = None;
            let ticket = self.issue();
            self.since = ticket;
            return Step::Preview(ticket);
        }
        let full_shown = self.shown.as_ref().is_some_and(|s| s.quality == Quality::Full && s.ticket >= self.since);
        if full_shown || self.pending.is_some() {
            return Step::Wait;
        }
        let ticket = self.issue();
        self.pending = Some(ticket);
        Step::Full(ticket)
    }

    fn issue(&mut self) -> Ticket {
        self.next += 1;
        Ticket(self.next)
    }

    /// Hands over the picture asked for with `ticket`. Returns whether it is
    /// shown now.
    pub fn deliver(&mut self, ticket: Ticket, quality: Quality, picture: P) -> bool {
        let Some(request) = &self.request else { return false };
        let wanted = match quality {
            Quality::Preview => ticket >= self.since,
            Quality::Full => self.pending == Some(ticket),
        };
        if !wanted || self.shown.as_ref().is_some_and(|s| s.ticket >= ticket) {
            return false;
        }
        if quality == Quality::Full {
            self.pending = None;
        }
        self.shown = Some(Shown { ticket, request: request.clone(), quality, picture });
        true
    }

    /// The full picture that is being rendered, if any.
    pub fn pending(&self) -> Option<Ticket> {
        self.pending
    }

    /// The picture to show.
    pub fn shown(&self) -> Option<&Shown<P>> {
        self.shown.as_ref()
    }

    /// Whether the next frame has work to do even if nothing changes: a
    /// preview is shown and its full picture is not started yet.
    pub fn wants_frame(&self) -> bool {
        self.pending.is_none() && self.shown.as_ref().is_some_and(|s| s.quality == Quality::Preview)
    }
}

/// The size of previews: as large as the time budget allows on this
/// computer. Halved when a preview took more than twice the budget, a step
/// smaller when it took longer than the budget, a step larger when it took
/// less than half of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewSize {
    longest: usize,
}

impl Default for PreviewSize {
    fn default() -> Self {
        Self { longest: PREVIEW_START }
    }
}

impl PreviewSize {
    /// The longest side of the next preview, in pixels.
    pub fn longest_side(&self) -> usize {
        self.longest
    }

    /// The size of the preview of a full picture of `full` pixels.
    pub fn of(&self, full: [usize; 2]) -> [usize; 2] {
        scaled(full.map(|p| p as f32), self.longest)
    }

    /// Learns from a preview that took `elapsed`.
    pub fn record(&mut self, elapsed: Duration) {
        let longest = if elapsed > PREVIEW_BUDGET * 2 {
            self.longest / 2
        } else if elapsed > PREVIEW_BUDGET {
            self.longest * 4 / 5
        } else if elapsed < PREVIEW_BUDGET / 2 {
            self.longest * 5 / 4
        } else {
            self.longest
        };
        self.longest = longest.clamp(PREVIEW_SMALLEST, PREVIEW_LARGEST);
    }
}

/// The pictures of the 3D view of one volume: [`Progressive`] run with
/// previews on the calling thread and full pictures on a background job.
#[derive(Default)]
pub struct VolumePictures {
    prepared: Option<Arc<RenderVolume>>,
    progressive: Progressive<Arc<Rendering>>,
    /// The full picture in progress, and its ticket.
    job: Option<(Ticket, Job<Option<Rendering>>)>,
    preview: PreviewSize,
}

impl std::fmt::Debug for VolumePictures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VolumePictures")
            .field("shown", &self.progressive.shown().map(|s| (s.ticket, s.quality)))
            .field("pending", &self.progressive.pending())
            .field("preview", &self.preview)
            .finish_non_exhaustive()
    }
}

impl VolumePictures {
    /// Brings the picture up to date with `request` for `volume`, rendering a
    /// preview if the request changed, and returns the picture to show.
    /// `waker` is called when a full picture is ready.
    pub fn update(
        &mut self,
        volume: &Volume,
        request: &PictureRequest,
        waker: Option<Waker>,
    ) -> Option<&Shown<Arc<Rendering>>> {
        // The volume is prepared once (shared, not copied); another volume starts afresh.
        let prepared = match &self.prepared {
            Some(prepared) if prepared.is_for(volume) => prepared.clone(),
            _ => {
                self.cancel();
                self.progressive = Progressive::default();
                let prepared = Arc::new(RenderVolume::new(volume));
                self.prepared = Some(prepared.clone());
                prepared
            }
        };
        // A finished full picture is handed over first, so that it is not wasted
        // when the request changes in this very frame.
        if let Some((ticket, job)) = &mut self.job
            && let Some(result) = job.poll()
        {
            if let Some(picture) = result {
                self.progressive.deliver(*ticket, Quality::Full, Arc::new(picture));
            }
            self.job = None;
        }
        let step = self.progressive.update(request);
        // The job in progress is always the one still wanted.
        if self.job.as_ref().is_some_and(|(ticket, _)| self.progressive.pending() != Some(*ticket)) {
            self.cancel();
        }
        match step {
            Step::Wait => {}
            Step::Preview(ticket) => {
                let started = Instant::now();
                let size = self.preview.of(request.size);
                let picture = render(&prepared, &request.camera, &request.style, size, Quality::Preview, &|| false);
                self.preview.record(started.elapsed());
                if let Some(picture) = picture {
                    self.progressive.deliver(ticket, Quality::Preview, Arc::new(picture));
                }
            }
            Step::Full(ticket) => {
                let request = request.clone();
                let job = Job::spawn("render3d", waker, move |progress| {
                    let cancelled = || progress.is_cancelled();
                    render(&prepared, &request.camera, &request.style, request.size, Quality::Full, &cancelled)
                });
                self.job = Some((ticket, job));
            }
        }
        self.progressive.shown()
    }

    /// The picture to show.
    pub fn shown(&self) -> Option<&Shown<Arc<Rendering>>> {
        self.progressive.shown()
    }

    /// Whether a full picture is being rendered.
    pub fn is_refining(&self) -> bool {
        self.job.is_some()
    }

    /// Whether the next frame has work to do even if nothing changes (it
    /// starts the full picture of the preview on screen).
    pub fn wants_frame(&self) -> bool {
        self.progressive.wants_frame()
    }

    /// The size of the next preview.
    pub fn preview_size(&self) -> PreviewSize {
        self.preview
    }

    fn cancel(&mut self) {
        if let Some((_, job)) = self.job.take() {
            job.cancel();
        }
    }
}

impl Drop for VolumePictures {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use summrise_imaging::render3d::{Look, turned};
    use summrise_imaging::{DQuat, DVec3, VolumeGeometry, Window};
    use summrise_testkit::PhantomOptions;

    use super::*;
    use crate::series::testing::{load, phantom};

    fn request(turn: f64) -> PictureRequest {
        let geometry =
            VolumeGeometry::new([10, 10, 10], DVec3::ZERO, [DVec3::X, DVec3::Y, DVec3::Z]).expect("geometry");
        let camera = Camera::fit(&geometry, turned(DQuat::IDENTITY, turn, 0.0));
        let style = Style { look: Look::Bone, window: Window::new(40.0, 400.0), invert: false };
        PictureRequest::new(camera, style, [640.0, 480.0])
    }

    fn shown(p: &Progressive<&'static str>) -> Option<(&'static str, Quality)> {
        p.shown().map(|s| (s.picture, s.quality))
    }

    #[test]
    fn a_preview_is_shown_at_once_and_the_full_picture_replaces_it() {
        let mut p = Progressive::default();
        let first = request(0.0);
        let Step::Preview(ticket) = p.update(&first) else { panic!("a preview first") };
        assert!(p.deliver(ticket, Quality::Preview, "preview"));
        assert_eq!(shown(&p), Some(("preview", Quality::Preview)));
        assert!(p.wants_frame(), "the next frame starts the full picture");
        // The request stays the same for a frame: the full picture is started, once.
        let Step::Full(full) = p.update(&first) else { panic!("then the full picture") };
        assert_eq!(p.pending(), Some(full));
        assert!(!p.wants_frame());
        assert_eq!(p.update(&first), Step::Wait);
        assert_eq!(shown(&p), Some(("preview", Quality::Preview)), "until it is ready");
        assert!(p.deliver(full, Quality::Full, "full"));
        assert_eq!(shown(&p), Some(("full", Quality::Full)));
        assert_eq!(p.pending(), None);
        // Nothing more to do while nothing changes.
        assert_eq!(p.update(&first), Step::Wait);
        assert_eq!(p.update(&first), Step::Wait);
        assert_eq!(p.shown().map(|s| &s.request), Some(&first));
    }

    #[test]
    fn while_dragging_every_frame_shows_a_preview_and_no_full_picture_is_started() {
        let mut p = Progressive::default();
        for frame in 0..10 {
            let request = request(f64::from(frame) * 2.0);
            let Step::Preview(ticket) = p.update(&request) else { panic!("frame {frame}: a preview") };
            assert!(p.deliver(ticket, Quality::Preview, "preview"));
            assert_eq!(p.shown().map(|s| &s.request), Some(&request), "frame {frame}: the picture of this frame");
            assert_eq!(p.pending(), None);
        }
        // The pointer stops: the full picture follows.
        assert!(matches!(p.update(&request(18.0)), Step::Full(_)));
    }

    #[test]
    fn a_stale_full_picture_never_replaces_a_newer_preview() {
        let mut p = Progressive::default();
        let (before, after) = (request(0.0), request(10.0));
        let Step::Preview(t1) = p.update(&before) else { panic!("preview") };
        assert!(p.deliver(t1, Quality::Preview, "preview before"));
        let Step::Full(stale) = p.update(&before) else { panic!("full") };
        // The user turns the object while the full picture is rendered.
        let Step::Preview(t3) = p.update(&after) else { panic!("preview") };
        assert_eq!(p.pending(), None, "the full picture in progress is no longer wanted");
        assert!(p.deliver(t3, Quality::Preview, "preview after"));
        // The stale full picture arrives late: it is not shown.
        assert!(!p.deliver(stale, Quality::Full, "stale full"));
        assert_eq!(shown(&p), Some(("preview after", Quality::Preview)));
        assert_eq!(p.shown().map(|s| &s.request), Some(&after));
        // Nor is a late preview of the earlier request.
        assert!(!p.deliver(t1, Quality::Preview, "late preview"));
        // The full picture of the new request replaces the preview.
        let Step::Full(full) = p.update(&after) else { panic!("full") };
        assert!(p.deliver(full, Quality::Full, "full after"));
        assert_eq!(shown(&p), Some(("full after", Quality::Full)));
        // Delivered twice, it is shown once.
        assert!(!p.deliver(full, Quality::Full, "again"));
    }

    #[test]
    fn a_stale_full_picture_arriving_before_the_new_preview_is_dropped_too() {
        let mut p = Progressive::default();
        let (before, after) = (request(0.0), request(10.0));
        let Step::Preview(t1) = p.update(&before) else { panic!("preview") };
        assert!(p.deliver(t1, Quality::Preview, "preview before"));
        let Step::Full(stale) = p.update(&before) else { panic!("full") };
        let Step::Preview(_) = p.update(&after) else { panic!("preview") };
        // The new preview could not be rendered (yet); the old full picture arrives.
        assert!(!p.deliver(stale, Quality::Full, "stale full"));
        assert_eq!(shown(&p), Some(("preview before", Quality::Preview)));
        // The next frame asks for the full picture of the new request.
        assert!(matches!(p.update(&after), Step::Full(_)));
    }

    #[test]
    fn preview_size_follows_the_time_previews_take() {
        let mut size = PreviewSize::default();
        assert_eq!(size.longest_side(), 256);
        assert_eq!(size.of([1024, 768]), [256, 192]);
        assert_eq!(size.of([100, 50]), [100, 50], "never larger than the full picture");
        size.record(Duration::from_millis(30));
        assert_eq!(size.longest_side(), 128, "much too slow: halved");
        size.record(Duration::from_millis(15));
        assert_eq!(size.longest_side(), 102, "a little too slow");
        size.record(Duration::from_millis(9));
        assert_eq!(size.longest_side(), 102, "within the budget");
        for _ in 0..20 {
            size.record(Duration::from_millis(1));
        }
        assert_eq!(size.longest_side(), 384, "fast: up to the largest preview");
        for _ in 0..20 {
            size.record(Duration::from_millis(100));
        }
        assert_eq!(size.longest_side(), 96, "slow: down to the smallest");
    }

    #[test]
    fn full_pictures_have_the_viewport_size_capped() {
        let camera = request(0.0).camera;
        let style = request(0.0).style;
        assert_eq!(PictureRequest::new(camera, style, [800.0, 600.0]).size, [800, 600]);
        assert_eq!(PictureRequest::new(camera, style, [2048.0, 1536.0]).size, [1024, 768]);
        assert_eq!(PictureRequest::new(camera, style, [0.0, f32::NAN]).size, [1, 1]);
    }

    /// The executor with real jobs: a preview at once, the full picture later,
    /// a new request cancels the job in progress.
    #[test]
    fn pictures_of_a_volume_refine_in_the_background() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let ct = load(&index.studies[0].series[0]);
        let volume = ct.volume().expect("volume");
        let style = Style { look: Look::Bone, window: Window::new(40.0, 400.0), invert: false };
        let at = |turn: f64| {
            PictureRequest::new(
                Camera::fit(&volume.geometry, turned(DQuat::IDENTITY, turn, 0.0)),
                style,
                [300.0, 200.0],
            )
        };
        let mut pictures = VolumePictures::default();
        let shown = pictures.update(volume, &at(0.0), None).expect("a preview at once");
        assert_eq!(shown.quality, Quality::Preview);
        assert!(shown.picture.width <= 256 && shown.picture.rgba.iter().any(|&b| b > 0));
        assert!(pictures.wants_frame() && !pictures.is_refining());
        // The next frame starts the full picture; later frames collect it.
        pictures.update(volume, &at(0.0), None);
        assert!(pictures.is_refining());
        let start = Instant::now();
        while pictures.shown().is_none_or(|s| s.quality != Quality::Full) {
            assert!(start.elapsed() < Duration::from_secs(30), "the full picture takes too long");
            std::thread::sleep(Duration::from_millis(1));
            pictures.update(volume, &at(0.0), None);
        }
        let full = pictures.shown().expect("shown");
        assert_eq!((full.picture.width, full.picture.height), (300, 200));
        assert_eq!(full.request, at(0.0));
        assert!(!pictures.is_refining() && !pictures.wants_frame());
        // Turned: a preview at once; the job of the next frame is cancelled by a further turn.
        assert_eq!(pictures.update(volume, &at(10.0), None).map(|s| s.quality), Some(Quality::Preview));
        pictures.update(volume, &at(10.0), None);
        assert!(pictures.is_refining());
        let shown = pictures.update(volume, &at(20.0), None).expect("shown");
        assert_eq!((shown.quality, &shown.request), (Quality::Preview, &at(20.0)));
        assert!(!pictures.is_refining(), "the job for the previous turn is cancelled");
        // Another volume starts afresh.
        let other = load(&index.studies[0].series[1]);
        let shown = pictures.update(other.volume().expect("volume"), &at(20.0), None).expect("shown");
        assert_eq!(shown.quality, Quality::Preview);
    }
}
