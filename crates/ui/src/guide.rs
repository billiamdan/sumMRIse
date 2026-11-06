//! The plain-language guide in the interface: the "About this series" card,
//! the "Where is this image?" figure, the info marks of the inspector rows
//! and the explanations of the contrast presets.
//!
//! What is said, and the geometry behind the figure, come from
//! `summrise_core::guide`; this module only lays it out and draws it. The
//! figure is a schematic and says so: where a scan lies inside the body is
//! not known, only where the image on screen lies inside the scanned block.

use egui::{
    Align2, CollapsingHeader, Color32, CornerRadius, FontId, Frame, Label, Margin, Painter, Pos2, Rect, Response,
    RichText, Role, Sense, Shape, Stroke, StrokeKind, Tooltip, Ui, Vec2, WidgetInfo, pos2, vec2,
};
use egui_material_icons::icons::ICON_INFO;
use summrise_core::dicom::{Series, SummaryRow};
use summrise_core::guide::{self, Block, BodyRegion, Placement, ScreenPlane, SeriesGuide};
use summrise_core::imaging::{DVec3, WindowPreset, orientation_label};
use summrise_core::{DisplayOrientation, Session, Settings, ViewKind};
use summrise_i18n::tr;

use crate::render::{Displayed, RenderCache};
use crate::theme::{self, Palette};
use crate::widgets::icon;

/// Widest an explanation is laid out, in points.
const EXPLANATION_WIDTH: f32 = 340.0;

// ---------------------------------------------------------------------------
// What is explained

/// What the inspector explains: the selected series and the image that the
/// active view shows.
pub(crate) struct Scene<'a> {
    pub series: &'a Series,
    /// The image on screen, when it has a position in the body.
    pub plane: Option<ScreenPlane>,
    /// For the 3D view: the patient directions of screen-right and
    /// screen-down of its camera. The picture is not a slice, so the figure
    /// marks no image.
    pub camera: Option<(DVec3, DVec3)>,
}

impl<'a> Scene<'a> {
    /// `None` when explanations are switched off or no series is selected.
    /// `orientation` is how the active view turns and mirrors its picture.
    pub(crate) fn of(
        settings: &Settings,
        session: &'a Session,
        renders: &RenderCache,
        view: ViewKind,
        orientation: DisplayOrientation,
    ) -> Option<Self> {
        if !settings.show_explanations {
            return None;
        }
        let (key, series) = session.active()?;
        let plane = renders.get(key, view).and_then(|rendered| screen_plane(&rendered.displayed, orientation));
        Some(Self { series, plane, camera: None })
    }

    /// The scene of the 3D view, seen through a camera with these screen axes.
    pub(crate) fn with_camera(self, camera: Option<(DVec3, DVec3)>) -> Self {
        match camera {
            Some(_) => Self { plane: None, camera, ..self },
            None => self,
        }
    }
}

/// The image a viewport shows in patient space, taken from what was rendered
/// for it and turned and mirrored as it is on screen — so it is true for
/// acquired images and every kind of reformat, in every display orientation.
fn screen_plane(displayed: &Displayed, orientation: DisplayOrientation) -> Option<ScreenPlane> {
    let (right, down) = displayed.screen_axes()?;
    let (right, down) = orientation.screen_axes(right, down);
    let (width, height) = displayed.pixel_size();
    let (x, y) = (width.saturating_sub(1) as f64, height.saturating_sub(1) as f64);
    // The corners of the box on screen, top left first: the picture points shown there.
    let corner = |s: f64, t: f64| {
        let (u, v) = orientation.to_image(s, t);
        displayed.pixel_to_patient(u * x, v * y)
    };
    Some(ScreenPlane {
        right,
        down,
        corners: [corner(0.0, 0.0)?, corner(1.0, 0.0)?, corner(1.0, 1.0)?, corner(0.0, 1.0)?],
    })
}

/// Where a series lies in the body, as far as its headers tell.
struct Located {
    /// The block of the body that the images cover.
    block: Option<Block>,
    /// Body Part Examined as recorded, and the region it stands for.
    body_part: Option<String>,
    region: Option<BodyRegion>,
}

impl Located {
    fn of(series: &Series) -> Self {
        let body_part = series.acquisition.body_part.clone().or_else(|| series.body_part.clone());
        Self {
            block: guide::scanned_block(series),
            region: body_part.as_deref().and_then(guide::body_region),
            body_part,
        }
    }
}

/// The description of one series, computed once (per language).
struct Described {
    series_uid: String,
    language: &'static str,
    guide: SeriesGuide,
    located: Located,
}

/// The guide at the top of the inspector's overview.
#[derive(Default)]
pub(crate) struct Panel {
    described: Option<Described>,
}

impl Panel {
    fn describe(&mut self, series: &Series) -> &Described {
        let language = summrise_i18n::active_language();
        let stale = self.described.as_ref().is_none_or(|d| d.series_uid != series.uid || d.language != language);
        if stale {
            self.described = None;
        }
        self.described.get_or_insert_with(|| Described {
            series_uid: series.uid.clone(),
            language,
            guide: guide::describe_series(series),
            located: Located::of(series),
        })
    }

    /// The "About this series" card and the "Where is this image?" figure.
    pub(crate) fn show(&mut self, ui: &mut Ui, scene: &Scene<'_>) {
        let p = theme::palette(ui);
        let described = self.describe(scene.series);
        CollapsingHeader::new(RichText::new(tr!("About this series")).strong().color(p.text))
            .id_salt("guide-about")
            .default_open(true)
            .show(ui, |ui| about_card(ui, &described.guide));
        CollapsingHeader::new(RichText::new(tr!("Where is this image?")).strong().color(p.text))
            .id_salt("guide-where")
            .default_open(true)
            .show(ui, |ui| where_figure(ui, &described.located, scene.plane.as_ref(), scene.camera));
        ui.add_space(4.0);
    }
}

// ---------------------------------------------------------------------------
// About this series

fn wrapped(ui: &mut Ui, text: RichText) -> Response {
    ui.add(Label::new(text).wrap())
}

fn about_card(ui: &mut Ui, guide: &SeriesGuide) {
    let p = theme::palette(ui);
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;
            wrapped(ui, RichText::new(&guide.headline).size(14.5).strong().color(p.text));
            ui.add_space(4.0);
            for statement in &guide.statements {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    // A dot at the height of the first line marks each statement.
                    let (dot, _) = ui.allocate_exact_size(vec2(6.0, 17.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 2.2, p.accent);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        wrapped(ui, RichText::new(&statement.text).color(p.text));
                        wrapped(ui, RichText::new(&statement.basis).small().color(p.text_weak));
                    });
                });
                ui.add_space(3.0);
            }
            ui.separator();
            wrapped(ui, RichText::new(guide::disclaimer()).small().color(p.text));
            if guide.reads_settings {
                wrapped(ui, RichText::new(guide::sampling_note()).small().color(p.text_weak));
            }
        });
}

// ---------------------------------------------------------------------------
// Where is this image?

/// Size of the body outline figure, in points.
const BODY_FIGURE: Vec2 = vec2(100.0, 218.0);
/// Size of the scanned-block figure, in points.
const BLOCK_FIGURE: Vec2 = vec2(176.0, 136.0);

