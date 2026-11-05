//! UI-agnostic viewing state for a series: contrast window, slice positions,
//! MPR crosshair, slab projection, zoom/pan per view.
//!
//! The state is created when the first image of a series is decoded and is
//! never changed by images arriving afterwards: window, zoom, pan and
//! position are the user's (the series' own initial window is fixed by that
//! first image).

use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use summrise_dicom::FrameRef;
use summrise_imaging::window::subsample;
use summrise_imaging::{
    CT_PRESETS, DVec3, MprAxis, MprExtent, PresetSource, Slab, SlabMode, VolumeGeometry, Window, WindowPreset,
    auto_window,
};

use crate::series::SeriesData;

/// Arrangement of viewports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Layout {
    /// The acquired images, one at a time.
    #[default]
    Single,
    /// Axial, coronal and sagittal reformats side by side.
    Mpr,
    /// Acquired images plus the three reformats.
    Quad,
}

impl Layout {
    pub const ALL: [Self; 3] = [Self::Single, Self::Mpr, Self::Quad];

    pub fn views(self) -> &'static [ViewKind] {
        match self {
            Self::Single => &[ViewKind::Stack],
            Self::Mpr => {
                &[ViewKind::Mpr(MprAxis::Axial), ViewKind::Mpr(MprAxis::Coronal), ViewKind::Mpr(MprAxis::Sagittal)]
            }
            Self::Quad => &[
                ViewKind::Stack,
                ViewKind::Mpr(MprAxis::Axial),
                ViewKind::Mpr(MprAxis::Coronal),
                ViewKind::Mpr(MprAxis::Sagittal),
            ],
        }
    }

    pub fn needs_volume(self) -> bool {
        self != Self::Single
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "Images",
            Self::Mpr => "MPR",
            Self::Quad => "2 × 2",
        }
    }
}

/// What a viewport shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewKind {
    /// Original frames in acquisition order.
    Stack,
    /// Reformatted plane.
    Mpr(MprAxis),
}

impl ViewKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stack => "Acquired",
            Self::Mpr(a) => a.label(),
        }
    }
}

/// Primary-button mouse tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Tool {
    #[default]
    WindowLevel,
    Pan,
    Zoom,
}

impl Tool {
    pub const ALL: [Self; 3] = [Self::WindowLevel, Self::Pan, Self::Zoom];

    pub fn label(self) -> &'static str {
        match self {
            Self::WindowLevel => "Contrast",
            Self::Pan => "Pan",
            Self::Zoom => "Zoom",
        }
    }
}

/// Zoom (1 = fit) and pan (screen points) of one viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTransform {
    pub zoom: f32,
    pub pan: [f32; 2],
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self { zoom: 1.0, pan: [0.0, 0.0] }
    }
}

impl ViewTransform {
    pub const MIN_ZOOM: f32 = 0.1;
    pub const MAX_ZOOM: f32 = 32.0;

    /// Zooms by `factor` keeping the point `anchor` (relative to the viewport
    /// centre, in points) fixed on screen.
    pub fn zoom_about(&mut self, factor: f32, anchor: [f32; 2]) {
        let new_zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        let f = new_zoom / self.zoom;
        self.pan = [anchor[0] - (anchor[0] - self.pan[0]) * f, anchor[1] - (anchor[1] - self.pan[1]) * f];
        self.zoom = new_zoom;
    }
}

/// Viewing state of one series.
#[derive(Debug, Clone)]
pub struct ViewState {
    pub window: Window,
    /// Name of the active preset; `None` after manual adjustment.
    pub preset: Option<String>,
    pub invert: bool,
    /// Index into the acquired frames.
    pub slice: usize,
    /// MPR crosshair (patient coordinates, mm).
    pub cursor: DVec3,
    pub slab: Option<Slab>,
    transforms: HashMap<ViewKind, ViewTransform>,
}

