//! UI-agnostic viewing state for a series: contrast window, slice positions,
//! MPR crosshair, tilt of the reformat planes, slab projection, zoom/pan and
//! display orientation per view, and the camera and look of the 3D view.
//!
//! The state is created when the first image of a series is decoded and is
//! never changed by images arriving afterwards: window, zoom, pan and
//! position are the user's (the series' own initial window is fixed by that
//! first image).

use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use summrise_dicom::FrameRef;
use summrise_i18n::{tr, tr_ctx};
use summrise_imaging::render3d::Look;
use summrise_imaging::window::subsample;
use summrise_imaging::{
    CT_PRESETS, Colormap, DQuat, DVec3, MprAxis, MprExtent, PresetSource, Slab, SlabMode, VolumeGeometry, Window,
    WindowPreset, auto_window,
};

use crate::measure::{MeasureKind, Measurements};
use crate::orientation::DisplayOrientation;
use crate::pet::Fusion;
use crate::series::SeriesData;
use crate::stacks::StackWindow;

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
    /// The volume as a 3D object.
    Volume,
}

impl Layout {
    pub const ALL: [Self; 4] = [Self::Single, Self::Mpr, Self::Quad, Self::Volume];

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
            Self::Volume => &[ViewKind::Volume],
        }
    }

    pub fn needs_volume(self) -> bool {
        self != Self::Single
    }

    pub fn label(self) -> &'static str {
        match self {
            // Translators: the layouts of the toolbar: the acquired images one at a time, the three reformats
            // (multiplanar reformatting, MPR), the images with the three reformats (2 × 2), the 3D view.
            Self::Single => tr_ctx!("layout", "Images"),
            Self::Mpr => tr_ctx!("layout", "MPR"),
            Self::Quad => tr_ctx!("layout", "2 × 2"),
            Self::Volume => tr!("3D"),
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
    /// The volume as a 3D object (see `volume3d`).
    Volume,
}

impl ViewKind {
    pub fn label(self) -> &'static str {
        match self {
            // Translators: the view of the images as the scanner made them (not reformatted).
            Self::Stack => tr_ctx!("view", "Acquired"),
            Self::Mpr(a) => a.label(),
            Self::Volume => tr!("3D"),
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
    /// Draws and adjusts measurements; stays chosen until another tool is.
    Measure(MeasureKind),
}

impl Tool {
    pub const ALL: [Self; 3] = [Self::WindowLevel, Self::Pan, Self::Zoom];

    pub fn label(self) -> &'static str {
        match self {
            // Translators: the mouse tools: dragging changes the contrast, moves the image or zooms it.
            Self::WindowLevel => tr_ctx!("mouse tool", "Contrast"),
            Self::Pan => tr_ctx!("mouse tool", "Pan"),
            Self::Zoom => tr_ctx!("mouse tool", "Zoom"),
            Self::Measure(kind) => kind.label(),
        }
    }

    /// Whether this is one of the measuring tools.
    pub fn is_measuring(self) -> bool {
        matches!(self, Self::Measure(_))
    }
}

/// Zoom (1 = fit), pan (screen points) and display orientation of one viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTransform {
    pub zoom: f32,
    pub pan: [f32; 2],
    /// How the picture is turned and mirrored on screen.
    pub orientation: DisplayOrientation,
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self { zoom: 1.0, pan: [0.0, 0.0], orientation: DisplayOrientation::STANDARD }
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
    /// The contrast table (VOI LUT) the acquired images are shown through,
    /// an index into `SeriesData::stack_luts`; `None`: the window. Reformats
    /// and slabs always use the window (see `contrast`).
    pub lut: Option<usize>,
    /// Scanner graphics are drawn over the acquired images.
    pub show_graphics: bool,
    pub invert: bool,
    /// Index into the acquired frames of the current stack; for the index
    /// among all images of the series use [`Self::slot_index`] (see `stacks`).
    pub slice: usize,
    /// MPR crosshair (patient coordinates, mm).
    pub cursor: DVec3,
    pub slab: Option<Slab>,
    /// Rotation of the three reformat planes about the crosshair (see `planes`).
    pub(crate) plane_rotation: DQuat,
    /// The stack shown (see `stacks`); 0 for a series that is not split.
    pub(crate) stack: usize,
    /// Windows of the stacks not shown, for stacks that keep a window each.
    pub(crate) stack_windows: HashMap<usize, StackWindow>,
    /// The window shown while the current stack's own default is not known
    /// yet (no image of it decoded); replaced by that default, if unchanged.
    pub(crate) pending_window: Option<StackWindow>,
    /// Orientation of the camera of the 3D view (see `volume3d`).
    pub(crate) camera: DQuat,
    /// How the 3D view draws the volume.
    pub look: Look,
    /// Colour map of a PET or nuclear-medicine series (see `pet`).
    pub(crate) colormap: Colormap,
    /// A PET drawn over this series, and how (see `pet`).
    pub(crate) fusion: Option<Fusion>,
    /// What the user measured on the pictures of this series (see `measure`).
    pub measurements: Measurements,
    transforms: HashMap<ViewKind, ViewTransform>,
}