/// The figure and the sentences for the image on screen (`plane`), or for the
/// 3D view seen with the screen axes `camera`: the same sentences about the
/// viewing direction, and no image marked in the figure.
fn where_figure(ui: &mut Ui, located: &Located, plane: Option<&ScreenPlane>, camera: Option<(DVec3, DVec3)>) {
    let p = theme::palette(ui);
    let placement = plane.zip(located.block.as_ref()).map(|(plane, block)| guide::place(block, plane));
    let axes = plane.map(|plane| (plane.right, plane.down)).or(camera);
    let direction = axes.and_then(|(right, down)| guide::viewing_direction(right, down));
    let note = |ui: &mut Ui, text: String| wrapped(ui, RichText::new(text).small().color(p.text_weak));
    let sentences = |ui: &mut Ui| match &direction {
        Some(direction) => {
            let title = if camera.is_some() && plane.is_none() { tr!("3D view") } else { direction.title };
            wrapped(ui, RichText::new(title).strong().color(p.text));
            wrapped(ui, RichText::new(direction.looking).color(p.text));
            wrapped(ui, RichText::new(direction.screen_left).color(p.text));
            ui.add_space(2.0);
        }
        // Nothing is said while the first image is still loading.
        None if located.block.is_some() => {}
        None => {
            wrapped(
                ui,
                RichText::new(tr!(
                    "These images carry no position information, so the figure cannot show where the image lies."
                ))
                .color(p.text),
            );
            ui.add_space(2.0);
        }
    };

    match (located.region, &placement) {
        (Some(region), _) => {
            ui.horizontal_top(|ui| {
                let (rect, response) = ui.allocate_exact_size(BODY_FIGURE, Sense::hover());
                if ui.is_rect_visible(rect) {
                    paint_body(ui.painter(), rect, &p, region, placement.as_ref());
                }
                let label = if placement.is_some() {
                    tr!("Schematic body outline: the examined region is highlighted and the image on screen is marked.")
                } else {
                    tr!("Schematic body outline: the examined region is highlighted.")
                };
                response.widget_info(|| WidgetInfo::labeled(Role::Image, true, label));
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    sentences(ui);
                    let part = located.body_part.as_deref().unwrap_or_default().trim();
                    note(
                        ui,
                        tr!(
                            "Schematic, not to scale. Highlighted: the body region recorded for this series (“{part}”).",
                            part = part
                        ),
                    );
                    if placement.is_some() {
                        note(
                            ui,
                            tr!("The coloured line or band marks the image on screen within the scanned block.")
                                .to_owned(),
                        );
                    }
                    if area(region).bands.len() > 1 {
                        note(
                            ui,
                            tr!("Both sides are highlighted, because the images do not say which side was examined.")
                                .to_owned(),
                        );
                    }
                    note(ui, tr!("R and L mark the patient's right and left.").to_owned());
                });
            });
        }
        (None, Some(placement)) => {
            let (rect, response) = ui.allocate_exact_size(BLOCK_FIGURE, Sense::hover());
            if ui.is_rect_visible(rect) {
                paint_block(ui.painter(), rect, &p, placement);
            }
            response.widget_info(|| {
                WidgetInfo::labeled(
                    Role::Image,
                    true,
                    tr!("Schematic of the scanned block with the image on screen inside it."),
                )
            });
            sentences(ui);
            note(
                ui,
                tr!("Schematic, not to scale. The box is the scanned block and the coloured sheet is the image on screen.")
                    .to_owned(),
            );
            note(
                ui,
                tr!("R is the patient's right, L left, A front, P back, S head and I feet, as at the edges of the image.")
                    .to_owned(),
            );
            note(
                ui,
                tr!("No body outline is shown, because the images name no body region that this guide recognises.")
                    .to_owned(),
            );
        }
        (None, None) => sentences(ui),
    }
}

/// Colours of the figures, from the palette of the theme.
struct FigureColors {
    /// The body outline and the edges of the block.
    outline: Color32,
    /// Edges of the block that lie behind it.
    hidden: Color32,
    /// The examined region.
    region: Color32,
    /// The image on screen.
    plane: Color32,
    letters: Color32,
}

impl FigureColors {
    fn of(p: &Palette) -> Self {
        Self {
            outline: p.text_weak.gamma_multiply(0.42),
            hidden: p.text_weak.gamma_multiply(0.38),
            region: p.accent.gamma_multiply(0.8),
            plane: p.warn,
            letters: p.text_weak,
        }
    }
}

/// Parts of the body outline.
mod part {
    pub(super) const HEAD: u8 = 1;
    pub(super) const NECK: u8 = 1 << 1;
    pub(super) const TRUNK: u8 = 1 << 2;
    pub(super) const ARMS: u8 = 1 << 3;
    pub(super) const HANDS: u8 = 1 << 4;
    pub(super) const LEGS: u8 = 1 << 5;
    pub(super) const FEET: u8 = 1 << 6;
    /// Not part of the outline: only drawn as a highlight.
    pub(super) const SPINE: u8 = 1 << 7;
    pub(super) const BODY: u8 = HEAD | NECK | TRUNK | ARMS | HANDS | LEGS | FEET;
}

