//! Volume rendering: a CPU ray caster that shows a volume as a shaded 3D
//! object or as a maximum intensity projection, seen through an orthographic
//! camera.
//!
//! The renderer is pure and deterministic: the same volume, camera, style and
//! size give the same pixels on every platform and with any number of
//! threads. It sits behind one call — [`render`]: a [`RenderVolume`], a
//! [`Camera`], a [`Style`] and an output size in, a [`Rendering`] out — so
//! that another backend (a GPU ray caster) can replace it without changing
//! its callers.
//!
//! Each pixel casts one ray along the viewing direction. Samples lie on
//! planes perpendicular to it, a fixed distance apart, and are interpolated
//! trilinearly from the voxels (the volume is sampled where it is, never
//! copied). A look is a transfer function — opacity and colour over value,
//! piecewise linear — composited front to back with diffuse shading from the
//! value gradient and the light at the viewer. Rays stop once they are
//! opaque, and skip the blocks of a coarse min/max grid in which the look
//! sees nothing; neither changes the picture beyond rounding.

use std::sync::Arc;

use glam::{DMat3, DQuat, DVec3, Mat3A, Vec3A};
use rayon::prelude::*;
use summrise_i18n::{tr, tr_ctx};

use crate::geometry::EdgeLabels;
use crate::mpr::MprAxis;
use crate::plane::PlaneBasis;
use crate::volume::{Volume, VolumeGeometry, VoxelBuffer};
use crate::window::{VoiFunction, Window};

/// Edge length, in voxels, of a block of the min/max grid.
const BLOCK: usize = 8;
const BLOCK_SHIFT: u32 = BLOCK.trailing_zeros();

/// A ray stops once less than this share of what lies behind can still show:
/// the rest changes a pixel by less than half a grey level.
const TRANSPARENCY_LEFT: f32 = 1.0 / 512.0;

/// Diffuse shading: the share of light every surface gets, and the share
/// that depends on how directly it faces the viewer (and the light).
const AMBIENT: f32 = 0.25;
const DIFFUSE: f32 = 0.75;

/// Half the edge, in voxels, of the cube across which the gradient is taken.
const GRADIENT_REACH: f32 = 1.5;

/// Entries of the lookup table of a transfer function.
const TABLE_SIZE: usize = 4096;

/// Most sampling steps across the bounding sphere of a volume (normal data
/// needs fewer: about 1700 for a 512 × 512 × 300 CT in full quality).
const MAX_SAMPLES: f64 = 4096.0;

// ---------------------------------------------------------------------------
// Camera

/// An orthographic camera: from which side the volume is seen and which part
/// of it fills the picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Rotation applied to the coronal basis (`MprAxis::Coronal.basis()`):
    /// the identity looks from the front with the head up and the patient's
    /// right on the left of the picture.
    pub orientation: DQuat,
    /// Patient position (mm) shown at the centre of the picture.
    pub center: DVec3,
    /// Millimetres across the shorter side of the picture.
    pub field_mm: f64,
}

impl Camera {
    /// The camera that shows the whole volume from `orientation`: centred on
    /// the volume, with the shorter side of the picture spanning its bounding
    /// sphere, so that no part of it leaves the picture however it is turned.
    pub fn fit(geometry: &VolumeGeometry, orientation: DQuat) -> Self {
        let center = geometry.center();
        let [nx, ny, nz] = geometry.dims.map(|n| n as f64 - 0.5);
        let radius = (0..8)
            .map(|corner| {
                let ijk = DVec3::new(
                    if corner & 1 == 0 { -0.5 } else { nx },
                    if corner & 2 == 0 { -0.5 } else { ny },
                    if corner & 4 == 0 { -0.5 } else { nz },
                );
                (geometry.to_patient(ijk) - center).length()
            })
            .fold(0.0, f64::max);
        Self { orientation, center, field_mm: 2.0 * radius }
    }

    /// Patient directions of screen-right and screen-down, and the viewing
    /// direction (`right × down`). The identity orientation gives the coronal
    /// basis bit for bit.
    pub fn basis(&self) -> PlaneBasis {
        PlaneBasis::rotated(MprAxis::Coronal, self.orientation)
    }

    /// Orientation letters for the edges of the picture, computed from the
    /// patient directions that point right and down on screen, as for every
    /// other view.
    pub fn edge_labels(&self) -> EdgeLabels {
        let basis = self.basis();
        EdgeLabels::new(basis.right, basis.down)
    }

    /// Millimetres per pixel of a picture of `size` (columns, rows) pixels.
    pub fn mm_per_pixel(&self, size: [usize; 2]) -> f64 {
        self.field_mm / size[0].min(size[1]).max(1) as f64
    }