impl ViewState {
    /// The state a series opens with, once its first image is decoded.
    pub fn new(series: &SeriesData) -> Self {
        let (window, preset) = crate::pet::opening_window(series, series.presets(), series.initial_window());
        let mut state = Self {
            window,
            preset: Some(preset),
            lut: crate::contrast::default_lut(series.presets(), series.luts()),
            show_graphics: true,
            invert: series.monochrome1(),
            slice: series.initial_index(),
            // Known from the headers, so the crosshair is in place when the volume arrives.
            cursor: series.geometry().map_or(DVec3::ZERO, VolumeGeometry::center),
            slab: None,
            plane_rotation: DQuat::IDENTITY,
            stack: 0,
            stack_windows: HashMap::new(),
            pending_window: None,
            camera: DQuat::IDENTITY,
            look: Look::default_for(series.units() == Some("HU")),
            colormap: crate::pet::default_colormap(series),
            fusion: None,
            measurements: Measurements::default(),
            transforms: HashMap::new(),
        };
        // Stacks that keep a window each open with the first stack's own.
        state.open_stack_window(series);
        // The crosshair starts on the plane of the image shown (the centre of the volume
        // lies between two images when their number is even): reformats that appear
        // later show the plane the user has been looking at, not one beside it.
        state.set_slice(ViewKind::Stack, series, state.slice);
        state
    }

    pub fn transform(&self, kind: ViewKind) -> ViewTransform {
        self.transforms.get(&kind).copied().unwrap_or_default()
    }

    pub fn transform_mut(&mut self, kind: ViewKind) -> &mut ViewTransform {
        self.transforms.entry(kind).or_default()
    }

    /// Resets zoom, pan and display orientation of every view.
    pub fn reset_transforms(&mut self) {
        self.transforms.clear();
    }

    /// Whether the view is still as the series opened it: nothing the user
    /// can set (contrast and contrast table, scanner graphics, inversion,
    /// position, stack, the windows of other stacks, slab, tilt of the
    /// planes, colour map, a PET drawn over it, measurements, zoom, pan, turn
    /// and flip of a view, turn and look of the 3D view) was changed.
    pub fn is_pristine(&self, series: &SeriesData) -> bool {
        let fresh = Self::new(series);
        self.window == fresh.window
            && self.preset == fresh.preset
            && self.lut == fresh.lut
            && self.show_graphics == fresh.show_graphics
            && self.invert == fresh.invert
            && self.slice == fresh.slice
            && self.stack == fresh.stack
            && self.stack_windows_pristine(series)
            && self.slab == fresh.slab
            && self.plane_rotation == fresh.plane_rotation
            && self.camera == fresh.camera
            && self.look == fresh.look
            && self.colormap == fresh.colormap
            && self.fusion.is_none()
            && self.measurements.is_empty()
            && self.transforms.values().all(|transform| *transform == ViewTransform::default())
    }

