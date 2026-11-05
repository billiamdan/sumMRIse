//! VOI (value of interest) windowing — contrast and brightness.
//!
//! Implements the DICOM VOI LUT functions (PS3.3 C.11.2.1.2 / C.11.2.1.3),
//! clinical CT presets, statistical auto-windowing and conversion of grey
//! values to display RGBA.

use std::borrow::Cow;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// VOI LUT Function (0028,1056).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum VoiFunction {
    #[default]
    Linear,
    LinearExact,
    Sigmoid,
}

impl VoiFunction {
    pub fn from_dicom(value: &str) -> Option<Self> {
        match value.trim() {
            "LINEAR" => Some(Self::Linear),
            "LINEAR_EXACT" => Some(Self::LinearExact),
            "SIGMOID" => Some(Self::Sigmoid),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::LinearExact => "Linear (exact)",
            Self::Sigmoid => "Sigmoid",
        }
    }
}

/// A window centre/width pair (in modality units, e.g. HU for CT).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub center: f64,
    pub width: f64,
    pub function: VoiFunction,
}

impl Window {
    pub const MIN_WIDTH: f64 = 1.0;

    pub fn new(center: f64, width: f64) -> Self {
        Self { center, width: width.max(Self::MIN_WIDTH), function: VoiFunction::Linear }
    }

    pub fn with_function(mut self, function: VoiFunction) -> Self {
        self.function = function;
        self
    }

    /// Window spanning `[lo, hi]` exactly.
    pub fn from_range(lo: f64, hi: f64) -> Self {
        Self::new((lo + hi) / 2.0, hi - lo).with_function(VoiFunction::LinearExact)
    }

    pub fn low(&self) -> f64 {
        self.center - self.width / 2.0
    }

    pub fn high(&self) -> f64 {
        self.center + self.width / 2.0
    }

    /// Maps a modality value to normalised display intensity in `[0, 1]`.
    #[inline]
    pub fn apply(&self, x: f64) -> f64 {
        let (c, w) = (self.center, self.width.max(Self::MIN_WIDTH));
        match self.function {
            VoiFunction::Linear => {
                // PS3.3 C.11.2.1.2.1
                let lo = c - 0.5 - (w - 1.0) / 2.0;
                let hi = c - 0.5 + (w - 1.0) / 2.0;
                if x <= lo {
                    0.0
                } else if x > hi {
                    1.0
                } else {
                    ((x - (c - 0.5)) / (w - 1.0) + 0.5).clamp(0.0, 1.0)
                }
            }
            VoiFunction::LinearExact => {
                if x <= c - w / 2.0 {
                    0.0
                } else if x > c + w / 2.0 {
                    1.0
                } else {
                    (x - c) / w + 0.5
                }
            }
            VoiFunction::Sigmoid => 1.0 / (1.0 + (-4.0 * (x - c) / w).exp()),
        }
    }

    /// Adjusts the window from a pointer drag: horizontal changes the width
    /// (contrast), vertical changes the centre (brightness).
    pub fn dragged(self, dx: f64, dy: f64, units_per_pixel: f64) -> Self {
        Self {
            center: self.center + dy * units_per_pixel,
            width: (self.width + dx * units_per_pixel).max(Self::MIN_WIDTH),
            function: self.function,
        }
    }
}

/// Where a window preset comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PresetSource {
    /// Stored in the DICOM file by the scanner or radiographer.
    Dicom,
    /// Built-in clinical preset.
    Clinical,
    /// Computed from the image histogram.
    Auto,
    /// Full range of values present in the image.
    FullRange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowPreset {
    pub name: Cow<'static, str>,
    pub window: Window,
    pub source: PresetSource,
}

impl WindowPreset {
    const fn clinical(name: &'static str, width: f64, level: f64) -> Self {
        Self {
            name: Cow::Borrowed(name),
            window: Window { center: level, width, function: VoiFunction::Linear },
            source: PresetSource::Clinical,
        }
    }
}

/// Common CT windows in Hounsfield units (width / level).
pub const CT_PRESETS: &[WindowPreset] = &[
    WindowPreset::clinical("Brain", 80.0, 40.0),
    WindowPreset::clinical("Subdural", 200.0, 75.0),
    WindowPreset::clinical("Stroke", 40.0, 40.0),
    WindowPreset::clinical("Head soft tissue", 380.0, 40.0),
    WindowPreset::clinical("Temporal bone", 2800.0, 600.0),
    WindowPreset::clinical("Lung", 1500.0, -600.0),
    WindowPreset::clinical("Mediastinum", 350.0, 50.0),
    WindowPreset::clinical("Abdomen", 400.0, 50.0),
    WindowPreset::clinical("Liver", 150.0, 30.0),
    WindowPreset::clinical("Bone", 1800.0, 400.0),
    WindowPreset::clinical("Angio", 600.0, 300.0),
];

/// Robust automatic window from image statistics: the 0.5–99.5 percentile
/// range, ignoring `NaN` and a dominant background/padding value.
pub fn auto_window(samples: impl IntoIterator<Item = f32>) -> Option<Window> {
    let mut values: Vec<f32> = samples.into_iter().filter(|v| v.is_finite()).collect();
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f32::total_cmp);
    let min = values[0];
    let max = values[values.len() - 1];
    let background = values.partition_point(|&v| v <= min);
    if background > values.len() / 10 && background < values.len() {
        values.drain(..background);
    }
    let pct = |p: f64| values[((values.len() - 1) as f64 * p).round() as usize];
    let (mut lo, mut hi) = (f64::from(pct(0.005)), f64::from(pct(0.995)));
    if hi - lo < 1.0 {
        (lo, hi) = (f64::from(min), f64::from(max));
    }
    if hi - lo < 1.0 {
        hi = lo + 1.0;
    }
    Some(Window::new((lo + hi) / 2.0, hi - lo))
}

