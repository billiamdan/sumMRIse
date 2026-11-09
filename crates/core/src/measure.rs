//! Length, angle and region measurements: what a measurement is, which picture
//! it belongs to, how it is drawn, adjusted and removed, and what it reads.
//!
//! A measurement keeps its points in patient coordinates when its picture has
//! a position in the patient, and in pixels otherwise (D1). [`Measurements`]
//! is part of the viewing state of a series, so it lives as long as the study
//! is open and is never written anywhere (D7). The maths is in
//! `summrise_imaging::measure`; the pointer and the drawing are in `ui`, which
//! speaks to this module in pixels of the picture (fractional, with pixel
//! centres at whole numbers), whatever its zoom and display orientation.

use serde::{Deserialize, Serialize};
use summrise_dicom::FrameRef;
use summrise_dicom::format::number;
use summrise_i18n::{tr, tr_ctx, tr_n};
use summrise_imaging::fusion::PixelGrid;
use summrise_imaging::measure::{Region, RegionShape, RegionStatistics, angle_degrees, distance, region_statistics};
use summrise_imaging::{
    Calibration, DVec3, ImagePlane, MprExtent, MprImage, Padding, PlaneBasis, SlabMode, VolumeGeometry, VoxelBuffer,
};

use crate::pet::value_text;
use crate::series::{ImageSlot, SeriesData};

/// The measuring tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MeasureKind {
    /// The distance between two points.
    Length,
    /// The angle at a vertex between the lines to two other points.
    Angle,
    /// An ellipse: its area and the statistics of the values inside.
    Ellipse,
    /// A rectangle: its area and the statistics of the values inside.
    Rectangle,
}

impl MeasureKind {
    pub const ALL: [Self; 4] = [Self::Length, Self::Angle, Self::Ellipse, Self::Rectangle];

    pub fn label(self) -> &'static str {
        match self {
            // Translators: the measuring tools: a distance, an angle and two shapes of a region.
            Self::Length => tr_ctx!("measuring tool", "Length"),
            Self::Angle => tr_ctx!("measuring tool", "Angle"),
            Self::Ellipse => tr_ctx!("measuring tool", "Ellipse"),
            Self::Rectangle => tr_ctx!("measuring tool", "Rectangle"),
        }
    }

    fn shape(self) -> Option<RegionShape> {
        match self {
            Self::Length | Self::Angle => None,
            Self::Ellipse => Some(RegionShape::Ellipse),
            Self::Rectangle => Some(RegionShape::Rectangle),
        }
    }
}

/// The picture a measurement belongs to (D2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Home {
    /// An acquired image: its index among all images of the series
    /// (`ViewState::slot_index`), whichever image set it is in.
    Image(usize),
    /// A plane of a reformat of one image set.
    Plane(PlaneHome),
}

/// A reformat plane: its orientation, its position along its normal, the step
/// between its slices (all in patient space) and the image set it is cut from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneHome {
    right: DVec3,
    down: DVec3,
    position: f64,
    step: f64,
    stack: usize,
}

/// Two directions count as one below this distance of their unit vectors
/// (about 0.0001°), which forgives rounding but not a tilt.
const SAME_DIRECTION: f64 = 2e-6;

impl Home {
    /// Whether a measurement with this home is shown on the picture `shown`:
    /// on its own image only, or while the reformat has the same orientation
    /// and is within half a step of the position where it was drawn.
    pub fn shows_on(&self, shown: &Self) -> bool {
        match (self, shown) {
            (Self::Image(a), Self::Image(b)) => a == b,
            (Self::Plane(a), Self::Plane(b)) => {
                a.stack == b.stack
                    && (a.right - b.right).length() < SAME_DIRECTION
                    && (a.down - b.down).length() < SAME_DIRECTION
                    && (a.position - b.position).abs() <= a.step / 2.0 + 1e-6
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Grid {
    /// The picture has a position in the patient: points are patient coordinates.
    Patient(PixelGrid),
    /// It has not: points are pixels (column, row, 0), with this spacing
    /// (row spacing, column spacing) when one is known.
    Pixels { spacing: Option<[f64; 2]> },
}

/// A picture as measuring sees it: which picture it is, how its pixels map to
/// positions and where its scale comes from. Made per picture, so that it
/// works the same in a single view and in a tile.
#[derive(Debug, Clone, Copy)]
pub struct Picture {
    home: Home,
    width: usize,
    height: usize,
    grid: Grid,
    calibration: Calibration,
    /// Reformats: the distance between the slices of the series, mm.
    slice_spacing: Option<f64>,
}

impl Picture {
    /// Acquired image `index` of the series (`ViewState::slot_index`) of
    /// `size` columns and rows.
    pub fn acquired(
        index: usize,
        size: (u32, u32),
        plane: Option<&ImagePlane>,
        spacing: Option<[f64; 2]>,
        calibration: Calibration,
    ) -> Self {
        let grid = plane.map_or(Grid::Pixels { spacing }, |plane| Grid::Patient(PixelGrid::of_plane(plane)));
        Self {
            home: Home::Image(index),
            width: size.0 as usize,
            height: size.1 as usize,
            grid,
            calibration,
            slice_spacing: None,
        }
    }

    /// A reformat of image set `stack`, whose volume has `geometry`.
    pub fn reformat(image: &MprImage, stack: usize, geometry: &VolumeGeometry) -> Self {
        let normal = image.right.cross(image.down);
        let basis = PlaneBasis { right: image.right, down: image.down, normal };
        let step = MprExtent::of_plane(geometry, &basis).step;
        Self {
            home: Home::Plane(PlaneHome {
                right: image.right,
                down: image.down,
                position: image.origin.dot(normal),
                step,
                stack,
            }),
            width: image.width,
            height: image.height,
            grid: Grid::Patient(PixelGrid::of_reformat(image)),
            calibration: Calibration::PixelSpacing,
            slice_spacing: Some(geometry.spacing().z),
        }
    }

    /// Whether what is measured here is marked approximate: a reformat.
    pub fn is_approximate(&self) -> bool {
        matches!(self.home, Home::Plane(_))
    }

    /// The nearest point inside the picture, up to the outer edge of its pixels.
    pub fn clamp(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (x.max(-0.5).min(self.width as f64 - 0.5), y.max(-0.5).min(self.height as f64 - 0.5))
    }

    /// The pixel position of the stored point `point`.
    pub fn to_pixel(self, point: DVec3) -> (f64, f64) {
        match self.grid {
            Grid::Patient(grid) => {
                let d = point - grid.origin;
                (d.dot(grid.column) / grid.column.length_squared(), d.dot(grid.row) / grid.row.length_squared())
            }
            Grid::Pixels { .. } => (point.x, point.y),
        }
    }

    fn to_stored(self, (x, y): (f64, f64)) -> DVec3 {
        match self.grid {
            Grid::Patient(grid) => grid.to_patient(x, y),
            Grid::Pixels { .. } => DVec3::new(x, y, 0.0),
        }
    }

    /// The stored point in millimetres; in pixels where nothing gives a scale.
    fn physical(&self, point: DVec3) -> DVec3 {
        match self.grid {
            Grid::Pixels { spacing: Some([row, column]) } => DVec3::new(point.x * column, point.y * row, 0.0),
            Grid::Patient(_) | Grid::Pixels { spacing: None } => point,
        }
    }

    /// Area of one pixel in mm², when the picture has a scale.
    fn pixel_area(&self) -> Option<f64> {
        match self.grid {
            Grid::Patient(grid) => Some(grid.column.length() * grid.row.length()),
            Grid::Pixels { spacing } => spacing.map(|[row, column]| row * column),
        }
    }
}

/// The values of a picture, for region statistics.
#[derive(Debug, Clone, Copy)]
pub enum Values<'a> {
    /// The stored modality values of an acquired image.
    Grey { data: &'a VoxelBuffer, width: usize, height: usize, padding: Option<Padding> },
    /// The resampled values of a reformat, and the projection of a thick slab.
    Reformat { image: &'a MprImage, slab: Option<SlabMode> },
    /// A colour image has no values to count.
    Colour,
    /// The image is not decoded yet.
    Pending,
    /// The image could not be decoded.
    Failed,
}

impl<'a> Values<'a> {
    /// The values of acquired image `index` of `series`.
    pub fn of_frame(series: &'a SeriesData, index: usize) -> Self {
        match series.image(index) {
            ImageSlot::Ready(FrameRef::Gray { width, height, data }) => {
                Self::Grey { data, width: width as usize, height: height as usize, padding: series.padding(index) }
            }
            ImageSlot::Ready(FrameRef::Rgb { .. }) => Self::Colour,
            ImageSlot::Pending => Self::Pending,
            ImageSlot::Failed(_) => Self::Failed,
        }
    }

    fn statistics(&self, region: &Region) -> Option<RegionStatistics> {
        match *self {
            Self::Grey { data, width, height, padding } => {
                Some(region_statistics(region, width, height, |x, y| data.get(y * width + x), padding))
            }
            Self::Reformat { image, .. } => {
                let width = image.width;
                Some(region_statistics(region, width, image.height, |x, y| image.values[y * width + x], None))
            }
            Self::Colour | Self::Pending | Self::Failed => None,
        }
    }
}

/// What a measurement reads, in words.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Readout {
    /// The label on the picture, line by line.
    pub lines: Vec<String>,
    /// Its tooltip: where the scale comes from and what is approximate.
    pub details: Vec<String>,
}

/// What every measuring tool and result says about itself.
pub fn approximate_notice() -> &'static str {
    // Translators: keep this statement unambiguous and at least as strong as the English.
    tr!("Approximate — not for diagnosis")
}

fn marked(picture: &Picture, text: String) -> String {
    if picture.is_approximate() { format!("≈ {text}") } else { text }
}

fn uncalibrated(value: String) -> String {
    // Translators: a result in pixels of an image without a scale, such as "123 px (uncalibrated)".
    tr!("{value} (uncalibrated)", value = value)
}

/// The note under a size on an image calibrated at the detector only.
fn at_detector_note() -> &'static str {
    // Translators: a size on an X-ray image is the size at the detector; the body part itself may be smaller.
    tr!("at detector — true size may be smaller")
}

/// Millimetres, or pixels and "uncalibrated"; marked as the picture requires.
fn length_text(picture: &Picture, length: f64) -> String {
    match picture.pixel_area() {
        Some(_) => marked(picture, format!("{length:.1} mm")),
        None => uncalibrated(format!("{length:.0} px")),
    }
}

fn area_text(picture: &Picture, region: &Region) -> String {
    let in_pixels = region.area_in_pixels();
    let line = |value: String| {
        // Translators: the area of a region, with its unit, such as "Area 2.0 cm²".
        tr!("Area {value}", value = value)
    };
    match picture.pixel_area() {
        Some(pixel) => {
            let mm2 = in_pixels * pixel;
            let value = if mm2 < 100.0 { format!("{mm2:.1} mm²") } else { format!("{:.1} cm²", mm2 / 100.0) };
            marked(picture, line(value))
        }
        None => uncalibrated(line(format!("{in_pixels:.0} px²"))),
    }
}

/// Where the scale of `picture` comes from, for a tooltip.
fn scale_note(picture: &Picture) -> String {
    if let Some(spacing) = picture.slice_spacing {
        return tr!(
            "Computed on a reformat, so approximate: the slices of this series are {spacing} mm apart, and detail across them is coarser than within them.",
            spacing = number(spacing, 2)
        );
    }
    match picture.calibration {
        Calibration::PixelSpacing => tr!("Scale from the Pixel Spacing recorded in the image.").to_owned(),
        Calibration::ImagerPixelSpacing => tr!(
            "Scale from the Imager Pixel Spacing recorded in the image: sizes at the detector. What lies in front of the detector appears larger on the image, so its true size may be smaller."
        )
        .to_owned(),
        Calibration::Uncalibrated => tr!(
            "This image records no pixel spacing: the result is in pixels, assumes square pixels and says nothing about true size."
        )
        .to_owned(),
    }
}

/// One measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    id: u64,
    kind: MeasureKind,
    home: Home,
    /// Two points (a length, or opposite corners of a region), or three (the
    /// arms of an angle with the vertex in the middle). While an angle is
    /// being drawn, one to three.
    points: Vec<DVec3>,
}

impl Measurement {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn kind(&self) -> MeasureKind {
        self.kind
    }