/// A convex piece of the body outline, in figure space: x from the patient's
/// right (negative) to the patient's left (positive), y from the top of the
/// head (0) to the soles (100). The outline is a front view, as if the
/// patient faced the viewer.
enum Piece {
    Ellipse {
        center: [f32; 2],
        radius: [f32; 2],
    },
    /// Corners in clockwise order.
    Polygon(&'static [[f32; 2]]),
}

/// The outline: (part, piece, whether it is repeated mirrored on the
/// patient's right side).
const OUTLINE: &[(u8, Piece, bool)] = &[
    (part::HEAD, Piece::Ellipse { center: [0.0, 7.2], radius: [5.3, 6.9] }, false),
    (part::NECK, Piece::Polygon(&[[-2.6, 12.6], [2.6, 12.6], [3.6, 18.4], [-3.6, 18.4]]), false),
    (
        part::TRUNK,
        Piece::Polygon(&[[-9.6, 17.6], [9.6, 17.6], [13.2, 20.4], [10.2, 37.0], [-10.2, 37.0], [-13.2, 20.4]]),
        false,
    ),
    (
        part::TRUNK,
        Piece::Polygon(&[[-10.2, 37.0], [10.2, 37.0], [11.6, 47.5], [10.0, 53.6], [-10.0, 53.6], [-11.6, 47.5]]),
        false,
    ),
    (part::ARMS, Piece::Polygon(&[[12.0, 19.0], [15.8, 20.6], [18.8, 35.0], [15.0, 35.9]]), true),
    (part::ARMS, Piece::Polygon(&[[15.0, 35.9], [18.8, 35.0], [21.0, 48.6], [18.1, 49.3]]), true),
    (part::HANDS, Piece::Ellipse { center: [20.2, 52.7], radius: [2.1, 3.7] }, true),
    (part::LEGS, Piece::Polygon(&[[0.9, 52.0], [11.1, 50.0], [9.5, 70.5], [3.3, 70.5]]), true),
    (part::LEGS, Piece::Polygon(&[[3.3, 70.5], [9.5, 70.5], [8.1, 91.6], [4.6, 91.6]]), true),
    (part::FEET, Piece::Ellipse { center: [7.1, 95.4], radius: [3.8, 2.8] }, true),
    (part::SPINE, Piece::Polygon(&[[-1.5, 11.5], [1.5, 11.5], [1.5, 52.0], [-1.5, 52.0]]), false),
];

/// Half the width and the height of the figure space.
const BODY_HALF_WIDTH: f32 = 23.0;
const BODY_HEIGHT: f32 = 100.0;

/// Figure space to screen.
fn body_point(rect: Rect, [x, y]: [f32; 2]) -> Pos2 {
    let scale = (rect.height() / BODY_HEIGHT).min(rect.width() / (2.0 * BODY_HALF_WIDTH));
    pos2(rect.center().x + x * scale, rect.top() + y * scale)
}

fn paint_parts(painter: &Painter, rect: Rect, parts: u8, color: Color32) {
    for (part, piece, mirrored) in OUTLINE {
        if part & parts == 0 {
            continue;
        }
        let sides: &[f32] = if *mirrored { &[1.0, -1.0] } else { &[1.0] };
        for side in sides {
            match piece {
                Piece::Ellipse { center, radius } => {
                    let c = body_point(rect, [center[0] * side, center[1]]);
                    let edge = body_point(rect, [center[0] * side + radius[0], center[1] + radius[1]]);
                    painter.add(Shape::ellipse_filled(c, (edge - c).abs(), color));
                }
                Piece::Polygon(corners) => {
                    let points = corners.iter().map(|c| body_point(rect, [c[0] * side, c[1]])).collect();
                    painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
                }
            }
        }
    }
}

/// Where a body region lies in the outline.
struct Area {
    /// The parts of the outline that are highlighted …
    parts: u8,
    /// … inside these rectangles (figure space: x0, y0, x1, y1). The scanned
    /// block is assumed to lie in each of them; paired regions have two.
    bands: Vec<[f32; 4]>,
}

fn area(region: BodyRegion) -> Area {
    use BodyRegion::*;
    let center = |parts: u8, half_width: f32, top: f32, bottom: f32| Area {
        parts,
        bands: vec![[-half_width, top, half_width, bottom]],
    };
    let both =
        |parts: u8, [x0, y0, x1, y1]: [f32; 4]| Area { parts, bands: vec![[x0, y0, x1, y1], [-x1, y0, -x0, y1]] };
    let trunk = |top: f32, bottom: f32| center(part::TRUNK, 13.5, top, bottom);
    let neck_and_trunk = |bottom: f32| center(part::NECK | part::TRUNK, 13.5, 12.6, bottom);
    let spine = |top: f32, bottom: f32| center(part::SPINE, 9.0, top, bottom);
    match region {
        Head => center(part::HEAD, 7.0, 0.0, 14.2),
        Neck => center(part::NECK, 5.0, 12.6, 18.4),
        HeadNeck => center(part::HEAD | part::NECK, 7.0, 0.0, 18.4),
        Chest => trunk(17.6, 35.0),
        Abdomen => trunk(35.0, 45.0),
        Pelvis => trunk(44.0, 53.6),
        NeckChest => neck_and_trunk(35.0),
        ChestAbdomen => trunk(17.6, 45.0),
        AbdomenPelvis => trunk(35.0, 53.6),
        ChestAbdomenPelvis => trunk(17.6, 53.6),
        NeckChestAbdomen => neck_and_trunk(45.0),
        NeckChestAbdomenPelvis => neck_and_trunk(53.6),
        CervicalSpine => spine(11.5, 19.0),
        ThoracicSpine => spine(19.0, 36.0),
        LumbarSpine => spine(36.0, 45.5),
        SacralSpine => spine(45.5, 52.0),
        CervicothoracicSpine => spine(11.5, 36.0),
        ThoracolumbarSpine => spine(19.0, 45.5),
        WholeSpine => spine(11.5, 52.0),
        Shoulder => both(part::TRUNK | part::ARMS, [7.5, 17.0, 17.5, 25.0]),
        UpperArm => both(part::ARMS, [11.5, 20.0, 19.5, 35.4]),
        Elbow => both(part::ARMS, [13.5, 32.5, 20.5, 39.0]),
        Forearm => both(part::ARMS, [14.0, 36.0, 21.5, 49.3]),
        Wrist => both(part::ARMS | part::HANDS, [16.5, 46.5, 22.5, 51.0]),
        Hand => both(part::HANDS, [17.5, 48.8, 23.0, 56.6]),
        WholeArm => both(part::ARMS | part::HANDS, [11.5, 18.5, 23.0, 56.6]),
        Hip => both(part::TRUNK | part::LEGS, [3.0, 45.5, 12.5, 57.0]),
        Thigh => both(part::LEGS, [0.5, 52.5, 11.5, 67.5]),
        Knee => both(part::LEGS, [2.5, 65.5, 10.5, 75.5]),
        LowerLeg => both(part::LEGS, [3.0, 73.5, 10.0, 91.6]),
        Ankle => both(part::LEGS | part::FEET, [3.5, 88.5, 10.5, 94.5]),
        Foot => both(part::FEET, [3.0, 92.4, 11.5, 98.4]),
        WholeLeg => both(part::LEGS | part::FEET, [0.5, 50.0, 11.5, 98.4]),
        WholeBody => center(part::BODY, BODY_HALF_WIDTH, 0.0, BODY_HEIGHT),
    }
}

fn band_rect(rect: Rect, [x0, y0, x1, y1]: [f32; 4]) -> Rect {
    Rect::from_min_max(body_point(rect, [x0, y0]), body_point(rect, [x1, y1]))
}

/// The corners of the image on screen in a front view of the body: the
/// scanned block is fitted into `band` with its true proportions; the
/// patient's right is on the left and the head end at the top.
fn front_view(band: Rect, placement: &Placement) -> [Pos2; 4] {
    let (width, height) = (placement.size_mm[0].max(0.0) as f32, placement.size_mm[2].max(0.0) as f32);
    let scale = [(width, band.width()), (height, band.height())]
        .into_iter()
        .filter(|(mm, _)| *mm > 0.0)
        .map(|(mm, points)| points / mm)
        .fold(f32::INFINITY, f32::min);
    let size = if scale.is_finite() { vec2(width, height) * scale } else { Vec2::ZERO };
    let fit = Rect::from_center_size(band.center(), size);
    placement.corners.map(|c| pos2(fit.left() + c[0] as f32 * fit.width(), fit.bottom() - c[2] as f32 * fit.height()))
}

/// Draws the image on screen: a line when it is seen edge-on, a band
/// otherwise.
fn paint_plane(painter: &Painter, points: &[Pos2; 4], color: Color32) {
    let twice_area: f32 =
        (0..4).map(|i| points[i].x * points[(i + 1) % 4].y - points[(i + 1) % 4].x * points[i].y).sum();
    if twice_area.abs() >= 8.0 {
        painter.add(Shape::convex_polygon(points.to_vec(), color.gamma_multiply(0.35), Stroke::new(1.5, color)));
        return;
    }
    let (mut a, mut b) = (points[0], points[0]);
    for (i, p) in points.iter().enumerate() {
        for q in &points[i + 1..] {
            if p.distance_sq(*q) > a.distance_sq(b) {
                (a, b) = (*p, *q);
            }
        }
    }
    // A little longer than the block is wide, so that the line stands out from the outline.
    let along = if a == b { Vec2::X } else { (b - a).normalized() };
    painter.line_segment([a - along * 3.0, b + along * 3.0], Stroke::new(2.0, color));
}

fn paint_body(painter: &Painter, rect: Rect, p: &Palette, region: BodyRegion, placement: Option<&Placement>) {
    let colors = FigureColors::of(p);
    paint_parts(painter, rect, part::BODY, colors.outline);
    let area = area(region);
    for band in &area.bands {
        paint_parts(&painter.with_clip_rect(band_rect(rect, *band)), rect, area.parts, colors.region);
    }
    // A front view: the patient's right is on the left, as in the images.
    let font = FontId::proportional(10.5);
    painter.text(rect.left_top(), Align2::LEFT_TOP, orientation_label(-DVec3::X), font.clone(), colors.letters);
    painter.text(rect.right_top(), Align2::RIGHT_TOP, orientation_label(DVec3::X), font, colors.letters);
    if let Some(placement) = placement {
        for band in &area.bands {
            paint_plane(painter, &front_view(band_rect(rect, *band), placement), colors.plane);
        }
    }
}

/// How far a step towards the back of the block moves a point to the right
/// and upwards in the oblique projection of the block figure, relative to a
/// step of the same length across or up (half length at 30°).
const DEPTH: Vec2 = vec2(0.433, -0.25);

/// The oblique projection of the scanned block into `rect`: maps fractions
/// of the block (x from the patient's right to left, y from front to back, z
/// from feet to head) to the screen. The front of the block faces the
/// viewer, the patient's right is on the left and the head end at the top.
fn block_projection(rect: Rect, size_mm: [f64; 3]) -> impl Fn([f32; 3]) -> Pos2 {
    // The proportions of the block; no side thinner than 15 % of the longest,
    // so that a single slice still reads as a box.
    let longest = size_mm.iter().fold(0.0f64, |a, b| a.max(*b)).max(1e-6);
    let side = size_mm.map(|mm| ((mm / longest) as f32).clamp(0.15, 1.0));
    let extent = vec2(side[0] + side[1] * DEPTH.x, side[2] - side[1] * DEPTH.y);
    let inner = rect.shrink2(vec2(18.0, 16.0));
    let scale = (inner.width() / extent.x).min(inner.height() / extent.y);
    let origin = inner.center() + vec2(-extent.x, extent.y) * (scale / 2.0);
    move |[x, y, z]: [f32; 3]| origin + (vec2(x * side[0], -z * side[2]) + DEPTH * (y * side[1])) * scale
}

fn paint_block(painter: &Painter, rect: Rect, p: &Palette, placement: &Placement) {
    let colors = FigureColors::of(p);
    let project = block_projection(rect, placement.size_mm);
    let corner = |index: usize| project([(index & 1) as f32, ((index >> 1) & 1) as f32, ((index >> 2) & 1) as f32]);
    // The corner at the back, bottom and patient's right is hidden by the block.
    const HIDDEN: usize = 0b010;
    let edges = (0..8usize)
        .flat_map(|a| [1usize, 2, 4].into_iter().filter(move |bit| a & bit == 0).map(move |bit| (a, a | bit)));
    let (hidden, visible): (Vec<_>, Vec<_>) = edges.partition(|(a, b)| *a == HIDDEN || *b == HIDDEN);
    for (a, b) in hidden {
        painter.line_segment([corner(a), corner(b)], Stroke::new(1.0, colors.hidden));
    }
    paint_plane(painter, &placement.corners.map(|c| project(c.map(|v| v as f32))), colors.plane);
    for (a, b) in visible {
        painter.line_segment([corner(a), corner(b)], Stroke::new(1.2, colors.outline.gamma_multiply(2.0)));
    }
    // The letters the viewports show at the edges of the image.
    let font = FontId::proportional(11.0);
    let letter = |at: [f32; 3], offset: Vec2, anchor: Align2, direction: DVec3| {
        painter.text(project(at) + offset, anchor, orientation_label(direction), font.clone(), colors.letters);
    };
    letter([0.0, 0.0, 0.5], vec2(-5.0, 0.0), Align2::RIGHT_CENTER, -DVec3::X);
    letter([1.0, 1.0, 0.5], vec2(5.0, 0.0), Align2::LEFT_CENTER, DVec3::X);
    letter([0.5, 1.0, 1.0], vec2(0.0, -3.0), Align2::CENTER_BOTTOM, DVec3::Z);
    letter([0.5, 0.0, 0.0], vec2(0.0, 3.0), Align2::CENTER_TOP, -DVec3::Z);
    letter([0.0, 0.0, 0.0], vec2(-4.0, 3.0), Align2::RIGHT_TOP, -DVec3::Y);
    letter([1.0, 1.0, 1.0], vec2(4.0, -2.0), Align2::LEFT_BOTTOM, DVec3::Y);
}

// ---------------------------------------------------------------------------
// Explanations of inspector rows and presets

/// A small mark after a label. Its explanation shows while the pointer is
/// over it and while it has keyboard focus (egui's own tooltips react to the
/// pointer only); a click gives it the focus.
pub(crate) fn info_mark(ui: &mut Ui, subject: &str, explanation: &str) -> Response {
    let p = theme::palette(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::click());
    let name = tr!("About {subject}", subject = subject);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &name));
    // Read out with the name by screen readers.
    ui.ctx().accesskit_node_builder(response.id, |node| node.set_description(explanation));
    if response.clicked() {
        response.request_focus();
    }
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
    // Nothing is drawn, and no explanation shown, for a mark scrolled out of view.
    if ui.is_rect_visible(rect) {
        let active = response.hovered() || response.has_focus();
        let color = if active { p.accent } else { p.text_weak };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon(ICON_INFO), FontId::proportional(13.5), color);
        if response.has_focus() {
            ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0, p.accent), StrokeKind::Outside);
        }
        if active {
            Tooltip::for_widget(&response).show(|ui| {
                ui.set_max_width(EXPLANATION_WIDTH);
                ui.add(Label::new(explanation).wrap());
            });
        }
    }
    response
}