impl ViewState {
    /// The state a series opens with, once its first image is decoded.
    pub fn new(series: &SeriesData) -> Self {
        Self {
            window: series.initial_window(),
            preset: Some(series.presets().first().map_or_else(|| "Auto".to_owned(), |p| p.name.to_string())),
            invert: series.monochrome1(),
            slice: series.initial_index(),
            // Known from the headers, so the crosshair is in place when the volume arrives.
            cursor: series.geometry().map_or(DVec3::ZERO, VolumeGeometry::center),
            slab: None,
            transforms: HashMap::new(),
        }
    }

    pub fn transform(&self, kind: ViewKind) -> ViewTransform {
        self.transforms.get(&kind).copied().unwrap_or_default()
    }

    pub fn transform_mut(&mut self, kind: ViewKind) -> &mut ViewTransform {
        self.transforms.entry(kind).or_default()
    }

    /// Resets zoom and pan of every view.
    pub fn reset_transforms(&mut self) {
        self.transforms.clear();
    }

    /// Resets contrast and geometry but keeps the current position.
    pub fn reset(&mut self, series: &SeriesData) {
        let fresh = Self::new(series);
        self.window = fresh.window;
        self.preset = fresh.preset;
        self.invert = fresh.invert;
        self.slab = None;
        self.reset_transforms();
    }

    /// All presets applicable to `series`: stored DICOM windows, clinical CT
    /// windows (for Hounsfield data), an automatic window from the images
    /// loaded so far and, once every image is loaded, the full range.
    pub fn presets(series: &SeriesData) -> Vec<WindowPreset> {
        let mut out = series.presets().to_vec();
        if series.units() == Some("HU") {
            out.extend(CT_PRESETS.iter().cloned());
        }
        if !series.is_color() {
            out.push(WindowPreset {
                name: Cow::Borrowed("Auto"),
                window: auto_for(series),
                source: PresetSource::Auto,
            });
            // The values actually present are only known when loading is complete.
            if series.is_complete() {
                let (lo, hi) = series.value_range();
                out.push(WindowPreset {
                    name: Cow::Borrowed("Full range"),
                    window: Window::from_range(f64::from(lo), f64::from(hi)),
                    source: PresetSource::FullRange,
                });
            }
        }
        out
    }

    pub fn apply_preset(&mut self, preset: &WindowPreset) {
        self.window = preset.window;
        self.preset = Some(preset.name.to_string());
    }

    /// Pointer-drag contrast adjustment, scaled to the series' value range
    /// (an estimate from the headers until every image is loaded).
    pub fn drag_window(&mut self, dx: f64, dy: f64, series: &SeriesData) {
        let (lo, hi) = series.value_range();
        let units_per_point = (f64::from(hi - lo) / 1024.0).max(0.05);
        self.window = self.window.dragged(dx, dy, units_per_point);
        self.preset = None;
    }

    pub fn slice_count(&self, kind: ViewKind, series: &SeriesData) -> usize {
        match (kind, series.volume()) {
            (ViewKind::Stack, _) => series.len(),
            (ViewKind::Mpr(axis), Some(v)) => MprExtent::of(&v.geometry, axis).slice_count(),
            (ViewKind::Mpr(_), None) => 0,
        }
    }

    pub fn slice_index(&self, kind: ViewKind, series: &SeriesData) -> usize {
        match (kind, series.volume()) {
            (ViewKind::Stack, _) => self.slice.min(series.len().saturating_sub(1)),
            (ViewKind::Mpr(axis), Some(v)) => MprExtent::of(&v.geometry, axis).index_of(self.cursor.dot(axis.normal())),
            (ViewKind::Mpr(_), None) => 0,
        }
    }

    /// Position (mm along the view normal) of an MPR view.
    pub fn mpr_position(&self, axis: MprAxis) -> f64 {
        self.cursor.dot(axis.normal())
    }

