//! Pixel data decoding: transfer syntaxes, Modality LUT (rescale to e.g.
//! Hounsfield units), photometric interpretation, multi-frame objects and
//! assembly into regular volumes.

use std::borrow::Cow;
use std::panic::{AssertUnwindSafe, catch_unwind};

use dicom_dictionary_std::tags;
use dicom_object::InMemDicomObject;
use dicom_pixeldata::{DecodedPixelData, PhotometricInterpretation, PixelDecoder, PlanarConfiguration};
use log::{debug, warn};
use rayon::prelude::*;
use summrise_imaging::window::subsample;
use summrise_imaging::{
    Frame, FramePixels, ImagePlane, PresetSource, VoiFunction, Volume, VolumeError, VolumeGeometry, VoxelBuffer,
    Window, WindowPreset, auto_window, sort_by_position,
};

use crate::attrs;
use crate::error::{DicomError, Result};
use crate::index::{InstanceHeader, Series};
use crate::progress::Progress;

/// Display order entry of a decoded series.
#[derive(Debug, Clone)]
pub struct FrameInfo {
    /// Index into [`Series::instances`].
    pub instance: usize,
    /// Frame number within that instance.
    pub frame: u32,
    pub instance_number: Option<i32>,
    pub plane: Option<ImagePlane>,
    pub pixel_spacing: Option<[f64; 2]>,
}

/// Pixel storage of a decoded series.
#[derive(Debug, Clone)]
pub enum SeriesImages {
    /// Regular, evenly spaced grey-scale stack: enables MPR.
    Volume(Volume),
    /// Any other set of frames (mixed orientations, colour, projections…).
    Frames(Vec<Frame>),
}

/// Borrowed view of one displayed frame.
#[derive(Debug, Clone, Copy)]
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
            SeriesImages::Volume(v) => {
                let [w, h, _] = v.geometry.dims;
                v.slices.get(index).map(|data| FrameRef::Gray { width: w as u32, height: h as u32, data })
            }
            SeriesImages::Frames(frames) => frames.get(index).map(|f| match &f.pixels {
                FramePixels::Gray(data) => FrameRef::Gray { width: f.width, height: f.height, data },
                FramePixels::Rgb(data) => FrameRef::Rgb { width: f.width, height: f.height, data },
            }),
        }
    }

    pub fn volume(&self) -> Option<&Volume> {
        match &self.images {
            SeriesImages::Volume(v) => Some(v),
            SeriesImages::Frames(_) => None,
        }
    }

    pub fn byte_size(&self) -> usize {
        match &self.images {
            SeriesImages::Volume(v) => v.byte_size(),
            SeriesImages::Frames(f) => f.iter().map(Frame::byte_size).sum(),
        }
    }
}

struct DecodedInstance {
    frames: Vec<Frame>,
    monochrome1: bool,
    presets: Vec<WindowPreset>,
    rescale_type: Option<String>,
}

