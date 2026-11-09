//! The viewports on screen: which series and which view each shows, which one
//! is active, how they follow each other and how many images each shows at
//! once (`openspec/changes/series-comparison/design.md`).
//!
//! The Images, MPR, 2 × 2 and 3D layouts are presets: a grid filled with
//! viewports of the one active series. A multi-series [`Grid`] shows acquired
//! images of any series, each in a viewport of its own. What belongs to a
//! series (contrast, position, zoom, pan, turn and mirror) stays in its
//! [`ViewState`], so two viewports of one series with one view show the same
//! thing. Nothing here is saved.
//!
//! Linked viewports follow by patient position: only header geometry and the
//! image set shown count, so this works while a series still loads, and image
//! numbers play no part.

use std::collections::HashMap;

use summrise_i18n::tr;
use summrise_imaging::crossref::{self, LINK_ANGLE_DEGREES, NearestSlice};
use summrise_imaging::{DVec3, ImagePlane};

use crate::series::SeriesData;
use crate::session::{SeriesKey, Session};
use crate::view::{Layout, ViewKind, ViewState};

mod studies;
pub use studies::{comparable_studies, matching_series, same_patient};

/// The view state of every series.
pub type Views = HashMap<SeriesKey, ViewState>;

/// An arrangement of viewports for several series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Grid {
    SideBySide,
    Stacked,
    Four,
}

impl Grid {
    pub const ALL: [Self; 3] = [Self::SideBySide, Self::Stacked, Self::Four];

    /// Number of viewports.
    pub fn count(self) -> usize {
        match self {
            Self::SideBySide | Self::Stacked => 2,
            Self::Four => 4,
        }
    }

    /// Columns and rows.
    pub fn shape(self) -> (usize, usize) {
        match self {
            Self::SideBySide => (2, 1),
            Self::Stacked => (1, 2),
            Self::Four => (2, 2),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            // Translators: layouts of the Compare menu: how many series are shown at once and how they are arranged.
            Self::SideBySide => tr!("Two series side by side"),
            Self::Stacked => tr!("Two series, one above the other"),
            Self::Four => tr!("Four series"),
        }
    }
}

/// How many images a viewport of acquired images shows at once: a square grid
/// of tiles, 1 × 1 up to 5 × 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tiles {
    side: u8,
}

impl Tiles {
    pub const ONE: Self = Self { side: 1 };
    pub const ALL: [Self; 5] =
        [Self { side: 1 }, Self { side: 2 }, Self { side: 3 }, Self { side: 4 }, Self { side: 5 }];
    /// A grid whose tiles would be narrower or lower than this (points) is not offered.
    pub const MIN_TILE_POINTS: f32 = 96.0;
    /// Space between tiles, in points.
    pub const GAP_POINTS: f32 = 2.0;

    /// Tiles per row and per column.
    pub fn side(self) -> usize {
        usize::from(self.side)
    }

    /// Tiles in the grid.
    pub fn count(self) -> usize {
        self.side() * self.side()
    }

    /// "3 × 3".
    pub fn label(self) -> String {
        format!("{0} × {0}", self.side)
    }

    /// Whether the tiles stay large enough in an area of `size` points. One
    /// image always fits.
    pub fn fits(self, size: [f32; 2]) -> bool {
        let side = f32::from(self.side);
        let tile = |length: f32| (length - Self::GAP_POINTS * (side - 1.0)) / side;
        self == Self::ONE || (tile(size[0]) >= Self::MIN_TILE_POINTS && tile(size[1]) >= Self::MIN_TILE_POINTS)
    }

    /// This grid if it fits in `size`, else the largest smaller one that does.
    pub fn largest_fitting(self, size: [f32; 2]) -> Self {
        Self::ALL.into_iter().rev().find(|t| t.side <= self.side && t.fits(size)).unwrap_or(Self::ONE)
    }

    /// The next larger grid that fits in `size`, after the largest one back to a single image.
    pub fn next_fitting(self, size: [f32; 2]) -> Self {
        Self::ALL.into_iter().find(|t| t.side > self.side && t.fits(size)).unwrap_or(Self::ONE)
    }

    /// What each tile shows, in reading order, of an image set of `len`
    /// images with the current image at `first` (all counted from 0): the
    /// first tile the current image, the following ones the next images,
    /// `None` past the last image.
    pub fn images(self, first: usize, len: usize) -> impl Iterator<Item = Option<usize>> {
        (0..self.count()).map(move |tile| Some(first + tile).filter(|&image| image < len))
    }
}

