//! Turns a series + viewing state into GPU textures, re-rendering only when
//! an input (slice, window, slab, inversion…) actually changed — or when the
//! displayed image arrives while the series is still loading.

use std::collections::HashMap;

use egui::{ColorImage, Context, TextureHandle, TextureOptions, Vec2, vec2};
use summrise_core::dicom::FrameRef;
use summrise_core::imaging::window::{gray_to_rgba, rgb_to_rgba};
use summrise_core::imaging::{DVec3, ImagePlane, MprAxis, MprImage, PlaneBasis, SlabMode, VoiFunction, reslice_plane};
use summrise_core::{ImageSlot, SeriesData, SeriesKey, ViewKind, ViewState};

use crate::fidelity;

/// Largest reformatted image edge (pixels).
const MPR_MAX_DIM: usize = 768;

#[derive(Debug, Clone, PartialEq)]
struct RenderKey {
    position: u64,
    /// The stack shown, whose volume the reformats show.
    stack: usize,
    /// Orientation of a reformat plane; changes when the planes are tilted.
    plane: Option<PlaneBasis>,
    /// What the displayed slot holds; changes when its image is decoded.
    picture: Picture,
    window: (u64, u64, VoiFunction),
    invert: bool,
    slab: Option<(SlabMode, u64)>,
    smooth: bool,
    /// Acquired images: the contrast table shown instead of the window, and
    /// whether scanner graphics are drawn (display-fidelity).
    contrast_table: Option<usize>,
    graphics: bool,
    /// Colour map, and a PET drawn over the picture (see `pet`).
    look: crate::pet::Look,
}

/// What a viewport shows at the place of the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Picture {
    /// The image itself.
    Image,
    /// A placeholder: the image is not decoded yet.
    Loading,
    /// A placeholder: the image could not be decoded, and why.
    Failed(String),
}

impl Picture {
    fn of(slot: ImageSlot<'_>) -> Self {
        match slot {
            ImageSlot::Ready(_) => Self::Image,
            ImageSlot::Pending => Self::Loading,
            ImageSlot::Failed(reason) => Self::Failed(reason.to_owned()),
        }
    }
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

    /// Millimetres per pixel vertically, when known.
    pub(crate) fn mm_per_pixel_y(&self) -> Option<f64> {
        match self {
            Self::Frame { spacing, .. } => spacing.map(|s| s[0]),
            Self::Mpr(m) => Some(m.spacing),
        }
    }

    pub(crate) fn value_at(&self, series: &SeriesData, x: usize, y: usize) -> Option<f32> {
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
    /// The last image rendered for this view. It is only what the view shows
    /// while `picture` is [`Picture::Image`]: a placeholder is never drawn
    /// with the pixels of another image.
    texture: Option<TextureHandle>,
    pub picture: Picture,
    /// Geometry of the displayed image, known from the headers even while
    /// its pixels are missing.
    pub displayed: Displayed,
}

impl Rendered {
    /// Whether a PET is drawn into this picture.
    #[cfg(test)]
    pub(crate) fn draws_overlay(&self) -> bool {
        self.key.look.draws_overlay()
    }

    /// The texture to draw, unless a placeholder stands in for the image.
    pub(crate) fn texture(&self) -> Option<&TextureHandle> {
        self.texture.as_ref().filter(|_| self.picture == Picture::Image)
    }
}

#[derive(Default)]
pub(crate) struct RenderCache {
    /// What each view shows, and (with the image's index) the pictures of the
    /// following tiles of a tile grid.
    entries: HashMap<(SeriesKey, ViewKind, Option<usize>), Rendered>,
}

impl RenderCache {
    /// Drops textures of series other than those in `keep`.
    pub(crate) fn retain_series(&mut self, keep: &[SeriesKey]) {
        self.entries.retain(|(k, _, _), _| keep.contains(k));
    }

    /// Drops the pictures of tiles other than those in `keep`, images of a
    /// series: what a grid no longer shows. At most 24 per viewport remain.
    pub(crate) fn retain_tiles(&mut self, keep: &[(SeriesKey, usize)]) {
        self.entries.retain(|(k, _, image), _| image.is_none_or(|image| keep.contains(&(*k, image))));
    }

    pub(crate) fn get(&self, series: SeriesKey, kind: ViewKind) -> Option<&Rendered> {
        self.entries.get(&(series, kind, None))
    }

    /// How many pictures are kept.
    #[cfg(test)]
    pub(crate) fn entries_for_test(&self) -> usize {
        self.entries.len()
    }

    /// The picture of acquired image `index` in a tile grid (not the first
    /// tile, which is what [`Self::get`] gives for the acquired view).
    pub(crate) fn get_tile(&self, series: SeriesKey, index: usize) -> Option<&Rendered> {
        self.entries.get(&(series, ViewKind::Stack, Some(index)))
    }

    /// Ensures an up-to-date texture exists for this view. `overlay` is the
    /// series a PET overlay of the view is drawn from, once it is ready.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update(
        &mut self,
        ctx: &Context,
        key: SeriesKey,
        kind: ViewKind,
        series: &SeriesData,
        state: &ViewState,
        smooth: bool,
        overlay: Option<crate::pet::Overlay<'_>>,
    ) {
        self.update_entry(ctx, (key, kind, None), series, state, smooth, overlay);
    }