    pub fn set_slice(&mut self, kind: ViewKind, series: &SeriesData, index: usize) {
        match kind {
            ViewKind::Stack => {
                self.slice = index.min(series.len().saturating_sub(1));
                let plane = series.frames().get(self.slice).and_then(|f| f.plane);
                if let (Some(geometry), Some(plane)) = (series.geometry(), plane) {
                    // Keep the MPR crosshair on the displayed acquired slice.
                    let n = plane.normal();
                    self.cursor += n * (plane.position.dot(n) - self.cursor.dot(n));
                    self.cursor = clamp_to_volume(self.cursor, geometry);
                }
            }
            ViewKind::Mpr(axis) => {
                let Some(v) = series.volume() else { return };
                let ext = MprExtent::of(&v.geometry, axis);
                let n = axis.normal();
                let pos = ext.position(index.min(ext.slice_count() - 1));
                self.cursor += n * (pos - self.cursor.dot(n));
                self.sync_stack_to_cursor(series);
            }
        }
    }

    pub fn scroll(&mut self, kind: ViewKind, series: &SeriesData, delta: i64) {
        let count = self.slice_count(kind, series) as i64;
        if count == 0 {
            return;
        }
        let current = self.slice_index(kind, series) as i64;
        let next = (current + delta).clamp(0, count - 1) as usize;
        self.set_slice(kind, series, next);
    }

    /// Moves the MPR crosshair to `p` (clamped into the volume).
    pub fn set_cursor(&mut self, p: DVec3, series: &SeriesData) {
        if let Some(v) = series.volume() {
            self.cursor = clamp_to_volume(p, &v.geometry);
            self.sync_stack_to_cursor(series);
        }
    }

    fn sync_stack_to_cursor(&mut self, series: &SeriesData) {
        if let Some(v) = series.volume() {
            let k = v.geometry.to_index(self.cursor).z.round();
            self.slice = (k.max(0.0) as usize).min(series.len().saturating_sub(1));
        }
    }

    pub fn set_slab(&mut self, mode: Option<SlabMode>, thickness_mm: f64) {
        self.slab = mode.map(|mode| Slab { mode, thickness_mm: thickness_mm.max(0.0) });
    }
}

fn clamp_to_volume(p: DVec3, g: &VolumeGeometry) -> DVec3 {
    let max = DVec3::new((g.dims[0] - 1) as f64, (g.dims[1] - 1) as f64, (g.dims[2] - 1) as f64);
    g.to_patient(g.to_index(p).clamp(DVec3::ZERO, max))
}

/// Automatic window from image statistics, sampled evenly over the images
/// that are loaded (all of them once the series is complete).
fn auto_for(series: &SeriesData) -> Window {
    let loaded: Vec<FrameRef<'_>> = (0..series.len()).filter_map(|i| series.frame(i)).collect();
    let samples: Vec<f32> = loaded
        .iter()
        .step_by(loaded.len().div_ceil(16).max(1))
        .flat_map(|f| match f {
            FrameRef::Gray { data, .. } => subsample(data.len(), 32_768, |i| data.get(i)),
            FrameRef::Rgb { .. } => Vec::new(),
        })
        .collect();
    auto_window(samples).unwrap_or_else(|| series.initial_window())
}

#[cfg(test)]
mod tests {
    use summrise_dicom::plan_series;
    use summrise_testkit::PhantomOptions;

    use super::*;
    use crate::series::testing::{fill_from_disc, load, phantom};

    fn ct() -> (tempfile::TempDir, SeriesData) {
        let (dir, index) = phantom(&PhantomOptions::small());
        let series = load(&index.studies[0].series[0]);
        (dir, series)
    }

    fn preset_names(series: &SeriesData) -> Vec<String> {
        ViewState::presets(series).iter().map(|p| p.name.to_string()).collect()
    }

    #[test]
    fn presets_include_dicom_clinical_and_auto() {
        let (_d, series) = ct();
        let names = preset_names(&series);
        assert_eq!(&names[..2], ["Mediastinum", "Lung"]);
        assert!(names.contains(&"Brain".to_owned()));
        assert!(names.ends_with(&["Auto".to_owned(), "Full range".to_owned()]));
    }