/// The manual link of a viewport: equal distances from where it was linked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ManualLink {
    /// The direction in which distances count, the same for every viewport of the link.
    normal: DVec3,
    /// Position (mm along `normal`) of the slice shown when it was linked.
    origin_mm: f64,
}

/// Whether a viewport follows the active one.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Link {
    /// Follows by patient position, when the series allow it.
    #[default]
    Automatic,
    /// Follows by equal distances, from the offset at the moment of linking.
    Manual(ManualLink),
    /// Switched off by the user.
    Off,
}

/// How two viewports are linked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Position,
    Manual,
}

/// Why a viewport with its link on follows nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotLinked {
    /// There is no other series to follow.
    Alone,
    /// The series do not record the same Frame of Reference.
    NoCommonFrame,
    /// The slice directions differ by 30° or more.
    OtherDirection,
    /// The images do not say where they are.
    NoPosition,
    /// The link of the viewport it would follow is switched off.
    OtherLinkOff,
}

/// What the link indicator of a viewport says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStatus {
    Linked,
    Manual,
    Off,
    NotLinked(NotLinked),
}

/// A change of zoom or pan by the user, to be followed by linked viewports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    /// Zoom by `factor` about `anchor` (relative to the centre of the picture, in points).
    Zoom { factor: f32, anchor: [f32; 2] },
    /// The picture moved by this distance on screen, in points.
    Pan { delta: [f32; 2] },
}

/// One viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// The series it shows; `None`: it waits for one.
    pub series: Option<SeriesKey>,
    pub view: ViewKind,
    pub link: Link,
    pub tiles: Tiles,
}

impl Viewport {
    fn new(series: Option<SeriesKey>, view: ViewKind) -> Self {
        Self { series, view, link: Link::Automatic, tiles: Tiles::ONE }
    }
}

/// The viewports on screen.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// The multi-series grid; `None`: a preset of one series.
    grid: Option<Grid>,
    viewports: Vec<Viewport>,
    active: usize,
    /// Centre of the image each viewport showed at the end of the last frame.
    positions: Vec<Option<DVec3>>,
    /// Viewports to bring to the position of another one (target, from), as
    /// soon as the view of the target exists.
    aligning: Vec<(usize, usize)>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            grid: None,
            viewports: vec![Viewport::new(None, ViewKind::Stack)],
            active: 0,
            positions: vec![None],
            aligning: Vec::new(),
        }
    }
}

/// The patient-space plane of the image a viewport shows, from the headers.
fn current_plane(viewport: &Viewport, session: &Session, views: &Views) -> Option<ImagePlane> {
    let key = viewport.series?;
    let data = session.data(key)?;
    // Before its first image is decoded, the image a series will open on.
    let slot = views.get(&key).map_or_else(|| data.initial_index(), |state| state.slot_index(&data));
    data.frames().get(slot)?.plane
}

/// The planes of the image set a series shows, except those whose direction
/// differs by [`LINK_ANGLE_DEGREES`] or more from `direction` (the other
/// planes of a three-plane localizer): one per image of the set.
fn slices_of(series: &SeriesData, state: Option<&ViewState>, direction: DVec3) -> Vec<Option<ImagePlane>> {
    let len = series.stack_len();
    let stack = state.map_or(0, ViewState::stack);
    let along = |plane: &ImagePlane| crossref::slice_angle_degrees(plane.normal(), direction) < LINK_ANGLE_DEGREES;
    series.frames().iter().skip(stack * len).take(len).map(|frame| frame.plane.filter(along)).collect()
}

fn same_frame_of_reference(session: &Session, a: SeriesKey, b: SeriesKey) -> bool {
    let frame = |key| session.series(key).and_then(|series| series.frame_of_reference.as_deref());
    frame(a).is_some() && frame(a) == frame(b)
}

impl Workspace {
    /// The multi-series grid, if one is on screen.
    pub fn grid(&self) -> Option<Grid> {
        self.grid
    }

    /// Whether a multi-series grid is on screen.
    pub fn is_comparing(&self) -> bool {
        self.grid.is_some()
    }

    pub fn viewports(&self) -> &[Viewport] {
        &self.viewports
    }

