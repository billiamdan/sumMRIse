//! Background work. Each job runs on its own thread (with rayon parallelism
//! inside) and wakes the UI when it finishes, so the UI thread never blocks
//! on disc I/O or decoding.
//!
//! * [`Job`] — one result (scanning a disc, reading metadata).
//! * [`Stream`] — many results as they become ready (thumbnails).
//! * [`SeriesLoader`] — the images of a series, nearest to the displayed one
//!   first, into a [`SeriesData`] that the UI reads while it fills.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use summrise_i18n::tr;

use summrise_dicom::{
    DecodedInstance, DicomError, FrameInfo, InstanceHeader, Media, Progress, ProgressSnapshot, Series,
};

use crate::series::SeriesData;

/// Called from worker threads to request a UI repaint.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// A single-result background job with progress and cancellation.
#[derive(Debug)]
pub struct Job<T> {
    progress: Progress,
    rx: Receiver<T>,
    finished: bool,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(name: &str, waker: Option<Waker>, work: impl FnOnce(&Progress) -> T + Send + 'static) -> Self {
        let progress = Progress::new();
        let (tx, rx) = mpsc::channel();
        let worker_progress = progress.clone();
        let spawned = std::thread::Builder::new().name(format!("summrise-{name}")).spawn(move || {
            let result = work(&worker_progress);
            let _ = tx.send(result);
            if let Some(wake) = waker {
                wake();
            }
        });
        if let Err(e) = spawned {
            log::error!("failed to spawn worker thread: {e}");
        }
        Self { progress, rx, finished: false }
    }

    /// Takes the result once the job has finished.
    pub fn poll(&mut self) -> Option<T> {
        if self.finished {
            return None;
        }
        match self.rx.try_recv() {
            Ok(v) => {
                self.finished = true;
                Some(v)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.finished = true;
                None
            }
        }
    }

    pub fn progress(&self) -> ProgressSnapshot {
        self.progress.snapshot()
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

/// A job that streams many results (e.g. thumbnails) as they become ready.
#[derive(Debug)]
pub struct Stream<T> {
    progress: Progress,
    rx: Receiver<T>,
}

impl<T: Send + 'static> Stream<T> {
    pub fn spawn(
        name: &str,
        waker: Option<Waker>,
        work: impl FnOnce(&Progress, &(dyn Fn(T) + Sync)) + Send + 'static,
    ) -> Self {
        let progress = Progress::new();
        let (tx, rx) = mpsc::channel();
        let worker_progress = progress.clone();
        let spawned = std::thread::Builder::new().name(format!("summrise-{name}")).spawn(move || {
            let emit = |item: T| {
                let _ = tx.send(item);
                if let Some(wake) = &waker {
                    wake();
                }
            };
            work(&worker_progress, &emit);
        });
        if let Err(e) = spawned {
            log::error!("failed to spawn worker thread: {e}");
        }
        Self { progress, rx }
    }

    /// Drains all results produced so far.
    pub fn drain(&self) -> Vec<T> {
        self.rx.try_iter().collect()
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }
}

impl<T> Drop for Stream<T> {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

/// Where a [`SeriesLoader`] gets its images from. Reading and decoding are
/// separate steps because they run on different threads: one thread reads
/// (sequential access suits optical media), several decode from memory.
pub trait ImageSource: Send + Sync {
    /// Reads the encoded bytes of one file from `media`, the media of the
    /// index: a folder, or an archive or disc image that is read in memory.
    fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError>;

    /// Decodes every frame of a file from the bytes [`Self::read`] returned.
    fn decode(&self, header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError>;
}

/// The DICOM files named by the index.
#[derive(Debug, Clone, Copy, Default)]
pub struct DicomFiles;

impl ImageSource for DicomFiles {
    fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError> {
        summrise_dicom::read_instance(media, header)
    }