    /// The opposite corners of a region, in pixels of `picture`.
    pub fn corners(&self, picture: &Picture) -> Option<((f64, f64), (f64, f64))> {
        self.kind.shape().map(|_| (picture.to_pixel(self.points[0]), picture.to_pixel(self.points[1])))
    }

    /// The region, in pixels of `picture`.
    pub fn region(&self, picture: &Picture) -> Option<Region> {
        let (a, b) = self.corners(picture)?;
        Some(Region::new(self.kind.shape()?, a, b))
    }

    /// The handles, in pixels of `picture`: the points of a length or an
    /// angle, the corners of a rectangle, the ends of the axes of an ellipse.
    pub fn handles(&self, picture: &Picture) -> Vec<(f64, f64)> {
        let Some((a, b)) = self.corners(picture) else {
            return self.points.iter().map(|p| picture.to_pixel(*p)).collect();
        };
        if self.kind == MeasureKind::Rectangle {
            vec![a, (b.0, a.1), b, (a.0, b.1)]
        } else {
            let middle = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            vec![(middle.0, a.1), (b.0, middle.1), (middle.0, b.1), (a.0, middle.1)]
        }
    }

    /// The point at pixel `at` of `picture`, on the plane this measurement
    /// belongs to: the reformat shown may lie up to half a step beside it.
    fn stored_at(&self, picture: &Picture, at: (f64, f64)) -> DVec3 {
        let point = picture.to_stored(at);
        match self.home {
            Home::Image(_) => point,
            Home::Plane(home) => {
                let normal = home.right.cross(home.down);
                point + normal * (home.position - point.dot(normal))
            }
        }
    }

    /// The points after handle `handle` has been moved to pixel `at`.
    fn with_handle(&self, picture: &Picture, handle: usize, at: (f64, f64)) -> Vec<DVec3> {
        let mut points = self.points.clone();
        let Some((a, b)) = self.corners(picture) else {
            if let Some(point) = points.get_mut(handle) {
                *point = self.stored_at(picture, at);
            }
            return points;
        };
        let (a, b) = match (self.kind, handle) {
            (MeasureKind::Rectangle, 0) => (at, b),
            (MeasureKind::Rectangle, 1) => ((a.0, at.1), (at.0, b.1)),
            (MeasureKind::Rectangle, 2) => (a, at),
            (MeasureKind::Rectangle, 3) => ((at.0, a.1), (b.0, at.1)),
            // The ends of an ellipse's axes change its size in one direction.
            (_, 0) => ((a.0, at.1), b),
            (_, 1) => (a, (at.0, b.1)),
            (_, 2) => (a, (b.0, at.1)),
            (_, 3) => ((at.0, a.1), b),
            _ => (a, b),
        };
        points[0] = self.stored_at(picture, a);
        points[1] = self.stored_at(picture, b);
        points
    }

    /// Whether these points make a measurement: a length has length, the arms
    /// of an angle have length, a region has an extent in both directions.
    fn is_valid(&self, picture: &Picture) -> bool {
        const NOTHING: f64 = 1e-6;
        match (self.kind, self.points.as_slice()) {
            (MeasureKind::Length, [a, b]) => distance(*a, *b) > NOTHING,
            (MeasureKind::Angle, [a, v, b]) => distance(*a, *v) > NOTHING && distance(*b, *v) > NOTHING,
            (MeasureKind::Ellipse | MeasureKind::Rectangle, [_, _]) => {
                self.region(picture).is_some_and(|region| region.size().0 > NOTHING && region.size().1 > NOTHING)
            }
            _ => false,
        }
    }

    /// The length in the unit of `picture`, for a length measurement.
    fn length(&self, picture: &Picture) -> Option<f64> {
        match (self.kind, self.points.as_slice()) {
            (MeasureKind::Length, [a, b]) => Some(distance(picture.physical(*a), picture.physical(*b))),
            _ => None,
        }
    }

    /// The angle in degrees, for an angle measurement with all three points.
    fn angle(&self, picture: &Picture) -> Option<f64> {
        match (self.kind, self.points.as_slice()) {
            (MeasureKind::Angle, [a, v, b]) => {
                angle_degrees(picture.physical(*a), picture.physical(*v), picture.physical(*b))
            }
            _ => None,
        }
    }

    /// What this measurement reads on `picture`, with the values of that
    /// picture in `units`.
    pub fn readout(&self, picture: &Picture, values: &Values<'_>, units: Option<&str>) -> Readout {
        let mut readout = Readout { lines: Vec::new(), details: vec![scale_note(picture)] };
        match self.kind {
            MeasureKind::Length => {
                if let Some(length) = self.length(picture) {
                    readout.lines.push(length_text(picture, length));
                    self.note_at_detector(picture, &mut readout);
                }
            }
            MeasureKind::Angle => {
                if let Some(angle) = self.angle(picture) {
                    let text = format!("{angle:.1}°");
                    readout.lines.push(if picture.pixel_area().is_some() {
                        marked(picture, text)
                    } else {
                        uncalibrated(text)
                    });
                }
            }
            MeasureKind::Ellipse | MeasureKind::Rectangle => {
                if let Some(region) = self.region(picture) {
                    readout.lines.push(area_text(picture, &region));
                    self.note_at_detector(picture, &mut readout);
                    read_values(picture, &region, values, units, &mut readout);
                }
            }
        }
        readout.details.push(format!("{}.", approximate_notice()));
        readout
    }