    /// The index of the active viewport.
    pub fn active(&self) -> usize {
        self.active
    }

    pub fn active_viewport(&self) -> Option<&Viewport> {
        self.viewports.get(self.active)
    }

    /// The series on screen, each once.
    pub fn shown_series(&self) -> Vec<SeriesKey> {
        let mut keys: Vec<SeriesKey> = Vec::with_capacity(self.viewports.len());
        for key in self.viewports.iter().filter_map(|viewport| viewport.series) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }

    /// Once per frame while a preset is shown: its viewports, one per view of
    /// `layout`, all of `series`, the one of `active_view` active. A tile grid
    /// stays with the acquired images, also when the series changes. Does
    /// nothing while a multi-series grid is shown.
    pub fn show_preset(&mut self, layout: Layout, series: Option<SeriesKey>, active_view: ViewKind) {
        if self.grid.is_some() {
            return;
        }
        let views = layout.views();
        if !self.viewports.iter().map(|viewport| viewport.view).eq(views.iter().copied()) {
            let tiles = self.viewports.iter().find(|v| v.view == ViewKind::Stack).map_or(Tiles::ONE, |v| v.tiles);
            self.viewports = views
                .iter()
                .map(|&view| Viewport {
                    tiles: if view == ViewKind::Stack { tiles } else { Tiles::ONE },
                    ..Viewport::new(series, view)
                })
                .collect();
            self.positions = vec![None; views.len()];
        }
        for viewport in &mut self.viewports {
            viewport.series = series;
        }
        self.active = views.iter().position(|&view| view == active_view).unwrap_or(0);
    }

    /// Shows `grid`. Coming from a preset, the first viewport keeps the series
    /// on screen and its tile grid. Viewports beyond the new grid go; if the
    /// active one is among them, the first becomes active. Empty viewports
    /// wait for a series.
    pub fn set_grid(&mut self, grid: Grid, active_series: Option<SeriesKey>) {
        let tiles = self.viewports.iter().find(|v| v.view == ViewKind::Stack).map_or(Tiles::ONE, |v| v.tiles);
        let mut viewports = if self.grid.is_some() {
            std::mem::take(&mut self.viewports)
        } else {
            self.active = 0;
            vec![Viewport { tiles, ..Viewport::new(active_series, ViewKind::Stack) }]
        };
        viewports.resize(grid.count(), Viewport::new(None, ViewKind::Stack));
        if self.active >= viewports.len() {
            self.active = 0;
        }
        self.viewports = viewports;
        self.positions.resize(self.viewports.len(), None);
        self.aligning.retain(|&(target, from)| target < self.viewports.len() && from < self.viewports.len());
        self.grid = Some(grid);
    }

    /// Leaves the multi-series grid for the preset [`Self::show_preset`] says.
    pub fn leave_grid(&mut self) {
        if self.grid.take().is_some() {
            // The tile grid of the active viewport goes with its series.
            let tiles = self.viewports.get(self.active).map_or(Tiles::ONE, |viewport| viewport.tiles);
            self.viewports = vec![Viewport { tiles, ..Viewport::new(None, ViewKind::Stack) }];
            self.positions = vec![None];
            self.aligning.clear();
            self.active = 0;
        }
    }

    /// Two series side by side, the first one active: what "Compare studies" opens.
    pub fn show_pair(&mut self, left: SeriesKey, right: SeriesKey) {
        let viewport = |key| Viewport::new(Some(key), ViewKind::Stack);
        self.grid = Some(Grid::SideBySide);
        self.viewports = vec![viewport(left), viewport(right)];
        self.positions = vec![None; 2];
        self.aligning.clear();
        self.active = 0;
    }

    /// Puts `key` into viewport `index` with a fresh link. It opens at the
    /// position of the viewport beside it, if the two are linked.
    pub fn assign(&mut self, index: usize, key: SeriesKey) {
        let Some(viewport) = self.viewports.get_mut(index) else { return };
        if viewport.series == Some(key) {
            return;
        }
        viewport.series = Some(key);
        viewport.link = Link::Automatic;
        self.positions[index] = None;
        self.aligning.retain(|&(target, _)| target != index);
        // The active viewport if that is another one, else any other that shows a series.
        let others = std::iter::once(self.active).chain(0..self.viewports.len());
        if let Some(from) = others.into_iter().find(|&i| i != index && self.viewports[i].series.is_some()) {
            self.aligning.push((index, from));
        }
    }