/// The key/value grid of an inspector section. With `explain`, rows that
/// have an explanation carry an info mark after their label.
pub(crate) fn summary_grid(ui: &mut Ui, id: impl egui::AsIdSalt, rows: &[SummaryRow], explain: bool) {
    let p = theme::palette(ui);
    egui::Grid::new(id).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        for row in rows {
            let label = RichText::new(&row.label).color(p.text_weak);
            match row.key.filter(|_| explain).and_then(guide::explain_parameter) {
                Some(explanation) => {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.label(label);
                        info_mark(ui, &row.label, explanation);
                    });
                }
                None => {
                    ui.label(label);
                }
            }
            ui.add(Label::new(&row.value).wrap().selectable(true));
            ui.end_row();
        }
    });
}

/// The tooltip of an entry of the contrast preset menu.
pub(crate) trait PresetHint {
    /// Shows `hint` (where the window comes from) and, with `explain`, what
    /// the window is for and which values it shows.
    fn preset_hint(self, hint: &str, explain: bool, preset: &WindowPreset, units: Option<&str>) -> Self;
}

impl PresetHint for Response {
    fn preset_hint(self, hint: &str, explain: bool, preset: &WindowPreset, units: Option<&str>) -> Self {
        if !explain {
            return self.on_hover_text(hint);
        }
        // As `on_hover_text` does: a screen reader reads the hint after the name of the entry.
        self.ctx.accesskit_node_builder(self.id, |node| node.set_description(hint));
        self.on_hover_ui(|ui| {
            let p = theme::palette(ui);
            let explanation = guide::explain_preset(preset, units);
            ui.set_max_width(EXPLANATION_WIDTH);
            ui.add(Label::new(RichText::new(hint).small().color(p.text_weak)).wrap());
            ui.add(Label::new(explanation.purpose).wrap());
            ui.add(Label::new(RichText::new(explanation.range).small()).wrap());
        })
    }
}

