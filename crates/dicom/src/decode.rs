//! Pixel data decoding: transfer syntaxes, Modality LUT (rescale to e.g.
//! Hounsfield units), photometric interpretation, multi-frame objects and
//! assembly into regular volumes.
//!
//! A series is displayed in two steps. [`plan_series`] derives the display
//! order and the volume geometry from the indexed headers, before any pixel
//! data is read; [`read_instance`] and [`decode_instance`] then supply the
//! pixels one file at a time, in whatever order suits the viewer.
//! [`load_series`] does all of it at once for callers that want a complete
//! series.

use std::borrow::Cow;
use std::panic::{AssertUnwindSafe, catch_unwind};

use dicom_dictionary_std::tags;
use dicom_object::file::ReadPreamble;
use dicom_object::{DefaultDicomObject, InMemDicomObject, OpenFileOptions};
use dicom_pixeldata::{DecodedPixelData, PhotometricInterpretation, PixelDecoder, PlanarConfiguration};
use log::{debug, warn};
use rayon::prelude::*;
use summrise_i18n::{tr, tr_n};
use summrise_imaging::window::subsample;
use summrise_imaging::{
    Frame, FramePixels, ImagePlane, PresetSource, VoiFunction, Volume, VolumeError, VolumeGeometry, VoxelBuffer,
    Window, WindowPreset, auto_window, sort_by_position,
};

use crate::attrs;
use crate::error::{DicomError, Result};
use crate::index::{InstanceHeader, Series};
use crate::media::Media;
use crate::pet::{self, SuvRefusal, SuvScale};
use crate::pixels::{self, InstanceExtras};
use crate::progress::Progress;
use crate::stacks::{self, FrameValues, Stacks};

/// Display order entry of a series.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameInfo {
    /// Index into [`Series::instances`].
    pub instance: usize,
    /// Frame number within that instance.
    pub frame: u32,
    pub instance_number: Option<i32>,
    pub plane: Option<ImagePlane>,
    pub pixel_spacing: Option<[f64; 2]>,
    /// Columns of the image.
    pub width: u32,
    /// Rows of the image.
    pub height: u32,
}

/// Pixel storage of a decoded series.
#[derive(Debug, Clone)]
pub enum SeriesImages {
    /// Regular, evenly spaced grey-scale stack: enables MPR.
    Volume(Volume),
    /// A series split into stacks ([`LoadedSeries::stacks`]): one volume per
    /// stack, all with the same geometry.
    Stacks(Vec<Volume>),
    /// Any other set of frames (mixed orientations, colour, projections…).
    Frames(Vec<Frame>),
}