    /// Follows the session: the active viewport of a multi-series grid shows
    /// the series the session has as the active one, however it was chosen.
    pub fn adopt(&mut self, active_series: Option<SeriesKey>) {
        if self.grid.is_some()
            && let Some(key) = active_series
        {
            self.assign(self.active, key);
        }
    }

    /// Makes viewport `index` active and returns what it shows.
    pub fn activate(&mut self, index: usize) -> Option<SeriesKey> {
        if index < self.viewports.len() {
            self.active = index;
        }
        self.viewports.get(self.active)?.series
    }

    /// The viewport after the active one, in reading order.
    pub fn next_viewport(&self) -> usize {
        (self.active + 1) % self.viewports.len().max(1)
    }

    /// The tile grid of viewport `index`.
    pub fn set_tiles(&mut self, index: usize, tiles: Tiles) {
        if let Some(viewport) = self.viewports.get_mut(index)
            && viewport.view == ViewKind::Stack
        {
            viewport.tiles = tiles;
        }
    }

    // -----------------------------------------------------------------------
    // Links

    /// Switches the link of viewport `index` off, or back on: then it goes to
    /// the position of the active viewport, or, if it is the active one, the
    /// others come to its position.
    pub fn toggle_link(&mut self, index: usize) {
        let active = self.active;
        let Some(viewport) = self.viewports.get_mut(index) else { return };
        viewport.link = if viewport.link == Link::Off { Link::Automatic } else { Link::Off };
        if viewport.link == Link::Automatic {
            if index == active {
                let others = (0..self.viewports.len()).filter(|&other| other != active);
                self.aligning.extend(others.map(|other| (other, active)));
            } else {
                self.aligning.push((index, active));
            }
        }
    }

    /// The viewports "link from here" would link to the active one: those
    /// with their link on that show another series in a similar slice direction.
    fn manual_partners(&self, session: &Session, views: &Views) -> Vec<(usize, ImagePlane)> {
        let Some(active) = self.active_viewport() else { return Vec::new() };
        let Some(reference) = current_plane(active, session, views) else { return Vec::new() };
        (0..self.viewports.len())
            .filter(|&i| i != self.active)
            .filter(|&i| self.viewports[i].link != Link::Off && self.viewports[i].series != active.series)
            .filter_map(|i| Some((i, current_plane(&self.viewports[i], session, views)?)))
            .filter(|(_, plane)| crossref::slice_angle_degrees(plane.normal(), reference.normal()) < LINK_ANGLE_DEGREES)
            .collect()
    }

    /// Whether "link from here" has anything to link.
    pub fn can_link_from_here(&self, session: &Session, views: &Views) -> bool {
        self.grid.is_some() && !self.manual_partners(session, views).is_empty()
    }

    /// "Link from here": the viewports keep their present offset from the
    /// active one and move by equal distances from now on, in the same
    /// direction of the patient.
    pub fn link_from_here(&mut self, session: &Session, views: &Views) {
        let partners = self.manual_partners(session, views);
        let Some(active) = self.active_viewport() else { return };
        let Some(reference) = current_plane(active, session, views) else { return };
        if partners.is_empty() {
            return;
        }
        let direction = reference.normal();
        let manual = |plane: &ImagePlane| {
            let own = plane.normal();
            let normal = if own.dot(direction) < 0.0 { -own } else { own };
            Link::Manual(ManualLink { normal, origin_mm: plane.position.dot(normal) })
        };
        for (index, plane) in &partners {
            self.viewports[*index].link = manual(plane);
        }
        self.viewports[self.active].link = manual(&reference);
    }

    /// Whether any viewport has a manual link.
    pub fn has_manual_links(&self) -> bool {
        self.viewports.iter().any(|viewport| matches!(viewport.link, Link::Manual(_)))
    }

    /// Back from manual links to following by patient position.
    pub fn automatic_links(&mut self) {
        for viewport in &mut self.viewports {
            if matches!(viewport.link, Link::Manual(_)) {
                viewport.link = Link::Automatic;
            }
        }
    }