    /// Resets contrast (of every stack), colour map and geometry and switches
    /// a PET overlay off, but keeps the current position (the image and the
    /// stack) and what was measured; a measurement being drawn is dropped.
    pub fn reset(&mut self, series: &SeriesData) {
        let fresh = Self::new(series);
        self.window = fresh.window;
        self.preset = fresh.preset;
        self.lut = fresh.lut;
        self.show_graphics = fresh.show_graphics;
        self.invert = fresh.invert;
        self.slab = None;
        self.plane_rotation = fresh.plane_rotation;
        self.colormap = fresh.colormap;
        self.fusion = None;
        self.reset_stack_windows(series);
        self.camera = fresh.camera;
        self.look = fresh.look;
        self.measurements.cancel();
        self.reset_transforms();
    }

    /// All presets applicable to `series` (to its stack 0); see
    /// [`Self::presets_for`].
    pub fn presets(series: &SeriesData) -> Vec<WindowPreset> {
        Self::presets_for(series, 0)
    }

    /// All presets applicable to stack `stack` of `series`: stored DICOM
    /// windows, clinical CT windows (for Hounsfield data), an automatic window
    /// from the images loaded so far and, once every image is loaded, the full
    /// range. Stacks that keep a window each have their own stored windows,
    /// automatic window and full range.
    pub fn presets_for(series: &SeriesData, stack: usize) -> Vec<WindowPreset> {
        let mut out = series.stack_presets(stack).to_vec();
        if series.units() == Some("HU") {
            out.extend(CT_PRESETS.iter().cloned());
        }
        // For uptake values: SUV 0–5, 0–7, 0–10 and 0–15.
        out.extend(crate::pet::uptake_presets(series));
        if !series.is_color() {
            out.push(WindowPreset {
                name: Cow::Borrowed("Auto"),
                window: auto_for(series, stack),
                source: PresetSource::Auto,
            });
            // The values actually present are only known when loading is complete.
            if series.is_complete() {
                let (lo, hi) = series.stack_value_range(stack);
                out.push(WindowPreset {
                    name: Cow::Borrowed("Full range"),
                    window: Window::from_range(f64::from(lo), f64::from(hi)),
                    source: PresetSource::FullRange,
                });
            }
        }
        out
    }

    /// Applies a window preset (and leaves a contrast table).
    pub fn apply_preset(&mut self, preset: &WindowPreset) {
        self.window = preset.window;
        self.preset = Some(preset.name.to_string());
        self.lut = None;
    }

    /// Pointer-drag contrast adjustment, scaled to the value range of the
    /// series or its stack (an estimate from the headers until every image is
    /// loaded). Adjusting by hand leaves a contrast table for the window.
    pub fn drag_window(&mut self, dx: f64, dy: f64, series: &SeriesData) {
        let (lo, hi) = series.stack_value_range(self.stack);
        let units_per_point = (f64::from(hi - lo) / 1024.0).max(0.05);
        self.window = self.window.dragged(dx, dy, units_per_point);
        self.preset = None;
        self.lut = None;
    }

    pub fn slice_count(&self, kind: ViewKind, series: &SeriesData) -> usize {
        match (kind, series.stack_volume(self.stack)) {
            (ViewKind::Stack, _) => series.stack_len(),
            (ViewKind::Mpr(axis), Some(v)) => MprExtent::of_plane(&v.geometry, &self.plane_basis(axis)).slice_count(),
            (ViewKind::Mpr(_), None) => 0,
            // No slices: the 3D view shows the whole volume.
            (ViewKind::Volume, _) => 0,
        }
    }

    pub fn slice_index(&self, kind: ViewKind, series: &SeriesData) -> usize {
        match (kind, series.stack_volume(self.stack)) {
            (ViewKind::Stack, _) => self.slice.min(series.stack_len().saturating_sub(1)),
            (ViewKind::Mpr(axis), Some(v)) => {
                let basis = self.plane_basis(axis);
                MprExtent::of_plane(&v.geometry, &basis).index_of(self.cursor.dot(basis.normal))
            }
            (ViewKind::Mpr(_), None) => 0,
            (ViewKind::Volume, _) => 0,
        }
    }

    /// Position (mm along the view normal) of an MPR view.
    pub fn mpr_position(&self, axis: MprAxis) -> f64 {
        self.cursor.dot(self.plane_basis(axis).normal)
    }