    /// Where the patient position `p` is drawn in a picture of `size`
    /// pixels: column and row, pixel centres at whole numbers.
    pub fn to_pixel(&self, p: DVec3, size: [usize; 2]) -> (f64, f64) {
        let basis = self.basis();
        let scale = self.mm_per_pixel(size);
        let offset = p - self.center;
        (
            size[0] as f64 / 2.0 - 0.5 + offset.dot(basis.right) / scale,
            size[1] as f64 / 2.0 - 0.5 + offset.dot(basis.down) / scale,
        )
    }
}

/// The orientation `orientation` turned as by a drag on screen: the object
/// turns about an axis in the screen plane so that its near side follows the
/// pointer `right_degrees` to the right and `down_degrees` downwards. A drag
/// to the right brings the side on the left of the picture to the front.
pub fn turned(orientation: DQuat, right_degrees: f64, down_degrees: f64) -> DQuat {
    let rotation = rotation_or_identity(orientation);
    let basis = PlaneBasis::rotated(MprAxis::Coronal, rotation);
    // The object turns about this axis; the camera turns the other way round it.
    let angle = right_degrees.hypot(down_degrees).to_radians();
    match (basis.right * down_degrees - basis.down * right_degrees).try_normalize() {
        Some(axis) if angle.is_finite() => (DQuat::from_axis_angle(axis, -angle) * rotation).normalize(),
        // Nothing to turn: the orientation stays as it is, bit for bit.
        _ => orientation,
    }
}

fn rotation_or_identity(rotation: DQuat) -> DQuat {
    Some(rotation)
        .filter(|r| r.is_finite() && r.length_squared() > 1e-12)
        .map(DQuat::normalize)
        .filter(|r| r.is_finite())
        .unwrap_or(DQuat::IDENTITY)
}

/// The six standard directions from which the 3D view looks at the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardView {
    /// From the front, head up: the patient's right on the left (as coronal).
    Front,
    /// From the back, head up: the patient's left on the left.
    Back,
    /// From the patient's left side, head up: the front on the left (as sagittal).
    Left,
    /// From the patient's right side, head up: the back on the left.
    Right,
    /// From above the head, front up: the patient's left on the left.
    Top,
    /// From below the feet, front up: the patient's right on the left (as axial).
    Bottom,
}

impl StandardView {
    pub const ALL: [Self; 6] = [Self::Front, Self::Back, Self::Left, Self::Right, Self::Top, Self::Bottom];

    /// The camera orientation of this view (a rotation of the coronal basis).
    pub fn orientation(self) -> DQuat {
        use std::f64::consts::FRAC_1_SQRT_2 as H;
        match self {
            Self::Front => DQuat::IDENTITY,
            Self::Back => DQuat::from_xyzw(0.0, 0.0, 1.0, 0.0),
            Self::Left => DQuat::from_xyzw(0.0, 0.0, H, H),
            Self::Right => DQuat::from_xyzw(0.0, 0.0, -H, H),
            Self::Top => DQuat::from_xyzw(0.0, H, -H, 0.0),
            Self::Bottom => DQuat::from_xyzw(H, 0.0, 0.0, H),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Front => tr!("From the front"),
            Self::Back => tr!("From the back"),
            Self::Left => tr!("From the patient's left"),
            Self::Right => tr!("From the patient's right"),
            Self::Top => tr!("From above the head"),
            Self::Bottom => tr!("From below the feet"),
        }
    }
}

// ---------------------------------------------------------------------------
// Looks

/// How a volume is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Look {
    /// Bone as an ivory surface (CT, Hounsfield units).
    Bone,
    /// The outline of the body (CT).
    Skin,
    /// Contrast-filled vessels in red and bone in ivory (contrast-enhanced CT).
    VesselsAndBone,
    /// A surface whose opacity follows the contrast window (any units).
    Surface,
    /// Maximum intensity projection through the contrast window.
    Mip,
}

