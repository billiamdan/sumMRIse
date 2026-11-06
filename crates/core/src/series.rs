//! The incremental store of one series: its display order and volume
//! geometry, planned from the headers, and one write-once slot per displayed
//! image that the loader fills as files are decoded.
//!
//! Readers (the UI thread) never take a lock: slots and the completion record
//! are [`OnceLock`]s and the counters are atomics. The order of the slots is
//! fixed before any pixel data is read, so an image never changes its number
//! or its slice position while the series loads, and an image that is not
//! loaded is reported as such — never as a neighbouring image.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use summrise_dicom::{
    DecodedInstance, DicomError, FrameExtras, FrameInfo, FrameRef, SeriesPlan, Stacks, decode_warning,
};
use summrise_imaging::{
    Frame, FramePixels, GraphicsPlane, Padding, VoiLut, Volume, VolumeError, VolumeGeometry, VoxelBuffer, Window,
    WindowPreset,
};

/// Reason recorded for frames that a file announces in its header but does
/// not contain.
fn missing_frame() -> &'static str {
    summrise_i18n::tr!("the file holds fewer frames than its header announces")
}

/// What is known about one displayed image.
#[derive(Debug, Clone, Copy)]
pub enum ImageSlot<'a> {
    /// Not decoded yet.
    Pending,
    Ready(FrameRef<'a>),
    /// Decoding failed; the reason is kept for display.
    Failed(&'a str),
}

enum Slot {
    Gray {
        width: u32,
        height: u32,
        data: Arc<VoxelBuffer>,
        /// Smallest and largest finite value that is not padding, for the
        /// series' value range.
        range: Option<(f32, f32)>,
        /// The values that are padding (display-fidelity D5).
        padding: Option<Padding>,
        /// Scanner graphics over the image (display-fidelity D6).
        graphics: Vec<Arc<GraphicsPlane>>,
    },
    Rgb {
        width: u32,
        height: u32,
        data: Vec<u8>,
        graphics: Vec<Arc<GraphicsPlane>>,
    },
    Failed(String),
}

impl Slot {
    fn byte_size(&self) -> usize {
        match self {
            Self::Gray { data, .. } => data.byte_size(),
            Self::Rgb { data, .. } => data.len(),
            Self::Failed(_) => 0,
        }
    }

    fn is_image(&self) -> bool {
        !matches!(self, Self::Failed(_))
    }

    /// A decoded frame with its padding and scanner graphics.
    fn new(frame: Frame, extras: FrameExtras) -> Self {
        let FrameExtras { padding, graphics } = extras;
        match frame.pixels {
            FramePixels::Gray(data) => {
                let range = summrise_imaging::padding::min_max(&data, padding);
                Self::Gray { width: frame.width, height: frame.height, data: Arc::new(data), range, padding, graphics }
            }
            FramePixels::Rgb(data) => Self::Rgb { width: frame.width, height: frame.height, data, graphics },
        }
    }

    fn graphics(&self) -> &[Arc<GraphicsPlane>] {
        match self {
            Self::Gray { graphics, .. } | Self::Rgb { graphics, .. } => graphics,
            Self::Failed(_) => &[],
        }
    }
}

/// Display parameters fixed by the first decoded image. They are never
/// recomputed while further images arrive.
struct FirstImage {
    presets: Vec<WindowPreset>,
    window: Window,
    /// Contrast tables stored in the image (VOI LUT Sequence).
    luts: Vec<VoiLut>,
}

/// What is only known once every image has been decoded or has failed.
struct Outcome {
    /// One volume per stack (one for a series that is not split); empty
    /// when the images do not form volumes.
    volumes: Vec<Volume>,
    volume_issue: Option<VolumeError>,
    value_range: (f32, f32),
    /// Value range of each stack, for stacks with windows of their own.
    stack_ranges: Vec<(f32, f32)>,
    warnings: Vec<String>,
    /// Set when not a single image could be decoded.
    failure: Option<String>,
}

/// One series as it is displayed: complete from the start in everything the
/// headers tell (order, positions, total), progressively filled with pixels.
pub struct SeriesData {
    plan: SeriesPlan,
    /// Display indices of the frames of each file (indexed like `Series::instances`).
    slots_by_instance: Vec<Vec<usize>>,
    slots: Vec<OnceLock<Slot>>,
    filled: AtomicUsize,
    bytes: AtomicUsize,
    first: OnceLock<FirstImage>,
    /// The first decoded image of each stack, for a series whose stacks
    /// differ in contrast and so keep a window each; empty otherwise.
    stack_first: Vec<OnceLock<FirstImage>>,
    outcome: OnceLock<Outcome>,
    /// Whether a decoded image has scanner graphics.
    graphics_seen: AtomicBool,
}

impl std::fmt::Debug for SeriesData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeriesData")
            .field("images", &self.len())
            .field("loaded", &self.loaded())
            .field("complete", &self.is_complete())
            .finish_non_exhaustive()
    }
}