    fn note_at_detector(&self, picture: &Picture, readout: &mut Readout) {
        if picture.calibration == Calibration::ImagerPixelSpacing && !picture.is_approximate() {
            readout.lines.push(at_detector_note().to_owned());
        }
    }
}

/// The statistics of a region in its label and its tooltip.
fn read_values(picture: &Picture, region: &Region, values: &Values<'_>, units: Option<&str>, readout: &mut Readout) {
    let Some(statistics) = values.statistics(region) else {
        readout.lines.push(
            match values {
                Values::Colour => tr!("Statistics not available for colour images"),
                Values::Pending => tr!("Values not loaded yet"),
                _ => tr!("No values: the image could not be decoded"),
            }
            .to_owned(),
        );
        return;
    };
    if let Values::Reformat { slab: Some(mode), .. } = values {
        // Translators: statistics of a thick slab: {mode} is MIP, MinIP or Average.
        readout.lines.push(tr!("{mode} values", mode = mode.label()));
    }
    let text = |value: f64| value_text(units, value);
    match statistics.summary {
        Some(summary) => {
            // Translators: statistics of a region: the mean and the standard deviation (SD) of its values.
            let spread = tr!("Mean {mean} · SD {sd}", mean = text(summary.mean), sd = text(summary.std_dev));
            // Translators: statistics of a region: the smallest and the largest of its values.
            let range =
                tr!("Min {min} · Max {max}", min = text(f64::from(summary.min)), max = text(f64::from(summary.max)));
            readout.lines.push(marked(picture, spread));
            readout.lines.push(marked(picture, range));
        }
        None if statistics.left_out > 0 => readout.lines.push(tr!("No values: outside the scanned area").to_owned()),
        None => readout.lines.push(tr!("Too small: no pixel centre lies inside").to_owned()),
    }
    if statistics.included > 0 {
        readout.details.push(tr_n!("Values of {n} pixel.", "Values of {n} pixels.", statistics.included));
    }
    if statistics.left_out > 0 && statistics.included > 0 {
        readout.details.push(tr_n!(
            "{n} pixel outside the scanned area is left out.",
            "{n} pixels outside the scanned area are left out.",
            statistics.left_out
        ));
    }
}

/// What is taken hold of when a measurement is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grip {
    /// One of [`Measurement::handles`].
    Handle(usize),
    /// The whole measurement.
    Body,
}

#[derive(Debug, Clone)]
struct Edit {
    id: u64,
    grip: Grip,
    /// Where it was taken hold of, in pixels.
    from: (f64, f64),
    /// The points before, to move from and to return to.
    original: Vec<DVec3>,
}

/// The measurements of one series for the session, with what is selected and
/// what is being drawn or edited.
#[derive(Debug, Clone, Default)]
pub struct Measurements {
    items: Vec<Measurement>,
    selected: Option<u64>,
    /// What is being drawn; its last point follows the pointer.
    draft: Option<Measurement>,
    edit: Option<Edit>,
    next_id: u64,
}

impl Measurements {
    /// Whether the series has no finished measurement.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// The finished measurements shown on `picture`.
    pub fn on_picture<'a>(&'a self, picture: &'a Picture) -> impl Iterator<Item = &'a Measurement> {
        self.items.iter().filter(|m| m.home.shows_on(&picture.home))
    }

    /// What is being drawn on `picture`, if anything.
    pub fn draft_on(&self, picture: &Picture) -> Option<&Measurement> {
        self.draft.as_ref().filter(|m| m.home.shows_on(&picture.home))
    }

    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    pub fn select(&mut self, id: Option<u64>) {
        self.selected = id.filter(|id| self.items.iter().any(|m| m.id == *id));
    }

    /// Whether a measurement is being drawn or edited.
    pub fn is_busy(&self) -> bool {
        self.draft.is_some() || self.edit.is_some()
    }

    /// The tool chosen now (`None`: not a measuring tool): what is being drawn
    /// with another tool is dropped, an edit without a measuring tool undone.
    pub fn tool_changed(&mut self, kind: Option<MeasureKind>) {
        if self.draft.as_ref().is_some_and(|draft| Some(draft.kind) != kind) {
            self.draft = None;
        }
        if kind.is_none() && self.edit.is_some() {
            self.cancel();
        }
    }

    /// Starts drawing a measurement of `kind` at pixel `at` of `picture`,
    /// instead of whatever was being drawn.
    pub fn begin(&mut self, kind: MeasureKind, picture: &Picture, at: (f64, f64)) {
        let point = picture.to_stored(picture.clamp(at));
        self.draft = Some(Measurement { id: 0, kind, home: picture.home, points: vec![point, point] });
        self.selected = None;
    }

    /// The free end of what is being drawn follows the pointer at pixel `at`.
    pub fn follow(&mut self, picture: &Picture, at: (f64, f64)) {
        if let Some(draft) = self.draft.as_mut().filter(|m| m.home.shows_on(&picture.home)) {
            let point = draft.stored_at(picture, picture.clamp(at));
            if let Some(last) = draft.points.last_mut() {
                *last = point;
            }
        }
    }

    /// Finishes what is being drawn, if it amounts to something: it becomes
    /// the selected measurement. Otherwise it is dropped.
    pub fn finish(&mut self, picture: &Picture) -> Option<u64> {
        let draft = self.draft.take()?;
        (draft.home.shows_on(&picture.home) && draft.is_valid(picture)).then(|| self.keep(draft))
    }

    /// A click at pixel `at` while drawing an angle: fixes the current point;
    /// the third one finishes it. A click at the point before does nothing,
    /// and a click on another picture starts again there.
    pub fn place(&mut self, picture: &Picture, at: (f64, f64)) -> Option<u64> {
        let at = picture.clamp(at);
        let drawing = |m: &&mut Measurement| m.kind == MeasureKind::Angle && m.home.shows_on(&picture.home);
        let Some(draft) = self.draft.as_mut().filter(drawing) else {
            self.begin(MeasureKind::Angle, picture, at);
            return None;
        };
        let point = draft.stored_at(picture, at);
        let fixed = draft.points.len() - 1;
        if fixed > 0 && distance(point, draft.points[fixed - 1]) < 1e-6 {
            return None;
        }
        draft.points[fixed] = point;
        if draft.points.len() < 3 {
            draft.points.push(point);
            return None;
        }
        let draft = self.draft.take()?;
        Some(self.keep(draft))
    }

    fn keep(&mut self, mut measurement: Measurement) -> u64 {
        self.next_id += 1;
        measurement.id = self.next_id;
        self.items.push(measurement);
        self.selected = Some(self.next_id);
        self.next_id
    }

    /// Esc: drops what is being drawn and returns what is being edited to
    /// where it was. Returns whether there was something to cancel.
    pub fn cancel(&mut self) -> bool {
        let edit = self.edit.take();
        if let Some(edit) = &edit
            && let Some(measurement) = self.items.iter_mut().find(|m| m.id == edit.id)
        {
            measurement.points.clone_from(&edit.original);
        }
        edit.is_some() || self.draft.take().is_some()
    }

    /// Takes hold of measurement `id` at pixel `at` of `picture` and selects it.
    pub fn grab(&mut self, picture: &Picture, id: u64, grip: Grip, at: (f64, f64)) {
        if let Some(measurement) = self.items.iter().find(|m| m.id == id) {
            let original = measurement.points.clone();
            self.edit = Some(Edit { id, grip, from: picture.clamp(at), original });
            self.selected = Some(id);
        }
    }

    /// Moves what was taken hold of to pixel `at`. It stays inside the
    /// picture, and a change that would leave no measurement is not made.
    pub fn drag(&mut self, picture: &Picture, at: (f64, f64)) {
        let Some(edit) = &self.edit else { return };
        let Some(measurement) = self.items.iter_mut().find(|m| m.id == edit.id) else {
            self.edit = None;
            return;
        };
        if !measurement.home.shows_on(&picture.home) {
            return;
        }
        let at = picture.clamp(at);
        let before = std::mem::replace(&mut measurement.points, edit.original.clone());
        let points = match edit.grip {
            Grip::Handle(handle) => match measurement.handles(picture).get(handle) {
                // It moves with the pointer, from where it was: it does not jump to the pointer.
                Some(&(x, y)) => {
                    let to = picture.clamp((x + at.0 - edit.from.0, y + at.1 - edit.from.1));
                    measurement.with_handle(picture, handle, to)
                }
                None => before.clone(),
            },
            Grip::Body => moved(measurement, picture, edit, at),
        };
        measurement.points = points;
        if !measurement.is_valid(picture) {
            measurement.points = before;
        }
    }