impl Look {
    /// The looks offered for a volume, with or without Hounsfield units.
    pub fn available(hounsfield: bool) -> &'static [Self] {
        if hounsfield {
            &[Self::Bone, Self::Skin, Self::VesselsAndBone, Self::Mip]
        } else {
            &[Self::Surface, Self::Mip]
        }
    }

    /// The look a volume is first shown with.
    pub fn default_for(hounsfield: bool) -> Self {
        if hounsfield { Self::Bone } else { Self::Surface }
    }

    /// Whether the contrast window changes the picture.
    pub fn follows_window(self) -> bool {
        matches!(self, Self::Surface | Self::Mip)
    }

    pub fn label(self) -> &'static str {
        match self {
            // Translators: the looks of the 3D view: how it draws the volume.
            Self::Bone => tr_ctx!("3D look", "Bone"),
            Self::Skin => tr_ctx!("3D look", "Skin"),
            Self::VesselsAndBone => tr_ctx!("3D look", "Vessels and bone"),
            Self::Surface => tr_ctx!("3D look", "Surface"),
            Self::Mip => tr!("MIP"),
        }
    }

    /// What the look shows, for its tooltip.
    pub fn description(self) -> &'static str {
        match self {
            Self::Bone => tr!("Shows bone (from 300 HU) as a shaded surface; soft tissue is left out."),
            Self::Skin => tr!("Shows the outer surface of the body (from about -600 HU)."),
            Self::VesselsAndBone => tr!(
                "For scans with contrast agent: contrast-filled vessels (from 150 HU) in red, bone (from about 450 HU) in ivory."
            ),
            Self::Surface => {
                tr!("Shows a surface whose opacity follows the contrast window: values above the window are opaque.")
            }
            Self::Mip => tr!(
                "Maximum intensity projection: the brightest value along each line of sight, through the contrast window."
            ),
        }
    }

    /// Control points (value, opacity per millimetre, colour) of the
    /// transfer function of a look with fixed ranges (in Hounsfield units).
    fn points(self) -> &'static [(f32, f32, [f32; 3])] {
        const IVORY: [f32; 3] = [0.93, 0.89, 0.80];
        const BRIGHT_IVORY: [f32; 3] = [1.0, 0.97, 0.92];
        match self {
            Self::Bone => &[(300.0, 0.0, [0.80, 0.70, 0.56]), (500.0, 0.9, IVORY), (1500.0, 0.95, BRIGHT_IVORY)],
            Self::Skin => &[(-600.0, 0.0, [0.80, 0.56, 0.46]), (-300.0, 0.9, [0.93, 0.75, 0.63])],
            // Vessels are kept fairly transparent per millimetre: the edge of every bone
            // passes through the vessel range too (partial volume), and would otherwise
            // look red.
            Self::VesselsAndBone => &[
                (150.0, 0.0, [0.55, 0.05, 0.05]),
                (200.0, 0.25, [0.80, 0.12, 0.10]),
                (350.0, 0.3, [0.88, 0.24, 0.18]),
                (450.0, 0.9, IVORY),
                (1500.0, 0.95, BRIGHT_IVORY),
            ],
            Self::Surface | Self::Mip => &[],
        }
    }
}

/// Colour of the [`Look::Surface`] look.
const SURFACE_COLOR: [f32; 3] = [0.90, 0.86, 0.78];

/// Everything about a picture except the camera and the size: the look, and
/// the contrast settings of the looks that follow the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub look: Look,
    pub window: Window,
    /// Inverts the grey scale of [`Look::Mip`]; the shaded looks ignore it.
    pub invert: bool,
}

/// How finely a picture is sampled along the rays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quality {
    /// One sample per voxel spacing: quick, for pictures shown while turning.
    Preview,
    /// Two samples per voxel spacing.
    Full,
}

impl Quality {
    /// Distance between samples along a ray, in voxel spacings (the smallest
    /// spacing of the volume).
    fn step(self) -> f64 {
        match self {
            Self::Preview => 1.0,
            Self::Full => 0.5,
        }
    }
}

// ---------------------------------------------------------------------------
// Prepared volume

/// A volume prepared for rendering: the volume itself, sharing its slices
/// (nothing is copied), and the smallest and largest value of every block of
/// 8 × 8 × 8 voxels for empty-space skipping, computed once.
#[derive(Debug, Clone)]
pub struct RenderVolume {
    volume: Volume,
    grid: Grid,
}

impl RenderVolume {
    pub fn new(volume: &Volume) -> Self {
        Self { volume: volume.clone(), grid: Grid::build(volume) }
    }

    pub fn volume(&self) -> &Volume {
        &self.volume
    }

    /// Whether this was prepared for `volume`: the same geometry and the
    /// very same slices.
    pub fn is_for(&self, volume: &Volume) -> bool {
        self.volume.geometry == volume.geometry
            && self.volume.slices.len() == volume.slices.len()
            && self.volume.slices.iter().zip(&volume.slices).all(|(a, b)| Arc::ptr_eq(a, b))
    }

    /// Smallest and largest value of the volume.
    pub fn value_range(&self) -> (f32, f32) {
        self.grid.range
    }
}