    /// How viewports `a` and `b` are linked, if they are: both with their link
    /// on, and either both manual or in one Frame of Reference with slice
    /// directions less than 30° apart.
    pub fn link_between(&self, a: usize, b: usize, session: &Session, views: &Views) -> Option<LinkKind> {
        let (first, second) = (self.viewports.get(a)?, self.viewports.get(b)?);
        if a == b || self.grid.is_none() || first.link == Link::Off || second.link == Link::Off {
            return None;
        }
        let (key_a, key_b) = (first.series?, second.series?);
        if matches!((first.link, second.link), (Link::Manual(_), Link::Manual(_))) {
            return Some(LinkKind::Manual);
        }
        if !same_frame_of_reference(session, key_a, key_b) {
            return None;
        }
        let (plane_a, plane_b) = (current_plane(first, session, views)?, current_plane(second, session, views)?);
        (crossref::slice_angle_degrees(plane_a.normal(), plane_b.normal()) < LINK_ANGLE_DEGREES)
            .then_some(LinkKind::Position)
    }

    /// What the link indicator of viewport `index` says.
    pub fn link_status(&self, index: usize, session: &Session, views: &Views) -> LinkStatus {
        let Some(viewport) = self.viewports.get(index) else { return LinkStatus::Off };
        match viewport.link {
            Link::Off => LinkStatus::Off,
            Link::Manual(_) => LinkStatus::Manual,
            Link::Automatic if index == self.active => {
                let linked = (0..self.viewports.len()).any(|i| self.link_between(i, index, session, views).is_some());
                if linked {
                    LinkStatus::Linked
                } else {
                    LinkStatus::NotLinked(self.why_not_linked(index, session, views))
                }
            }
            Link::Automatic => match self.link_between(self.active, index, session, views) {
                Some(_) => LinkStatus::Linked,
                None => LinkStatus::NotLinked(self.why_not_linked(index, session, views)),
            },
        }
    }

    /// Why viewport `index`, whose link is on, follows nothing: compared with
    /// the active one, or for the active one with the first other series.
    fn why_not_linked(&self, index: usize, session: &Session, views: &Views) -> NotLinked {
        let other = if index == self.active {
            (0..self.viewports.len()).find(|&i| i != index && self.viewports[i].series.is_some())
        } else {
            Some(self.active).filter(|&i| self.viewports[i].series.is_some())
        };
        let (Some(other), Some(own)) = (other, self.viewports[index].series) else { return NotLinked::Alone };
        let Some(key) = self.viewports[other].series else { return NotLinked::Alone };
        if self.viewports[other].link == Link::Off {
            return NotLinked::OtherLinkOff;
        }
        if !same_frame_of_reference(session, own, key) {
            return NotLinked::NoCommonFrame;
        }
        match (
            current_plane(&self.viewports[index], session, views),
            current_plane(&self.viewports[other], session, views),
        ) {
            (Some(_), Some(_)) => NotLinked::OtherDirection,
            _ => NotLinked::NoPosition,
        }
    }

    /// The patient position viewport `to` should show given what viewport
    /// `from` shows.
    fn target_of(&self, from: usize, to: usize, kind: LinkKind, session: &Session, views: &Views) -> Option<DVec3> {
        let plane = current_plane(self.viewports.get(from)?, session, views)?;
        match kind {
            LinkKind::Position => Some(crossref::centre(&plane)),
            LinkKind::Manual => {
                let Link::Manual(source) = self.viewports[from].link else { return None };
                let Link::Manual(follower) = self.viewports[to].link else { return None };
                let moved = plane.position.dot(source.normal) - source.origin_mm;
                let anchor = current_plane(&self.viewports[to], session, views)?.position;
                let target = follower.origin_mm + moved;
                Some(anchor + follower.normal * (target - anchor.dot(follower.normal)))
            }
        }
    }

    /// The slice viewport `to` should show given what viewport `from` shows.
    fn nearest_for(&self, from: usize, to: usize, session: &Session, views: &Views) -> Option<NearestSlice> {
        let kind = self.link_between(from, to, session, views)?;
        let key = self.viewports[to].series?;
        let data = session.data(key)?;
        let direction = current_plane(&self.viewports[from], session, views)?.normal();
        let slices = slices_of(&data, views.get(&key), direction);
        crossref::nearest_slice(&slices, self.target_of(from, to, kind, session, views)?)
    }