/// Evenly strided subsample (at most `max` values) for statistics.
pub fn subsample(len: usize, max: usize, get: impl Fn(usize) -> f32) -> Vec<f32> {
    let stride = len.div_ceil(max.max(1)).max(1);
    (0..len).step_by(stride).map(get).collect()
}

/// Converts `len` grey values to RGBA8 through `window`. Non-finite values
/// (outside the volume) are rendered as opaque black.
pub fn gray_to_rgba(len: usize, get: impl Fn(usize) -> f32 + Sync, window: &Window, invert: bool) -> Vec<u8> {
    let mut out = vec![0u8; len * 4];
    out.par_chunks_mut(4 * 4096).enumerate().for_each(|(chunk, px)| {
        let base = chunk * 4096;
        for (n, rgba) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let v = get(base + n);
            let g = if v.is_finite() {
                let t = window.apply(f64::from(v));
                let t = if invert { 1.0 - t } else { t };
                (t * 255.0).round() as u8
            } else {
                0
            };
            rgba.copy_from_slice(&[g, g, g, 255]);
        }
    });
    out
}

/// Converts interleaved RGB8 to RGBA8.
pub fn rgb_to_rgba(rgb: &[u8], invert: bool) -> Vec<u8> {
    rgb.as_chunks::<3>()
        .0
        .iter()
        .flat_map(|p| if invert { [255 - p[0], 255 - p[1], 255 - p[2], 255] } else { [p[0], p[1], p[2], 255] })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_matches_dicom_definition() {
        // PS3.3 example: center 2048, width 4096 on 12-bit data.
        let w = Window::new(2048.0, 4096.0);
        assert_eq!(w.apply(0.0), 0.0);
        assert_eq!(w.apply(4095.0), 1.0);
        assert!((w.apply(2047.5) - 0.5).abs() < 1e-9);
        // Width 1 degenerates into a threshold at c - 0.5.
        let t = Window::new(10.0, 1.0);
        assert_eq!(t.apply(9.5), 0.0);
        assert_eq!(t.apply(9.6), 1.0);
    }

    #[test]
    fn linear_exact_and_sigmoid() {
        let w = Window::new(40.0, 80.0).with_function(VoiFunction::LinearExact);
        assert_eq!(w.apply(0.0), 0.0);
        assert_eq!(w.apply(40.0), 0.5);
        assert_eq!(w.apply(80.0), 1.0);
        let s = Window::new(0.0, 100.0).with_function(VoiFunction::Sigmoid);
        assert!((s.apply(0.0) - 0.5).abs() < 1e-12);
        assert!(s.apply(100.0) > 0.98 && s.apply(-100.0) < 0.02);
    }

    #[test]
    fn brain_preset_maps_hounsfield_units() {
        let brain = CT_PRESETS.iter().find(|p| p.name == "Brain").expect("preset");
        assert_eq!(brain.window.apply(-1000.0), 0.0); // air is black
        assert_eq!(brain.window.apply(1000.0), 1.0); // bone is white
        let grey_matter = brain.window.apply(40.0);
        assert!((grey_matter - 0.5).abs() < 0.01);
    }

    #[test]
    fn auto_window_ignores_background() {
        // 80% background at 0, tissue values 100..=199.
        let mut v = vec![0.0f32; 4000];
        v.extend((0..1000).map(|i| 100.0 + (i % 100) as f32));
        let w = auto_window(v).expect("window");
        assert!(w.low() >= 99.0 && w.high() <= 200.0, "{w:?}");
        assert!(auto_window(Vec::<f32>::new()).is_none());
        let flat = auto_window(vec![5.0; 10]).expect("flat window");
        assert!(flat.width >= 1.0);
    }

    #[test]
    fn drag_adjusts_width_and_center() {
        let w = Window::new(40.0, 80.0).dragged(10.0, -5.0, 2.0);
        assert_eq!(w.width, 100.0);
        assert_eq!(w.center, 30.0);
        assert_eq!(Window::new(0.0, 2.0).dragged(-100.0, 0.0, 1.0).width, Window::MIN_WIDTH);
    }

    #[test]
    fn rgba_conversion_handles_nan_and_invert() {
        let data = [-1000.0f32, 40.0, 1000.0, f32::NAN];
        let w = Window::new(40.0, 80.0);
        let rgba = gray_to_rgba(4, |i| data[i], &w, false);
        assert_eq!(&rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&rgba[8..12], &[255, 255, 255, 255]);
        assert_eq!(&rgba[12..16], &[0, 0, 0, 255]);
        let inv = gray_to_rgba(4, |i| data[i], &w, true);
        assert_eq!(&inv[0..4], &[255, 255, 255, 255]);
        assert_eq!(&inv[12..16], &[0, 0, 0, 255], "background stays black");
        assert_eq!(rgb_to_rgba(&[1, 2, 3], false), vec![1, 2, 3, 255]);
    }
}