    /// Lets go of what was taken hold of.
    pub fn release(&mut self) {
        self.edit = None;
    }

    /// Removes the selected measurement. Returns whether there was one.
    pub fn delete_selected(&mut self) -> bool {
        let Some(id) = self.selected.take() else { return false };
        self.edit = None;
        self.items.retain(|m| m.id != id);
        true
    }

    /// Removes every measurement of the series.
    pub fn clear(&mut self) {
        self.items.clear();
        (self.selected, self.draft, self.edit) = (None, None, None);
    }
}

/// The points of `measurement` moved as a whole by what the pointer did since
/// it took hold, no further than the edges of the picture.
fn moved(measurement: &Measurement, picture: &Picture, edit: &Edit, at: (f64, f64)) -> Vec<DVec3> {
    let pixels: Vec<(f64, f64)> = edit.original.iter().map(|p| picture.to_pixel(*p)).collect();
    let limit = |delta: f64, of: fn(&(f64, f64)) -> f64, size: usize| {
        let (low, high) =
            pixels.iter().map(of).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
        delta.max(-0.5 - low).min(size as f64 - 0.5 - high)
    };
    let dx = limit(at.0 - edit.from.0, |p| p.0, picture.width);
    let dy = limit(at.1 - edit.from.1, |p| p.1, picture.height);
    pixels.iter().map(|&(x, y)| measurement.stored_at(picture, (x + dx, y + dy))).collect()
}

#[cfg(test)]
mod tests {
    use summrise_dicom::{Progress, plan_series, scan};
    use summrise_imaging::{MprAxis, Slab, reslice_plane};
    use summrise_testkit::measure::{self as phantoms, write_measure_study};
    use summrise_testkit::{CT_SOFT_TISSUE_HU, MARKER_POSITION, MARKER_RADIUS_MM, PhantomOptions};

    use super::*;
    use crate::series::testing::{fill_from_disc, load, phantom};
    use crate::view::{Tool, ViewKind, ViewState};

    const NO_VALUES: Values<'static> = Values::Pending;

    /// An axial image of `columns` × `rows` with this spacing at the origin.
    fn axial(index: usize, columns: u32, rows: u32, spacing: [f64; 2]) -> Picture {
        let plane = ImagePlane::from_dicom([0.0; 3], [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], spacing, rows, columns);
        Picture::acquired(index, (columns, rows), plane.as_ref(), Some(spacing), Calibration::PixelSpacing)
    }

    /// Draws a measurement of two points and returns it.
    fn draw(store: &mut Measurements, kind: MeasureKind, picture: &Picture, a: (f64, f64), b: (f64, f64)) -> u64 {
        store.begin(kind, picture, a);
        store.follow(picture, b);
        store.finish(picture).expect("a measurement")
    }

    fn item(store: &Measurements, id: u64) -> &Measurement {
        store.items.iter().find(|m| m.id == id).expect("the measurement")
    }

    fn lines(store: &Measurements, id: u64, picture: &Picture) -> Vec<String> {
        item(store, id).readout(picture, &NO_VALUES, None).lines
    }

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn a_tool_keeps_its_kind() {
        assert_eq!(Tool::Measure(MeasureKind::Angle).label(), "Angle");
        assert!(Tool::Measure(MeasureKind::Length).is_measuring() && !Tool::Pan.is_measuring());
    }

    /// Non-square pixels (spec): 100 columns read 50.0 mm, 100 rows 100.0 mm.
    #[test]
    fn lengths_on_non_square_pixels() {
        let picture = axial(0, 200, 200, [1.0, 0.5]);
        let mut store = Measurements::default();
        let across = draw(&mut store, MeasureKind::Length, &picture, (20.0, 40.0), (120.0, 40.0));
        let down = draw(&mut store, MeasureKind::Length, &picture, (20.0, 40.0), (20.0, 140.0));
        assert_eq!(lines(&store, across, &picture), ["50.0 mm"]);
        assert_eq!(lines(&store, down, &picture), ["100.0 mm"]);
    }

    #[test]
    fn an_angle_takes_three_clicks_and_reads_in_patient_space() {
        let picture = axial(0, 200, 200, [1.0, 0.5]);
        let mut store = Measurements::default();
        // The arms along a row and along a column: 90.0° (spec).
        assert_eq!(store.place(&picture, (120.0, 40.0)), None);
        assert_eq!(store.len(), 0, "nothing is kept while it is being drawn");
        assert_eq!(store.place(&picture, (20.0, 40.0)), None, "the vertex");
        // A click at the vertex again is no point.
        assert_eq!(store.place(&picture, (20.0, 40.0)), None);
        let id = store.place(&picture, (20.0, 140.0)).expect("three points");
        assert_eq!(lines(&store, id, &picture), ["90.0°"]);
        assert_eq!(store.selected(), Some(id));
        assert!(!store.is_busy());
        // 100 columns are 50 mm, 100 rows are 100 mm: not the 45° it looks like in pixels.
        store.place(&picture, (120.0, 40.0));
        store.place(&picture, (20.0, 40.0));
        let id = store.place(&picture, (120.0, 140.0)).expect("three points");
        assert_eq!(lines(&store, id, &picture), ["63.4°"]);
        // The line at 180° is an angle too.
        store.place(&picture, (20.0, 40.0));
        store.place(&picture, (40.0, 40.0));
        let id = store.place(&picture, (60.0, 40.0)).expect("three points");
        assert_eq!(lines(&store, id, &picture), ["180.0°"]);
    }

    /// Area in physical units (spec): 40 × 20 pixels of 0.5 mm read 2.0 cm².
    #[test]
    fn areas_are_in_physical_units_with_mm2_below_100() {
        let picture = axial(0, 64, 64, [0.5, 0.5]);
        let mut store = Measurements::default();
        let large = draw(&mut store, MeasureKind::Rectangle, &picture, (10.0, 10.0), (50.0, 30.0));
        assert_eq!(lines(&store, large, &picture)[0], "Area 2.0 cm²");
        let small = draw(&mut store, MeasureKind::Rectangle, &picture, (10.0, 10.0), (20.0, 20.0));
        assert_eq!(lines(&store, small, &picture)[0], "Area 25.0 mm²");
        // Just under and at the limit.
        let under = draw(&mut store, MeasureKind::Rectangle, &picture, (0.0, 0.0), (19.9, 20.0));
        assert_eq!(lines(&store, under, &picture)[0], "Area 99.5 mm²");
        let at = draw(&mut store, MeasureKind::Rectangle, &picture, (0.0, 0.0), (20.0, 20.0));
        assert_eq!(lines(&store, at, &picture)[0], "Area 1.0 cm²");
        // π/4 of the box: 20 mm × 20 mm.
        let ellipse = draw(&mut store, MeasureKind::Ellipse, &picture, (10.0, 10.0), (50.0, 50.0));
        assert_eq!(lines(&store, ellipse, &picture)[0], "Area 3.1 cm²");
        // Non-square pixels: 100 columns of 0.5 mm × 50 rows of 1.0 mm.
        let picture = axial(1, 200, 200, [1.0, 0.5]);
        let wide = draw(&mut store, MeasureKind::Rectangle, &picture, (0.0, 0.0), (100.0, 50.0));
        assert_eq!(lines(&store, wide, &picture)[0], "Area 25.0 cm²");
    }

