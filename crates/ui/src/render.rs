//! Turns decoded series + viewing state into GPU textures, re-rendering only
//! when an input (slice, window, slab, inversion…) actually changed.

use std::collections::HashMap;

use egui::{ColorImage, Context, TextureHandle, TextureOptions, Vec2, vec2};
use summrise_core::dicom::{FrameRef, LoadedSeries};
use summrise_core::imaging::window::{gray_to_rgba, rgb_to_rgba};
use summrise_core::imaging::{DVec3, ImagePlane, MprAxis, MprImage, SlabMode, VoiFunction, reslice};
use summrise_core::{SeriesKey, ViewKind, ViewState};

/// Largest reformatted image edge (pixels).
const MPR_MAX_DIM: usize = 768;

#[derive(Debug, Clone, PartialEq)]
struct RenderKey {
    position: u64,
    window: (u64, u64, VoiFunction),
    invert: bool,
    slab: Option<(SlabMode, u64)>,
    smooth: bool,
}

/// Geometry of what is currently displayed in a viewport.
pub(crate) enum Displayed {
    Frame { index: usize, width: u32, height: u32, plane: Option<ImagePlane>, spacing: Option<[f64; 2]> },
    Mpr(Box<MprImage>),
}

impl Displayed {
    pub(crate) fn pixel_size(&self) -> (usize, usize) {
        match self {
            Self::Frame { width, height, .. } => (*width as usize, *height as usize),
            Self::Mpr(m) => (m.width, m.height),
        }
    }

    /// Display size in arbitrary units with the correct physical aspect ratio.
    pub(crate) fn physical_size(&self) -> Vec2 {
        match self {
            Self::Frame { width, height, spacing, .. } => {
                let [row, col] = spacing.unwrap_or([1.0, 1.0]);
                vec2((f64::from(*width) * col) as f32, (f64::from(*height) * row) as f32)
            }
            Self::Mpr(m) => vec2(m.width as f32, m.height as f32),
        }
    }

    /// Patient-space directions of screen-right and screen-down.
    pub(crate) fn screen_axes(&self) -> Option<(DVec3, DVec3)> {
        match self {
            Self::Frame { plane, .. } => plane.map(|p| (p.row_dir, p.col_dir)),
            Self::Mpr(m) => Some((m.right, m.down)),
        }
    }

    pub(crate) fn pixel_to_patient(&self, x: f64, y: f64) -> Option<DVec3> {
        match self {
            Self::Frame { plane, .. } => plane.map(|p| p.pixel_to_patient(x, y)),
            Self::Mpr(m) => Some(m.pixel_to_patient(x, y)),
        }
    }

    pub(crate) fn patient_to_pixel(&self, p: DVec3) -> Option<(f64, f64)> {
        match self {
            Self::Frame { plane: Some(pl), .. } => {
                let d = p - pl.position;
                Some((d.dot(pl.row_dir) / pl.col_spacing, d.dot(pl.col_dir) / pl.row_spacing))
            }
            Self::Frame { plane: None, .. } => None,
            Self::Mpr(m) => Some(m.patient_to_pixel(p)),
        }
    }

    /// Millimetres per pixel horizontally, when known.
    pub(crate) fn mm_per_pixel_x(&self) -> Option<f64> {
        match self {
            Self::Frame { spacing, .. } => spacing.map(|s| s[1]),
            Self::Mpr(m) => Some(m.spacing),
        }
    }

    pub(crate) fn value_at(&self, series: &LoadedSeries, x: usize, y: usize) -> Option<f32> {
        match self {
            Self::Frame { index, .. } => match series.frame(*index)? {
                FrameRef::Gray { width, height, data } => {
                    (x < width as usize && y < height as usize).then(|| data.get(y * width as usize + x))
                }
                FrameRef::Rgb { .. } => None,
            },
            Self::Mpr(m) => m.value_at(x, y),
        }
    }
}

pub(crate) struct Rendered {
    key: RenderKey,
    pub texture: TextureHandle,
    pub displayed: Displayed,
}

#[derive(Default)]
pub(crate) struct RenderCache {
    entries: HashMap<(SeriesKey, ViewKind), Rendered>,
}

impl RenderCache {
    /// Drops textures of series other than `keep`.
    pub(crate) fn retain_series(&mut self, keep: Option<SeriesKey>) {
        self.entries.retain(|(k, _), _| Some(*k) == keep);
    }

    pub(crate) fn get(&self, series: SeriesKey, kind: ViewKind) -> Option<&Rendered> {
        self.entries.get(&(series, kind))
    }

    /// Ensures an up-to-date texture exists for this view.
    pub(crate) fn update(
        &mut self,
        ctx: &Context,
        key: SeriesKey,
        kind: ViewKind,
        series: &LoadedSeries,
        state: &ViewState,
        smooth: bool,
    ) {
        let position = match kind {
            ViewKind::Stack => state.slice_index(kind, series) as u64,
            ViewKind::Mpr(axis) => state.mpr_position(axis).to_bits(),
        };
        let render_key = RenderKey {
            position,
            window: (state.window.center.to_bits(), state.window.width.to_bits(), state.window.function),
            invert: state.invert,
            slab: match kind {
                ViewKind::Mpr(_) => state.slab.map(|s| (s.mode, s.thickness_mm.to_bits())),
                ViewKind::Stack => None,
            },
            smooth,
        };
        if self.entries.get(&(key, kind)).is_some_and(|r| r.key == render_key) {
            return;
        }
        let Some((image, displayed)) = (match kind {
            ViewKind::Stack => render_frame(series, state),
            ViewKind::Mpr(axis) => render_mpr(series, state, axis),
        }) else {
            self.entries.remove(&(key, kind));
            return;
        };
        let options = if smooth { TextureOptions::LINEAR } else { TextureOptions::NEAREST };
        match self.entries.get_mut(&(key, kind)) {
            Some(r) => {
                r.texture.set(image, options);
                r.key = render_key;
                r.displayed = displayed;
            }
            None => {
                let texture = ctx.load_texture(format!("view-{}-{}-{kind:?}", key.study, key.series), image, options);
                self.entries.insert((key, kind), Rendered { key: render_key, texture, displayed });
            }
        }
    }
}

fn render_frame(series: &LoadedSeries, state: &ViewState) -> Option<(ColorImage, Displayed)> {
    let index = state.slice_index(ViewKind::Stack, series);
    let frame = series.frame(index)?;
    let info = series.frames.get(index)?;
    let (width, height) = frame.size();
    let rgba = match frame {
        FrameRef::Gray { data, .. } => gray_to_rgba(data.len(), |i| data.get(i), &state.window, state.invert),
        FrameRef::Rgb { data, .. } => rgb_to_rgba(data, state.invert),
    };
    let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
    Some((image, Displayed::Frame { index, width, height, plane: info.plane, spacing: info.pixel_spacing }))
}

fn render_mpr(series: &LoadedSeries, state: &ViewState, axis: MprAxis) -> Option<(ColorImage, Displayed)> {
    let volume = series.volume()?;
    let mpr = reslice(volume, axis, state.mpr_position(axis), state.slab, MPR_MAX_DIM);
    let rgba = gray_to_rgba(mpr.values.len(), |i| mpr.values[i], &state.window, state.invert);
    let image = ColorImage::from_rgba_unmultiplied([mpr.width, mpr.height], &rgba);
    Some((image, Displayed::Mpr(Box::new(mpr))))
}