    #[test]
    fn state_works_on_a_partly_loaded_series() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let ct = &index.studies[0].series[0];
        let series = SeriesData::new(plan_series(ct));
        fill_from_disc(&series, ct, 12);
        let mut state = ViewState::new(&series);
        // Opens on the first decoded image with its stored window.
        assert_eq!((state.slice, state.window.center, state.window.width), (12, 40.0, 400.0));
        assert_eq!(state.preset.as_deref(), Some("Mediastinum"));
        // The crosshair is already where the volume will be.
        assert_eq!(Some(state.cursor), series.geometry().map(VolumeGeometry::center));
        // The counter shows the final total; scrolling into unloaded images is allowed.
        assert_eq!(state.slice_count(ViewKind::Stack, &series), 24);
        state.scroll(ViewKind::Stack, &series, 100);
        assert_eq!(state.slice_index(ViewKind::Stack, &series), 23);
        assert!(series.frame(23).is_none(), "not loaded yet");
        let z = series.frames()[23].plane.expect("plane").position.z;
        assert!((state.cursor.z - z).abs() < 1e-6, "crosshair follows the stack while loading");
        // No reformats yet.
        assert_eq!(state.slice_count(ViewKind::Mpr(MprAxis::Axial), &series), 0);
        // "Full range" needs every image; "Auto" uses what is loaded.
        let names = preset_names(&series);
        assert!(names.contains(&"Auto".to_owned()) && !names.contains(&"Full range".to_owned()), "{names:?}");
        // Drag sensitivity comes from the estimated range (12 bits: 4 HU per point).
        state.apply_preset(&ViewState::presets(&series)[1]);
        state.drag_window(10.0, 0.0, &series);
        assert!((state.window.width - 1540.0).abs() < 0.5, "{}", state.window.width);

        // The remaining images arrive: nothing the user set changes.
        let (window, slice, cursor) = (state.window, state.slice, state.cursor);
        for instance in (0..24).filter(|&i| i != 12) {
            fill_from_disc(&series, ct, instance);
        }
        series.finish();
        assert_eq!(state.window, window);
        assert_eq!((state.slice_index(ViewKind::Stack, &series), state.cursor), (slice, cursor));
        assert_eq!(state.slice_index(ViewKind::Mpr(MprAxis::Axial), &series), 0, "axial reformat on the same slice");
        assert_eq!(state.slice_count(ViewKind::Mpr(MprAxis::Axial), &series), 24);
        assert!(preset_names(&series).ends_with(&["Auto".to_owned(), "Full range".to_owned()]));
    }

    #[test]
    fn stack_and_mpr_positions_stay_in_sync() {
        let (_d, series) = ct();
        let mut state = ViewState::new(&series);
        state.set_slice(ViewKind::Stack, &series, 3);
        let z = series.frames()[3].plane.expect("plane").position.z;
        assert!((state.cursor.z - z).abs() < 1e-6, "crosshair follows the acquired slice");
        // Scrolling the axial reformat moves the acquired stack too.
        let before = state.slice;
        state.scroll(ViewKind::Mpr(MprAxis::Axial), &series, 2);
        assert_ne!(state.slice, before);
        assert_eq!(state.slice_count(ViewKind::Mpr(MprAxis::Axial), &series), 24);
    }

    #[test]
    fn dragging_marks_window_as_custom() {
        let (_d, series) = ct();
        let mut state = ViewState::new(&series);
        let w0 = state.window;
        state.drag_window(100.0, 0.0, &series);
        assert!(state.window.width > w0.width);
        assert!(state.preset.is_none());
        state.reset(&series);
        assert_eq!(state.window, w0);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut t = ViewTransform::default();
        t.zoom_about(2.0, [10.0, 0.0]);
        assert_eq!(t.zoom, 2.0);
        assert_eq!(t.pan, [-10.0, 0.0]);
    }
}