/// Smallest and largest value per block. Block `b` along an axis covers the
/// voxels `8b ..= 8b + 8` (one voxel overlap), which are all the voxels a
/// trilinear sample at a position from `8b` to `8b + 8` uses.
#[derive(Debug, Clone)]
struct Grid {
    blocks: [usize; 3],
    min: Vec<f32>,
    max: Vec<f32>,
    range: (f32, f32),
}

impl Grid {
    fn build(volume: &Volume) -> Self {
        let dims = volume.geometry.dims;
        let blocks = dims.map(|n| (n - 1) / BLOCK + 1);
        let span = |b: usize, n: usize| (b * BLOCK)..=((b + 1) * BLOCK).min(n - 1);
        let rows: Vec<(Vec<f32>, Vec<f32>)> = (0..blocks[2] * blocks[1])
            .into_par_iter()
            .map(|row| {
                let (bk, bj) = (row / blocks[1], row % blocks[1]);
                let mut lo = vec![f32::INFINITY; blocks[0]];
                let mut hi = vec![f32::NEG_INFINITY; blocks[0]];
                for k in span(bk, dims[2]) {
                    let slice = &volume.slices[k];
                    for j in span(bj, dims[1]) {
                        let start = j * dims[0];
                        for (bi, (lo, hi)) in lo.iter_mut().zip(hi.iter_mut()).enumerate() {
                            let (min, max) = row_range(slice, start, span(bi, dims[0]));
                            *lo = lo.min(min);
                            *hi = hi.max(max);
                        }
                    }
                }
                (lo, hi)
            })
            .collect();
        let (min, max): (Vec<Vec<f32>>, Vec<Vec<f32>>) = rows.into_iter().unzip();
        let (min, max) = (min.concat(), max.concat());
        let lowest = min.iter().copied().fold(f32::INFINITY, f32::min);
        let highest = max.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let range = if lowest <= highest { (lowest, highest) } else { (0.0, 0.0) };
        Self { blocks, min, max, range }
    }

    #[inline]
    fn index(&self, b: [usize; 3]) -> usize {
        (b[2] * self.blocks[1] + b[1]) * self.blocks[0] + b[0]
    }
}

/// Smallest and largest finite value of `slice[start + i]` for `i` in `columns`.
fn row_range(slice: &VoxelBuffer, start: usize, columns: std::ops::RangeInclusive<usize>) -> (f32, f32) {
    let range = (start + columns.start())..=(start + columns.end());
    let fold = |(lo, hi): (f32, f32), v: f32| if v.is_finite() { (lo.min(v), hi.max(v)) } else { (lo, hi) };
    let empty = (f32::INFINITY, f32::NEG_INFINITY);
    match slice {
        VoxelBuffer::I16(v) => v[range].iter().map(|&x| f32::from(x)).fold(empty, fold),
        VoxelBuffer::U16(v) => v[range].iter().map(|&x| f32::from(x)).fold(empty, fold),
        VoxelBuffer::F32(v) => v[range].iter().copied().fold(empty, fold),
    }
}

// ---------------------------------------------------------------------------
// Sampling

/// Trilinear interpolation at a voxel index position, clamped into the volume.
trait Sample: Sync {
    fn sample(&self, p: Vec3A) -> f32;
}

/// The renderer's sampler: the interpolation of `Volume::sample`, in single
/// precision and clamped everywhere (the gradient reaches a voxel beyond the
/// border), reading the shared slices of the volume.
struct Voxels<'a> {
    slices: Vec<&'a VoxelBuffer>,
    dims: [usize; 3],
    max: Vec3A,
}

impl<'a> Voxels<'a> {
    fn new(volume: &'a Volume) -> Self {
        let dims = volume.geometry.dims;
        let max = Vec3A::new((dims[0] - 1) as f32, (dims[1] - 1) as f32, (dims[2] - 1) as f32);
        Self { slices: volume.slices.iter().map(|s| &**s).collect(), dims, max }
    }
}

impl Sample for Voxels<'_> {
    #[inline]
    fn sample(&self, p: Vec3A) -> f32 {
        let p = p.clamp(Vec3A::ZERO, self.max);
        let [nx, ny, nz] = self.dims;
        let (i0, j0, k0) = (p.x as usize, p.y as usize, p.z as usize);
        let (i1, j1, k1) = ((i0 + 1).min(nx - 1), (j0 + 1).min(ny - 1), (k0 + 1).min(nz - 1));
        let (fx, fy, fz) = (p.x - i0 as f32, p.y - j0 as f32, p.z - k0 as f32);
        let (s0, s1) = (self.slices[k0], self.slices[k1]);
        let (r0, r1) = (j0 * nx, j1 * nx);
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let c00 = lerp(s0.get(r0 + i0), s0.get(r0 + i1), fx);
        let c10 = lerp(s0.get(r1 + i0), s0.get(r1 + i1), fx);
        let c01 = lerp(s1.get(r0 + i0), s1.get(r0 + i1), fx);
        let c11 = lerp(s1.get(r1 + i0), s1.get(r1 + i1), fx);
        lerp(lerp(c00, c10, fy), lerp(c01, c11, fy), fz)
    }
}