    /// The same for the tile that shows acquired image `index`: its picture
    /// is rendered once and kept until its image, window or colour map changes.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_tile(
        &mut self,
        ctx: &Context,
        key: SeriesKey,
        index: usize,
        series: &SeriesData,
        state: &ViewState,
        smooth: bool,
        overlay: Option<crate::pet::Overlay<'_>>,
    ) {
        self.update_entry(ctx, (key, ViewKind::Stack, Some(index)), series, state, smooth, overlay);
    }

    fn update_entry(
        &mut self,
        ctx: &Context,
        entry: (SeriesKey, ViewKind, Option<usize>),
        series: &SeriesData,
        state: &ViewState,
        smooth: bool,
        overlay: Option<crate::pet::Overlay<'_>>,
    ) {
        let (key, kind, tile) = entry;
        let (position, picture) = match kind {
            ViewKind::Stack => {
                let index = tile.unwrap_or_else(|| state.slot_index(series));
                (index as u64, Picture::of(series.image(index)))
            }
            ViewKind::Mpr(axis) => (state.mpr_position(axis).to_bits(), Picture::Image),
            // Drawn by `volume_view`.
            ViewKind::Volume => return,
        };
        let render_key = RenderKey {
            position,
            stack: state.stack(),
            plane: if let ViewKind::Mpr(axis) = kind { Some(state.plane_basis(axis)) } else { None },
            picture,
            window: (state.window.center.to_bits(), state.window.width.to_bits(), state.window.function),
            invert: state.invert,
            slab: match kind {
                ViewKind::Mpr(_) => state.slab.map(|s| (s.mode, s.thickness_mm.to_bits())),
                ViewKind::Stack => None,
                ViewKind::Volume => None,
            },
            smooth,
            contrast_table: state.lut.filter(|_| kind == ViewKind::Stack),
            graphics: kind == ViewKind::Stack && state.show_graphics,
            look: crate::pet::Look::of(state, overlay),
        };
        if self.entries.get(&entry).is_some_and(|r| r.key == render_key) {
            return;
        }
        let Some((image, displayed)) = (match (kind, tile) {
            (ViewKind::Stack, Some(index)) => render_image(series, state, index, overlay),
            (ViewKind::Stack, None) => render_frame(series, state, overlay),
            (ViewKind::Mpr(axis), _) => render_mpr(series, state, axis, overlay),
            (ViewKind::Volume, _) => None,
        }) else {
            self.entries.remove(&entry);
            return;
        };
        let options = if smooth { TextureOptions::LINEAR } else { TextureOptions::NEAREST };
        let load = |image| ctx.load_texture(format!("view-{}-{}-{kind:?}", key.study, key.series), image, options);
        let picture = render_key.picture.clone();
        match self.entries.get_mut(&entry) {
            Some(r) => {
                // A placeholder leaves the old texture allocated but undrawn (see `Rendered::texture`).
                if let Some(image) = image {
                    match &mut r.texture {
                        Some(texture) => texture.set(image, options),
                        None => r.texture = Some(load(image)),
                    }
                }
                r.key = render_key;
                r.picture = picture;
                r.displayed = displayed;
            }
            None => {
                let texture = image.map(load);
                self.entries.insert(entry, Rendered { key: render_key, texture, picture, displayed });
            }
        }
    }
}

/// The acquired image at the current position: its pixels if they are
/// decoded, and in any case its geometry, which is known from the headers.
pub(crate) fn render_frame(
    series: &SeriesData,
    state: &ViewState,
    overlay: Option<crate::pet::Overlay<'_>>,
) -> Option<(Option<ColorImage>, Displayed)> {
    render_image(series, state, state.slot_index(series), overlay)
}

/// Acquired image `index`, whichever it is (a tile shows another than the
/// current one), through the contrast, the colour map and the graphics of
/// the view: its pixels if they are decoded, and in any case its geometry.
pub(crate) fn render_image(
    series: &SeriesData,
    state: &ViewState,
    index: usize,
    overlay: Option<crate::pet::Overlay<'_>>,
) -> Option<(Option<ColorImage>, Displayed)> {
    let info = series.frames().get(index)?;
    let frame = series.frame(index);
    let (width, height) = frame.map_or((info.width, info.height), |f| f.size());
    let image = frame.map(|frame| {
        let mut rgba = match frame {
            FrameRef::Gray { data, .. } => fidelity::grey_rgba(series, index, data, state),
            FrameRef::Rgb { data, .. } => rgb_to_rgba(data, state.invert),
        };
        // The colour map and a PET over the picture, then the scanner graphics over both.
        crate::pet::compose_frame(&mut rgba, frame, info.plane.as_ref(), state, overlay);
        fidelity::draw_graphics(&mut rgba, (width, height), series, index, state);
        ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba)
    });
    Some((image, Displayed::Frame { index, width, height, plane: info.plane, spacing: info.pixel_spacing }))
}

pub(crate) fn render_mpr(
    series: &SeriesData,
    state: &ViewState,
    axis: MprAxis,
    overlay: Option<crate::pet::Overlay<'_>>,
) -> Option<(Option<ColorImage>, Displayed)> {
    let volume = series.stack_volume(state.stack())?;
    let mpr = reslice_plane(volume, &state.plane_basis(axis), state.mpr_position(axis), state.slab, MPR_MAX_DIM);
    let mut rgba = gray_to_rgba(mpr.values.len(), |i| mpr.values[i], &state.window, state.invert);
    crate::pet::compose_reformat(&mut rgba, &mpr, state, overlay);
    let image = ColorImage::from_rgba_unmultiplied([mpr.width, mpr.height], &rgba);
    Some((Some(image), Displayed::Mpr(Box::new(mpr))))
}