/// Borrowed view of one displayed frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameRef<'a> {
    Gray { width: u32, height: u32, data: &'a VoxelBuffer },
    Rgb { width: u32, height: u32, data: &'a [u8] },
}

impl FrameRef<'_> {
    pub fn size(&self) -> (u32, u32) {
        match *self {
            Self::Gray { width, height, .. } | Self::Rgb { width, height, .. } => (width, height),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedSeries {
    pub series_uid: String,
    pub modality: String,
    pub frames: Vec<FrameInfo>,
    pub images: SeriesImages,
    /// Windows stored in the DICOM files (VOI LUT module).
    pub presets: Vec<WindowPreset>,
    pub initial_window: Window,
    /// Smallest and largest modality value in the series.
    pub value_range: (f32, f32),
    /// MONOCHROME1: minimum value is displayed white.
    pub monochrome1: bool,
    pub is_color: bool,
    /// Unit of grey values, e.g. "HU" for CT.
    pub units: Option<String>,
    /// Why multiplanar reformatting is unavailable, if it is.
    pub volume_issue: Option<VolumeError>,
    pub warnings: Vec<String>,
    /// How the series is split into stacks, if it holds several sets of
    /// images at the same positions; `frames` is then stack-major.
    pub stacks: Option<Stacks>,
}

impl LoadedSeries {
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn frame(&self, index: usize) -> Option<FrameRef<'_>> {
        match &self.images {
            SeriesImages::Volume(v) => volume_slice(v, index),
            SeriesImages::Stacks(volumes) => {
                let len = volumes.first()?.geometry.dims[2].max(1);
                volume_slice(volumes.get(index / len)?, index % len)
            }
            SeriesImages::Frames(frames) => frames.get(index).map(|f| match &f.pixels {
                FramePixels::Gray(data) => FrameRef::Gray { width: f.width, height: f.height, data },
                FramePixels::Rgb(data) => FrameRef::Rgb { width: f.width, height: f.height, data },
            }),
        }
    }

    /// The volume of the series, or of its first stack.
    pub fn volume(&self) -> Option<&Volume> {
        self.stack_volume(0)
    }

    /// The volume of stack `stack` (0 for a series that is not split).
    pub fn stack_volume(&self, stack: usize) -> Option<&Volume> {
        match &self.images {
            SeriesImages::Volume(v) => (stack == 0).then_some(v),
            SeriesImages::Stacks(volumes) => volumes.get(stack),
            SeriesImages::Frames(_) => None,
        }
    }

    pub fn byte_size(&self) -> usize {
        match &self.images {
            SeriesImages::Volume(v) => v.byte_size(),
            SeriesImages::Stacks(volumes) => volumes.iter().map(Volume::byte_size).sum(),
            SeriesImages::Frames(f) => f.iter().map(Frame::byte_size).sum(),
        }
    }
}

/// Slice `index` of a volume as a displayed frame.
fn volume_slice(v: &Volume, index: usize) -> Option<FrameRef<'_>> {
    let [w, h, _] = v.geometry.dims;
    v.slices.get(index).map(|data| FrameRef::Gray { width: w as u32, height: h as u32, data })
}

/// How a series will be displayed, as far as its headers tell: one entry per
/// image in display order, and whether the images form a regular volume.
/// Nothing here changes while the pixel data is decoded.
#[derive(Debug, Clone)]
pub struct SeriesPlan {
    pub series_uid: String,
    pub modality: String,
    /// Every displayed image, in display order.
    pub frames: Vec<FrameInfo>,
    pub is_color: bool,
    /// MONOCHROME1: minimum value is displayed white.
    pub monochrome1: bool,
    /// Unit of grey values, e.g. "HU" for CT.
    pub units: Option<String>,
    /// Range the modality values can span, estimated from Bits Stored, Pixel
    /// Representation and the rescale values (see
    /// [`InstanceHeader::value_range_estimate`]). The values actually present
    /// are only known after decoding.
    pub value_range: (f32, f32),
    /// Geometry of the volume the images form, if they form one.
    pub geometry: Option<VolumeGeometry>,
    /// Why multiplanar reformatting will be unavailable, if the headers
    /// already rule it out.
    pub volume_issue: Option<VolumeError>,
    /// How the series is split into stacks, if it holds several sets of
    /// images at the same positions: `frames` is then stack-major (stack 0
    /// sorted by position, then stack 1 …) and `geometry` is that of every
    /// stack.
    pub stacks: Option<Stacks>,
    /// For a PET series: the factor that turns its stored values into SUV
    /// (applied by [`SuvScale::apply`] as images are decoded), or why there is
    /// none. `None` for other series.
    pub suv: Option<Result<SuvScale, SuvRefusal>>,
}

impl SeriesPlan {
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Number of stacks: 1 for a series that is not split.
    pub fn stack_count(&self) -> usize {
        self.stacks.as_ref().map_or(1, Stacks::count)
    }

    /// Images in every stack: all of them for a series that is not split.
    pub fn stack_len(&self) -> usize {
        self.stacks.as_ref().map_or(self.frames.len(), |s| s.len)
    }

    /// Display indices of the frames of each file: `[instance][frame]`, with
    /// `instance` indexing [`Series::instances`].
    pub fn slots_by_instance(&self) -> Vec<Vec<usize>> {
        let instances = self.frames.iter().map(|f| f.instance + 1).max().unwrap_or(0);
        let mut slots = vec![Vec::new(); instances];
        for (slot, info) in self.frames.iter().enumerate() {
            let of_instance: &mut Vec<usize> = &mut slots[info.instance];
            let frame = info.frame as usize;
            if of_instance.len() <= frame {
                of_instance.resize(frame + 1, slot);
            }
            of_instance[frame] = slot;
        }
        slots
    }

    /// The reformatting verdict once every image has been decoded or has
    /// failed (`decoded` tells which, by display index). Images that failed
    /// keep their place in the series, so the stack has a hole: the verdict
    /// of the headers becomes "not evenly spaced". In a series split into
    /// stacks the first stack with a hole decides, for every stack.
    pub fn final_volume_issue(&self, decoded: impl Fn(usize) -> bool) -> Option<VolumeError> {
        if let Some(issue) = &self.volume_issue {
            return Some(issue.clone());
        }
        if (0..self.frames.len()).all(&decoded) {
            return None;
        }
        let (n, len) = (self.frames.len(), self.stack_len().max(1));
        let stack = (0..n).step_by(len).map(|start| start..(start + len).min(n)).find(|s| !s.clone().all(&decoded))?;
        let remaining: Vec<ImagePlane> =
            stack.filter(|&i| decoded(i)).filter_map(|i| self.frames.get(i).and_then(|f| f.plane)).collect();
        Some(match VolumeGeometry::from_sorted_planes(&remaining) {
            Err(issue @ VolumeError::IrregularSpacing { .. }) => issue,
            // The hole is at an end of the stack (or too little is left to tell).
            _ => VolumeError::IrregularSpacing { max_deviation_mm: self.geometry.map_or(0.0, |g| g.spacing().z) },
        })
    }
}

/// Plans how `series` will be displayed from its indexed headers alone: the
/// display order, whether the images form a regular volume (and its
/// geometry), and the display parameters that do not depend on pixel data.
/// Decoding then only fills in pixels, so no image changes place while a
/// series loads.
pub fn plan_series(series: &Series) -> SeriesPlan {
    let infos = frame_infos(series);
    let is_color = series.instances.iter().any(InstanceHeader::is_color);
    let Order { frames: order, verdict, stacks } = display_order(&infos, &frame_values(series, &infos), is_color);
    let frames = order.iter().map(|&i| infos[i].clone()).collect();
    let value_range = series
        .instances
        .iter()
        .filter_map(InstanceHeader::value_range_estimate)
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
        .map_or((0.0, 255.0), |(lo, hi)| (lo as f32, hi as f32));
    let suv = pet::uptake(series);
    let value_range = suv.as_ref().and_then(|s| s.as_ref().ok()).map_or(value_range, |s| s.range(value_range));
    SeriesPlan {
        series_uid: series.uid.clone(),
        modality: series.modality.clone(),
        frames,
        is_color,
        // Presentation LUT Shape INVERSE is honoured like MONOCHROME1 (grey-scale images only).
        monochrome1: series
            .instances
            .iter()
            .any(|i| i.photometric == "MONOCHROME1" || (i.fidelity.inverse && !i.is_color())),
        units: pet::display_unit(suv.as_ref(), series.pet.as_ref())
            .or_else(|| units(series.instances.iter().find_map(|i| i.rescale_type.clone()), &series.modality)),
        value_range,
        geometry: verdict.as_ref().ok().copied(),
        volume_issue: verdict.err(),
        stacks,
        suv,
    }
}

/// How `series` is split into stacks, from its headers, as [`plan_series`]
/// splits it: for callers that need nothing else of the plan (the series
/// browser says which series have stacks before any is loaded).
pub fn series_stacks(series: &Series) -> Option<Stacks> {
    let infos = frame_infos(series);
    let is_color = series.instances.iter().any(InstanceHeader::is_color);
    display_order(&infos, &frame_values(series, &infos), is_color).stacks
}

/// Every frame of every file of `series`, in index order.
fn frame_infos(series: &Series) -> Vec<FrameInfo> {
    series
        .instances
        .iter()
        .enumerate()
        .flat_map(|(index, header)| {
            (0..header.frames).map(move |frame| FrameInfo {
                instance: index,
                frame,
                instance_number: header.instance_number,
                plane: header.frame_planes.get(frame as usize).copied().flatten(),
                pixel_spacing: header.pixel_spacing,
                width: header.cols,
                height: header.rows,
            })
        })
        .collect()
}

/// The values that tell stacks apart, of each of `infos`.
fn frame_values<'a>(series: &'a Series, infos: &[FrameInfo]) -> Vec<&'a FrameValues> {
    infos
        .iter()
        .map(|f| {
            let values = series.instances.get(f.instance).map(|h| h.frame_values.as_slice()).unwrap_or_default();
            stacks::of_frame(values, f.frame)
        })
        .collect()
}

/// Unit of the grey values: the Rescale Type, or Hounsfield units for CT.
fn units(rescale_type: Option<String>, modality: &str) -> Option<String> {
    rescale_type.filter(|u| !u.is_empty() && u != "US").or_else(|| (modality == "CT").then(|| "HU".to_owned()))
}

/// The notice shown when some, but not all, images of a series failed to decode.
pub fn decode_warning(failed: usize, first_reason: &str) -> String {
    tr_n!(
        "{n} image could not be decoded: {reason}",
        "{n} images could not be decoded: {reason}",
        failed,
        reason = first_reason
    )
}

/// Everything decoded from one file.
#[derive(Debug, Clone)]
pub struct DecodedInstance {
    /// All frames of the file, in frame order.
    pub frames: Vec<Frame>,
    pub monochrome1: bool,
    /// Windows stored in the file (VOI LUT module).
    pub presets: Vec<WindowPreset>,
    pub rescale_type: Option<String>,
    /// Contrast tables, padding and scanner graphics.
    pub extras: InstanceExtras,
}

impl DecodedInstance {
    /// The window this image opens with: its first stored window, else an
    /// automatic window from the statistics of frame `frame` (padding left
    /// out), else the whole `value_range` (colour images).
    pub fn initial_window(&self, frame: usize, value_range: (f32, f32)) -> Window {
        self.presets
            .first()
            .map(|p| p.window)
            .or_else(|| pixels::auto_window_of(self.frames.get(frame)?, self.extras.padding(frame)))
            .unwrap_or_else(|| Window::from_range(f64::from(value_range.0), f64::from(value_range.1)))
    }
}