    fn study() -> (tempfile::TempDir, summrise_dicom::StudyIndex) {
        let dir = tempfile::tempdir().expect("tempdir");
        write_measure_study(dir.path()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        (dir, index)
    }

    fn picture_of(series: &SeriesData, meta: &summrise_dicom::Series, index: usize) -> Picture {
        let info = &series.frames()[index];
        let calibration = meta.instances[info.instance].calibration;
        Picture::acquired(index, (info.width, info.height), info.plane.as_ref(), info.pixel_spacing, calibration)
    }

    /// The files record where the scale comes from, and the label says it.
    #[test]
    fn the_calibration_comes_from_the_images() {
        let (_dir, index) = study();
        let series = &index.studies[0].series;
        let calibration = |n: usize| series[n].instances[0].calibration;
        assert_eq!(calibration(phantoms::NON_SQUARE), Calibration::PixelSpacing);
        assert_eq!(series[phantoms::NON_SQUARE].instances[0].pixel_spacing, Some([1.0, 0.5]));
        assert_eq!(calibration(phantoms::PROJECTION), Calibration::ImagerPixelSpacing);
        assert_eq!(series[phantoms::PROJECTION].instances[0].pixel_spacing, Some([0.25, 0.25]));
        assert_eq!(calibration(phantoms::NO_POSITION), Calibration::PixelSpacing);

        // Projection X-ray (spec): millimetres at the detector, 80 columns of 0.25 mm.
        let data = load(&series[phantoms::PROJECTION]);
        let picture = picture_of(&data, &series[phantoms::PROJECTION], 0);
        let mut store = Measurements::default();
        let id = draw(&mut store, MeasureKind::Length, &picture, (10.0, 10.0), (90.0, 10.0));
        let readout = item(&store, id).readout(&picture, &Values::of_frame(&data, 0), data.units());
        assert_eq!(readout.lines, ["20.0 mm", "at detector — true size may be smaller"]);
        assert!(readout.details[0].contains("Imager Pixel Spacing"), "{:?}", readout.details);
        // A region there says so too: 40 × 40 pixels of 0.25 mm.
        let id = draw(&mut store, MeasureKind::Rectangle, &picture, (10.0, 10.0), (50.0, 50.0));
        let readout = item(&store, id).readout(&picture, &Values::of_frame(&data, 0), data.units());
        assert_eq!(readout.lines[0], "Area 1.0 cm²");
        assert_eq!(readout.lines[1], "at detector — true size may be smaller");
        // Values `column` from 10 to 50.
        assert_eq!(readout.lines[2], "Mean 30 · SD 11.8");
        assert_eq!(readout.lines[3], "Min 10 · Max 50");

        // Spacing without position: measured from the pixels and the spacing; no note.
        let data = load(&series[phantoms::NO_POSITION]);
        let picture = picture_of(&data, &series[phantoms::NO_POSITION], 0);
        let id = draw(&mut store, MeasureKind::Length, &picture, (10.0, 10.0), (90.0, 10.0));
        assert_eq!(lines(&store, id, &picture), ["20.0 mm"], "80 columns of 0.25 mm");
        let id = draw(&mut store, MeasureKind::Length, &picture, (10.0, 10.0), (10.0, 90.0));
        assert_eq!(lines(&store, id, &picture), ["40.0 mm"], "80 rows of 0.5 mm");
        let id = draw(&mut store, MeasureKind::Rectangle, &picture, (0.0, 0.0), (40.0, 80.0));
        assert_eq!(lines(&store, id, &picture)[0], "Area 4.0 cm²", "10 mm × 40 mm");
    }

    /// Screenshot without spacing (spec), and its region: pixels, marked
    /// uncalibrated; colour images have no statistics (spec).
    #[test]
    fn an_image_without_spacing_is_measured_in_pixels_and_a_colour_image_has_no_statistics() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let meta = &index.studies[0].series[3];
        assert_eq!(meta.instances[0].calibration, Calibration::Uncalibrated);
        let data = load(meta);
        let picture = picture_of(&data, meta, 0);
        let mut store = Measurements::default();
        let id = draw(&mut store, MeasureKind::Length, &picture, (10.0, 10.0), (133.0, 10.0));
        let values = Values::of_frame(&data, 0);
        let readout = item(&store, id).readout(&picture, &values, data.units());
        assert_eq!(readout.lines, ["123 px (uncalibrated)"]);
        assert!(readout.details[0].contains("no pixel spacing"), "{:?}", readout.details);
        store.place(&picture, (50.0, 10.0));
        store.place(&picture, (10.0, 10.0));
        let id = store.place(&picture, (10.0, 50.0)).expect("three points");
        assert_eq!(lines(&store, id, &picture), ["90.0° (uncalibrated)"]);
        let id = draw(&mut store, MeasureKind::Ellipse, &picture, (10.0, 10.0), (50.0, 30.0));
        let readout = item(&store, id).readout(&picture, &values, data.units());
        assert_eq!(readout.lines, ["Area 628 px² (uncalibrated)", "Statistics not available for colour images"]);
    }

    #[test]
    fn statistics_are_of_the_stored_values_and_not_of_the_window() {
        let (_dir, index) = study();
        let meta = &index.studies[0].series[phantoms::SQUARE];
        let data = load(meta);
        let picture = picture_of(&data, meta, 0);
        let values = Values::of_frame(&data, 0);
        let mut store = Measurements::default();
        // Columns 2 to 4 and rows 3 to 4: values 23, 24, 33, 34, 43, 44.
        let id = draw(&mut store, MeasureKind::Rectangle, &picture, (1.6, 2.6), (4.4, 4.4));
        let readout = item(&store, id).readout(&picture, &values, Some("HU"));
        assert_eq!(readout.lines[1], "Mean 33.5 HU · SD 8.2 HU");
        assert_eq!(readout.lines[2], "Min 23 HU · Max 44 HU");
        assert_eq!(readout.details[1], "Values of 6 pixels.");
        // Between the pixel centres.
        let id = draw(&mut store, MeasureKind::Rectangle, &picture, (1.2, 1.2), (1.8, 1.8));
        let readout = item(&store, id).readout(&picture, &values, Some("HU"));
        assert_eq!(readout.lines[1], "Too small: no pixel centre lies inside");
        // An image that is not there says so.
        let readout = item(&store, id).readout(&picture, &Values::Pending, Some("HU"));
        assert_eq!(readout.lines[1], "Values not loaded yet");
        let readout = item(&store, id).readout(&picture, &Values::Failed, Some("HU"));
        assert_eq!(readout.lines[1], "No values: the image could not be decoded");
    }