/// Decodes every frame of `series` (in parallel) and assembles a volume when
/// the geometry allows it.
pub fn load_series(series: &Series, progress: &Progress) -> Result<LoadedSeries> {
    progress.begin("Decoding images", series.instances.len());
    let decoded: Vec<std::result::Result<DecodedInstance, String>> = series
        .instances
        .par_iter()
        .map(|header| {
            if progress.is_cancelled() {
                return Err("cancelled".into());
            }
            let r = catch_unwind(AssertUnwindSafe(|| decode_instance(header)))
                .unwrap_or_else(|_| Err("decoder panicked on malformed data".into()));
            progress.advance();
            r
        })
        .collect();
    if progress.is_cancelled() {
        return Err(DicomError::Cancelled);
    }

    let mut infos = Vec::new();
    let mut frames = Vec::new();
    let mut failures = Vec::new();
    let mut presets = Vec::new();
    let mut monochrome1 = false;
    let mut rescale_type = None;
    for (index, (header, result)) in series.instances.iter().zip(decoded).enumerate() {
        match result {
            Ok(inst) => {
                if presets.is_empty() {
                    presets = inst.presets;
                }
                monochrome1 |= inst.monochrome1;
                rescale_type = rescale_type.or(inst.rescale_type);
                for (f, frame) in inst.frames.into_iter().enumerate() {
                    infos.push(FrameInfo {
                        instance: index,
                        frame: f as u32,
                        instance_number: header.instance_number,
                        plane: header.frame_planes.get(f).copied().flatten(),
                        pixel_spacing: header.pixel_spacing,
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
        let reason = failures.first().cloned().unwrap_or_else(|| "no pixel data".to_owned());
        return Err(DicomError::DecodeFailed(reason));
    }
    let mut warnings = Vec::new();
    if !failures.is_empty() {
        warn!("{} of {} images failed to decode", failures.len(), series.instances.len());
        warnings.push(format!("{} image(s) could not be decoded: {}", failures.len(), failures[0]));
    }

    let is_color = frames.iter().any(|f| matches!(f.pixels, FramePixels::Rgb(_)));
    let (order, volume_issue) = display_order(&infos, &frames, is_color);
    let mut slots: Vec<Option<(FrameInfo, Frame)>> = infos.into_iter().zip(frames).map(Some).collect();
    let (infos, frames): (Vec<FrameInfo>, Vec<Frame>) = order.iter().filter_map(|&i| slots[i].take()).unzip();

    let value_range = gray_range(&frames).unwrap_or((0.0, 255.0));
    let initial_window = presets.first().map(|p| p.window).unwrap_or_else(|| {
        let mid = frames.len() / 2;
        auto_window(frames[mid..=mid].iter().chain(frames.iter().step_by(frames.len().div_ceil(8).max(1))).flat_map(
            |f| match &f.pixels {
                FramePixels::Gray(b) => subsample(b.len(), 65_536, |i| b.get(i)),
                FramePixels::Rgb(_) => Vec::new(),
            },
        ))
        .unwrap_or_else(|| Window::from_range(f64::from(value_range.0), f64::from(value_range.1)))
    });

    let volume = match &volume_issue {
        None => {
            let planes: Vec<ImagePlane> = infos.iter().filter_map(|i| i.plane).collect();
            VolumeGeometry::from_sorted_planes(&planes).ok().and_then(|geometry| {
                let slices = frames
                    .iter()
                    .filter_map(|f| match &f.pixels {
                        FramePixels::Gray(b) => Some(b.clone()),
                        FramePixels::Rgb(_) => None,
                    })
                    .collect();
                Volume::new(geometry, slices)
            })
        }
        Some(_) => None,
    };
    let images = match volume {
        Some(v) => SeriesImages::Volume(v),
        None => SeriesImages::Frames(frames),
    };
    let units = rescale_type
        .filter(|u| !u.is_empty() && u != "US")
        .or_else(|| (series.modality == "CT").then(|| "HU".to_owned()));

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
    })
}

/// Spatial order for regular stacks (aligned with acquisition order), else
/// instance/frame order. Also reports why the stack is not a volume.
fn display_order(infos: &[FrameInfo], frames: &[Frame], is_color: bool) -> (Vec<usize>, Option<VolumeError>) {
    let natural: Vec<usize> = {
        let mut o: Vec<usize> = (0..infos.len()).collect();
        o.sort_by_key(|&i| (infos[i].instance_number.unwrap_or(i32::MAX), infos[i].instance, infos[i].frame));
        o
    };
    if is_color {
        return (natural, Some(VolumeError::NotGrayscale));
    }
    let planes: Option<Vec<ImagePlane>> = infos.iter().map(|i| i.plane).collect();
    let Some(planes) = planes else {
        return (natural, Some(VolumeError::NoGeometry));
    };
    if planes.len() < 2 {
        return (natural, Some(VolumeError::TooFewSlices));
    }
    let same_size = frames.iter().all(|f| f.width == frames[0].width && f.height == frames[0].height);
    if !same_size || !planes.iter().all(|p| p.same_grid(&planes[0])) {
        return (natural, Some(VolumeError::InconsistentGrid));
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
        Ok(_) => (spatial, None),
        Err(VolumeError::DuplicatePositions) => (natural, Some(VolumeError::DuplicatePositions)),
        Err(e) => (spatial, Some(e)),
    }
}

fn gray_range(frames: &[Frame]) -> Option<(f32, f32)> {
    frames
        .par_iter()
        .filter_map(|f| match &f.pixels {
            FramePixels::Gray(b) => b.min_max(),
            FramePixels::Rgb(_) => None,
        })
        .reduce_with(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

fn decode_instance(header: &InstanceHeader) -> std::result::Result<DecodedInstance, String> {
    let obj = dicom_object::open_file(&header.path).map_err(|e| e.to_string())?;
    let decoded = obj.decode_pixel_data().map_err(|e| e.to_string())?;
    let pi = decoded.photometric_interpretation().clone();
    let spp = decoded.samples_per_pixel();
    let (width, height) = (decoded.columns(), decoded.rows());
    let mut frames = Vec::with_capacity(decoded.number_of_frames() as usize);
    for f in 0..decoded.number_of_frames() {
        let pixels = match (spp, &pi) {
            (1, PhotometricInterpretation::Monochrome1 | PhotometricInterpretation::Monochrome2) => {
                let values: Vec<f32> = decoded.to_vec_frame(f).map_err(|e| e.to_string())?;
                FramePixels::Gray(VoxelBuffer::compact(values))
            }
            (3, _) => FramePixels::Rgb(rgb_frame(&decoded, f)?),
            (_, other) => return Err(format!("{other} images are not supported yet")),
        };
        frames.push(Frame { width, height, pixels });
    }
    let base: &InMemDicomObject = &obj;
    Ok(DecodedInstance {
        frames,
        monochrome1: pi == PhotometricInterpretation::Monochrome1,
        presets: dicom_windows(base),
        rescale_type: attrs::text(base, tags::RESCALE_TYPE),
    })
}

fn rgb_frame(decoded: &DecodedPixelData<'_>, frame: u32) -> std::result::Result<Vec<u8>, String> {
    if decoded.bits_allocated() != 8 {
        return Err(format!("{}-bit colour images are not supported yet", decoded.bits_allocated()));
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
        other => return Err(format!("{other} colour images are not supported yet")),
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
                    .map_or_else(|| format!("Window {}", i + 1), |e| title_case(e)),
            ),
            window: Window::new(c, w).with_function(function),
            source: PresetSource::Dicom,
        })
        .collect()
}

fn title_case(s: &str) -> String {
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

/// Decodes only the representative image of `series` and renders a
/// thumbnail no larger than `max_size` pixels per side.
pub fn thumbnail(series: &Series, max_size: u32) -> Result<Thumbnail> {
    let header = series.representative().ok_or_else(|| DicomError::DecodeFailed("empty series".into()))?;
    let inst = catch_unwind(AssertUnwindSafe(|| decode_instance(header)))
        .unwrap_or_else(|_| Err("decoder panicked".into()))
        .map_err(DicomError::DecodeFailed)?;
    let frame = inst.frames.get(inst.frames.len() / 2).ok_or_else(|| DicomError::DecodeFailed("no frames".into()))?;
    let rgba = match &frame.pixels {
        FramePixels::Gray(b) => {
            let window = inst
                .presets
                .first()
                .map(|p| p.window)
                .or_else(|| auto_window(subsample(b.len(), 65_536, |i| b.get(i))));
            let window = window.unwrap_or_else(|| Window::new(127.5, 256.0));
            summrise_imaging::window::gray_to_rgba(b.len(), |i| b.get(i), &window, inst.monochrome1)
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
    use super::*;

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