impl SeriesData {
    /// An empty store for the series described by `plan`.
    pub fn new(plan: SeriesPlan) -> Self {
        let slots_by_instance = plan.slots_by_instance();
        let slots = (0..plan.frames.len()).map(|_| OnceLock::new()).collect();
        let separate = plan.stacks.as_ref().filter(|s| !s.kind.is_time_like());
        let stack_first = separate.map_or_else(Vec::new, |s| (0..s.count()).map(|_| OnceLock::new()).collect());
        Self {
            plan,
            slots_by_instance,
            slots,
            filled: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            first: OnceLock::new(),
            stack_first,
            outcome: OnceLock::new(),
            graphics_seen: AtomicBool::new(false),
        }
    }

    /// What the headers say about the series.
    pub fn plan(&self) -> &SeriesPlan {
        &self.plan
    }

    pub fn modality(&self) -> &str {
        &self.plan.modality
    }

    /// Number of displayed images — the final total, known from the start.
    pub fn len(&self) -> usize {
        self.plan.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plan.frames.is_empty()
    }

    /// Every displayed image in display order, loaded or not.
    pub fn frames(&self) -> &[FrameInfo] {
        &self.plan.frames
    }

    /// The image shown when the series is opened, and the one loaded first:
    /// the middle image of the (first) stack.
    pub fn initial_index(&self) -> usize {
        self.stack_len() / 2
    }

    /// How the series is split into stacks; `None` when it is not.
    pub fn stacks(&self) -> Option<&Stacks> {
        self.plan.stacks.as_ref()
    }

    /// Number of stacks: 1 for a series that is not split.
    pub fn stack_count(&self) -> usize {
        self.plan.stack_count()
    }

    /// Images in every stack: [`Self::len`] for a series that is not split.
    /// Image `i` of stack `s` is display index `s × stack_len + i`.
    pub fn stack_len(&self) -> usize {
        self.plan.stack_len()
    }

    /// What stack `stack` is, e.g. "Time point 2 / 3"; `None` for a series
    /// that is not split.
    pub fn stack_label(&self, stack: usize) -> Option<String> {
        self.stacks().map(|s| s.label(stack))
    }

    /// Whether every stack keeps a window of its own: stacks that differ in
    /// contrast (echoes, b-values …), not time points or heart phases.
    pub fn separate_windows(&self) -> bool {
        !self.stack_first.is_empty()
    }

    /// What the first decoded image of `stack` fixed: of the stack itself
    /// when the stacks keep a window each, else of the series.
    fn first_of(&self, stack: usize) -> Option<&FirstImage> {
        if self.separate_windows() { self.stack_first.get(stack)?.get() } else { self.first.get() }
    }

    /// The window stack `stack` opens with: the first stored window of its
    /// first decoded image, or an automatic window computed from that image
    /// (the series' window when the stacks share one). `None` until an image
    /// of that stack is decoded.
    pub fn stack_initial_window(&self, stack: usize) -> Option<Window> {
        self.first_of(stack).map(|f| f.window)
    }

    /// Windows stored in the first decoded image of `stack` (of the series
    /// when the stacks share one window).
    pub fn stack_presets(&self, stack: usize) -> &[WindowPreset] {
        self.first_of(stack).map_or(&[], |f| f.presets.as_slice())
    }

    /// Smallest and largest modality value of `stack` (of the series when
    /// the stacks share one window): exact once the series is complete, until
    /// then the range the stored values can span.
    pub fn stack_value_range(&self, stack: usize) -> (f32, f32) {
        match self.outcome.get() {
            Some(outcome) if self.separate_windows() => {
                outcome.stack_ranges.get(stack).copied().unwrap_or(outcome.value_range)
            }
            _ => self.value_range(),
        }
    }