// ---------------------------------------------------------------------------
// Transfer functions

/// A transfer function as a lookup table over value: opacity per sample
/// (for the sampling step of the picture) and colour, interpolated linearly
/// between the entries; values outside the table take its end entries.
struct Table {
    first: f32,
    /// Values per entry.
    step: f32,
    alpha: Vec<f32>,
    color: Vec<Vec3A>,
    /// Entries with an opacity above 0 before each entry.
    opaque_before: Vec<u32>,
    /// The value up to which the look shows nothing and beyond which it
    /// shows something (all looks begin so), if there is one.
    onset: Option<f32>,
}

impl Table {
    /// The table of `style` for samples `step_mm` apart. Fixed looks span
    /// −1024 to 3071 HU; looks that follow the window span `range`.
    fn new(style: &Style, range: (f32, f32), step_mm: f32) -> Self {
        let (first, last) = if style.look.follows_window() { range } else { (-1024.0, 3071.0) };
        let step = ((last - first) / (TABLE_SIZE - 1) as f32).max(f32::MIN_POSITIVE);
        let mut alpha = Vec::with_capacity(TABLE_SIZE);
        let mut color = Vec::with_capacity(TABLE_SIZE);
        for n in 0..TABLE_SIZE {
            let value = first + n as f32 * step;
            let (per_mm, rgb) = match style.look {
                Look::Surface => (style.window.apply(f64::from(value)) as f32, SURFACE_COLOR),
                Look::Mip => (0.0, [0.0; 3]),
                look => piecewise(look.points(), value),
            };
            // Opacity of a layer `step_mm` thick, from the opacity of a 1 mm layer.
            let per_mm = per_mm.clamp(0.0, 1.0);
            alpha.push(if per_mm >= 1.0 { 1.0 } else { 1.0 - (1.0 - per_mm).powf(step_mm) });
            color.push(Vec3A::from_array(rgb));
        }
        let mut opaque_before = Vec::with_capacity(TABLE_SIZE + 1);
        let mut count = 0u32;
        opaque_before.push(0);
        for &a in &alpha {
            count += u32::from(a > 0.0);
            opaque_before.push(count);
        }
        // Linear between the entries: nothing up to the entry before the first opaque one.
        let onset = alpha.iter().position(|&a| a > 0.0).filter(|&i| i > 0).map(|i| first + (i - 1) as f32 * step);
        Self { first, step, alpha, color, opaque_before, onset }
    }

    /// Where within a step the ray enters what the look shows, for a step
    /// that ends at a sample of `value` and begins at a sample of `before`
    /// that showed nothing: the fraction of the step before the onset of the
    /// look, taking the values between two samples as linear. `None` when the
    /// step does not cross the onset.
    fn entering(&self, before: f32, value: f32) -> Option<f32> {
        let onset = self.onset?;
        (before < onset && onset < value).then(|| (onset - before) / (value - before))
    }

    #[inline]
    fn position(&self, value: f32) -> f32 {
        ((value - self.first) / self.step).clamp(0.0, (TABLE_SIZE - 1) as f32)
    }

    /// Opacity per sample and colour of `value`.
    #[inline]
    fn lookup(&self, value: f32) -> (f32, Vec3A) {
        let f = self.position(value);
        let i = f as usize;
        let j = (i + 1).min(TABLE_SIZE - 1);
        let t = f - i as f32;
        let alpha = self.alpha[i] + (self.alpha[j] - self.alpha[i]) * t;
        (alpha, self.color[i] + (self.color[j] - self.color[i]) * t)
    }

    /// Whether every value from `lo` to `hi` is fully transparent (with one
    /// entry to spare on either side, against rounding).
    fn transparent(&self, lo: f32, hi: f32) -> bool {
        if lo > hi {
            return true;
        }
        let from = (self.position(lo) as usize).saturating_sub(1);
        let to = (self.position(hi).ceil() as usize + 1).min(TABLE_SIZE - 1);
        self.opaque_before[to + 1] == self.opaque_before[from]
    }
}