    fn decode(&self, _header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError> {
        summrise_dicom::decode_instance(bytes)
    }
}

/// The threads that decode the files the loaders read, one file at a time
/// each.
///
/// Every decoder is a rayon pool of a single thread. The pixel decoder
/// parallelises internally with rayon; in a pool of its own that work stays
/// on the decoder's thread, where a shared pool scatters it in small pieces
/// over threads that are busy with other files (measured on a 300-slice CT:
/// seven times slower). It also keeps decoding out of rayon's global pool,
/// which the UI thread uses to render the displayed image.
pub struct Decoders {
    workers: Vec<Decoder>,
}

struct Decoder {
    pool: rayon::ThreadPool,
    /// Files this decoder holds: the one it decodes and those waiting for it.
    load: Arc<AtomicUsize>,
}

impl std::fmt::Debug for Decoders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoders").field("threads", &self.workers.len()).finish()
    }
}

impl Decoders {
    /// Starts up to `threads` decoders; `None` if not even one could be started.
    pub fn new(threads: usize) -> Option<Self> {
        let workers: Vec<Decoder> = (0..threads.max(1))
            .filter_map(|i| {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .thread_name(move |_| format!("summrise-decode-{i}"))
                    .panic_handler(|_| log::error!("a decoding task panicked"))
                    .build()
                    .map_err(|e| log::error!("failed to start a decoding thread: {e}"))
                    .ok()
            })
            .map(|pool| Decoder { pool, load: Arc::new(AtomicUsize::new(0)) })
            .collect();
        (!workers.is_empty()).then_some(Self { workers })
    }

    /// Number of files that can be decoded at the same time.
    pub fn threads(&self) -> usize {
        self.workers.len()
    }

    /// Runs `task` on the decoder that has the least to do.
    fn spawn(&self, task: impl FnOnce() + Send + 'static) {
        let Some(decoder) = self.workers.iter().min_by_key(|d| d.load.load(Ordering::Relaxed)) else {
            return task();
        };
        decoder.load.fetch_add(1, Ordering::Relaxed);
        let load = Unload(decoder.load.clone());
        decoder.pool.spawn(move || {
            let _load = load;
            task();
        });
    }
}

/// Takes a file off a decoder's load when its task ends, however it ends.
struct Unload(Arc<AtomicUsize>);

impl Drop for Unload {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Files read in one go before the loader looks again at where the viewer
/// is. Runs keep reads mostly sequential on optical media.
const RUN_LENGTH: usize = 8;

/// Most bytes read ahead of the decoders. A larger file still loads, alone.
const READ_AHEAD_BYTES: usize = 256 << 20;

/// Decides which file to read next: the one holding the image nearest to the
/// displayed one, then on through a run of consecutive images.
struct Schedule {
    /// File (index into `Series::instances`) of each displayed image.
    instance_of: Vec<usize>,
    /// Displayed images of each file.
    slots_of: Vec<Vec<usize>>,
    /// Displayed images whose file has been handed out.
    requested: Vec<bool>,
    /// Number of displayed images whose file has not been handed out.
    waiting: usize,
    run: Option<Run>,
}

/// A run of consecutive files, leading away from the displayed image.
#[derive(Debug, Clone, Copy)]
struct Run {
    /// The image the run goes on with.
    next: Option<usize>,
    forward: bool,
    /// Files the run may still take.
    left: usize,
}

impl Run {
    /// A run that begins with the file about to be handed out.
    fn start(forward: bool) -> Self {
        Self { next: None, forward, left: RUN_LENGTH - 1 }
    }

    /// The next image of the run if it still needs its file, and the rest of the run.
    fn continued(self, requested: &[bool]) -> Option<(usize, Self)> {
        let slot = self.next.filter(|&slot| self.left > 0 && requested.get(slot) == Some(&false))?;
        Some((slot, Self { left: self.left - 1, ..self }))
    }

    /// The run after a file with the images `slots` was handed out: it goes
    /// on beyond the far end of that file, whose frames all come with it.
    fn beyond(self, slots: &[usize]) -> Self {
        let far = if self.forward { slots.iter().copied().max() } else { slots.iter().copied().min() };
        let next = far.and_then(|slot| if self.forward { slot.checked_add(1) } else { slot.checked_sub(1) });
        Self { next, ..self }
    }
}

impl Schedule {
    fn new(frames: &[FrameInfo]) -> Self {
        let instance_of: Vec<usize> = frames.iter().map(|f| f.instance).collect();
        let mut slots_of = vec![Vec::new(); instance_of.iter().map(|i| i + 1).max().unwrap_or(0)];
        for (slot, &instance) in instance_of.iter().enumerate() {
            slots_of[instance].push(slot);
        }
        Self { requested: vec![false; instance_of.len()], waiting: instance_of.len(), instance_of, slots_of, run: None }
    }

    /// Whether every file has been handed out.
    fn is_done(&self) -> bool {
        self.waiting == 0
    }