    #[test]
    fn padding_is_left_out_of_the_statistics() {
        let dir = tempfile::tempdir().expect("tempdir");
        summrise_testkit::fidelity::write_fidelity_study(dir.path()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        let meta = &index.studies[0].series[summrise_testkit::fidelity::PADDED_CT];
        let data = load(meta);
        let picture = picture_of(&data, meta, 0);
        let values = Values::of_frame(&data, 0);
        let mut store = Measurements::default();
        // The top left corner of the slice, all padding.
        let id = draw(&mut store, MeasureKind::Rectangle, &picture, (0.0, 0.0), (3.0, 3.0));
        let readout = item(&store, id).readout(&picture, &values, data.units());
        assert_eq!(readout.lines[1], "No values: outside the scanned area");
        // The whole slice: the smallest value is air, not the padding value of −3024 HU.
        let id = draw(&mut store, MeasureKind::Rectangle, &picture, (0.0, 0.0), (63.0, 63.0));
        let readout = item(&store, id).readout(&picture, &values, data.units());
        assert!(readout.lines[2].starts_with("Min -1000 HU"), "{:?}", readout.lines);
        let inside = (0..64u32)
            .flat_map(|y| (0..64u32).map(move |x| (x, y)))
            .filter(|&(x, y)| !summrise_testkit::fidelity::padded_ct_is_padding(x, y))
            .count();
        assert_eq!(readout.details[1], format!("Values of {inside} pixels."));
        assert_eq!(readout.details[2], format!("{} pixels outside the scanned area are left out.", 64 * 64 - inside));
    }

    #[test]
    fn statistics_of_a_pet_are_in_suv_and_of_a_ct_under_a_pet_in_hu() {
        let dir = tempfile::tempdir().expect("tempdir");
        let options = summrise_testkit::pet::PetStudyOptions::default();
        summrise_testkit::pet::write_pet_study(dir.path(), &options).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        let series = &index.studies[0].series;
        let pet = load(&series[summrise_testkit::pet::PET]);
        // A region in the hot sphere (SUV 5) at the marker position.
        let slice = (0..pet.len())
            .min_by_key(|&i| {
                let z = pet.frames()[i].plane.expect("plane").position.z;
                ((z - MARKER_POSITION[2]).abs() * 100.0) as i64
            })
            .expect("images");
        let picture = picture_of(&pet, &series[summrise_testkit::pet::PET], slice);
        let marker = picture.to_pixel(DVec3::from_array(MARKER_POSITION));
        let a = (marker.0 - 1.0, marker.1 - 1.0);
        let b = (marker.0 + 1.0, marker.1 + 1.0);
        let mut store = Measurements::default();
        let id = draw(&mut store, MeasureKind::Rectangle, &picture, a, b);
        let readout = item(&store, id).readout(&picture, &Values::of_frame(&pet, slice), pet.units());
        assert!(readout.lines[1].starts_with("Mean SUV 5"), "{:?}", readout.lines);

        // The CT shown under a PET: statistics of the CT, in its unit.
        let ct = load(&series[summrise_testkit::pet::CT]);
        let picture = picture_of(&ct, &series[summrise_testkit::pet::CT], 0);
        let id = draw(&mut store, MeasureKind::Rectangle, &picture, (0.0, 0.0), (3.0, 3.0));
        let readout = item(&store, id).readout(&picture, &Values::of_frame(&ct, 0), ct.units());
        assert_eq!(readout.lines[1], "Mean -1000 HU · SD 0 HU");
    }

    /// Task 4.1: on the 256-pixel synthetic CT, the marker sphere measures its
    /// 20 mm, soft tissue about 40 HU, air −1000 HU with no deviation.
    #[test]
    fn known_sizes_and_values_of_the_256_pixel_ct() {
        let dir = tempfile::tempdir().expect("tempdir");
        let options = PhantomOptions {
            ct_size: 256,
            ct_slices: 120,
            include_localizer: false,
            include_secondary_capture: false,
            include_clutter: false,
            mr_slices: 2,
            mr_size: 16,
            ..PhantomOptions::default()
        };
        summrise_testkit::write_phantom_study(dir.path(), &options).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        let meta = &index.studies[0].series[0];
        let data = SeriesData::new(plan_series(meta));
        // The image through the centre of the sphere, in the files as on screen.
        let slot = (0..data.len())
            .min_by_key(|&i| {
                let z = data.frames()[i].plane.expect("plane").position.z;
                ((z - MARKER_POSITION[2]).abs() * 1000.0) as i64
            })
            .expect("images");
        let instance = data.frames()[slot].instance;
        fill_from_disc(&data, meta, instance);
        let picture = picture_of(&data, meta, slot);
        let values = Values::of_frame(&data, slot);
        let Values::Grey { data: pixels, width, .. } = values else { panic!("a grey image") };

        // Across the sphere: from its first to its last pixel in the row through the centre.
        let centre = picture.to_pixel(DVec3::from_array(MARKER_POSITION));
        let row = centre.1.round() as usize;
        let hot: Vec<usize> = (0..width).filter(|x| pixels.get(row * width + x) == 300.0).collect();
        let (first, last) = (hot[0] as f64 - 0.5, hot[hot.len() - 1] as f64 + 0.5);
        let mut store = Measurements::default();
        let id = draw(&mut store, MeasureKind::Length, &picture, (first, row as f64), (last, row as f64));
        let label = &lines(&store, id, &picture)[0];
        let diameter: f64 = label.trim_end_matches(" mm").parse().expect("millimetres");
        assert!((19.0..=21.0).contains(&diameter), "{label}");
        assert!((MARKER_RADIUS_MM * 2.0 - diameter).abs() <= 1.0, "{label}");

        // Soft tissue: between the lungs, in front of the vessel (x near 0, y −60 mm).
        let tissue = picture.to_pixel(DVec3::new(0.0, -60.0, MARKER_POSITION[2]));
        let id = draw(
            &mut store,
            MeasureKind::Ellipse,
            &picture,
            (tissue.0 - 8.0, tissue.1 - 8.0),
            (tissue.0 + 8.0, tissue.1 + 8.0),
        );
        let readout = item(&store, id).readout(&picture, &values, data.units());
        let mean: f64 = readout.lines[1]
            .strip_prefix("Mean ")
            .and_then(|rest| rest.split(" HU").next())
            .and_then(|text| text.parse().ok())
            .unwrap_or_else(|| panic!("{:?}", readout.lines));
        assert!((37.0..=43.0).contains(&mean), "{:?}", readout.lines);
        assert!((mean - CT_SOFT_TISSUE_HU).abs() <= 3.0);

        // Air, in a corner of the image.
        let id = draw(&mut store, MeasureKind::Ellipse, &picture, (4.0, 4.0), (24.0, 24.0));
        let readout = item(&store, id).readout(&picture, &values, data.units());
        assert_eq!(readout.lines[1], "Mean -1000 HU · SD 0 HU");
        assert_eq!(readout.lines[2], "Min -1000 HU · Max -1000 HU");
        // Spacing is 240 / 256 = 0.9375 mm.
        assert_eq!(meta.instances[0].pixel_spacing, Some([0.9375, 0.9375]));
    }

    #[test]
    fn a_measurement_stays_on_its_image_whichever_set_it_is_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        summrise_testkit::stacks::write_stacks_study(dir.path()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        let meta = &index.studies[0].series[summrise_testkit::stacks::DYNAMIC];
        let data = load(meta);
        assert!(data.stack_count() > 1);
        let mut state = ViewState::new(&data);
        let shown = |state: &ViewState| picture_of(&data, meta, state.slot_index(&data));

        // Scrolling away and back (spec): hidden on the next image, shown again, unchanged.
        let picture = shown(&state);
        let id = draw(&mut state.measurements, MeasureKind::Length, &picture, (5.0, 5.0), (15.0, 5.0));
        let before = item(&state.measurements, id).clone();
        assert_eq!(state.measurements.on_picture(&picture).count(), 1);
        state.scroll(ViewKind::Stack, &data, 1);
        assert_eq!(state.measurements.on_picture(&shown(&state)).count(), 0);
        state.scroll(ViewKind::Stack, &data, -1);
        assert_eq!(state.measurements.on_picture(&shown(&state)).next(), Some(&before));

        // The same position in another image set is another image.
        state.step_stack(&data, 1);
        assert_eq!(state.measurements.on_picture(&shown(&state)).count(), 0);
        let there = shown(&state);
        draw(&mut state.measurements, MeasureKind::Length, &there, (1.0, 1.0), (9.0, 1.0));
        assert_eq!(state.measurements.on_picture(&there).count(), 1);
        state.step_stack(&data, -1);
        assert_eq!(state.measurements.on_picture(&shown(&state)).next(), Some(&before));
    }

    /// A reformat of `state` as the viewer renders it.
    fn reformat(series: &SeriesData, state: &ViewState, axis: MprAxis) -> MprImage {
        let volume = series.stack_volume(state.stack()).expect("volume");
        reslice_plane(volume, &state.plane_basis(axis), state.mpr_position(axis), state.slab, 512)
    }

    /// Reformat of thick slices (spec), with the visibility of D2.
    #[test]
    fn a_measurement_on_a_reformat_belongs_to_its_plane() {
        // 5 mm slice spacing and 5 mm pixels.
        let options = PhantomOptions { ct_size: 48, ct_slices: 40, ..PhantomOptions::small() };
        let (_dir, index) = phantom(&options);
        let data = load(&index.studies[0].series[0]);
        let geometry = data.volume().expect("volume").geometry;
        let mut state = ViewState::new(&data);
        let axis = MprAxis::Sagittal;
        let kind = ViewKind::Mpr(axis);
        state.set_slice(kind, &data, 20);
        let image = reformat(&data, &state, axis);
        let picture = Picture::reformat(&image, 0, &geometry);

        // 40 mm along the head–foot direction: screen-down in a sagittal view.
        let start = picture.to_pixel(DVec3::new(state.cursor.x, 0.0, 40.0));
        let end = picture.to_pixel(DVec3::new(state.cursor.x, 0.0, 0.0));
        let id = draw(&mut state.measurements, MeasureKind::Length, &picture, start, end);
        let values = Values::Reformat { image: &image, slab: None };
        let readout = item(&state.measurements, id).readout(&picture, &values, data.units());
        assert_eq!(readout.lines, ["≈ 40.0 mm"]);
        assert!(readout.details[0].contains("the slices of this series are 5 mm apart"), "{:?}", readout.details);

        // Within half a step of the position it is shown, beyond it not; the step is 5 mm.
        let shown = |state: &ViewState| {
            let image = reformat(&data, state, axis);
            state.measurements.on_picture(&Picture::reformat(&image, 0, &geometry)).count()
        };
        let cursor = state.cursor;
        assert_eq!(shown(&state), 1);
        state.cursor = cursor + DVec3::new(2.4, 0.0, 0.0);
        assert_eq!(shown(&state), 1, "2.4 mm beside it");
        state.cursor = cursor + DVec3::new(-2.4, 0.0, 0.0);
        assert_eq!(shown(&state), 1, "2.4 mm on the other side");
        state.cursor = cursor + DVec3::new(2.6, 0.0, 0.0);
        assert_eq!(shown(&state), 0, "2.6 mm beside it");
        state.cursor = cursor;
        // Scrolling away and back, by whole slices.
        state.scroll(kind, &data, 1);
        assert_eq!(shown(&state), 0);
        state.scroll(kind, &data, -1);
        assert_eq!(shown(&state), 1);
        // Tilting away hides it; returning shows it again.
        state.tilt_planes(MprAxis::Axial, 2f64.to_radians());
        assert_eq!(shown(&state), 0, "tilted by 2°");
        state.tilt_planes(MprAxis::Axial, (-2f64).to_radians());
        assert_eq!(shown(&state), 1, "tilted back");
        state.tilt_planes(MprAxis::Coronal, 0.001);
        assert_eq!(shown(&state), 0, "tilted by less than a tenth of a degree");
        state.reset_planes();
        assert_eq!(shown(&state), 1);
        // Another view at the same place is another plane; so is another image set.
        let axial = reformat(&data, &state, MprAxis::Axial);
        assert_eq!(state.measurements.on_picture(&Picture::reformat(&axial, 0, &geometry)).count(), 0);
        assert_eq!(state.measurements.on_picture(&Picture::reformat(&image, 1, &geometry)).count(), 0);
    }

    /// An edit on a reformat half a step beside the plane stays on the plane,
    /// so the length is not lengthened by the distance between the two.
    #[test]
    fn editing_beside_the_plane_stays_on_the_plane() {
        let options = PhantomOptions { ct_size: 48, ct_slices: 40, ..PhantomOptions::small() };
        let (_dir, index) = phantom(&options);
        let data = load(&index.studies[0].series[0]);
        let geometry = data.volume().expect("volume").geometry;
        let mut state = ViewState::new(&data);
        state.set_slice(ViewKind::Mpr(MprAxis::Sagittal), &data, 20);
        let image = reformat(&data, &state, MprAxis::Sagittal);
        let picture = Picture::reformat(&image, 0, &geometry);
        let id = draw(&mut state.measurements, MeasureKind::Length, &picture, (10.0, 10.0), (11.0, 10.0));
        let before = lines(&state.measurements, id, &picture);

        state.cursor += DVec3::new(2.0, 0.0, 0.0);
        let beside = reformat(&data, &state, MprAxis::Sagittal);
        let beside = Picture::reformat(&beside, 0, &geometry);
        assert_eq!(state.measurements.on_picture(&beside).count(), 1);
        // The other end, where it is: the length does not change.
        state.measurements.grab(&beside, id, Grip::Handle(1), (11.0, 10.0));
        state.measurements.drag(&beside, (11.0, 10.0));
        state.measurements.release();
        assert_eq!(lines(&state.measurements, id, &beside), before);
    }

    #[test]
    fn a_slab_gives_its_name_to_the_statistics() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let data = load(&index.studies[0].series[0]);
        let mut state = ViewState::new(&data);
        state.set_slab(Some(SlabMode::Mip), 20.0);
        let image = reformat(&data, &state, MprAxis::Axial);
        let geometry = data.volume().expect("volume").geometry;
        let picture = Picture::reformat(&image, 0, &geometry);
        let id = draw(&mut state.measurements, MeasureKind::Ellipse, &picture, (2.0, 2.0), (6.0, 6.0));
        let values = Values::Reformat { image: &image, slab: state.slab.map(|s: Slab| s.mode) };
        let readout = item(&state.measurements, id).readout(&picture, &values, data.units());
        assert!(readout.lines[0].starts_with("≈ Area "), "{:?}", readout.lines);
        assert_eq!(readout.lines[1], "MIP values");
        assert_eq!(readout.lines[2], "≈ Mean -1000 HU · SD 0 HU");
        assert_eq!(readout.lines[3], "≈ Min -1000 HU · Max -1000 HU");
    }