    pub fn image(&self, index: usize) -> ImageSlot<'_> {
        match self.slots.get(index).and_then(OnceLock::get) {
            None => ImageSlot::Pending,
            Some(Slot::Gray { width, height, data, .. }) => {
                ImageSlot::Ready(FrameRef::Gray { width: *width, height: *height, data })
            }
            Some(Slot::Rgb { width, height, data, .. }) => {
                ImageSlot::Ready(FrameRef::Rgb { width: *width, height: *height, data })
            }
            Some(Slot::Failed(reason)) => ImageSlot::Failed(reason),
        }
    }

    /// Pixels of image `index`, once it is decoded.
    pub fn frame(&self, index: usize) -> Option<FrameRef<'_>> {
        match self.image(index) {
            ImageSlot::Ready(frame) => Some(frame),
            ImageSlot::Pending | ImageSlot::Failed(_) => None,
        }
    }

    /// The values of image `index` that are padding (outside the scanned
    /// area), once it is decoded.
    pub fn padding(&self, index: usize) -> Option<Padding> {
        match self.slots.get(index).and_then(OnceLock::get) {
            Some(Slot::Gray { padding, .. }) => *padding,
            _ => None,
        }
    }

    /// The scanner graphics over image `index`, once it is decoded.
    pub fn graphics(&self, index: usize) -> &[Arc<GraphicsPlane>] {
        self.slots.get(index).and_then(OnceLock::get).map_or(&[], Slot::graphics)
    }

    /// Whether any image decoded so far has scanner graphics.
    pub fn has_graphics(&self) -> bool {
        self.graphics_seen.load(Ordering::Relaxed)
    }

    /// Contrast tables (VOI LUT Sequence) stored in the first decoded image
    /// of `stack` (of the series when the stacks share one window).
    pub fn stack_luts(&self, stack: usize) -> &[VoiLut] {
        self.first_of(stack).map_or(&[], |f| f.luts.as_slice())
    }

    /// Contrast tables stored in the first decoded image.
    pub fn luts(&self) -> &[VoiLut] {
        self.first.get().map_or(&[], |f| f.luts.as_slice())
    }

    /// Images dealt with so far (decoded or failed), out of [`Self::len`].
    pub fn loaded(&self) -> usize {
        self.filled.load(Ordering::Relaxed)
    }

    /// Whether a first image has been decoded, which fixes the initial window.
    pub fn is_viewable(&self) -> bool {
        self.first.get().is_some()
    }

    /// Whether every image has been decoded or has failed.
    pub fn is_complete(&self) -> bool {
        self.outcome.get().is_some()
    }

    /// Why the series cannot be displayed at all (no image could be decoded).
    pub fn failure(&self) -> Option<&str> {
        self.outcome.get().and_then(|o| o.failure.as_deref())
    }

    /// Windows stored in the first decoded image (VOI LUT module).
    pub fn presets(&self) -> &[WindowPreset] {
        self.first.get().map_or(&[], |f| f.presets.as_slice())
    }

    /// The first stored window of the first decoded image, or an automatic
    /// window computed from that image.
    pub fn initial_window(&self) -> Window {
        let (lo, hi) = self.plan.value_range;
        self.first.get().map_or_else(|| Window::from_range(f64::from(lo), f64::from(hi)), |f| f.window)
    }

    /// Smallest and largest modality value: exact once the series is complete,
    /// until then the range the stored values can span.
    pub fn value_range(&self) -> (f32, f32) {
        self.outcome.get().map_or(self.plan.value_range, |o| o.value_range)
    }

    /// MONOCHROME1: the minimum value is displayed white.
    pub fn monochrome1(&self) -> bool {
        self.plan.monochrome1
    }

    pub fn is_color(&self) -> bool {
        self.plan.is_color
    }

    /// Unit of grey values, e.g. "HU" for CT.
    pub fn units(&self) -> Option<&str> {
        self.plan.units.as_deref()
    }

    /// Geometry of the volume the images (every stack) form: planned from the
    /// headers while the series loads, that of the assembled volume afterwards.
    pub fn geometry(&self) -> Option<&VolumeGeometry> {
        match self.outcome.get() {
            Some(outcome) => outcome.volumes.first().map(|v| &v.geometry),
            None => self.plan.geometry.as_ref(),
        }
    }

    /// The volume for reformatting, available once the series is complete.
    /// For a series split into stacks this is the volume of stack 0; code
    /// that shows a stack uses [`Self::stack_volume`].
    pub fn volume(&self) -> Option<&Volume> {
        self.stack_volume(0)
    }

    /// The volume of stack `stack` (all stacks share one geometry), available
    /// once the series is complete.
    pub fn stack_volume(&self, stack: usize) -> Option<&Volume> {
        self.outcome.get().and_then(|o| o.volumes.get(stack))
    }

    /// Why multiplanar reformatting is unavailable, as far as is known: from
    /// the headers while loading, final once the series is complete.
    pub fn volume_issue(&self) -> Option<&VolumeError> {
        match self.outcome.get() {
            Some(outcome) => outcome.volume_issue.as_ref(),
            None => self.plan.volume_issue.as_ref(),
        }
    }

    pub fn warnings(&self) -> &[String] {
        self.outcome.get().map_or(&[], |o| o.warnings.as_slice())
    }

    /// Memory held by the images decoded so far.
    pub fn byte_size(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Display indices of the frames of file `instance`.
    pub(crate) fn slots_of(&self, instance: usize) -> &[usize] {
        self.slots_by_instance.get(instance).map_or(&[], Vec::as_slice)
    }

    /// Stores the frames of one decoded file (or the reason it failed) in
    /// their slots. `displayed` is the image on screen: when this is the
    /// first file to decode, the initial window comes from that image if the
    /// file holds it. Returns the slots that were filled.
    pub(crate) fn fill(&self, instance: usize, result: Result<DecodedInstance, String>, displayed: usize) -> &[usize] {
        let slots = self.slots_of(instance);
        match result {
            Ok(mut decoded) => {
                // A PET series' values become uptake values here, once, before anything reads them.
                if let Some(Ok(suv)) = &self.plan.suv {
                    suv.apply(&mut decoded);
                }
                let stack_first = self.stack_first_images(slots, &decoded, displayed);
                let first = (self.first.get().is_none() && !decoded.frames.is_empty()).then(|| {
                    let frame = slots
                        .iter()
                        .position(|&slot| slot == displayed)
                        .filter(|&frame| frame < decoded.frames.len())
                        .unwrap_or(decoded.frames.len() / 2);
                    let window = decoded.initial_window(frame, self.plan.value_range);
                    let luts = std::mem::take(&mut decoded.extras.voi_luts);
                    FirstImage { presets: std::mem::take(&mut decoded.presets), window, luts }
                });
                let mut extras = std::mem::take(&mut decoded.extras.frames).into_iter();
                let mut frames = decoded.frames.into_iter();
                for &slot in slots {
                    let image = match frames.next() {
                        Some(frame) => Slot::new(frame, extras.next().unwrap_or_default()),
                        None => Slot::Failed(missing_frame().to_owned()),
                    };
                    self.set(slot, image);
                }
                // Published after the pixels, so a viewable series can show its first image.
                if let Some(first) = first {
                    let _ = self.first.set(first);
                }
                for (stack, first) in stack_first {
                    if let Some(cell) = self.stack_first.get(stack) {
                        let _ = cell.set(first);
                    }
                }
            }
            Err(reason) => {
                for &slot in slots {
                    self.set(slot, Slot::Failed(reason.clone()));
                }
            }
        }
        slots
    }

    /// For stacks that keep a window each: the window and presets of every
    /// stack whose first image is in this file (the frames at `slots`). The
    /// window comes from the displayed image if the file holds it, else from
    /// the middle one of the file's frames in that stack.
    fn stack_first_images(
        &self,
        slots: &[usize],
        decoded: &DecodedInstance,
        displayed: usize,
    ) -> Vec<(usize, FirstImage)> {
        let len = self.stack_len().max(1);
        let frames = slots.len().min(decoded.frames.len());
        let mut out: Vec<(usize, FirstImage)> = Vec::new();
        for stack in slots[..frames].iter().map(|&slot| slot / len) {
            let known = self.stack_first.get(stack).is_none_or(|cell| cell.get().is_some());
            if known || out.iter().any(|(s, _)| *s == stack) {
                continue;
            }
            let in_stack: Vec<usize> = (0..frames).filter(|&f| slots[f] / len == stack).collect();
            let Some(&middle) = in_stack.get(in_stack.len() / 2) else { continue };
            let frame = in_stack.iter().copied().find(|&f| slots[f] == displayed).unwrap_or(middle);
            let window = decoded.initial_window(frame, self.plan.value_range);
            let luts = decoded.extras.voi_luts.clone();
            out.push((stack, FirstImage { presets: decoded.presets.clone(), window, luts }));
        }
        out
    }

    fn set(&self, slot: usize, value: Slot) {
        let Some(cell) = self.slots.get(slot) else { return };
        let size = value.byte_size();
        if !value.graphics().is_empty() {
            self.graphics_seen.store(true, Ordering::Relaxed);
        }
        if cell.set(value).is_ok() {
            self.bytes.fetch_add(size, Ordering::Relaxed);
            self.filled.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records what is only known at the end — the exact value range, the
    /// final reformatting verdict and the volume. Called once, by the loader,
    /// when every slot is filled; unfilled slots count as failed.
    pub(crate) fn finish(&self) {
        let slots: Vec<Option<&Slot>> = self.slots.iter().map(OnceLock::get).collect();
        let decoded = |index: usize| slots.get(index).copied().flatten().is_some_and(Slot::is_image);
        let failed = (0..slots.len()).filter(|&i| !decoded(i)).count();
        let first_reason = slots.iter().find_map(|slot| match slot {
            Some(Slot::Failed(reason)) => Some(reason.as_str()),
            None => Some(summrise_i18n::tr!("the image was not loaded")),
            Some(_) => None,
        });

        let range_of = |slots: &[Option<&Slot>]| {
            slots
                .iter()
                .filter_map(|slot| match slot {
                    Some(Slot::Gray { range, .. }) => *range,
                    _ => None,
                })
                .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
        };
        let value_range = range_of(&slots).unwrap_or((0.0, 255.0));
        let len = self.stack_len().max(1);
        let stack_ranges = if self.separate_windows() {
            slots.chunks(len).map(|stack| range_of(stack).unwrap_or(value_range)).collect()
        } else {
            Vec::new()
        };

        let mut volume_issue = self.plan.final_volume_issue(decoded);
        // One volume per stack, over the shared slots, with the shared geometry; padding
        // voxels are sampled like the outside of the volume.
        let volumes: Vec<Volume> = self
            .plan
            .geometry
            .filter(|_| volume_issue.is_none())
            .and_then(|geometry| {
                let slices: Option<Vec<(Arc<VoxelBuffer>, Option<Padding>)>> = slots
                    .iter()
                    .map(|slot| match slot {
                        Some(Slot::Gray { data, padding, .. }) => Some((data.clone(), *padding)),
                        _ => None,
                    })
                    .collect();
                slices?
                    .chunks(len)
                    .map(|stack| {
                        let (data, padding): (Vec<_>, Vec<_>) = stack.iter().cloned().unzip();
                        Volume::from_shared(geometry, data).map(|volume| volume.with_padding(padding))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if volumes.is_empty() && volume_issue.is_none() {
            // The decoded images do not have the size their headers announce.
            volume_issue = Some(VolumeError::InconsistentGrid);
        }

        if failed > 0 {
            log::warn!("{failed} of {} images failed to decode", slots.len());
        }
        let failure = (failed == slots.len()).then(|| {
            DicomError::DecodeFailed(first_reason.unwrap_or(summrise_i18n::tr!("no pixel data")).to_owned()).to_string()
        });
        let warnings = match first_reason {
            Some(reason) if failure.is_none() => vec![decode_warning(failed, reason)],
            _ => Vec::new(),
        };
        let _ = self.outcome.set(Outcome { volumes, volume_issue, value_range, stack_ranges, warnings, failure });
    }

    /// Gives up on the whole series, e.g. when its loader could not start.
    pub(crate) fn abort(&self, reason: &str) {
        for instance in 0..self.slots_by_instance.len() {
            self.fill(instance, Err(reason.to_owned()), 0);
        }
        self.finish();
    }
}

/// Helpers for the tests of this crate: the synthetic CD, and loading without
/// the background loader.
#[cfg(test)]
pub(crate) mod testing {
    use summrise_dicom::{Media, Progress, Series, StudyIndex, decode_instance, plan_series, read_instance};
    use summrise_testkit::{PhantomOptions, write_phantom_study};

    use super::SeriesData;

    /// Writes the synthetic CD and indexes it (CT, MR, localizer, colour).
    pub(crate) fn phantom(opts: &PhantomOptions) -> (tempfile::TempDir, StudyIndex) {
        let dir = tempfile::tempdir().expect("tempdir");
        write_phantom_study(dir.path(), opts).expect("fixture");
        let index = summrise_dicom::scan(dir.path(), &Progress::new()).expect("scan");
        (dir, index)
    }

    /// Decodes one file of the synthetic CD (a folder) into the store, on the calling thread.
    pub(crate) fn fill_from_disc(data: &SeriesData, series: &Series, instance: usize) {
        let file = read_instance(&Media::folder(), &series.instances[instance]);
        let result = file.and_then(decode_instance).map_err(|e| e.to_string());
        data.fill(instance, result, data.initial_index());
    }

    /// Loads a whole series on the calling thread.
    pub(crate) fn load(series: &Series) -> SeriesData {
        let data = SeriesData::new(plan_series(series));
        for instance in 0..series.instances.len() {
            fill_from_disc(&data, series, instance);
        }
        data.finish();
        data
    }
}

#[cfg(test)]
mod tests {
    use summrise_dicom::{DecodedInstance, FrameInfo, SeriesPlan, plan_series};
    use summrise_testkit::PhantomOptions;

    use super::testing::{fill_from_disc, load, phantom};
    use super::*;

    fn is_pending(data: &SeriesData, index: usize) -> bool {
        matches!(data.image(index), ImageSlot::Pending)
    }

    #[test]
    fn slots_are_write_once_and_fixed_in_place() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let ct = &index.studies[0].series[0];
        let data = SeriesData::new(plan_series(ct));
        // Everything the headers tell is there before any pixel data.
        assert_eq!((data.len(), data.loaded(), data.byte_size()), (24, 0, 0));
        assert!(!data.is_viewable() && !data.is_complete());
        assert!((0..24).all(|i| is_pending(&data, i)));
        assert_eq!(data.initial_index(), 12);
        assert_eq!(data.units(), Some("HU"));
        assert_eq!(data.geometry().map(|g| g.dims), Some([48, 48, 24]));
        assert_eq!(data.value_range(), (-1024.0, 3071.0), "estimate until complete");
        let positions = data.frames().to_vec();

        // One file arrives: its image is there, every other image is still marked as not loaded.
        fill_from_disc(&data, ct, 12);
        assert!(matches!(data.image(12), ImageSlot::Ready(frame) if frame.size() == (48, 48)));
        assert!((0..24).filter(|&i| i != 12).all(|i| is_pending(&data, i)));
        assert_eq!((data.loaded(), data.byte_size()), (1, 48 * 48 * 2));
        assert!(data.is_viewable() && !data.is_complete());
        assert!(data.volume().is_none(), "no volume before the series is complete");

        // A slot is never overwritten.
        let FrameRef::Gray { data: pixels, .. } = data.frame(12).expect("loaded") else { panic!("grey image") };
        let before = std::ptr::from_ref(pixels);
        data.fill(12, Err("late failure".to_owned()), 12);
        fill_from_disc(&data, ct, 12);
        let FrameRef::Gray { data: pixels, .. } = data.frame(12).expect("still loaded") else { panic!("grey image") };
        assert!(std::ptr::eq(before, pixels));
        assert_eq!((data.loaded(), data.byte_size()), (1, 48 * 48 * 2));
        assert_eq!(data.frames(), positions.as_slice(), "order and positions never change");
    }

    #[test]
    fn first_decoded_image_fixes_window_and_presets() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let ct = &index.studies[0].series[0];
        let data = SeriesData::new(plan_series(ct));
        assert!(data.presets().is_empty());
        assert_eq!(data.initial_window(), Window::from_range(-1024.0, 3071.0), "nothing decoded yet");
        fill_from_disc(&data, ct, 12);
        let names: Vec<_> = data.presets().iter().map(|p| p.name.to_string()).collect();
        assert_eq!(names, ["Mediastinum", "Lung"]);
        let window = data.initial_window();
        assert_eq!((window.center, window.width), (40.0, 400.0));
        // Further images never change them.
        for instance in (0..24).filter(|&i| i != 12) {
            fill_from_disc(&data, ct, instance);
        }
        data.finish();
        assert_eq!(data.initial_window(), window);
        assert_eq!(data.presets().len(), 2);
    }

    #[test]
    fn complete_series_has_volume_and_exact_value_range() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let data = load(&index.studies[0].series[0]);
        assert!(data.is_complete() && data.failure().is_none() && data.warnings().is_empty());
        assert_eq!(data.loaded(), 24);
        let (lo, hi) = data.value_range();
        assert!(lo == -1000.0 && hi == 700.0, "values present, not the estimate: {lo}..{hi}");
        let volume = data.volume().expect("regular stack");
        assert_eq!(volume.geometry.dims, [48, 48, 24]);
        assert_eq!(data.volume_issue(), None);
        // The volume is assembled over the loaded images, not copied from them.
        for (index, slice) in volume.slices.iter().enumerate() {
            let FrameRef::Gray { data: pixels, .. } = data.frame(index).expect("loaded") else { panic!("grey") };
            assert!(std::ptr::eq(Arc::as_ptr(slice), pixels));
        }
        assert_eq!(data.byte_size(), volume.byte_size());
    }

    #[test]
    fn failed_image_keeps_its_place_and_its_reason() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let ct = &index.studies[0].series[0];
        let data = SeriesData::new(plan_series(ct));
        data.fill(16, Err("unexpected end of pixel data".to_owned()), 12);
        assert!(matches!(data.image(16), ImageSlot::Failed("unexpected end of pixel data")));
        assert!(data.frame(16).is_none());
        assert_eq!(data.loaded(), 1);
        assert!(!data.is_viewable(), "a failed image is not a first image");
        for instance in (0..24).filter(|&i| i != 16) {
            fill_from_disc(&data, ct, instance);
        }
        data.finish();
        // The other images display normally, at their own numbers.
        assert!((0..24).filter(|&i| i != 16).all(|i| data.frame(i).is_some()));
        assert_eq!(data.len(), 24);
        assert_eq!(data.warnings(), ["1 image could not be decoded: unexpected end of pixel data"]);
        assert!(data.failure().is_none());
        // A hole in the stack: no reformats, as with a missing file.
        assert!(data.volume().is_none() && data.geometry().is_none());
        assert!(matches!(data.volume_issue(), Some(VolumeError::IrregularSpacing { .. })));
        assert_eq!(data.value_range(), (-1000.0, 700.0));
    }

    #[test]
    fn series_without_a_decodable_image_fails() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let data = SeriesData::new(plan_series(&index.studies[0].series[2]));
        data.abort("JPEG decoder is missing");
        assert!(data.is_complete() && !data.is_viewable());
        assert_eq!(data.failure(), Some("none of the images in this series could be decoded: JPEG decoder is missing"));
        assert!(data.warnings().is_empty());
        assert_eq!(data.loaded(), 3);
    }

    #[test]
    fn frames_missing_from_a_file_are_failed_slots() {
        // One file announcing three 2 × 2 frames, but holding two.
        let frame = |frame| FrameInfo {
            instance: 0,
            frame,
            instance_number: Some(1),
            plane: None,
            pixel_spacing: None,
            width: 2,
            height: 2,
        };
        let plan = SeriesPlan {
            series_uid: "1.2.3".to_owned(),
            modality: "US".to_owned(),
            frames: (0..3).map(frame).collect(),
            is_color: false,
            monochrome1: false,
            units: None,
            value_range: (0.0, 255.0),
            geometry: None,
            volume_issue: Some(VolumeError::NoGeometry),
            stacks: None,
            suv: None,
        };
        let data = SeriesData::new(plan);
        let image =
            |value: f32| Frame { width: 2, height: 2, pixels: FramePixels::Gray(VoxelBuffer::compact(vec![value; 4])) };
        let decoded = DecodedInstance {
            frames: vec![image(10.0), image(20.0)],
            monochrome1: false,
            presets: Vec::new(),
            rescale_type: None,
            extras: Default::default(),
        };
        // The second frame is on screen: the automatic window comes from it.
        assert_eq!(data.fill(0, Ok(decoded), 1), [0, 1, 2]);
        assert!(data.frame(0).is_some() && data.frame(1).is_some());
        assert!(matches!(data.image(2), ImageSlot::Failed(reason) if reason.contains("fewer frames")));
        assert_eq!(data.initial_window().center, 20.5);
        data.finish();
        assert_eq!(data.value_range(), (10.0, 20.0));
        assert_eq!(data.volume_issue(), Some(&VolumeError::NoGeometry));
        assert_eq!(data.warnings().len(), 1);
    }
}