    /// Whether the file of image `slot` has not been handed out yet.
    fn waits_for(&self, slot: usize) -> bool {
        self.requested.get(slot) == Some(&false)
    }

    /// The file to read next while image `displayed` is on screen, or `None`
    /// when every file has been handed out.
    fn next(&mut self, displayed: usize) -> Option<usize> {
        let displayed = displayed.min(self.requested.len().saturating_sub(1));
        // The displayed image goes first: a run is dropped as soon as the
        // viewer moves to an image that still waits for its file.
        let run = self.run.take().filter(|_| self.requested.get(displayed) != Some(&false));
        let (slot, run) = match run.and_then(|run| run.continued(&self.requested)) {
            Some(continued) => continued,
            None => {
                let slot = self.nearest(displayed)?;
                (slot, Run::start(slot >= displayed))
            }
        };
        let instance = self.instance_of[slot];
        let slots = &self.slots_of[instance];
        for &of_same_file in slots {
            if !std::mem::replace(&mut self.requested[of_same_file], true) {
                self.waiting -= 1;
            }
        }
        self.run = Some(run.beyond(slots));
        Some(instance)
    }

    /// The image nearest to `displayed` whose file has not been handed out
    /// (the later one when two are equally near).
    fn nearest(&self, displayed: usize) -> Option<usize> {
        let waiting = |slot: &usize| self.requested.get(*slot) == Some(&false);
        (0..self.requested.len())
            .find_map(|d| Some(displayed + d).filter(waiting).or_else(|| displayed.checked_sub(d).filter(waiting)))
    }
}

struct Control {
    /// The image on screen; loading concentrates around it.
    displayed: usize,
    paused: bool,
    /// Another series comes first: only the image on screen is read.
    background: bool,
    cancelled: bool,
    /// Files read but not yet decoded, and their size.
    in_flight: usize,
    in_flight_bytes: usize,
}

struct Shared {
    data: Arc<SeriesData>,
    /// Where the files of the series are read from.
    media: Media,
    control: Mutex<Control>,
    changed: Condvar,
    waker: Option<Waker>,
}

impl Shared {
    fn control(&self) -> MutexGuard<'_, Control> {
        self.control.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, control: MutexGuard<'a, Control>) -> MutexGuard<'a, Control> {
        self.changed.wait(control).unwrap_or_else(PoisonError::into_inner)
    }

    fn wake_ui(&self) {
        if let Some(wake) = &self.waker {
            wake();
        }
    }
}

/// A file on its way through the decoders. Dropping it frees its place, also
/// when decoding ends abnormally, so the loader can never stall.
struct InFlight<'a> {
    shared: &'a Shared,
    bytes: usize,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut control = self.shared.control();
        control.in_flight = control.in_flight.saturating_sub(1);
        control.in_flight_bytes = control.in_flight_bytes.saturating_sub(self.bytes);
        drop(control);
        self.shared.changed.notify_all();
    }
}

/// Loads the images of one series in the background, favouring those nearest
/// to the displayed one, into a [`SeriesData`] shared with the UI.
///
/// One coordinator thread picks the next file, reads its bytes and hands them
/// to the [`Decoders`]. It can be paused and resumed; what is already loaded
/// stays in the store. Dropping the loader stops it after the file in
/// progress.
pub struct SeriesLoader {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for SeriesLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let control = self.shared.control();
        f.debug_struct("SeriesLoader")
            .field("displayed", &control.displayed)
            .field("paused", &control.paused)
            .finish_non_exhaustive()
    }
}

impl SeriesLoader {
    /// Starts loading `series` into `data`, beginning with image `displayed`.
    /// The files are read from `media`, the media of the index the series is
    /// from, and decoded on `decoders`, or on the coordinator thread without one.
    pub fn spawn(
        series: Arc<Series>,
        media: Media,
        data: Arc<SeriesData>,
        source: Arc<dyn ImageSource>,
        decoders: Option<Arc<Decoders>>,
        displayed: usize,
        waker: Option<Waker>,
    ) -> Self {
        let control =
            Control { displayed, paused: false, background: false, cancelled: false, in_flight: 0, in_flight_bytes: 0 };
        let shared = Arc::new(Shared { data, media, control: Mutex::new(control), changed: Condvar::new(), waker });
        let coordinator = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("summrise-load".to_owned())
            .spawn(move || coordinate(&coordinator, &series, &source, decoders.as_deref()));
        if let Err(e) = spawned {
            log::error!("failed to spawn the loader thread: {e}");
            shared.data.abort(tr!("the loader could not be started"));
        }
        Self { shared }
    }