/// Reads the encoded bytes of one image file from `media`, the media of the
/// index the header is from. Reading is kept apart from decoding so that a
/// single thread can read from slow media (sequential access on optical
/// discs) while decoding runs in parallel from memory.
pub fn read_instance(media: &Media, header: &InstanceHeader) -> Result<Vec<u8>> {
    media.read(&header.path)
}

/// Decodes every frame of one file from its bytes, as read by
/// [`read_instance`]. Malformed data yields an error, never a panic.
pub fn decode_instance(bytes: Vec<u8>) -> Result<DecodedInstance> {
    catch_unwind(AssertUnwindSafe(|| decode_bytes(bytes)))
        // Translators: the reason an image could not be decoded: the decoder stopped on damaged data.
        .unwrap_or_else(|_| Err(tr!("decoder panicked on malformed data").into()))
        .map_err(DicomError::Image)
}

/// Decodes every frame of `series` at once (in parallel) and assembles a
/// volume when the geometry allows it. Nothing is available until the whole
/// series is decoded; viewers use [`plan_series`] and [`decode_instance`] to
/// show images as they arrive. The files are read from `media`, the media of
/// the index the series is from.
pub fn load_series(media: &Media, series: &Series, progress: &Progress) -> Result<LoadedSeries> {
    progress.begin(tr!("Decoding images"), series.instances.len());
    let decoded: Vec<std::result::Result<DecodedInstance, String>> = series
        .instances
        .par_iter()
        .map(|header| {
            if progress.is_cancelled() {
                return Err("cancelled".into());
            }
            let r = catch_unwind(AssertUnwindSafe(|| decode_file(media, header)))
                .unwrap_or_else(|_| Err(tr!("decoder panicked on malformed data").into()));
            progress.advance();
            r
        })
        .collect();
    if progress.is_cancelled() {
        return Err(DicomError::Cancelled);
    }

    let mut infos = Vec::new();
    let mut frames = Vec::new();
    let mut paddings = Vec::new();
    let mut failures = Vec::new();
    let mut presets = Vec::new();
    let mut monochrome1 = false;
    let mut rescale_type = None;
    let suv = pet::uptake(series);
    for (index, (header, result)) in series.instances.iter().zip(decoded).enumerate() {
        match result {
            Ok(mut inst) => {
                if let Some(Ok(scale)) = &suv {
                    scale.apply(&mut inst);
                }
                if presets.is_empty() {
                    presets = inst.presets;
                }
                monochrome1 |= inst.monochrome1;
                rescale_type = rescale_type.or(inst.rescale_type);
                for (f, frame) in inst.frames.into_iter().enumerate() {
                    paddings.push(inst.extras.padding(f));
                    infos.push(FrameInfo {
                        instance: index,
                        frame: f as u32,
                        instance_number: header.instance_number,
                        plane: header.frame_planes.get(f).copied().flatten(),
                        pixel_spacing: header.pixel_spacing,
                        width: frame.width,
                        height: frame.height,
                    });
                    frames.push(frame);
                }
            }
            Err(e) => {
                debug!("decode failed for {}: {e}", header.path.display());
                failures.push(e);
            }
        }
    }
    if frames.is_empty() {
        let reason = failures.first().cloned().unwrap_or_else(|| tr!("no pixel data").to_owned());
        return Err(DicomError::DecodeFailed(reason));
    }
    let mut warnings = Vec::new();
    if !failures.is_empty() {
        warn!("{} of {} images failed to decode", failures.len(), series.instances.len());
        warnings.push(decode_warning(failures.len(), &failures[0]));
    }

    let is_color = frames.iter().any(|f| matches!(f.pixels, FramePixels::Rgb(_)));
    let Order { frames: order, verdict, stacks } = display_order(&infos, &frame_values(series, &infos), is_color);
    let volume_issue = verdict.as_ref().err().cloned();
    let mut slots: Vec<Option<(FrameInfo, Frame)>> = infos.into_iter().zip(frames).map(Some).collect();
    let (infos, frames): (Vec<FrameInfo>, Vec<Frame>) = order.iter().filter_map(|&i| slots[i].take()).unzip();
    let paddings: Vec<_> = order.iter().map(|&i| paddings.get(i).copied().flatten()).collect();

    let value_range = gray_range(&frames, &paddings).unwrap_or((0.0, 255.0));
    let initial_window = presets.first().map(|p| p.window).unwrap_or_else(|| {
        let mid = frames.len() / 2;
        let sampled = std::iter::once(mid).chain((0..frames.len()).step_by(frames.len().div_ceil(8).max(1)));
        auto_window(sampled.flat_map(|i| match &frames[i].pixels {
            FramePixels::Gray(b) => {
                subsample(b.len(), 65_536, |j| summrise_imaging::padding::visible(b.get(j), paddings[i]))
            }
            FramePixels::Rgb(_) => Vec::new(),
        }))
        .unwrap_or_else(|| Window::from_range(f64::from(value_range.0), f64::from(value_range.1)))
    });

    // One volume, or one per stack, over the geometry the display order found.
    let stack_len = stacks.as_ref().map_or(frames.len(), |s| s.len).max(1);
    let volumes: Option<Vec<Volume>> = verdict.ok().filter(|_| volume_issue.is_none()).and_then(|geometry| {
        frames
            .chunks(stack_len)
            .zip(paddings.chunks(stack_len))
            .map(|(stack, padding)| {
                let slices = stack
                    .iter()
                    .filter_map(|f| match &f.pixels {
                        FramePixels::Gray(b) => Some(b.clone()),
                        FramePixels::Rgb(_) => None,
                    })
                    .collect();
                Volume::new(geometry, slices).map(|v| v.with_padding(padding.to_vec()))
            })
            .collect()
    });
    let images = match (volumes, &stacks) {
        (Some(volumes), Some(_)) => SeriesImages::Stacks(volumes),
        (Some(mut volumes), None) if volumes.len() == 1 => SeriesImages::Volume(volumes.remove(0)),
        _ => SeriesImages::Frames(frames),
    };
    let units = pet::display_unit(suv.as_ref(), series.pet.as_ref()).or_else(|| units(rescale_type, &series.modality));

    Ok(LoadedSeries {
        series_uid: series.uid.clone(),
        modality: series.modality.clone(),
        frames: infos,
        images,
        presets,
        initial_window,
        value_range,
        monochrome1,
        is_color,
        units,
        volume_issue,
        warnings,
        stacks,
    })
}

/// The display order of the frames of a series (indices into them), the
/// verdict about the volume they form (or each of their stacks forms), and
/// their stacks.
struct Order {
    frames: Vec<usize>,
    verdict: std::result::Result<VolumeGeometry, VolumeError>,
    stacks: Option<Stacks>,
}