/// Opacity and colour of a piecewise-linear function at `value`; beyond the
/// first and the last point the function keeps their values.
fn piecewise(points: &[(f32, f32, [f32; 3])], value: f32) -> (f32, [f32; 3]) {
    let Some(&(first, alpha, color)) = points.first() else { return (0.0, [0.0; 3]) };
    if value <= first {
        return (alpha, color);
    }
    for pair in points.windows(2) {
        let ((v0, a0, c0), (v1, a1, c1)) = (pair[0], pair[1]);
        if value <= v1 {
            let t = (value - v0) / (v1 - v0);
            let mix = |a: f32, b: f32| a + (b - a) * t;
            return (mix(a0, a1), [mix(c0[0], c1[0]), mix(c0[1], c1[1]), mix(c0[2], c1[2])]);
        }
    }
    points.last().map_or((0.0, [0.0; 3]), |&(_, alpha, color)| (alpha, color))
}

// ---------------------------------------------------------------------------
// Rendering

/// A rendered picture: premultiplied RGBA, row by row. Alpha is the opacity
/// of what the ray met; where a ray meets nothing the pixel is transparent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendering {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Rendering {
    /// The RGBA values of pixel (`x`, `y`).
    pub fn pixel(&self, x: usize, y: usize) -> Option<[u8; 4]> {
        (x < self.width && y < self.height).then(|| {
            let i = (y * self.width + x) * 4;
            [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
        })
    }
}

/// Which shortcuts a ray takes. Both leave the picture as it is (empty-space
/// skipping exactly, early termination within one grey level); the tests
/// compare against the plain ray caster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shortcuts {
    skip_empty: bool,
    stop_when_opaque: bool,
}

impl Shortcuts {
    const ALL: Self = Self { skip_empty: true, stop_when_opaque: true };
}