    /// Tells the loader which image is on screen. If that image is not loaded
    /// yet, it is read next, before the rest of the current run.
    pub fn set_displayed(&self, index: usize) {
        self.shared.control().displayed = index;
        self.shared.changed.notify_all();
    }

    /// While another series comes first, reads only the image on screen, so
    /// that the viewport shows something and follows; the rest waits. Cheap
    /// when nothing changes, so it can be called every frame.
    pub fn set_background(&self, background: bool) {
        let mut control = self.shared.control();
        if control.background != background {
            control.background = background;
            drop(control);
            self.shared.changed.notify_all();
        }
    }

    /// Stops handing out work after the file in progress. Loaded images are kept.
    pub fn pause(&self) {
        self.shared.control().paused = true;
    }

    /// Continues where [`Self::pause`] stopped, also out of the background.
    pub fn resume(&self) {
        let mut control = self.shared.control();
        control.paused = false;
        control.background = false;
        drop(control);
        self.shared.changed.notify_all();
    }

    pub fn is_paused(&self) -> bool {
        self.shared.control().paused
    }
}

impl Drop for SeriesLoader {
    fn drop(&mut self) {
        self.shared.control().cancelled = true;
        self.shared.changed.notify_all();
    }
}

/// Runs one step of an image source; a panic inside it becomes a failed image.
fn guarded<T>(step: impl FnOnce() -> Result<T, DicomError>) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(step))
        .unwrap_or_else(|_| Err(DicomError::Image(tr!("the image source panicked").to_owned())))
        .map_err(|e| e.to_string())
}

/// The coordinator: reads files in the order of the [`Schedule`] (the only
/// thread that touches the medium) and passes them on for decoding.
fn coordinate(shared: &Arc<Shared>, series: &Arc<Series>, source: &Arc<dyn ImageSource>, decoders: Option<&Decoders>) {
    let data = &shared.data;
    let workers = decoders.map_or(1, Decoders::threads);
    let mut schedule = Schedule::new(data.frames());
    // Once every file is handed out there is nothing a pause could hold back:
    // the files in flight are finished and the series is completed, also when
    // the user has moved on to another series in the meantime.
    while !schedule.is_done() {
        // Wait until the series is wanted and the decoders can take another file.
        let displayed = {
            let mut control = shared.control();
            loop {
                if control.cancelled {
                    return;
                }
                // Until a first image is decoded, files go through one at a
                // time: "the first decoded image", which fixes the initial
                // window, is then always the one nearest to the displayed
                // position, however the decoders are scheduled.
                let limit = if data.is_viewable() { workers } else { 1 };
                let room =
                    control.in_flight < limit && (control.in_flight == 0 || control.in_flight_bytes < READ_AHEAD_BYTES);
                // In the background only the image on screen, if its file is still to be read.
                let wanted = !control.background || schedule.waits_for(control.displayed);
                if room && !control.paused && wanted {
                    break control.displayed;
                }
                control = shared.wait(control);
            }
        };
        let Some(instance) = schedule.next(displayed) else { break };
        let bytes = match series.instances.get(instance) {
            Some(header) => guarded(|| source.read(&shared.media, header)),
            None => Err(tr!("the image is missing from the index").to_owned()),
        };
        let size = bytes.as_ref().map_or(0, Vec::len);
        {
            let mut control = shared.control();
            if control.cancelled {
                return;
            }
            control.in_flight += 1;
            control.in_flight_bytes += size;
        }
        let task = {
            let (shared, series, source) = (shared.clone(), series.clone(), source.clone());
            move || {
                let in_flight = InFlight { shared: &shared, bytes: size };
                let result = bytes.and_then(|bytes| guarded(|| source.decode(&series.instances[instance], bytes)));
                if let Err(reason) = &result {
                    log::debug!("image file {instance} of the series could not be loaded: {reason}");
                }
                let was_viewable = shared.data.is_viewable();
                let displayed = shared.control().displayed;
                let filled = shared.data.fill(instance, result, displayed);
                // Repaint for what the viewer is waiting for; progress
                // counters are picked up by the UI's periodic repaint.
                let opened = !was_viewable && shared.data.is_viewable();
                let show = opened || filled.contains(&shared.control().displayed);
                drop(in_flight);
                if show {
                    shared.wake_ui();
                }
            }
        };
        match decoders {
            Some(decoders) => decoders.spawn(task),
            None => task(),
        }
    }
    // Every file is handed out: wait for the last decodes, then assemble.
    {
        let mut control = shared.control();
        while control.in_flight > 0 {
            if control.cancelled {
                return;
            }
            control = shared.wait(control);
        }
    }
    data.finish();
    shared.wake_ui();
}