/// The guide in the viewer, driven offscreen on the synthetic CD. Set
/// `SUMMRISE_SNAPSHOT_DIR` to keep renderings (`guide-*.png`).
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use egui::{Key, Modifiers, Theme};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{Queryable, by};
    use summrise_core::dicom::{Progress, scan};
    use summrise_core::{SeriesKey, Waker};
    use summrise_testkit::{PATIENT_ID, PhantomOptions, write_phantom_study};

    use super::*;
    use crate::app::{Action, AppOptions, SumMRIseApp, Toasts};
    use crate::viewer::Viewer;

    const CT: SeriesKey = SeriesKey { study: 0, series: 0 };
    const MR: SeriesKey = SeriesKey { study: 0, series: 1 };
    const LOCALIZER: SeriesKey = SeriesKey { study: 0, series: 2 };
    const DOSE_REPORT: SeriesKey = SeriesKey { study: 0, series: 3 };

    const BODY_FIGURE_LABEL: &str =
        "Schematic body outline: the examined region is highlighted and the image on screen is marked.";
    const BLOCK_FIGURE_LABEL: &str = "Schematic of the scanned block with the image on screen inside it.";
    const SETTING: &str = "Show explanations of series, values and the viewing position";

    /// Everything `Viewer::show` needs.
    struct App {
        viewer: Viewer,
        settings: Settings,
        waker: Waker,
        actions: Vec<Action>,
        toasts: Toasts,
    }

    /// The viewer on the synthetic CD (the CT is selected).
    struct Fixture {
        harness: Harness<'static, App>,
        _dir: tempfile::TempDir,
    }

    fn snapshot_dir() -> Option<PathBuf> {
        std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from)
    }

    /// Keeps a rendering of what the harness shows, when snapshots are asked for.
    fn save<State>(harness: &mut Harness<'_, State>, name: &str) {
        if let Some(dir) = snapshot_dir() {
            let image = harness.render().expect("wgpu render");
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            image.save(dir.join(format!("{name}.png"))).expect("save png");
        }
    }

    fn phantom() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let options =
            PhantomOptions { ct_size: 96, ct_slices: 40, mr_size: 64, mr_slices: 16, ..PhantomOptions::default() };
        write_phantom_study(dir.path(), &options).expect("fixture");
        dir
    }

    fn open(settings: Settings) -> Fixture {
        let dir = phantom();
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        let session = Session::new(index, settings.memory_budget_bytes(), None);
        let app = App {
            viewer: Viewer::new(session, &settings),
            settings,
            waker: Arc::new(|| {}),
            actions: Vec::new(),
            toasts: Toasts::default(),
        };
        let mut builder = Harness::builder().with_size([1280.0, 900.0]).with_context_setup(theme::install);
        if snapshot_dir().is_some() {
            builder = builder.wgpu();
        }
        let harness = builder.build_ui_state(
            |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
            app,
        );
        let mut fixture = Fixture { harness, _dir: dir };
        fixture.wait_for(CT);
        fixture
    }

    impl Fixture {
        /// Runs UI frames until `done`.
        fn run_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
            let start = Instant::now();
            loop {
                self.harness.step();
                if done(self) {
                    break;
                }
                assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
                std::thread::sleep(Duration::from_millis(5));
            }
            self.harness.run_steps(3);
        }

        /// Runs UI frames until `series` is loaded and the metadata of its image is shown.
        fn wait_for(&mut self, series: SeriesKey) {
            self.run_until("the series", |f| f.harness.state().viewer.session.loaded(series).is_some());
            self.run_until("the metadata", |f| f.shows("Identifiers"));
        }

        fn select(&mut self, series: SeriesKey) {
            self.harness.state_mut().viewer.session.select_series(series);
            self.wait_for(series);
        }

        /// Whether something on screen is labelled exactly `label`.
        fn shows(&self, label: &str) -> bool {
            self.harness.query_all_by_label(label).next().is_some()
        }

        /// How many things on screen have `text` in their label.
        fn count_containing(&self, text: &str) -> usize {
            self.harness.query_all_by_label_contains(text).count()
        }

        fn snapshot(&mut self, name: &str) {
            save(&mut self.harness, name);
        }
    }

    /// Tasks 5.1, 5.2, 5.4 — the browser line, the card and the figure for the chest CT.
    #[test]
    fn chest_ct_is_described_in_the_browser_the_card_and_the_figure() {
        let mut f = open(Settings::default());
        // One short line under each series title.
        assert_eq!(f.harness.query_all_by_label("CT scan").count(), 2, "browser line and card headline");
        for line in ["MR scan, T2-weighted", "Localizer (planning overview)", "Dose report (a stored picture)"] {
            assert!(f.shows(line), "{line}");
        }
        // The card: headline, statements with their basis, the fixed line.
        assert!(f.shows("About this series"));
        assert!(
            f.shows("The slices are axial: horizontal cross-sections of the body, like the slices of a loaf of bread.")
        );
        assert!(f.shows("Recorded in the images: image orientation."));
        assert!(f.shows("Each image shows a slice of the body 5 mm thick."));
        assert!(f.shows("A contrast agent is recorded for this series: Iodinated contrast (synthetic)."));
        assert!(f.shows(guide::disclaimer()) && f.shows(guide::sampling_note()));
        // The figure: the body outline for Body Part Examined CHEST, and the viewing direction.
        assert!(f.shows("Where is this image?"));
        assert!(f.shows(BODY_FIGURE_LABEL) && !f.shows(BLOCK_FIGURE_LABEL));
        assert!(f.shows("Axial image"));
        assert!(f.shows("You are looking from the feet towards the head."));
        assert!(f.shows("The patient's right side is on the left of the screen."));
        assert_eq!(f.count_containing("Schematic, not to scale."), 1);
        assert_eq!(f.count_containing("(“CHEST”)"), 1);
        f.harness.check_accessibility();
        f.snapshot("guide-1-chest-ct");

        // The light theme.
        f.harness.ctx.set_theme(Theme::Light);
        f.harness.run_steps(3);
        f.snapshot("guide-1-chest-ct-light");
    }

    /// Task 5.4 — the sentences and the figure describe the view that is active.
    #[test]
    fn figure_follows_the_active_view() {
        let mut f = open(Settings::default());
        f.harness.get_by_label("MPR").click();
        f.harness.run_steps(4);
        assert!(f.shows("You are looking from the feet towards the head."), "the axial reformat is active first");

        // The view itself (its tilt handles are labelled with plane names too).
        f.harness.get(by().role(egui::accesskit::Role::Image).label_contains("Coronal ")).click();
        f.harness.run_steps(4);
        assert!(f.shows("Coronal image"));
        assert!(f.shows("You are looking from the front of the body towards the back."));
        assert!(f.shows("The patient's right side is on the left of the screen."));
        f.snapshot("guide-2-coronal-reformat");

        // The view itself (its tilt handles are labelled with plane names too).
        f.harness.get(by().role(egui::accesskit::Role::Image).label_contains("Sagittal ")).click();
        f.harness.run_steps(4);
        assert!(f.shows("Sagittal image"));
        assert!(f.shows("You are looking from the patient's left side towards the right."));
        assert!(f.shows("The front of the body is on the left of the screen."));
        f.snapshot("guide-2-sagittal-reformat");
    }

    /// The sentences describe the picture as it is on screen: turned and
    /// mirrored like the view, not as stored.
    #[test]
    fn sentences_follow_the_turn_and_mirror_of_the_view() {
        let mut f = open(Settings::default());
        assert!(f.shows("The patient's right side is on the left of the screen."));
        // Mirrored left to right (H): seen from the head, the patient's left side on the left.
        f.harness.key_press(Key::H);
        f.harness.run_steps(4);
        assert!(f.shows("You are looking from the head towards the feet."));
        assert!(f.shows("The patient's left side is on the left of the screen."));
        assert!(!f.shows("The patient's right side is on the left of the screen."));
        // Mirrored back and turned a quarter clockwise (]): the back of the body on the left.
        f.harness.key_press(Key::H);
        f.harness.key_press(Key::CloseBracket);
        f.harness.run_steps(4);
        assert!(f.shows("You are looking from the feet towards the head."));
        assert!(f.shows("The back of the body is on the left of the screen."));
        f.snapshot("guide-2-turned");
    }

    /// Task 5.3 — a row with an explanation carries a mark; pointer and keyboard show the text.
    #[test]
    fn parameter_explanations_show_on_hover_and_on_keyboard_focus() {
        let mut f = open(Settings::default());
        let voltage = guide::explain_parameter("KVP").expect("explanation");
        let kernel = guide::explain_parameter("ConvolutionKernel").expect("explanation");
        assert!(!f.shows(voltage) && !f.shows(kernel));

        // Pointer.
        f.harness.get_by_label("About Tube voltage").scroll_to_me();
        f.harness.run_steps(3);
        f.harness.get_by_label("About Tube voltage").hover();
        f.harness.run_steps(3);
        assert!(f.shows(voltage), "explanation while the pointer is over the mark");
        f.snapshot("guide-3-explanation-tooltip");

        // Keyboard: focus shows the explanation, Tab moves on to the next mark and hides it.
        f.harness.remove_cursor();
        f.harness.get_by_label("About Kernel").focus();
        f.harness.run_steps(3);
        assert!(f.harness.get_by_label("About Kernel").is_focused());
        assert!(f.shows(kernel) && !f.shows(voltage), "explanation while the mark has keyboard focus");
        f.snapshot("guide-3-explanation-focus");
        f.harness.key_press(Key::Tab);
        f.harness.run_steps(3);
        assert!(!f.shows(kernel), "the explanation leaves with the focus");
        assert!(!f.harness.get_by_label("About Kernel").is_focused());

        // Rows about the patient carry no mark; rows of the acquisition do.
        for label in ["About Name", "About Patient ID", "About Birth date", "About Age", "About Sex"] {
            assert!(!f.shows(label), "{label}");
        }
        for label in ["About Tube current", "About Slice thickness", "About Orientation", "About Rescale"] {
            assert!(f.shows(label), "{label}");
        }
        f.harness.check_accessibility();
    }

    /// Task 5.5 — each contrast preset says what the window is for.
    #[test]
    fn preset_menu_says_what_each_window_is_for() {
        let mut f = open(Settings::default());
        f.harness.get_by_role_and_label(Role::ComboBox, "Window").click();
        f.harness.run_steps(3);
        // The eighth entry is the built-in lung window, W 1500 / L -600 HU.
        f.harness.get_by_label("Lung   8").scroll_to_me();
        f.harness.run_steps(3);
        f.harness.get_by_label("Lung   8").hover();
        f.harness.run_steps(12);
        assert!(f.shows("Standard CT window"), "the existing hint stays");
        assert_eq!(f.count_containing("the air-filled lungs"), 1);
        assert_eq!(f.count_containing("densities from -1350 to 150 HU"), 1);
        f.snapshot("guide-4-preset-tooltip");
    }

    /// Task 5.4 — without a recognisable body part the figure is the scanned block;
    /// without geometry there is no figure.
    #[test]
    fn unknown_body_part_shows_the_block_and_pictures_show_no_figure() {
        let mut f = open(Settings::default());
        // The localizer records no body part.
        f.select(LOCALIZER);
        assert_eq!(f.harness.query_all_by_label("Localizer (planning overview)").count(), 2);
        assert!(f.shows("The images of this series lie in different directions."));
        assert!(f.shows(BLOCK_FIGURE_LABEL) && !f.shows(BODY_FIGURE_LABEL));
        assert_eq!(f.count_containing("The box is the scanned block"), 1);
        assert_eq!(f.count_containing("R is the patient's right, L left, A front, P back, S head and I feet"), 1);
        // The middle image of the three is the coronal one.
        assert!(f.shows("Coronal image"));
        assert!(f.shows("You are looking from the front of the body towards the back."));
        f.snapshot("guide-5-block-figure");

        // The stored picture has no position in the body.
        f.select(DOSE_REPORT);
        assert_eq!(f.harness.query_all_by_label("Dose report (a stored picture)").count(), 2);
        assert!(!f.shows(BLOCK_FIGURE_LABEL) && !f.shows(BODY_FIGURE_LABEL));
        assert!(f.shows("These images carry no position information, so the figure cannot show where the image lies."));
        assert!(f.shows(guide::disclaimer()) && !f.shows(guide::sampling_note()));
        f.snapshot("guide-6-stored-picture");

        // The sagittal MR.
        f.select(MR);
        assert_eq!(f.harness.query_all_by_label("MR scan, T2-weighted").count(), 2);
        assert!(f.shows("T2-weighted: on such images fluid tends to appear bright."));
        assert!(f.shows("Named by the scanner operator: the series description reads “T2 TSE SAG (synthetic)”."));
        assert!(f.shows("You are looking from the patient's left side towards the right."));
        assert!(f.shows("The front of the body is on the left of the screen."));
        f.harness.check_accessibility();
        f.snapshot("guide-7-sagittal-mr");
    }

    /// Task 6.2 — "Show explanations" off hides all of it.
    #[test]
    fn switched_off_nothing_of_the_guide_is_shown() {
        let mut f = open(Settings { show_explanations: false, ..Settings::default() });
        let absent = |f: &Fixture| {
            let labels =
                ["About this series", "Where is this image?", "CT scan", "MR scan, T2-weighted", "Axial image"];
            labels.iter().all(|label| !f.shows(label))
                && !f.shows(BODY_FIGURE_LABEL)
                && !f.shows(guide::disclaimer())
                && f.count_containing("About ") == 0
        };
        assert!(absent(&f));
        // The inspector itself is as before.
        assert!(f.shows("Tube voltage") && f.shows("120 kV") && f.shows("DISPLAY"));
        // The preset menu keeps its short hints, without the explanations.
        f.harness.get_by_role_and_label(Role::ComboBox, "Window").click();
        f.harness.run_steps(3);
        f.harness.get_by_label("Lung   8").scroll_to_me();
        f.harness.run_steps(3);
        f.harness.get_by_label("Lung   8").hover();
        f.harness.run_steps(12);
        assert!(f.shows("Standard CT window"));
        assert_eq!(f.count_containing("the air-filled lungs"), 0);
        f.harness.key_press(Key::Escape);
        f.harness.remove_cursor();
        f.harness.run_steps(3);
        f.harness.check_accessibility();
        f.snapshot("guide-8-switched-off");

        // Switched on again, everything is back.
        f.harness.state_mut().settings.show_explanations = true;
        f.harness.run_steps(3);
        assert!(!absent(&f));
        assert!(f.shows("About this series") && f.shows(BODY_FIGURE_LABEL) && f.shows("About Tube voltage"));
    }

    /// Task 6.1 — the setting is a checkbox in the settings window, on by default.
    #[test]
    fn setting_is_switched_in_the_settings_window() {
        let dir = phantom();
        let path = dir.path().to_path_buf();
        let mut builder = Harness::builder().with_size([1280.0, 900.0]);
        if snapshot_dir().is_some() {
            builder = builder.wgpu();
        }
        let mut harness = builder
            .build_eframe(move |cc| SumMRIseApp::new(cc, AppOptions { open: Some(path), ..AppOptions::default() }));
        let shows =
            |harness: &Harness<'_, SumMRIseApp>, label: &str| harness.query_all_by_label(label).next().is_some();
        let start = Instant::now();
        while !(shows(&harness, "About this series") && shows(&harness, "Identifiers")) {
            assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for the viewer");
            harness.step();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(shows(&harness, "CT scan"), "on by default");

        harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
        harness.run_steps(4);
        assert!(shows(&harness, "Explanations"));
        // Every control of the settings window has an accessible name.
        harness.check_accessibility();
        save(&mut harness, "guide-9-settings-window");
        harness.get_by_label(SETTING).click();
        harness.run_steps(4);
        assert!(!shows(&harness, "About this series") && !shows(&harness, "CT scan"));
        save(&mut harness, "guide-9-settings-off");
        // The keyboard switches it back: the checkbox takes the focus and Space toggles it.
        harness.get_by_label(SETTING).focus();
        harness.run_steps(2);
        harness.key_press(Key::Space);
        harness.run_steps(4);
        assert!(shows(&harness, "About this series") && shows(&harness, "CT scan"));
        // The new checkbox has a name (the memory slider of the window had none before this change).
        assert!(harness.unnamed_widgets().iter().all(|widget| !widget.starts_with("CheckBox")));
    }

    /// The guide reads no patient attributes: it is the same with privacy mode on.
    #[test]
    fn privacy_mode_does_not_change_the_guide() {
        let texts = |f: &Fixture| -> Vec<String> {
            let viewer = &f.harness.state().viewer;
            let series = viewer.session.series(CT).expect("CT");
            let described = guide::describe_series(series);
            let mut texts: Vec<String> = described.texts().map(str::to_owned).collect();
            texts.extend(guide::series_line(series).map(str::to_owned));
            texts
        };
        let open_mode = open(Settings::default());
        let mut private = open(Settings { hide_patient_details: true, ..Settings::default() });
        assert_eq!(texts(&open_mode), texts(&private));
        for text in texts(&private) {
            assert!(private.shows(&text), "shown in privacy mode: {text}");
            for identifier in ["PHANTOM", "SYNTHETIC", PATIENT_ID, "1970"] {
                assert!(!text.contains(identifier), "{identifier} in {text}");
            }
        }
        assert!(private.shows(BODY_FIGURE_LABEL));
        assert!(!private.shows("SYNTHETIC PHANTOM") && !private.shows(PATIENT_ID), "privacy mode itself still works");
        assert!(open_mode.shows("SYNTHETIC PHANTOM"));
        private.snapshot("guide-10-privacy-mode");
    }

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
    }

    fn close(a: Pos2, b: Pos2) -> bool {
        (a - b).length() < 1e-3
    }

    /// The front view of the body: head end up, the patient's right on the left,
    /// the block fitted with its proportions.
    #[test]
    fn front_view_places_the_image_truthfully() {
        // A block 200 mm wide and 100 mm tall in a 100 x 100 band: fitted to 100 x 50, centred.
        let band = rect(100.0, 200.0, 200.0, 300.0);
        let block = [200.0, 150.0, 100.0];
        let axial = |z: f64| Placement {
            corners: [[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]],
            size_mm: block,
        };
        // An axial image at the head end is a line along the top of the block …
        let points = front_view(band, &axial(1.0));
        assert!(close(points[0], pos2(100.0, 225.0)) && close(points[1], pos2(200.0, 225.0)), "{points:?}");
        assert!(points.iter().all(|p| p.y == 225.0), "edge-on: {points:?}");
        // … one three quarters of the way up from the feet end lies a quarter down.
        assert!(front_view(band, &axial(0.75)).iter().all(|p| (p.y - 237.5).abs() < 1e-3));
        assert!(front_view(band, &axial(0.0)).iter().all(|p| p.y == 275.0), "the feet end is at the bottom");
        // A sagittal image at the patient's left end is a line at the right of the figure.
        let sagittal =
            Placement { corners: [[1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]], size_mm: block };
        let points = front_view(band, &sagittal);
        assert!(points.iter().all(|p| p.x == 200.0), "{points:?}");
        assert!(close(points[0], pos2(200.0, 225.0)) && close(points[2], pos2(200.0, 275.0)), "{points:?}");
        // A coronal image is seen face-on: the whole front of the block.
        let coronal =
            Placement { corners: [[0.0, 0.5, 1.0], [1.0, 0.5, 1.0], [1.0, 0.5, 0.0], [0.0, 0.5, 0.0]], size_mm: block };
        let points = front_view(band, &coronal);
        assert!(close(points[0], pos2(100.0, 225.0)) && close(points[2], pos2(200.0, 275.0)), "{points:?}");
        // A single axial image has no height: a line through the middle of the band.
        let single = Placement { size_mm: [200.0, 150.0, 0.0], ..axial(0.5) };
        let points = front_view(band, &single);
        assert!(close(points[0], pos2(100.0, 250.0)) && close(points[1], pos2(200.0, 250.0)), "{points:?}");
        // A thin sagittal slab keeps its proportions: it stays narrow instead of filling the band.
        let slab = Placement { size_mm: [50.0, 300.0, 200.0], ..sagittal };
        let points = front_view(band, &slab);
        assert!(points.iter().all(|p| (p.x - 162.5).abs() < 1e-3), "25 points wide, centred: {points:?}");
    }

    /// The block figure: front face towards the viewer, the patient's right on the
    /// left, head up, back receding to the upper right.
    #[test]
    fn block_projection_keeps_the_radiological_view() {
        let frame = rect(0.0, 0.0, 176.0, 136.0);
        let project = block_projection(frame, [240.0, 240.0, 200.0]);
        let (origin, left, back, head) =
            (project([0.0; 3]), project([1.0, 0.0, 0.0]), project([0.0, 1.0, 0.0]), project([0.0, 0.0, 1.0]));
        assert!(left.x > origin.x && left.y == origin.y, "the patient's left is to the right");
        assert!(head.y < origin.y && head.x == origin.x, "the head end is up");
        assert!(back.x > origin.x && back.y < origin.y, "the back recedes to the upper right");
        // True proportions: 240 mm across and 200 mm up.
        let ratio = (left.x - origin.x) / (origin.y - head.y);
        assert!((ratio - 1.2).abs() < 1e-3, "{ratio}");
        // Everything stays inside the figure, with room for the letters.
        for index in 0..8 {
            let p = project([(index & 1) as f32, ((index >> 1) & 1) as f32, ((index >> 2) & 1) as f32]);
            assert!(frame.shrink(12.0).contains(p), "{p:?}");
        }
        // A single slice still reads as a box.
        let project = block_projection(frame, [240.0, 240.0, 0.0]);
        assert!(project([0.0; 3]).y - project([0.0, 0.0, 1.0]).y > 8.0);
    }

    /// Every body region has a place in the outline, inside the figure.
    #[test]
    fn every_region_lies_inside_the_outline() {
        use BodyRegion::*;
        let regions = [
            Head,
            Neck,
            HeadNeck,
            Chest,
            Abdomen,
            Pelvis,
            NeckChest,
            ChestAbdomen,
            AbdomenPelvis,
            ChestAbdomenPelvis,
            NeckChestAbdomen,
            NeckChestAbdomenPelvis,
            CervicalSpine,
            ThoracicSpine,
            LumbarSpine,
            SacralSpine,
            CervicothoracicSpine,
            ThoracolumbarSpine,
            WholeSpine,
            Shoulder,
            UpperArm,
            Elbow,
            Forearm,
            Wrist,
            Hand,
            WholeArm,
            Hip,
            Thigh,
            Knee,
            LowerLeg,
            Ankle,
            Foot,
            WholeLeg,
            WholeBody,
        ];
        for region in regions {
            let area = area(region);
            assert!(area.parts != 0 && !area.bands.is_empty(), "{region:?}");
            for [x0, y0, x1, y1] in &area.bands {
                assert!(x0 < x1 && y0 < y1, "{region:?}");
                assert!(
                    *x0 >= -BODY_HALF_WIDTH && *x1 <= BODY_HALF_WIDTH && *y0 >= 0.0 && *y1 <= BODY_HEIGHT,
                    "{region:?}"
                );
            }
            // Paired regions are mirror images: neither side is preferred.
            if let [a, b] = area.bands.as_slice() {
                assert_eq!([a[0], a[1], a[2], a[3]], [-b[2], b[1], -b[0], b[3]], "{region:?}");
            }
        }
        // Head above chest above pelvis above knees above feet; the patient's right is at negative x.
        let top = |region| area(region).bands[0][1];
        assert!(top(Head) < top(Chest) && top(Chest) < top(Pelvis) && top(Pelvis) < top(Knee) && top(Knee) < top(Foot));
    }

    /// Renders the figure for several regions and planes, in both themes.
    #[test]
    fn figures_render_in_both_themes() {
        let (x, y, z) = (DVec3::X, DVec3::Y, DVec3::Z);
        let block = Block { min: DVec3::new(-150.0, -100.0, -120.0), max: DVec3::new(150.0, 100.0, 120.0) };
        let image = |origin: DVec3, right: DVec3, down: DVec3, width: f64, height: f64| ScreenPlane {
            right,
            down,
            corners: [origin, origin + right * width, origin + right * width + down * height, origin + down * height],
        };
        let axial = image(DVec3::new(-150.0, -100.0, 60.0), x, y, 300.0, 200.0);
        let coronal = image(DVec3::new(-150.0, 20.0, 120.0), x, -z, 300.0, 240.0);
        let sagittal = image(DVec3::new(75.0, -100.0, 120.0), y, -z, 200.0, 240.0);
        // Halfway between axial and coronal.
        let tilt = DVec3::new(0.0, 1.0, 1.0).normalize();
        let oblique = image(DVec3::new(-150.0, -80.0, -40.0), x, tilt, 300.0, 200.0);
        let cases: Vec<(Option<&str>, ScreenPlane)> = vec![
            (Some("CHEST"), axial),
            (Some("CHEST"), coronal),
            (Some("LSPINE"), sagittal),
            (Some("KNEE"), axial),
            (Some("HEAD"), oblique),
            (None, axial),
            (None, coronal),
            (None, sagittal),
            (None, oblique),
        ];
        let located = |body_part: Option<&str>| Located {
            block: Some(block),
            body_part: body_part.map(str::to_owned),
            region: body_part.and_then(guide::body_region),
        };
        for theme in [Theme::Dark, Theme::Light] {
            let cases = cases.clone();
            let mut builder = Harness::builder().with_size([1500.0, 640.0]).with_context_setup(theme::install);
            if snapshot_dir().is_some() {
                builder = builder.wgpu();
            }
            let mut harness = builder.build_ui(move |ui| {
                ui.ctx().set_theme(theme);
                egui::CentralPanel::default_margins().show(ui, |ui| {
                    egui::Grid::new("figures").num_columns(5).spacing([16.0, 16.0]).show(ui, |ui| {
                        for (index, (body_part, plane)) in cases.iter().enumerate() {
                            ui.vertical(|ui| {
                                ui.set_width(280.0);
                                where_figure(ui, &located(*body_part), Some(plane), None);
                            });
                            if index == 4 {
                                ui.end_row();
                            }
                        }
                    });
                });
            });
            harness.run_steps(3);
            assert_eq!(harness.query_all_by_label(BODY_FIGURE_LABEL).count(), 5);
            assert_eq!(harness.query_all_by_label(BLOCK_FIGURE_LABEL).count(), 4);
            assert_eq!(harness.query_all_by_label("Tilted (oblique) image").count(), 2);
            assert_eq!(harness.query_all_by_label_contains("Both sides are highlighted").count(), 1, "the knees");
            save(&mut harness, if theme == Theme::Dark { "guide-figures-dark" } else { "guide-figures-light" });
        }
    }
}