/// Renders `volume` seen through `camera` in `style` into a picture of
/// `size` (columns, rows) pixels, in parallel over the rows. Returns `None`
/// when `cancelled` turns true before the picture is complete.
pub fn render(
    volume: &RenderVolume,
    camera: &Camera,
    style: &Style,
    size: [usize; 2],
    quality: Quality,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<Rendering> {
    render_with(volume, camera, style, size, quality, Shortcuts::ALL, cancelled)
}

fn render_with(
    volume: &RenderVolume,
    camera: &Camera,
    style: &Style,
    size: [usize; 2],
    quality: Quality,
    shortcuts: Shortcuts,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<Rendering> {
    let [width, height] = size;
    let mut rgba = vec![0u8; width * height * 4];
    if width == 0 || height == 0 || !(camera.field_mm.is_finite() && camera.field_mm > 0.0) {
        return Some(Rendering { width, height, rgba });
    }
    let setup = Setup::new(volume, camera, style, size, quality, shortcuts);
    Rows { setup: &setup, rgba: &mut rgba, cancelled }.run(&Voxels::new(&volume.volume));
    if cancelled() {
        return None;
    }
    Some(Rendering { width, height, rgba })
}

/// What every ray of one picture shares.
struct Setup<'a> {
    grid: &'a Grid,
    width: usize,
    /// Index-space position of the centre of pixel (0, 0) on the plane
    /// through the camera centre, and the steps to the next column and row.
    origin: DVec3,
    across: DVec3,
    down: DVec3,
    /// Index-space displacement per millimetre along the viewing direction.
    direction: Vec3A,
    /// Distance between samples, in millimetres.
    step: f32,
    /// The box the voxels fill, in index space.
    lower: Vec3A,
    upper: Vec3A,
    /// Turns an index-space gradient into a patient-space one.
    to_patient: Mat3A,
    /// Towards the viewer and the light, in patient space.
    light: Vec3A,
    kind: Kind,
    shortcuts: Shortcuts,
}

enum Kind {
    Composite {
        table: Table,
        /// Blocks in which the look sees nothing.
        empty: Vec<bool>,
    },
    Mip {
        window: Window,
        invert: bool,
        /// Above this value the window shows white, so a ray can stop.
        saturates: f32,
    },
}

impl<'a> Setup<'a> {
    fn new(
        volume: &'a RenderVolume,
        camera: &Camera,
        style: &Style,
        [width, height]: [usize; 2],
        quality: Quality,
        shortcuts: Shortcuts,
    ) -> Self {
        let geometry = &volume.volume.geometry;
        let basis = camera.basis();
        let scale = camera.mm_per_pixel([width, height]);
        let corner = camera.center
            + basis.right * ((0.5 - width as f64 / 2.0) * scale)
            + basis.down * ((0.5 - height as f64 / 2.0) * scale);
        // At most `MAX_SAMPLES` steps across the bounding sphere, also for volumes whose
        // voxels are very much smaller in one direction than the volume is long.
        let step = (geometry.min_spacing() * quality.step())
            .max(Camera::fit(geometry, DQuat::IDENTITY).field_mm / MAX_SAMPLES);
        let dims = geometry.dims.map(|n| n as f32);
        let jacobian = DMat3::from_cols(geometry.axes[0], geometry.axes[1], geometry.axes[2]);
        let kind = match style.look {
            Look::Mip => Kind::Mip {
                window: style.window,
                invert: style.invert,
                saturates: match style.window.function {
                    VoiFunction::Linear | VoiFunction::LinearExact => style.window.high() as f32,
                    VoiFunction::Sigmoid => f32::INFINITY,
                },
            },
            _ => {
                let table = Table::new(style, volume.grid.range, step as f32);
                let empty = volume.grid.min.iter().zip(&volume.grid.max).map(|(&lo, &hi)| table.transparent(lo, hi));
                Kind::Composite { empty: empty.collect(), table }
            }
        };
        Self {
            grid: &volume.grid,
            width,
            origin: geometry.to_index(corner),
            across: geometry.vector_to_index(basis.right * scale),
            down: geometry.vector_to_index(basis.down * scale),
            direction: geometry.vector_to_index(basis.normal).as_vec3a(),
            step: step as f32,
            lower: Vec3A::splat(-0.5),
            upper: Vec3A::new(dims[0] - 0.5, dims[1] - 0.5, dims[2] - 0.5),
            to_patient: Mat3A::from(jacobian.inverse().transpose().as_mat3()),
            light: (-basis.normal).as_vec3a(),
            kind,
            shortcuts,
        }
    }

    /// The sample numbers `k` (sample `k` lies `k × step` mm behind the
    /// plane through the camera centre) of the samples of a ray from `start`
    /// that lie inside the volume.
    fn samples(&self, start: Vec3A) -> Option<(i64, i64)> {
        let (mut near, mut far) = (f32::NEG_INFINITY, f32::INFINITY);
        for axis in 0..3 {
            let (o, d) = (start[axis], self.direction[axis]);
            if d.abs() < 1e-12 {
                if o < self.lower[axis] || o > self.upper[axis] {
                    return None;
                }
                continue;
            }
            let (t0, t1) = ((self.lower[axis] - o) / d, (self.upper[axis] - o) / d);
            near = near.max(t0.min(t1));
            far = far.min(t0.max(t1));
        }
        if !(near.is_finite() && far.is_finite()) {
            return None;
        }
        let (first, last) = ((near / self.step).ceil() as i64, (far / self.step).floor() as i64);
        (near <= far && first <= last).then_some((first, last))
    }

    /// The block of the min/max grid holding position `p`.
    #[inline]
    fn block(&self, p: Vec3A) -> [usize; 3] {
        let p = p.max(Vec3A::ZERO);
        let b = |v: f32, n: usize| ((v as usize) >> BLOCK_SHIFT).min(n - 1);
        [b(p.x, self.grid.blocks[0]), b(p.y, self.grid.blocks[1]), b(p.z, self.grid.blocks[2])]
    }

    /// The first sample number at or after `k + 1` that may lie outside block
    /// `b`, for a ray from `start`. Samples before it are inside the block by
    /// a safe margin.
    fn leave_block(&self, start: Vec3A, k: i64, b: [usize; 3]) -> i64 {
        const MARGIN: f32 = 1e-3;
        let mut exit = f32::INFINITY;
        for axis in 0..3 {
            let d = self.direction[axis];
            let bound = if d > 0.0 && b[axis] + 1 < self.grid.blocks[axis] {
                ((b[axis] + 1) * BLOCK) as f32 - MARGIN
            } else if d < 0.0 && b[axis] > 0 {
                (b[axis] * BLOCK) as f32 + MARGIN
            } else {
                continue;
            };
            exit = exit.min((bound - start[axis]) / d);
        }
        let leave = (exit / self.step).floor();
        if leave.is_finite() { (leave as i64).max(k + 1) } else { i64::MAX }
    }

    /// One shaded ray, composited front to back: premultiplied colour and opacity.
    fn composite<S: Sample>(&self, sampler: &S, start: Vec3A, table: &Table, empty: &[bool]) -> (Vec3A, f32) {
        let Some((first, last)) = self.samples(start) else { return (Vec3A::ZERO, 0.0) };
        let (mut color, mut alpha) = (Vec3A::ZERO, 0.0f32);
        let mut k = first;
        while k <= last {
            let p = start + self.direction * (k as f32 * self.step);
            if self.shortcuts.skip_empty {
                let b = self.block(p);
                if empty[self.grid.index(b)] {
                    k = self.leave_block(start, k, b);
                    continue;
                }
            }
            let value = sampler.sample(p);
            let (mut a, rgb) = table.lookup(value);
            if a > 0.0 {
                let mut lit = p;
                if alpha == 0.0 && k > first {
                    // The ray meets something for the first time. Only the part of the step
                    // beyond the surface counts, lit as the surface is: otherwise the depth
                    // of the surface would be rounded to whole steps, which draws bands
                    // across smooth surfaces.
                    let previous = start + self.direction * ((k - 1) as f32 * self.step);
                    if let Some(before) = table.entering(sampler.sample(previous), value) {
                        a = 1.0 - (1.0 - a).powf(1.0 - before);
                        lit = previous + self.direction * (before * self.step);
                    }
                }
                let weight = (1.0 - alpha) * a;
                color += rgb * (weight * self.shade(sampler, lit));
                alpha += weight;
                if self.shortcuts.stop_when_opaque && 1.0 - alpha < TRANSPARENCY_LEFT {
                    break;
                }
            }
            k += 1;
        }
        (color, alpha)
    }

    /// Diffuse shading at `p` from the central-difference gradient, with the
    /// light at the viewer; surfaces are lit from both sides.
    fn shade<S: Sample>(&self, sampler: &S, p: Vec3A) -> f32 {
        // The gradient across a cube around `p` from its eight corners: it averages over
        // the cube, not only along three lines, so that the steps a surface makes from
        // voxel to voxel do not show as ridges.
        let mut gradient = Vec3A::ZERO;
        for corner in 0..8 {
            let sign = Vec3A::new(
                if corner & 1 == 0 { -1.0 } else { 1.0 },
                if corner & 2 == 0 { -1.0 } else { 1.0 },
                if corner & 4 == 0 { -1.0 } else { 1.0 },
            );
            gradient += sign * sampler.sample(p + sign * GRADIENT_REACH);
        }
        let gradient = self.to_patient.mul_vec3a(gradient);
        match gradient.try_normalize() {
            Some(normal) => AMBIENT + DIFFUSE * normal.dot(self.light).abs(),
            None => 1.0,
        }
    }

    /// The largest value along a ray, or `None` when it misses the volume.
    /// With `stop_at` the ray ends once the value is above it.
    fn mip<S: Sample>(&self, sampler: &S, start: Vec3A, stop_at: f32) -> Option<f32> {
        let (first, last) = self.samples(start)?;
        let mut max = f32::NEG_INFINITY;
        let mut k = first;
        while k <= last {
            let p = start + self.direction * (k as f32 * self.step);
            if self.shortcuts.skip_empty && max > f32::NEG_INFINITY {
                let b = self.block(p);
                // Nothing in this block is brighter than what the ray has met.
                if self.grid.max[self.grid.index(b)] <= max {
                    k = self.leave_block(start, k, b);
                    continue;
                }
            }
            let value = sampler.sample(p);
            if value > max {
                max = value;
                if self.shortcuts.stop_when_opaque && max > stop_at {
                    break;
                }
            }
            k += 1;
        }
        Some(max)
    }

    /// Index-space start of the ray of pixel (`x`, `y`).
    fn start(&self, x: usize, y: usize) -> Vec3A {
        (self.origin + self.across * x as f64 + self.down * y as f64).as_vec3a()
    }

    fn pixel<S: Sample>(&self, sampler: &S, x: usize, y: usize) -> [u8; 4] {
        let start = self.start(x, y);
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        match &self.kind {
            Kind::Composite { table, empty } => {
                let (color, alpha) = self.composite(sampler, start, table, empty);
                [byte(color.x), byte(color.y), byte(color.z), byte(alpha)]
            }
            Kind::Mip { window, invert, saturates } => match self.mip(sampler, start, *saturates) {
                Some(max) => {
                    let level = window.apply(f64::from(max)) as f32;
                    let g = byte(if *invert { 1.0 - level } else { level });
                    [g, g, g, 255]
                }
                None => [0; 4],
            },
        }
    }
}

/// Renders the rows of a picture in parallel.
struct Rows<'a, 'b> {
    setup: &'a Setup<'b>,
    rgba: &'a mut [u8],
    cancelled: &'a (dyn Fn() -> bool + Sync),
}

impl Rows<'_, '_> {
    fn run<S: Sample>(self, sampler: &S) {
        let setup = self.setup;
        let cancelled = self.cancelled;
        self.rgba.par_chunks_mut(setup.width * 4).enumerate().for_each(|(y, row)| {
            if cancelled() {
                return;
            }
            for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                *px = setup.pixel(sampler, x, y);
            }
        });
    }
}

#[cfg(test)]
mod tests;