/// Test support: an image source that tests let through file by file.
#[cfg(test)]
pub(crate) mod testing {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use summrise_dicom::{DecodedInstance, DicomError, InstanceHeader, Media};

    use super::{DicomFiles, ImageSource};

    /// An artificially slow image source: every read waits until the test
    /// lets it through, so a test decides exactly how far loading has got.
    pub(crate) struct SlowSource {
        /// Reads let through so far.
        allowed: AtomicUsize,
        /// Reads begun, including the one waiting to be let through.
        started: AtomicUsize,
        /// Series folder and instance number of every read, in order.
        reads: Mutex<Vec<(String, i32)>>,
        /// Images that cannot be decoded.
        corrupt: Box<dyn Fn(&InstanceHeader) -> bool + Send + Sync>,
    }

    /// Name of the series folder a file lies in on the synthetic CD.
    pub(crate) fn folder(header: &InstanceHeader) -> String {
        header.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    impl SlowSource {
        pub(crate) fn new() -> Arc<Self> {
            Self::with_corrupt_images(|_| false)
        }

        pub(crate) fn with_corrupt_images(
            corrupt: impl Fn(&InstanceHeader) -> bool + Send + Sync + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                allowed: AtomicUsize::new(0),
                started: AtomicUsize::new(0),
                reads: Mutex::new(Vec::new()),
                corrupt: Box::new(corrupt),
            })
        }

        /// Lets `n` more reads through.
        pub(crate) fn allow(&self, n: usize) {
            self.allowed.fetch_add(n, Ordering::SeqCst);
        }

        /// From now on reads are as fast as the disc.
        pub(crate) fn allow_all(&self) {
            self.allowed.store(usize::MAX, Ordering::SeqCst);
        }

        pub(crate) fn started(&self) -> usize {
            self.started.load(Ordering::SeqCst)
        }