/// Spatial order for regular stacks (aligned with acquisition order), else
/// instance/frame order; a series that holds several sets of images at the
/// same positions is split into stacks (see [`crate::stacks`]). Also gives the
/// geometry of the volume the stack forms, or why it is not one. Needs nothing
/// but header values (`values` are those of each of `infos`), so the order is
/// the same whether it is planned before decoding or derived after it.
fn display_order(infos: &[FrameInfo], values: &[&FrameValues], is_color: bool) -> Order {
    let natural: Vec<usize> = {
        let mut o: Vec<usize> = (0..infos.len()).collect();
        o.sort_by_key(|&i| (infos[i].instance_number.unwrap_or(i32::MAX), infos[i].instance, infos[i].frame));
        o
    };
    let flat = |frames, verdict| Order { frames, verdict, stacks: None };
    if is_color {
        return flat(natural, Err(VolumeError::NotGrayscale));
    }
    let planes: Option<Vec<ImagePlane>> = infos.iter().map(|i| i.plane).collect();
    let Some(planes) = planes else {
        return flat(natural, Err(VolumeError::NoGeometry));
    };
    if planes.len() < 2 {
        return flat(natural, Err(VolumeError::TooFewSlices));
    }
    let same_size = infos.iter().all(|f| f.width == infos[0].width && f.height == infos[0].height);
    if !same_size || !planes.iter().all(|p| p.same_grid(&planes[0])) {
        return flat(natural, Err(VolumeError::InconsistentGrid));
    }
    let mut spatial = sort_by_position(&planes);
    let key = |i: usize| (infos[i].instance_number.unwrap_or(0), infos[i].frame);
    if let (Some(&first), Some(&last)) = (spatial.first(), spatial.last())
        && key(first) > key(last)
    {
        spatial.reverse();
    }
    let sorted: Vec<ImagePlane> = spatial.iter().map(|&i| planes[i]).collect();
    match VolumeGeometry::from_sorted_planes(&sorted) {
        Ok(geometry) => flat(spatial, Ok(geometry)),
        Err(VolumeError::DuplicatePositions) => match stacks::split(&planes, &natural, values, key) {
            Some(split) => Order { frames: split.order, verdict: split.verdict, stacks: Some(split.stacks) },
            // A cine of one slice is played in time order.
            None if stacks::at_one_position(&planes) => {
                flat(stacks::in_time_order(&natural, values), Err(VolumeError::DuplicatePositions))
            }
            None => flat(natural, Err(VolumeError::DuplicatePositions)),
        },
        Err(e) => flat(spatial, Err(e)),
    }
}