    #[test]
    fn values_outside_a_reformat_are_left_out() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let data = load(&index.studies[0].series[0]);
        let mut state = ViewState::new(&data);
        // Tilted, the corners of the picture lie outside the volume.
        state.tilt_planes(MprAxis::Axial, 30f64.to_radians());
        let image = reformat(&data, &state, MprAxis::Coronal);
        let geometry = data.volume().expect("volume").geometry;
        let picture = Picture::reformat(&image, 0, &geometry);
        assert_eq!(image.value_at(0, 0), None);
        // The row through the crosshair: partly outside the volume.
        let row = picture.to_pixel(state.cursor).1;
        let last = image.width as f64 - 1.0;
        let id = draw(&mut state.measurements, MeasureKind::Rectangle, &picture, (0.0, row - 2.0), (last, row + 2.0));
        let values = Values::Reformat { image: &image, slab: None };
        let readout = item(&state.measurements, id).readout(&picture, &values, data.units());
        assert!(readout.lines[1].starts_with("≈ Mean "), "{:?}", readout.lines);
        assert!(!readout.lines.iter().any(|line| line.contains("NaN")), "{:?}", readout.lines);
        assert!(readout.details[2].ends_with("outside the scanned area are left out."), "{:?}", readout.details);
        // Entirely outside: no values, and no number in their place.
        let id = draw(&mut state.measurements, MeasureKind::Rectangle, &picture, (0.0, 0.0), (1.0, 1.0));
        let readout = item(&state.measurements, id).readout(&picture, &values, data.units());
        assert_eq!(readout.lines[1], "No values: outside the scanned area");
    }

    #[test]
    fn handles_move_the_points_they_show() {
        let picture = axial(0, 100, 100, [1.0, 1.0]);
        let mut store = Measurements::default();
        // Adjusting an endpoint (spec): 20.0 mm dragged 5 mm further reads 25.0 mm.
        let id = draw(&mut store, MeasureKind::Length, &picture, (10.0, 10.0), (30.0, 10.0));
        assert_eq!(lines(&store, id, &picture), ["20.0 mm"]);
        store.grab(&picture, id, Grip::Handle(1), (30.0, 10.0));
        assert!(store.is_busy());
        store.drag(&picture, (35.0, 10.0));
        assert_eq!(lines(&store, id, &picture), ["25.0 mm"], "while dragging");
        store.release();
        assert!(!store.is_busy());

        // A rectangle has four corners; each moves its own two sides.
        let id = draw(&mut store, MeasureKind::Rectangle, &picture, (10.0, 10.0), (30.0, 20.0));
        let m = |s: &Measurements| item(s, id).handles(&picture);
        assert_eq!(m(&store), [(10.0, 10.0), (30.0, 10.0), (30.0, 20.0), (10.0, 20.0)]);
        store.grab(&picture, id, Grip::Handle(1), (30.0, 10.0));
        store.drag(&picture, (40.0, 5.0));
        store.release();
        assert_eq!(m(&store), [(10.0, 5.0), (40.0, 5.0), (40.0, 20.0), (10.0, 20.0)]);
        store.grab(&picture, id, Grip::Handle(3), (10.0, 20.0));
        store.drag(&picture, (0.0, 30.0));
        store.release();
        assert_eq!(m(&store), [(0.0, 5.0), (40.0, 5.0), (40.0, 30.0), (0.0, 30.0)]);

        // An ellipse has the ends of its axes; each moves one side.
        let id = draw(&mut store, MeasureKind::Ellipse, &picture, (10.0, 10.0), (30.0, 20.0));
        let m = |s: &Measurements| item(s, id).handles(&picture);
        assert_eq!(m(&store), [(20.0, 10.0), (30.0, 15.0), (20.0, 20.0), (10.0, 15.0)]);
        store.grab(&picture, id, Grip::Handle(0), (20.0, 10.0));
        store.drag(&picture, (99.0, 0.0));
        store.release();
        assert_eq!(m(&store), [(20.0, 0.0), (30.0, 10.0), (20.0, 20.0), (10.0, 10.0)]);
        store.grab(&picture, id, Grip::Handle(1), (30.0, 10.0));
        store.drag(&picture, (50.0, 0.0));
        store.release();
        assert_eq!(m(&store), [(30.0, 0.0), (50.0, 10.0), (30.0, 20.0), (10.0, 10.0)]);
        store.grab(&picture, id, Grip::Handle(2), (30.0, 20.0));
        store.drag(&picture, (0.0, 40.0));
        store.release();
        store.grab(&picture, id, Grip::Handle(3), (10.0, 20.0));
        store.drag(&picture, (20.0, 0.0));
        store.release();
        assert_eq!(m(&store), [(35.0, 0.0), (50.0, 20.0), (35.0, 40.0), (20.0, 20.0)]);

        // An angle's three points.
        store.place(&picture, (30.0, 10.0));
        store.place(&picture, (10.0, 10.0));
        let id = store.place(&picture, (10.0, 30.0)).expect("three points");
        store.grab(&picture, id, Grip::Handle(2), (10.0, 30.0));
        store.drag(&picture, (30.0, 30.0));
        store.release();
        assert_eq!(lines(&store, id, &picture), ["45.0°"]);
    }

    #[test]
    fn a_change_that_would_leave_nothing_is_not_made() {
        let picture = axial(0, 100, 100, [1.0, 1.0]);
        let mut store = Measurements::default();
        let id = draw(&mut store, MeasureKind::Length, &picture, (10.0, 10.0), (30.0, 10.0));
        store.grab(&picture, id, Grip::Handle(1), (30.0, 10.0));
        store.drag(&picture, (10.0, 10.0));
        assert_eq!(lines(&store, id, &picture), ["20.0 mm"], "kept: the two points would coincide");
        store.release();
        let id = draw(&mut store, MeasureKind::Rectangle, &picture, (10.0, 10.0), (30.0, 20.0));
        store.grab(&picture, id, Grip::Handle(2), (30.0, 20.0));
        store.drag(&picture, (30.0, 10.0));
        assert_eq!(item(&store, id).handles(&picture)[2], (30.0, 20.0), "kept: no height");
        store.release();
        store.place(&picture, (30.0, 10.0));
        store.place(&picture, (10.0, 10.0));
        let id = store.place(&picture, (10.0, 30.0)).expect("three points");
        store.grab(&picture, id, Grip::Handle(0), (30.0, 10.0));
        store.drag(&picture, (10.0, 10.0));
        assert_eq!(lines(&store, id, &picture), ["90.0°"], "kept: an arm without length");
        store.release();

        // Drawn without extent: nothing is kept.
        let before = store.len();
        store.begin(MeasureKind::Length, &picture, (10.0, 10.0));
        assert_eq!(store.finish(&picture), None);
        store.begin(MeasureKind::Ellipse, &picture, (10.0, 10.0));
        store.follow(&picture, (40.0, 10.0));
        assert_eq!(store.finish(&picture), None);
        assert_eq!(store.len(), before);
        assert!(!store.is_busy());
    }

    #[test]
    fn a_measurement_moves_as_a_whole_and_stays_inside_the_picture() {
        let picture = axial(0, 100, 50, [1.0, 1.0]);
        let mut store = Measurements::default();
        let id = draw(&mut store, MeasureKind::Length, &picture, (10.0, 10.0), (30.0, 20.0));
        let before = lines(&store, id, &picture);
        store.grab(&picture, id, Grip::Body, (20.0, 15.0));
        store.drag(&picture, (25.0, 18.0));
        let handles = item(&store, id).handles(&picture);
        assert!(close(handles[0], (15.0, 13.0)) && close(handles[1], (35.0, 23.0)), "{handles:?}");
        assert_eq!(lines(&store, id, &picture), before);
        // Up to the edge of the picture, and it keeps its shape.
        store.drag(&picture, (500.0, 500.0));
        let handles = item(&store, id).handles(&picture);
        assert!(close(handles[0], (79.5, 39.5)) && close(handles[1], (99.5, 49.5)), "{handles:?}");
        store.drag(&picture, (-500.0, -500.0));
        let handles = item(&store, id).handles(&picture);
        assert!(close(handles[0], (-0.5, -0.5)) && close(handles[1], (19.5, 9.5)), "{handles:?}");
        assert_eq!(lines(&store, id, &picture), before);
        store.release();
        // Points can only be placed in the picture.
        let id = draw(&mut store, MeasureKind::Length, &picture, (-40.0, 10.0), (500.0, 10.0));
        assert_eq!(item(&store, id).handles(&picture), [(-0.5, 10.0), (99.5, 10.0)]);
    }

    #[test]
    fn esc_cancels_what_is_being_drawn_or_edited() {
        let picture = axial(0, 100, 100, [1.0, 1.0]);
        let mut store = Measurements::default();
        assert!(!store.cancel(), "nothing to cancel");
        store.begin(MeasureKind::Length, &picture, (10.0, 10.0));
        store.follow(&picture, (30.0, 10.0));
        assert!(store.draft_on(&picture).is_some());
        assert!(store.cancel());
        assert!(store.is_empty() && store.draft_on(&picture).is_none());
        // Two points of an angle.
        store.place(&picture, (10.0, 10.0));
        store.place(&picture, (30.0, 10.0));
        assert!(store.cancel());
        assert!(store.is_empty() && !store.is_busy());
        // An edit returns to where it was.
        let id = draw(&mut store, MeasureKind::Length, &picture, (10.0, 10.0), (30.0, 10.0));
        store.grab(&picture, id, Grip::Handle(1), (30.0, 10.0));
        store.drag(&picture, (60.0, 10.0));
        assert_eq!(lines(&store, id, &picture), ["50.0 mm"]);
        assert!(store.cancel());
        assert_eq!(lines(&store, id, &picture), ["20.0 mm"]);
        assert!(!store.is_busy());
    }

    #[test]
    fn selecting_deleting_and_clearing() {
        let picture = axial(0, 100, 100, [1.0, 1.0]);
        let mut store = Measurements::default();
        let first = draw(&mut store, MeasureKind::Length, &picture, (10.0, 10.0), (30.0, 10.0));
        let second = draw(&mut store, MeasureKind::Rectangle, &picture, (40.0, 40.0), (60.0, 60.0));
        assert_eq!(store.selected(), Some(second), "what was drawn last is selected");
        store.select(Some(first));
        assert_eq!(store.selected(), Some(first));
        store.select(Some(99));
        assert_eq!(store.selected(), None, "nothing of that number");
        assert!(!store.delete_selected(), "nothing selected");
        assert_eq!(store.len(), 2);
        store.select(Some(first));
        assert!(store.delete_selected());
        assert_eq!(store.on_picture(&picture).map(Measurement::id).collect::<Vec<_>>(), [second]);
        assert_eq!(store.selected(), None);
        // Numbers are never reused, so a selection cannot pass to another.
        let third = draw(&mut store, MeasureKind::Length, &picture, (1.0, 1.0), (9.0, 1.0));
        assert!(third > second);
        store.clear();
        assert!(store.is_empty() && store.selected().is_none() && !store.is_busy());
    }

    /// A draft belongs to its picture: it is not shown or changed elsewhere,
    /// and a click for an angle on another picture starts there.
    #[test]
    fn what_is_being_drawn_stays_on_its_picture() {
        let (first, second) = (axial(0, 100, 100, [1.0, 1.0]), axial(1, 100, 100, [1.0, 1.0]));
        let mut store = Measurements::default();
        store.place(&first, (10.0, 10.0));
        assert!(store.draft_on(&first).is_some() && store.draft_on(&second).is_none());
        store.follow(&second, (50.0, 50.0));
        assert_eq!(store.draft_on(&first).expect("draft").handles(&first)[1], (10.0, 10.0), "unchanged");
        store.place(&second, (20.0, 20.0));
        assert!(store.draft_on(&first).is_none() && store.draft_on(&second).is_some());
        // A length dragged while the image changed under it is not kept on the wrong image.
        store.begin(MeasureKind::Length, &first, (10.0, 10.0));
        store.follow(&first, (30.0, 10.0));
        assert_eq!(store.finish(&second), None);
        assert!(store.is_empty());
    }

    /// Only that the notice was shown is remembered; settings stored before
    /// the field existed have not shown it.
    #[test]
    fn the_settings_hold_only_the_fact_that_the_notice_was_shown() {
        use serde::de::value::{Error, MapDeserializer};

        use crate::settings::Settings;

        assert!(!Settings::default().measure_notice_shown);
        let stored = [("show_overlays", false)];
        let settings = Settings::deserialize(MapDeserializer::<_, Error>::new(stored.into_iter())).expect("settings");
        assert!(!settings.measure_notice_shown && !settings.show_overlays);
        let stored = [("measure_notice_shown", true)];
        let settings = Settings::deserialize(MapDeserializer::<_, Error>::new(stored.into_iter())).expect("settings");
        assert!(settings.measure_notice_shown);
    }

    /// The state of a view with measurements: in use; "reset" keeps them and
    /// ends what is being drawn; nothing about them is in the settings.
    #[test]
    fn measurements_are_part_of_the_view_state() {
        let (_dir, index) = phantom(&PhantomOptions::small());
        let data = load(&index.studies[0].series[0]);
        let mut state = ViewState::new(&data);
        assert!(state.measurements.is_empty());
        assert!(state.is_pristine(&data));
        let picture = picture_of(&data, &index.studies[0].series[0], state.slot_index(&data));
        draw(&mut state.measurements, MeasureKind::Length, &picture, (1.0, 1.0), (9.0, 1.0));
        assert!(!state.is_pristine(&data), "a view with measurements is in use");
        state.measurements.begin(MeasureKind::Length, &picture, (5.0, 5.0));
        state.reset(&data);
        assert_eq!(state.measurements.len(), 1, "reset does not throw away what was measured");
        assert!(!state.measurements.is_busy());
        state.measurements.clear();
        state.reset(&data);
        assert!(state.is_pristine(&data));
    }
}