        /// Instance numbers read from one series folder, in the order they were read.
        pub(crate) fn reads_of(&self, series_folder: &str) -> Vec<i32> {
            let reads = self.reads.lock().expect("read log");
            reads.iter().filter(|(folder, _)| folder == series_folder).map(|(_, number)| *number).collect()
        }
    }

    impl ImageSource for SlowSource {
        fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError> {
            let turn = self.started.fetch_add(1, Ordering::SeqCst);
            while turn >= self.allowed.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            self.reads.lock().expect("read log").push((folder(header), header.instance_number.unwrap_or(0)));
            DicomFiles.read(media, header)
        }

        fn decode(&self, header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError> {
            if (self.corrupt)(header) {
                return Err(DicomError::Image("corrupt pixel data".to_owned()));
            }
            DicomFiles.decode(header, bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    use summrise_dicom::plan_series;
    use summrise_testkit::PhantomOptions;

    use super::testing::SlowSource;
    use super::*;
    use crate::series::testing::phantom;

    /// Waits until `done`, failing the test if it takes implausibly long.
    fn eventually(what: &str, done: impl Fn() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn wait<T: Send + 'static>(job: &mut Job<T>) -> T {
        let start = Instant::now();
        loop {
            if let Some(v) = job.poll() {
                return v;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "job timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn job_returns_result_and_wakes() {
        let woke = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let w = woke.clone();
        let mut job =
            Job::spawn("test", Some(Arc::new(move || w.store(true, std::sync::atomic::Ordering::SeqCst))), |p| {
                p.begin("work", 1);
                p.advance();
                42
            });
        assert_eq!(wait(&mut job), 42);
        assert!(job.is_finished());
        assert!(job.poll().is_none());
        assert!(woke.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// Images in display order, each named by the file that holds it.
    fn images(files: &[usize]) -> Vec<FrameInfo> {
        files
            .iter()
            .map(|&instance| FrameInfo {
                instance,
                frame: 0,
                instance_number: None,
                plane: None,
                pixel_spacing: None,
                width: 1,
                height: 1,
            })
            .collect()
    }

    /// A series of `n` single-frame files.
    fn single_frames(n: usize) -> Schedule {
        Schedule::new(&images(&(0..n).collect::<Vec<_>>()))
    }

    /// The next `n` files handed out while image `displayed` is on screen.
    fn take(schedule: &mut Schedule, displayed: usize, n: usize) -> Vec<usize> {
        (0..n).map_while(|_| schedule.next(displayed)).collect()
    }

    #[test]
    fn schedule_reads_in_runs_outwards_from_the_displayed_image() {
        let mut schedule = single_frames(30);
        let order = take(&mut schedule, 12, 100);
        // The displayed image first, then runs of eight consecutive images on alternating sides.
        let expected: Vec<usize> =
            (12..20).chain((4..12).rev()).chain(20..28).chain((0..4).rev()).chain(28..30).collect();
        assert_eq!(order, expected);
        assert_eq!(schedule.next(12), None, "every file is handed out exactly once");
    }

    #[test]
    fn schedule_turns_to_an_unloaded_displayed_image_at_once() {
        let mut schedule = single_frames(60);
        assert_eq!(take(&mut schedule, 30, 3), [30, 31, 32]);
        // The viewer jumps ahead: its image is next, then the images around it …
        assert_eq!(take(&mut schedule, 55, 13), [55, 56, 57, 58, 59, 54, 53, 52, 51, 50, 49, 48, 47]);
        // … and only then the gap, filled from the viewer's side.
        let rest = take(&mut schedule, 55, 100);
        assert_eq!(rest[..14], (33..=46).rev().collect::<Vec<_>>()[..]);
        assert_eq!(rest.len(), 60 - 16);
    }

    #[test]
    fn schedule_keeps_its_run_while_the_viewer_stays_on_loaded_images() {
        let mut schedule = single_frames(30);
        assert_eq!(take(&mut schedule, 12, 3), [12, 13, 14]);
        // Scrolling over images that are loaded or on their way does not break up the run.
        assert_eq!(take(&mut schedule, 13, 2), [15, 16]);
        assert_eq!(take(&mut schedule, 14, 3), [17, 18, 19]);
        // The run is over: the next one starts beside the viewer.
        assert_eq!(take(&mut schedule, 14, 2), [11, 10]);
    }

    #[test]
    fn schedule_hands_out_a_multi_frame_file_once() {
        // File 0 holds image 0, file 1 images 1 to 3, file 2 image 4.
        let mut schedule = Schedule::new(&images(&[0, 1, 1, 1, 2]));
        assert_eq!(take(&mut schedule, 2, 100), [1, 2, 0]);
    }

    #[test]
    fn schedule_keeps_its_runs_across_multi_frame_files() {
        // Eight files of two frames each; the viewer is on the first frame of the fifth file.
        let files: Vec<usize> = (0..8).flat_map(|file| [file, file]).collect();
        let mut schedule = Schedule::new(&images(&files));
        assert_eq!(take(&mut schedule, 8, 100), [4, 5, 6, 7, 3, 2, 1, 0], "sequential, as for single frames");
    }

    #[test]
    fn schedule_copes_with_empty_series_and_stale_positions() {
        assert_eq!(Schedule::new(&[]).next(0), None);
        // A displayed index beyond the series counts as its last image.
        assert_eq!(take(&mut single_frames(3), 99, 100), [2, 1, 0]);
    }

    #[test]
    fn decoders_give_every_file_a_thread_of_its_own() {
        let decoders = Decoders::new(4).expect("decoder threads");
        assert_eq!(decoders.threads(), 4);
        // Four files that take a while: each must be on a different thread, none queued behind another.
        let (names, started) = mpsc::channel();
        let finish = Arc::new(AtomicBool::new(false));
        for _ in 0..4 {
            let (names, finish) = (names.clone(), finish.clone());
            decoders.spawn(move || {
                let _ = names.send(std::thread::current().name().map(str::to_owned));
                while !finish.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
        }
        let mut threads: Vec<Option<String>> =
            (0..4).map(|_| started.recv_timeout(Duration::from_secs(30)).expect("all four run at once")).collect();
        finish.store(true, Ordering::SeqCst);
        threads.sort();
        let expected: Vec<Option<String>> = (0..4).map(|i| Some(format!("summrise-decode-{i}"))).collect();
        assert_eq!(threads, expected);
        // A task that panics takes neither its thread nor the process down.
        decoders.spawn(|| panic!("deliberate panic in a test: the decoder thread must survive it"));
        let (done, finished) = mpsc::channel();
        for _ in 0..4 {
            let done = done.clone();
            decoders.spawn(move || done.send(()).expect("test alive"));
        }
        assert_eq!((0..4).filter_map(|_| finished.recv_timeout(Duration::from_secs(30)).ok()).count(), 4);
    }

    #[test]
    fn loader_wakes_the_ui_for_what_the_viewer_waits_for() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let series = Arc::new(index.studies[0].series[0].clone());
        let data = Arc::new(SeriesData::new(plan_series(&series)));
        let slow = SlowSource::new();
        let wakes = Arc::new(AtomicUsize::new(0));
        let count = wakes.clone();
        let waker: Waker = Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        let decoders = Decoders::new(3).map(Arc::new);
        let loader =
            SeriesLoader::spawn(series, index.media.clone(), data.clone(), slow.clone(), decoders, 12, Some(waker));
        let settled = |loaded: usize| data.loaded() == loaded && slow.started() == loaded + 1;
        let woken = |times: usize| {
            eventually("the wake-up", || wakes.load(Ordering::SeqCst) >= times);
            std::thread::sleep(Duration::from_millis(20));
            assert_eq!(wakes.load(Ordering::SeqCst), times);
        };

        // The first image: the series can be shown.
        slow.allow(1);
        eventually("the first image", || settled(1));
        assert!(data.is_viewable() && data.frame(12).is_some());
        woken(1);
        // Images the viewer is not looking at arrive quietly; progress is picked up by the UI's own timer.
        slow.allow(3);
        eventually("three more images", || settled(4));
        woken(1);
        // The viewer moves to an image that is not loaded: it is read next and announced.
        loader.set_displayed(2);
        slow.allow(2);
        eventually("the displayed image", || settled(6));
        assert!(data.frame(2).is_some());
        woken(2);
        // Paused: nothing more is read, however long it takes.
        loader.pause();
        assert!(loader.is_paused());
        slow.allow_all();
        eventually("the read in progress", || data.loaded() == 7);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(data.loaded(), 7);
        // Resumed: the rest arrives, and completion is announced.
        loader.resume();
        eventually("the whole series", || data.is_complete());
        assert_eq!(data.loaded(), 24);
        woken(3);
    }

    #[test]
    fn series_left_during_its_last_file_is_still_completed() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        // The dose report: a single image.
        let series = Arc::new(index.studies[0].series[3].clone());
        let data = Arc::new(SeriesData::new(plan_series(&series)));
        let slow = SlowSource::new();
        let loader = SeriesLoader::spawn(
            series,
            index.media.clone(),
            data.clone(),
            slow.clone(),
            Decoders::new(2).map(Arc::new),
            0,
            None,
        );
        eventually("the read of its only file", || slow.started() == 1);
        // The user moves on while that file is being read.
        loader.pause();
        slow.allow_all();
        // Nothing is left to hold back: the series is finished, not left "paused at 1 / 1".
        eventually("the series to be finished", || data.is_complete());
        assert_eq!(data.loaded(), 1);
        assert!(data.frame(0).is_some() && data.failure().is_none());
    }

    #[test]
    fn dropped_loader_stops_after_the_file_in_progress() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let series = Arc::new(index.studies[0].series[0].clone());
        let data = Arc::new(SeriesData::new(plan_series(&series)));
        let slow = SlowSource::new();
        let loader = SeriesLoader::spawn(
            series,
            index.media.clone(),
            data.clone(),
            slow.clone(),
            Decoders::new(2).map(Arc::new),
            0,
            None,
        );
        slow.allow(2);
        eventually("two images", || data.loaded() == 2 && slow.started() == 3);
        drop(loader);
        slow.allow_all();
        // The loader gives up its share of the store: nothing keeps an evicted series in memory.
        eventually("the loader to let go", || Arc::strong_count(&data) == 1);
        assert_eq!(data.loaded(), 2, "the file that was being read is not decoded any more");
        assert!(!data.is_complete());
        assert_eq!(slow.started(), 3);
    }

    #[test]
    fn stream_emits_items() {
        let stream = Stream::spawn("test", None, |_, emit| (0..3).for_each(emit));
        let start = Instant::now();
        let mut items = Vec::new();
        while items.len() < 3 && start.elapsed() < Duration::from_secs(10) {
            items.extend(stream.drain());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(items, vec![0, 1, 2]);
    }
}