fn gray_range(frames: &[Frame], paddings: &[Option<summrise_imaging::Padding>]) -> Option<(f32, f32)> {
    frames
        .par_iter()
        .enumerate()
        .filter_map(|(i, f)| match &f.pixels {
            FramePixels::Gray(b) => summrise_imaging::padding::min_max(b, paddings.get(i).copied().flatten()),
            FramePixels::Rgb(_) => None,
        })
        .reduce_with(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

/// Opens and decodes one file from the media.
fn decode_file(media: &Media, header: &InstanceHeader) -> std::result::Result<DecodedInstance, String> {
    let obj = media.open_object(&header.path, None)?;
    decode_object(&obj)
}

/// Parses and decodes one file held in memory.
fn decode_bytes(bytes: Vec<u8>) -> std::result::Result<DecodedInstance, String> {
    // Treat the 128-byte preamble exactly as opening the file by path does.
    let preamble = if bytes.starts_with(b"DICM") && bytes.get(128..132) != Some(b"DICM".as_slice()) {
        ReadPreamble::Never
    } else {
        ReadPreamble::Always
    };
    let obj =
        OpenFileOptions::new().read_preamble(preamble).from_reader(bytes.as_slice()).map_err(|e| e.to_string())?;
    // The parsed object owns its own copy of the pixel data.
    drop(bytes);
    decode_object(&obj)
}

fn decode_object(obj: &DefaultDicomObject) -> std::result::Result<DecodedInstance, String> {
    let decoded = obj.decode_pixel_data().map_err(|e| e.to_string())?;
    // Stored values masked to Bits Stored, the modality transform, padding, palette and
    // scanner graphics: see `pixels` (display-fidelity D2, D3).
    let pixels::Decoded { frames, extras, inverted } = pixels::decode_frames(obj, &decoded, rgb_frame)?;
    let base: &InMemDicomObject = obj;
    Ok(DecodedInstance {
        frames,
        monochrome1: inverted,
        presets: dicom_windows(base),
        rescale_type: attrs::text(base, tags::RESCALE_TYPE).or_else(|| crate::fidelity::modality_lut_type(base)),
        extras,
    })
}

fn rgb_frame(decoded: &DecodedPixelData<'_>, frame: u32) -> std::result::Result<Vec<u8>, String> {
    if decoded.bits_allocated() != 8 {
        return Err(tr!("{bits}-bit colour images are not supported yet", bits = decoded.bits_allocated()));
    }
    let data = decoded.frame_data(frame).map_err(|e| e.to_string())?;
    let mut rgb = match decoded.planar_configuration() {
        PlanarConfiguration::Standard => data.to_vec(),
        PlanarConfiguration::PixelFirst => {
            let n = data.len() / 3;
            (0..n).flat_map(|i| [data[i], data[n + i], data[2 * n + i]]).collect()
        }
    };
    match decoded.photometric_interpretation() {
        PhotometricInterpretation::Rgb => {}
        PhotometricInterpretation::YbrFull | PhotometricInterpretation::YbrFull422 => ybr_to_rgb(&mut rgb),
        // Translators: {kind} is a DICOM photometric interpretation, such as YBR_RCT.
        other => return Err(tr!("{kind} colour images are not supported yet", kind = other)),
    }
    Ok(rgb)
}

/// ITU-R BT.601 full-range YCbCr → RGB (PS3.3 C.7.6.3.1.2).
fn ybr_to_rgb(px: &mut [u8]) {
    for p in px.as_chunks_mut::<3>().0 {
        let (y, cb, cr) = (f32::from(p[0]), f32::from(p[1]) - 128.0, f32::from(p[2]) - 128.0);
        p[0] = (y + 1.402 * cr).round().clamp(0.0, 255.0) as u8;
        p[1] = (y - 0.344_136 * cb - 0.714_136 * cr).round().clamp(0.0, 255.0) as u8;
        p[2] = (y + 1.772 * cb).round().clamp(0.0, 255.0) as u8;
    }
}

/// Window presets from the VOI LUT module (all values, with explanations),
/// falling back to Enhanced multi-frame functional groups.
pub(crate) fn dicom_windows(obj: &InMemDicomObject) -> Vec<WindowPreset> {
    let source = if obj.get(tags::WINDOW_CENTER).is_some() {
        Some(obj)
    } else {
        attrs::shared_group(obj).and_then(|g| attrs::functional(g, tags::FRAME_VOILUT_SEQUENCE)).or_else(|| {
            attrs::per_frame_groups(obj).first().and_then(|g| attrs::functional(g, tags::FRAME_VOILUT_SEQUENCE))
        })
    };
    let Some(src) = source else { return Vec::new() };
    let centers = attrs::floats(src, tags::WINDOW_CENTER);
    let widths = attrs::floats(src, tags::WINDOW_WIDTH);
    let explanations = attrs::texts(src, tags::WINDOW_CENTER_WIDTH_EXPLANATION);
    let function =
        attrs::text(src, tags::VOILUT_FUNCTION).and_then(|f| VoiFunction::from_dicom(&f)).unwrap_or_default();
    centers
        .iter()
        .zip(&widths)
        .enumerate()
        .filter(|(_, (c, w))| c.is_finite() && w.is_finite() && **w > 0.0)
        .map(|(i, (&c, &w))| WindowPreset {
            name: Cow::Owned(
                explanations
                    .get(i)
                    .filter(|e| !e.is_empty())
                    // Translators: the name of a contrast window stored in the images without a name of its own.
                    .map_or_else(|| tr!("Window {number}", number = i + 1), |e| title_case(e)),
            ),
            window: Window::new(c, w).with_function(function),
            source: PresetSource::Dicom,
        })
        .collect()
}

pub(crate) fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// An RGBA preview of a series (middle image, default window).
#[derive(Debug, Clone)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decodes only the representative image of `series` (read from `media`) and
/// renders a thumbnail no larger than `max_size` pixels per side.
pub fn thumbnail(media: &Media, series: &Series, max_size: u32) -> Result<Thumbnail> {
    let header = series.representative().ok_or_else(|| DicomError::DecodeFailed("empty series".into()))?;
    let inst = catch_unwind(AssertUnwindSafe(|| decode_file(media, header)))
        .unwrap_or_else(|_| Err("decoder panicked".into()))
        .map_err(DicomError::DecodeFailed)?;
    let middle = inst.frames.len() / 2;
    let frame = inst.frames.get(middle).ok_or_else(|| DicomError::DecodeFailed("no frames".into()))?;
    let rgba = match &frame.pixels {
        // As the series opens: the stored window, else the first contrast table, else an automatic
        // window; padding is background.
        FramePixels::Gray(b) => {
            let padding = inst.extras.padding(middle);
            let value = |i| summrise_imaging::padding::visible(b.get(i), padding);
            match (inst.presets.first(), inst.extras.voi_luts.first()) {
                (None, Some(table)) => {
                    summrise_imaging::window::lut_to_rgba(b.len(), value, &table.table, inst.monochrome1)
                }
                (stored, _) => {
                    let window = stored.map(|p| p.window).or_else(|| pixels::auto_window_of(frame, padding));
                    let window = window.unwrap_or_else(|| Window::new(127.5, 256.0));
                    summrise_imaging::window::gray_to_rgba(b.len(), value, &window, inst.monochrome1)
                }
            }
        }
        FramePixels::Rgb(rgb) => summrise_imaging::window::rgb_to_rgba(rgb, false),
    };
    Ok(downscale(frame.width, frame.height, &rgba, max_size))
}

/// Box-filter downscale of RGBA8 so that the longest side is ≤ `max`.
fn downscale(w: u32, h: u32, rgba: &[u8], max: u32) -> Thumbnail {
    let scale = (f64::from(w.max(h)) / f64::from(max.max(1))).max(1.0);
    let (tw, th) = (((f64::from(w) / scale).round() as u32).max(1), ((f64::from(h) / scale).round() as u32).max(1));
    let mut out = vec![0u8; (tw * th * 4) as usize];
    for ty in 0..th {
        let y0 = (f64::from(ty) * scale) as u32;
        let y1 = ((f64::from(ty + 1) * scale) as u32).clamp(y0 + 1, h);
        for tx in 0..tw {
            let x0 = (f64::from(tx) * scale) as u32;
            let x1 = ((f64::from(tx + 1) * scale) as u32).clamp(x0 + 1, w);
            let mut acc = [0u32; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += u32::from(rgba[i + c]);
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            let o = ((ty * tw + tx) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    Thumbnail { width: tw, height: th, rgba: out }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use summrise_testkit::stacks::{self as phantoms, write_stacks_study};
    use summrise_testkit::{PhantomOptions, write_phantom_study};

    use super::*;
    use crate::index::{Rescale, StudyIndex, scan};
    use crate::stacks::StackKind;

    /// The synthetic CD: CT, MR, three-plane localizer and colour capture.
    fn phantom() -> (tempfile::TempDir, StudyIndex) {
        let dir = tempfile::tempdir().expect("tempdir");
        write_phantom_study(dir.path(), &PhantomOptions::small()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        (dir, index)
    }

    /// Header of a 4 × 4 axial slice with 1 mm pixels at `position`.
    fn slice(instance_number: i32, position: [f64; 3]) -> InstanceHeader {
        InstanceHeader {
            path: PathBuf::from(format!("IM{instance_number:06}")),
            sop_instance_uid: format!("1.2.3.{instance_number}"),
            sop_class_uid: String::new(),
            transfer_syntax_uid: String::new(),
            instance_number: Some(instance_number),
            frames: 1,
            rows: 4,
            cols: 4,
            samples_per_pixel: 1,
            photometric: "MONOCHROME2".to_owned(),
            pixel_spacing: Some([1.0, 1.0]),
            calibration: Default::default(),
            frame_planes: vec![ImagePlane::from_dicom(position, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [1.0, 1.0], 4, 4)],
            image_type: Vec::new(),
            bits_stored: 12,
            signed: false,
            rescale: vec![Rescale::IDENTITY],
            rescale_type: None,
            frame_values: Vec::new(),
            fidelity: Default::default(),
        }
    }

    fn series(instances: Vec<InstanceHeader>) -> Series {
        Series {
            uid: "1.2.3".to_owned(),
            number: Some(1),
            description: None,
            modality: "MR".to_owned(),
            body_part: None,
            acquisition: Default::default(),
            frame_of_reference: None,
            pet: None,
            instances,
        }
    }

    /// Slices 2 mm apart at the given heights, numbered from 1 in that order.
    fn stack(heights: &[f64]) -> Series {
        series(heights.iter().enumerate().map(|(i, &z)| slice(i as i32 + 1, [0.0, 0.0, z])).collect())
    }

    fn instance_numbers(plan: &SeriesPlan) -> Vec<i32> {
        plan.frames.iter().map(|f| f.instance_number.unwrap_or(0)).collect()
    }

    #[test]
    fn plan_gives_todays_order_for_every_synthetic_series() {
        let (_dir, index) = phantom();
        let study = &index.studies[0];
        assert_eq!(study.series.len(), 4, "CT, MR, localizer, colour");
        for series in &study.series {
            let plan = plan_series(series);
            let loaded = load_series(&index.media, series, &Progress::new()).expect("decode");
            let title = series.title();
            // Same images in the same order, with the same positions and sizes.
            assert_eq!(plan.frames, loaded.frames, "{title}");
            assert_eq!(plan.volume_issue, loaded.volume_issue, "{title}");
            assert_eq!(plan.geometry, loaded.volume().map(|v| v.geometry), "{title}");
            assert_eq!(plan.is_color, loaded.is_color, "{title}");
            assert_eq!(plan.monochrome1, loaded.monochrome1, "{title}");
            assert_eq!(plan.units, loaded.units, "{title}");
            assert_eq!(plan.modality, loaded.modality, "{title}");
            let (lo, hi) = loaded.value_range;
            assert!(plan.value_range.0 <= lo && hi <= plan.value_range.1, "{title}: {:?}", plan.value_range);
        }

        // CT: files are stored shuffled; image 1 is the most superior slice.
        let ct = plan_series(&study.series[0]);
        assert_eq!(instance_numbers(&ct), (1..=24).collect::<Vec<_>>());
        let z: Vec<f64> = ct.frames.iter().map(|f| f.plane.map_or(0.0, |p| p.position.z)).collect();
        assert!(z.windows(2).all(|w| w[0] > w[1]), "superior to inferior: {z:?}");
        assert_eq!(ct.geometry.map(|g| g.dims), Some([48, 48, 24]));
        assert_eq!(ct.units.as_deref(), Some("HU"));
        // MR: sagittal stack running from patient left to right.
        let mr = plan_series(&study.series[1]);
        assert_eq!(instance_numbers(&mr), (1..=12).collect::<Vec<_>>());
        let x: Vec<f64> = mr.frames.iter().map(|f| f.plane.map_or(0.0, |p| p.position.x)).collect();
        assert!(x.windows(2).all(|w| w[0] > w[1]), "left to right: {x:?}");
        assert!(mr.volume_issue.is_none() && mr.units.is_none());
        // Localizer: three orientations, shown in instance order.
        let localizer = plan_series(&study.series[2]);
        assert_eq!(instance_numbers(&localizer), vec![1, 2, 3]);
        assert_eq!(localizer.volume_issue, Some(VolumeError::InconsistentGrid));
        // Colour secondary capture.
        let colour = plan_series(&study.series[3]);
        assert_eq!((colour.len(), colour.is_color), (1, true));
        assert_eq!(colour.volume_issue, Some(VolumeError::NotGrayscale));
        assert!(colour.geometry.is_none());
    }

    #[test]
    fn plan_orders_stacks_spatially_from_the_lowest_instance_number() {
        // Instance 1 is the lowest slice.
        let up = plan_series(&stack(&[0.0, 2.0, 4.0, 6.0, 8.0]));
        assert_eq!(instance_numbers(&up), vec![1, 2, 3, 4, 5]);
        assert_eq!(up.volume_issue, None);
        assert_eq!(up.geometry.map(|g| g.dims), Some([4, 4, 5]));
        // Instance 1 is the highest slice, files indexed in a shuffled order.
        let mut shuffled = stack(&[8.0, 6.0, 4.0, 2.0, 0.0]);
        shuffled.instances.swap(0, 3);
        shuffled.instances.swap(1, 4);
        let down = plan_series(&shuffled);
        assert_eq!(instance_numbers(&down), vec![1, 2, 3, 4, 5]);
        let z: Vec<f64> = down.frames.iter().map(|f| f.plane.map_or(0.0, |p| p.position.z)).collect();
        assert_eq!(z, vec![8.0, 6.0, 4.0, 2.0, 0.0]);
        assert_eq!(down.volume_issue, None);
    }

    #[test]
    fn plan_reports_why_a_stack_is_not_a_volume() {
        // A missing slice: still in spatial order, but not evenly spaced.
        let gap = plan_series(&stack(&[0.0, 2.0, 4.0, 8.0]));
        assert!(matches!(gap.volume_issue, Some(VolumeError::IrregularSpacing { .. })), "{:?}", gap.volume_issue);
        assert_eq!(instance_numbers(&gap), vec![1, 2, 3, 4]);
        assert!(gap.geometry.is_none());
        // Repeated positions that do not repeat evenly: instance order.
        let repeated = plan_series(&stack(&[2.0, 0.0, 2.0]));
        assert_eq!(repeated.volume_issue, Some(VolumeError::DuplicatePositions));
        assert_eq!(instance_numbers(&repeated), vec![1, 2, 3]);
        assert!(repeated.stacks.is_none());
        // Evenly repeated (two echoes that record nothing): two stacks of two slices.
        let echoes = plan_series(&stack(&[2.0, 0.0, 2.0, 0.0]));
        assert_eq!(echoes.volume_issue, None);
        assert_eq!(echoes.stacks.as_ref().map(|s| (s.kind, s.count(), s.len)), Some((StackKind::Part, 2, 2)));
        assert_eq!(instance_numbers(&echoes), vec![1, 2, 3, 4]);
        // One image only.
        assert_eq!(plan_series(&stack(&[0.0])).volume_issue, Some(VolumeError::TooFewSlices));
        // A projection image without patient geometry.
        let mut projection = stack(&[0.0, 2.0]);
        projection.instances[1].frame_planes = vec![None];
        assert_eq!(plan_series(&projection).volume_issue, Some(VolumeError::NoGeometry));
        // Images of different sizes.
        let mut mixed = stack(&[0.0, 2.0, 4.0]);
        mixed.instances[2].cols = 8;
        assert_eq!(plan_series(&mixed).volume_issue, Some(VolumeError::InconsistentGrid));
        // Colour.
        let mut colour = stack(&[0.0, 2.0, 4.0]);
        colour.instances[0].samples_per_pixel = 3;
        let colour = plan_series(&colour);
        assert_eq!(colour.volume_issue, Some(VolumeError::NotGrayscale));
        assert!(colour.is_color);
    }

    #[test]
    fn multi_frame_file_owns_one_slot_per_frame() {
        // A three-frame file (frames 4 mm, 2 mm and 0 mm high) after a single-frame file at 6 mm.
        let mut cine = slice(2, [0.0, 0.0, 4.0]);
        cine.frames = 3;
        cine.frame_planes = [4.0, 2.0, 0.0]
            .iter()
            .map(|&z| ImagePlane::from_dicom([0.0, 0.0, z], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], [1.0, 1.0], 4, 4))
            .collect();
        let plan = plan_series(&series(vec![slice(1, [0.0, 0.0, 6.0]), cine]));
        assert_eq!(plan.len(), 4);
        assert_eq!(plan.volume_issue, None);
        let order: Vec<(usize, u32)> = plan.frames.iter().map(|f| (f.instance, f.frame)).collect();
        assert_eq!(order, vec![(0, 0), (1, 0), (1, 1), (1, 2)]);
        assert_eq!(plan.slots_by_instance(), vec![vec![0], vec![1, 2, 3]]);
    }

    #[test]
    fn failed_images_make_the_stack_irregular() {
        let plan = plan_series(&stack(&[0.0, 2.0, 4.0, 6.0, 8.0]));
        assert_eq!(plan.final_volume_issue(|_| true), None);
        // A hole in the middle: exactly what a missing file gives.
        let Some(VolumeError::IrregularSpacing { max_deviation_mm }) = plan.final_volume_issue(|i| i != 2) else {
            panic!("expected irregular spacing");
        };
        assert!((max_deviation_mm - 4.0 / 3.0).abs() < 1e-9, "{max_deviation_mm}");
        // A hole at the end, or almost nothing left: the image keeps its place, so still no volume.
        for decoded in [(|i| i != 4) as fn(usize) -> bool, |i| i == 0] {
            assert_eq!(plan.final_volume_issue(decoded), Some(VolumeError::IrregularSpacing { max_deviation_mm: 2.0 }));
        }
        // A verdict of the headers is final.
        let echoes = plan_series(&stack(&[0.0, 0.0, 2.0]));
        assert_eq!(echoes.final_volume_issue(|i| i != 1), Some(VolumeError::DuplicatePositions));
    }

    /// The study of series with repeated positions (see `summrise_testkit::stacks`).
    fn stacks_study() -> (tempfile::TempDir, StudyIndex) {
        let dir = tempfile::tempdir().expect("tempdir");
        write_stacks_study(dir.path()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        (dir, index)
    }

    fn labels(plan: &SeriesPlan) -> Vec<String> {
        plan.stacks.as_ref().map_or_else(Vec::new, |s| (0..s.count()).map(|i| s.label(i)).collect())
    }

    fn heights(plan: &SeriesPlan) -> Vec<f64> {
        plan.frames.iter().map(|f| f.plane.map_or(f64::NAN, |p| p.position.z)).collect()
    }

    /// Task 2.3 — the number of stacks, their labels, the slot order, one
    /// shared geometry and the positions in mm.
    #[test]
    fn plan_splits_series_with_repeated_positions_into_stacks() {
        let (_dir, index) = stacks_study();
        let study = &index.studies[0];
        assert_eq!(study.series.len(), 7);
        let plan = |i: usize| plan_series(&study.series[i]);
        let twelve: Vec<f64> = (0..phantoms::SLICES).map(phantoms::slice_height).collect();
        assert_eq!(twelve.first().zip(twelve.last()), Some((&20.0, &75.0)));

        // (a) Time points: stack-major, every stack from S 20 to S 75 mm, numbered time point by time point.
        let dynamic = plan(phantoms::DYNAMIC);
        assert_eq!(labels(&dynamic), ["Time point 1 / 3", "Time point 2 / 3", "Time point 3 / 3"]);
        assert_eq!((dynamic.stack_count(), dynamic.stack_len(), dynamic.len()), (3, 12, 36));
        assert_eq!(heights(&dynamic), [twelve.clone(), twelve.clone(), twelve.clone()].concat());
        assert_eq!(instance_numbers(&dynamic), (1..=36).collect::<Vec<_>>());
        assert_eq!(dynamic.volume_issue, None);
        let geometry = dynamic.geometry.expect("one geometry for every stack");
        assert_eq!(geometry.dims, [48, 48, 12]);
        assert_eq!(geometry.origin, summrise_imaging::DVec3::new(-120.0, -120.0, 20.0), "R 120, A 120, S 20 mm");
        assert_eq!(geometry.spacing(), summrise_imaging::DVec3::new(5.0, 5.0, 5.0));
        assert!(dynamic.stacks.as_ref().is_some_and(|s| s.kind.is_time_like()));
        assert_eq!(dynamic.stacks.as_ref().map(Stacks::summary).as_deref(), Some("3 time points × 12 images"));

        // (b) Echoes, stored alternately: echo 1 (instances 1, 3, 5 …) first.
        let echoes = plan(phantoms::DUAL_ECHO);
        assert_eq!(labels(&echoes), ["Echo 1 / 2 · TE 15 ms", "Echo 2 / 2 · TE 90 ms"]);
        assert_eq!(heights(&echoes), [twelve.clone(), twelve.clone()].concat());
        let odd: Vec<i32> = (0..12).map(|k| 2 * k + 1).collect();
        assert_eq!(instance_numbers(&echoes)[..12], odd[..]);
        assert!(echoes.stacks.as_ref().is_some_and(|s| !s.kind.is_time_like()));

        // (c) Diffusion: one echo time, so the b-value decides.
        let diffusion = plan(phantoms::DIFFUSION);
        assert_eq!(labels(&diffusion), ["b = 0 s/mm²", "b = 1000 s/mm²"]);
        assert_eq!(diffusion.stacks.as_ref().map(|s| s.kind), Some(StackKind::Diffusion));

        // (d) Nothing recorded: parts in the order of occurrence, which is that of (a).
        let repeated = plan(phantoms::REPEATED);
        assert_eq!(labels(&repeated), ["Part 1 / 3", "Part 2 / 3", "Part 3 / 3"]);
        assert_eq!(instance_numbers(&repeated), instance_numbers(&dynamic));
        assert_eq!(repeated.geometry, dynamic.geometry);

        // (e) One image missing: not split; today's order and explanation.
        let incomplete = plan(phantoms::INCOMPLETE);
        assert!(incomplete.stacks.is_none());
        assert_eq!(incomplete.len(), 35);
        assert_eq!(incomplete.volume_issue, Some(VolumeError::DuplicatePositions));
        let numbers = instance_numbers(&incomplete);
        assert!(numbers.windows(2).all(|w| w[0] < w[1]), "instance order: {numbers:?}");

        // (f) All images at one position: one list in time order.
        let cine = plan(phantoms::CINE);
        assert!(cine.stacks.is_none());
        assert_eq!(cine.volume_issue, Some(VolumeError::DuplicatePositions));
        assert_eq!(instance_numbers(&cine), (1..=20).collect::<Vec<_>>());
        assert_eq!(study.series[phantoms::CINE].acquisition.display_frame_rate(), Some(25.0));

        // (g) One Enhanced file: its frames go to their stacks, ordered by position.
        let enhanced = plan(phantoms::ENHANCED);
        assert_eq!(labels(&enhanced), ["Echo 1 / 2 · TE 30 ms", "Echo 2 / 2 · TE 100 ms"]);
        let six: Vec<f64> = (0..6).map(|k| 20.0 + 10.0 * f64::from(k)).collect();
        assert_eq!(heights(&enhanced), [six.clone(), six].concat());
        let frames: Vec<u32> = enhanced.frames.iter().map(|f| f.frame).collect();
        assert_eq!(frames, [0, 2, 4, 6, 8, 10, 1, 3, 5, 7, 9, 11], "the frames are interleaved in the file");
        assert_eq!(enhanced.slots_by_instance(), vec![vec![0, 6, 1, 7, 2, 8, 3, 9, 4, 10, 5, 11]]);
        assert_eq!(enhanced.geometry.map(|g| g.dims), Some([48, 48, 6]));
        assert_eq!(series_stacks(&study.series[phantoms::ENHANCED]), enhanced.stacks);
    }

    /// The order planned from the headers is the order of `load_series`, and
    /// the decoded stacks are volumes with the shared geometry.
    #[test]
    fn plan_and_load_series_agree_on_the_stacks() {
        let (_dir, index) = stacks_study();
        for series in &index.studies[0].series {
            let plan = plan_series(series);
            let loaded = load_series(&index.media, series, &Progress::new()).expect("decode");
            let title = series.title();
            assert_eq!(plan.frames, loaded.frames, "{title}");
            assert_eq!(plan.stacks, loaded.stacks, "{title}");
            assert_eq!(plan.volume_issue, loaded.volume_issue, "{title}");
            for stack in 0..plan.stack_count() {
                let volume = loaded.stack_volume(stack);
                assert_eq!(plan.geometry, volume.map(|v| v.geometry), "{title}: stack {stack}");
            }
        }
        // The marker (column 32, row 17 of the slice at S 50 mm) has the value of its time point.
        let dynamic =
            load_series(&index.media, &index.studies[0].series[phantoms::DYNAMIC], &Progress::new()).expect("decode");
        assert!(matches!(dynamic.images, SeriesImages::Stacks(ref v) if v.len() == 3));
        for (stack, &value) in phantoms::DYNAMIC_MARKER.iter().enumerate() {
            let volume = dynamic.stack_volume(stack).expect("volume");
            let marker = summrise_imaging::DVec3::from_array(summrise_testkit::MARKER_POSITION);
            assert_eq!(volume.value_at_patient(marker), Some(value as f32), "time point {}", stack + 1);
            // The flat index of that image is stack × 12 + 6.
            let Some(FrameRef::Gray { data, width, .. }) = dynamic.frame(stack * 12 + 6) else { panic!("grey") };
            assert_eq!(data.get(17 * width as usize + 32), value as f32);
        }
        assert_eq!(dynamic.byte_size(), 36 * 48 * 48 * 2);
    }

    #[test]
    fn a_failed_image_in_one_stack_takes_the_reformats_of_all() {
        let (_dir, index) = stacks_study();
        let plan = plan_series(&index.studies[0].series[phantoms::DYNAMIC]);
        assert_eq!(plan.final_volume_issue(|_| true), None);
        // Image 6 of time point 2 (display index 12 + 5) failed: a hole in the middle of that stack.
        let issue = plan.final_volume_issue(|i| i != 17);
        assert!(matches!(issue, Some(VolumeError::IrregularSpacing { .. })), "{issue:?}");
    }

    #[test]
    fn decodes_one_file_from_its_bytes() {
        let (_dir, index) = phantom();
        for series in &index.studies[0].series {
            let header = series.representative().expect("instance");
            let bytes = read_instance(&index.media, header).expect("read");
            let from_bytes = decode_instance(bytes).expect("decode from memory");
            let from_file = decode_file(&index.media, header).expect("decode from disc");
            assert_eq!(from_bytes.frames, from_file.frames, "{}", series.title());
            assert_eq!(from_bytes.presets, from_file.presets);
            assert_eq!(from_bytes.monochrome1, from_file.monochrome1);
            assert_eq!(from_bytes.rescale_type, from_file.rescale_type);
            assert_eq!(from_bytes.frames.len(), header.frames as usize);
            assert_eq!((from_bytes.frames[0].width, from_bytes.frames[0].height), (header.cols, header.rows));
        }
    }

    #[test]
    fn undecodable_files_are_errors_not_panics() {
        let (_dir, index) = phantom();
        let header = &index.studies[0].series[0].instances[0];
        let bytes = read_instance(&index.media, header).expect("read");
        // Not DICOM at all, empty, and cut off in the middle of the pixel data.
        assert!(matches!(decode_instance(vec![0x42; 300]), Err(DicomError::Image(_))));
        assert!(matches!(decode_instance(Vec::new()), Err(DicomError::Image(_))));
        assert!(decode_instance(bytes[..bytes.len() - 1000].to_vec()).is_err());
        // A file that has gone missing (scratched disc).
        let mut missing = header.clone();
        missing.path.set_file_name("IM999999");
        assert!(matches!(read_instance(&index.media, &missing), Err(DicomError::Io { .. })));
    }

    #[test]
    fn estimates_value_range_from_header_values() {
        // 12 bits unsigned with the usual CT intercept.
        let mut ct = slice(1, [0.0; 3]);
        ct.rescale = vec![Rescale { slope: 1.0, intercept: -1024.0 }];
        assert_eq!(ct.value_range_estimate(), Some((-1024.0, 3071.0)));
        // 16 bits signed.
        let mut signed = slice(1, [0.0; 3]);
        (signed.bits_stored, signed.signed) = (16, true);
        assert_eq!(signed.value_range_estimate(), Some((-32768.0, 32767.0)));
        // A slope stretches the range; a negative slope turns it round.
        let mut scaled = slice(1, [0.0; 3]);
        (scaled.bits_stored, scaled.rescale) = (8, vec![Rescale { slope: -2.0, intercept: 10.0 }]);
        assert_eq!(scaled.value_range_estimate(), Some((-500.0, 10.0)));
        // Frames rescaled individually: the union.
        let mut frames = slice(1, [0.0; 3]);
        frames.bits_stored = 8;
        frames.rescale = vec![Rescale { slope: 1.0, intercept: 0.0 }, Rescale { slope: 2.0, intercept: -100.0 }];
        assert_eq!(frames.value_range_estimate(), Some((-100.0, 410.0)));
        // Colour images have no modality values.
        let mut colour = slice(1, [0.0; 3]);
        colour.samples_per_pixel = 3;
        assert_eq!(colour.value_range_estimate(), None);

        // The plan takes the union over the series, and (0, 255) for colour.
        let plan = plan_series(&series(vec![ct, signed]));
        assert_eq!(plan.value_range, (-32768.0, 32767.0));
        assert_eq!(plan_series(&series(vec![colour])).value_range, (0.0, 255.0));
    }

    #[test]
    fn index_reads_the_values_the_estimate_needs() {
        let (_dir, index) = phantom();
        let study = &index.studies[0];
        let ct = &study.series[0].instances[0];
        assert_eq!((ct.bits_stored, ct.signed), (12, false));
        assert_eq!(ct.rescale, vec![Rescale { slope: 1.0, intercept: -1024.0 }]);
        assert_eq!(ct.rescale_type.as_deref(), Some("HU"));
        assert_eq!(plan_series(&study.series[0]).value_range, (-1024.0, 3071.0));
        // MR: 12 bits without rescale values; the 8-bit localizer.
        assert_eq!(study.series[1].instances[0].rescale, vec![Rescale::IDENTITY]);
        assert_eq!(plan_series(&study.series[1]).value_range, (0.0, 4095.0));
        assert_eq!(plan_series(&study.series[2]).value_range, (0.0, 255.0));
    }

    #[test]
    fn first_image_window_is_the_stored_one_or_automatic() {
        let (_dir, index) = phantom();
        let decode = |series: usize| {
            let header = index.studies[0].series[series].representative().expect("instance");
            decode_instance(read_instance(&index.media, header).expect("read")).expect("decode")
        };
        // CT: the first stored window (mediastinum).
        let ct = decode(0).initial_window(0, (-1024.0, 3071.0));
        assert_eq!((ct.center, ct.width), (40.0, 400.0));
        // Localizer without stored windows: from the statistics of that image.
        let localizer = decode(2);
        assert!(localizer.presets.is_empty());
        let auto = localizer.initial_window(0, (0.0, 255.0));
        assert!(auto.low() >= 0.0 && auto.high() <= 255.0 && auto.width < 255.0, "{auto:?}");
        // Colour: nothing to window, the whole range.
        assert_eq!(decode(3).initial_window(0, (0.0, 255.0)), Window::from_range(0.0, 255.0));
    }

    #[test]
    fn converts_ybr_to_rgb() {
        let mut px = [128, 128, 128, 255, 128, 128];
        ybr_to_rgb(&mut px);
        assert_eq!(px, [128, 128, 128, 255, 255, 255]);
    }

    #[test]
    fn title_cases_explanations() {
        assert_eq!(title_case("MEDIASTINUM"), "Mediastinum");
        assert_eq!(title_case("soft TISSUE"), "Soft Tissue");
    }

    #[test]
    fn downscales_keeping_aspect() {
        let t = downscale(4, 2, &[255u8; 32], 2);
        assert_eq!((t.width, t.height), (2, 1));
        assert!(t.rgba.iter().all(|&v| v == 255));
    }
}