    /// Whether the position of the active viewport lies beyond what viewport
    /// `index`, linked to it, covers; that viewport then stays on its nearest
    /// end image.
    pub fn is_outside_range(&self, index: usize, session: &Session, views: &Views) -> bool {
        self.nearest_for(self.active, index, session, views).is_some_and(|nearest| nearest.outside)
    }

    /// The plane of the active viewport's current image, for viewport `index`
    /// to draw where it cuts its images: for another viewport of the same
    /// Frame of Reference. (Whether there is a line depends on each image.)
    pub fn cross_reference(&self, index: usize, session: &Session, views: &Views) -> Option<ImagePlane> {
        let active = self.active_viewport()?;
        let other = self.viewports.get(index)?;
        if index == self.active || self.grid.is_none() {
            return None;
        }
        same_frame_of_reference(session, active.series?, other.series?)
            .then(|| current_plane(active, session, views))
            .flatten()
    }

    /// Once per frame, after what the user did was applied to the views:
    /// when the image of the active viewport moved, the viewports linked to
    /// it go to the slice nearest to its position. Switching the active
    /// viewport moves nothing. Returns whether any view moved.
    pub fn follow(&mut self, session: &Session, views: &mut Views) -> bool {
        if self.grid.is_none() {
            return false;
        }
        let mut moved = false;
        for (target, from) in std::mem::take(&mut self.aligning) {
            match self.bring(from, target, session, views) {
                Bring::Moved => moved = true,
                Bring::NoView => self.aligning.push((target, from)),
                Bring::Nothing => {}
            }
        }
        let now = self.position_of(self.active, session, views);
        let was = self.positions.get(self.active).copied().flatten();
        if now.is_some() && was.is_some() && now != was {
            for other in (0..self.viewports.len()).filter(|&other| other != self.active) {
                match self.bring(self.active, other, session, views) {
                    Bring::Moved => moved = true,
                    Bring::NoView => self.aligning.push((other, self.active)),
                    Bring::Nothing => {}
                }
            }
        }
        for index in 0..self.viewports.len() {
            self.positions[index] = self.position_of(index, session, views);
        }
        moved
    }

    fn position_of(&self, index: usize, session: &Session, views: &Views) -> Option<DVec3> {
        Some(crossref::centre(&current_plane(self.viewports.get(index)?, session, views)?))
    }

    /// Takes viewport `to` to the slice nearest to what viewport `from` shows.
    fn bring(&self, from: usize, to: usize, session: &Session, views: &mut Views) -> Bring {
        let (Some(key_from), Some(key_to)) =
            (self.viewports.get(from).and_then(|v| v.series), self.viewports.get(to).and_then(|v| v.series))
        else {
            return Bring::Nothing;
        };
        if key_from == key_to {
            return Bring::Nothing;
        }
        let Some(nearest) = self.nearest_for(from, to, session, views) else { return Bring::Nothing };
        let (Some(data), Some(state)) = (session.data(key_to), views.get_mut(&key_to)) else {
            return Bring::NoView;
        };
        if state.slice_index(ViewKind::Stack, &data) == nearest.index {
            return Bring::Nothing;
        }
        state.set_slice(ViewKind::Stack, &data, nearest.index);
        Bring::Moved
    }

    /// Applies a zoom or pan the user made in viewport `source` to the
    /// viewports linked to it. The link is relative: each keeps whatever
    /// difference it had. A series shown twice moves once.
    pub fn follow_gesture(&self, source: usize, gesture: Gesture, session: &Session, views: &mut Views) {
        let Some(own) = self.viewports.get(source).and_then(|viewport| viewport.series) else { return };
        let mut done = vec![own];
        for other in 0..self.viewports.len() {
            if self.link_between(source, other, session, views).is_none() {
                continue;
            }
            let Some(key) = self.viewports[other].series.filter(|key| !done.contains(key)) else { continue };
            done.push(key);
            let Some(state) = views.get_mut(&key) else { continue };
            let transform = state.transform_mut(ViewKind::Stack);
            match gesture {
                Gesture::Zoom { factor, anchor } => transform.zoom_about(factor, anchor),
                Gesture::Pan { delta } => transform.pan = [transform.pan[0] + delta[0], transform.pan[1] + delta[1]],
            }
        }
    }
}

enum Bring {
    Moved,
    /// The target has no view yet: its series has no decoded image.
    NoView,
    Nothing,
}

#[cfg(test)]
mod tests;