    pub fn set_slice(&mut self, kind: ViewKind, series: &SeriesData, index: usize) {
        match kind {
            ViewKind::Stack => {
                self.slice = index.min(series.stack_len().saturating_sub(1));
                let plane = series.frames().get(self.slot_index(series)).and_then(|f| f.plane);
                if let (Some(geometry), Some(plane)) = (series.geometry(), plane) {
                    // Keep the MPR crosshair on the displayed acquired slice.
                    let n = plane.normal();
                    self.cursor += n * (plane.position.dot(n) - self.cursor.dot(n));
                    self.cursor = clamp_to_volume(self.cursor, geometry);
                }
            }
            ViewKind::Mpr(axis) => {
                let Some(v) = series.stack_volume(self.stack) else { return };
                let basis = self.plane_basis(axis);
                let ext = MprExtent::of_plane(&v.geometry, &basis);
                let n = basis.normal;
                let pos = ext.position(index.min(ext.slice_count() - 1));
                self.cursor += n * (pos - self.cursor.dot(n));
                self.sync_stack_to_cursor(series);
            }
            ViewKind::Volume => {}
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
        if let Some(v) = series.stack_volume(self.stack) {
            self.cursor = clamp_to_volume(p, &v.geometry);
            self.sync_stack_to_cursor(series);
        }
    }

    fn sync_stack_to_cursor(&mut self, series: &SeriesData) {
        if let Some(v) = series.stack_volume(self.stack) {
            let k = v.geometry.to_index(self.cursor).z.round();
            self.slice = (k.max(0.0) as usize).min(series.stack_len().saturating_sub(1));
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
/// that are loaded (all of them once the series is complete) — of stack
/// `stack` only, when the stacks keep a window each.
fn auto_for(series: &SeriesData, stack: usize) -> Window {
    let images = if series.separate_windows() {
        let len = series.stack_len();
        stack * len..(stack + 1) * len
    } else {
        0..series.len()
    };
    let loaded: Vec<(usize, FrameRef<'_>)> = images.filter_map(|i| Some((i, series.frame(i)?))).collect();
    let samples: Vec<f32> = loaded
        .iter()
        .step_by(loaded.len().div_ceil(16).max(1))
        .flat_map(|(index, f)| match f {
            // Padding is left out (display-fidelity D5).
            FrameRef::Gray { data, .. } => subsample(data.len(), 32_768, |i| {
                summrise_imaging::padding::visible(data.get(i), series.padding(*index))
            }),
            FrameRef::Rgb { .. } => Vec::new(),
        })
        .collect();
    auto_window(samples).or_else(|| series.stack_initial_window(stack)).unwrap_or_else(|| series.initial_window())
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
        // The crosshair is already where the volume will be, on the plane of the image shown.
        let center = series.geometry().map(VolumeGeometry::center).expect("planned geometry");
        let shown = series.frames()[12].plane.expect("plane").position.z;
        assert_eq!((state.cursor.x, state.cursor.y), (center.x, center.y));
        assert!((state.cursor.z - shown).abs() < 1e-9, "crosshair at {} mm, image at {shown} mm", state.cursor.z);
        assert_eq!(state.mpr_position(MprAxis::Axial), state.cursor.z);
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
    fn view_is_pristine_until_the_user_changes_something() {
        let (_d, series) = ct();
        let mut state = ViewState::new(&series);
        assert!(state.is_pristine(&series));
        // Contrast, position, zoom and pan each count; undoing them counts too.
        state.drag_window(10.0, 0.0, &series);
        assert!(!state.is_pristine(&series));
        state.reset(&series);
        assert!(state.is_pristine(&series));
        state.scroll(ViewKind::Stack, &series, 1);
        assert!(!state.is_pristine(&series));
        state.scroll(ViewKind::Stack, &series, -1);
        assert!(state.is_pristine(&series));
        state.transform_mut(ViewKind::Stack).zoom_about(2.0, [0.0, 0.0]);
        assert!(!state.is_pristine(&series));
        *state.transform_mut(ViewKind::Stack) = ViewTransform::default();
        assert!(state.is_pristine(&series), "a fitted view is as it opened");
        state.invert = true;
        assert!(!state.is_pristine(&series));
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut t = ViewTransform::default();
        t.zoom_about(2.0, [10.0, 0.0]);
        assert_eq!(t.zoom, 2.0);
        assert_eq!(t.pan, [-10.0, 0.0]);
    }
}
