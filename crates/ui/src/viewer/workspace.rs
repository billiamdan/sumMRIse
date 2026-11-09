//! The viewer's side of the viewports: keeping the workspace and the session
//! in step, carrying out what is chosen in the Compare menu or with its keys,
//! and what a viewport without a picture does.

use egui::{Id, Rect, Ui};
use summrise_core::dicom::format::number;
use summrise_core::workspace::matching_series;
use summrise_core::{Layout, OverBudget, SeriesKey, Tiles, ViewKind};
use summrise_i18n::tr;

use super::Viewer;
use crate::app::Toasts;
use crate::compare::{self, Command, ViewportInfo};
use crate::tiles;
use crate::viewport::ViewportResult;

/// A viewport as it was drawn in the last frame.
#[derive(Debug, Clone, Copy)]
pub(super) struct DrawnCell {
    pub rect: Rect,
    /// The tile grid it showed.
    pub tiles: Tiles,
}

/// What is said when the series on screen exceed the memory limit.
pub(super) fn memory_notice(over: OverBudget) -> String {
    let megabytes = |bytes: usize| number(bytes as f64 / f64::from(1u32 << 20), 1);
    tr!(
        "The series on screen need about {needed} MB together, more than the memory limit of {limit} MB. They stay loaded; the limit can be raised in Settings.",
        needed = megabytes(over.needed_bytes),
        limit = megabytes(over.budget_bytes)
    )
}

impl Viewer {
    /// Once per frame, before anything is drawn: the active viewport follows
    /// the series selected, the session keeps what is on screen loaded and
    /// the render cache lets go of what is not.
    pub(super) fn prepare_workspace(&mut self, toasts: &mut Toasts) {
        self.workspace.adopt(self.session.active_series);
        let comparing = self.workspace.is_comparing();
        let shown = if comparing { self.workspace.shown_series() } else { Vec::new() };
        self.session.set_shown(&shown);
        let over = self.session.over_budget();
        if let Some(notice) = over.filter(|_| self.over_budget.is_none()) {
            toasts.warn(memory_notice(notice));
        }
        self.over_budget = over;
        let active = self.session.active_series;
        let keep = if comparing { shown } else { active.into_iter().collect() };
        self.renders.retain_series(&keep);
        self.volume_view.retain_series(active);
    }

    /// The index of the active viewport.
    pub(super) fn active_index(&self) -> usize {
        if self.workspace.is_comparing() {
            self.workspace.active()
        } else {
            self.workspace.viewports().iter().position(|viewport| viewport.view == self.active_view).unwrap_or(0)
        }
    }

    /// The tile grid of the active viewport, as it was last drawn.
    pub(super) fn shown_tiles(&self) -> Tiles {
        self.cells.get(self.active_index()).map_or(Tiles::ONE, |cell| cell.tiles)
    }

    /// How far Page Up and Page Down go: a whole grid, or ten images.
    pub(super) fn page_step(&self) -> i64 {
        self.cells.get(self.active_index()).map_or(10, |cell| match cell.tiles {
            Tiles::ONE => 10,
            tiles => tiles.count() as i64,
        })
    }

    /// The size of the area the tiles of the active viewport share, if it
    /// shows acquired images of a series of more than one image.
    fn active_tile_area(&self) -> Option<[f32; 2]> {
        let index = self.active_index();
        let viewport = self.workspace.viewports().get(index)?;
        let series = self.session.data(viewport.series?)?;
        if viewport.view != ViewKind::Stack || series.stack_len() < 2 {
            return None;
        }
        let multi_view = self.workspace.viewports().len() > 1;
        let size = tiles::area(self.cells.get(index)?.rect, multi_view).size();
        Some([size.x, size.y])
    }

    /// The Compare menu of the toolbar.
    pub(super) fn compare_menu(&mut self, ui: &mut Ui) {
        let tile_area = self.active_tile_area();
        let tiles = self.cells.get(self.active_index()).map_or(Tiles::ONE, |cell| cell.tiles);
        let input = compare::MenuInput {
            workspace: &self.workspace,
            session: &self.session,
            views: &self.views,
            central: self.central,
            tile_area,
            tiles,
        };
        if let Some(command) = compare::menu(ui, &input) {
            self.run_command(command);
        }
    }

    /// Carries out what is chosen in the Compare menu or with its keys.
    pub(super) fn run_command(&mut self, command: Command) {
        match command {
            Command::Grid(None) => self.use_preset(Layout::Single),
            Command::Grid(Some(grid)) => {
                self.layout = Layout::Single;
                self.active_view = ViewKind::Stack;
                self.workspace.set_grid(grid, self.session.active_series);
                self.follow_active_viewport();
            }
            Command::Tiles(tiles) => self.workspace.set_tiles(self.active_index(), tiles),
            Command::NextTiles => {
                let index = self.active_index();
                if let (Some(area), Some(cell)) = (self.active_tile_area(), self.cells.get(index)) {
                    self.workspace.set_tiles(index, cell.tiles.next_fitting(area));
                }
            }
            Command::ToggleLink => self.workspace.toggle_link(self.active_index()),
            Command::LinkFromHere => self.workspace.link_from_here(&self.session, &self.views),
            Command::AutomaticLinks => self.workspace.automatic_links(),
            Command::NextViewport => self.activate_viewport(self.workspace.next_viewport()),
            Command::CompareStudies { earlier, later } => self.compare_studies(earlier, later),
        }
    }

    /// Back to a layout of the one series on screen in the active viewport,
    /// or, if that was empty, of the first on screen.
    pub(super) fn use_preset(&mut self, layout: Layout) {
        self.layout = layout;
        if self.workspace.is_comparing() {
            let first = self.workspace.shown_series().first().copied();
            self.workspace.leave_grid();
            if self.session.active_series.is_none()
                && let Some(key) = first.or_else(|| self.session.default_series(self.session.active_study))
            {
                self.session.select_series(key);
            }
        }
    }

    /// The matching series of two studies side by side, the earlier study on
    /// the left; each study's default series when no series match.
    fn compare_studies(&mut self, earlier: usize, later: usize) {
        let matching = matching_series(self.session.studies(), earlier, later, self.session.active_series);
        let defaults = || Some((self.session.default_series(earlier)?, self.session.default_series(later)?));
        let Some((left, right)) = matching.or_else(defaults) else { return };
        self.layout = Layout::Single;
        self.active_view = ViewKind::Stack;
        self.workspace.show_pair(left, right);
        self.session.select_series(left);
    }

    /// Makes viewport `index` the active one: its series and its view are
    /// what the toolbar, the keys, the inspector and the guide act on.
    pub(super) fn activate_viewport(&mut self, index: usize) {
        if self.workspace.is_comparing() {
            self.workspace.activate(index);
            self.follow_active_viewport();
        } else if let Some(viewport) = self.workspace.viewports().get(index) {
            self.active_view = viewport.view;
        }
    }

    /// The session takes the series of the active viewport as the active one.
    fn follow_active_viewport(&mut self) {
        match self.workspace.active_viewport().and_then(|viewport| viewport.series) {
            Some(key) if self.session.active_series != Some(key) => self.session.select_series(key),
            Some(_) => {}
            None => self.session.deselect(),
        }
    }

    /// A viewport of a multi-series layout without a picture: waiting for a
    /// series, or for the first image of its series. Clicking it makes it the
    /// active one.
    pub(super) fn waiting_viewport(&mut self, ui: &mut Ui, rect: Rect, id: Id, index: usize, key: Option<SeriesKey>) {
        if !self.workspace.is_comparing() {
            self.placeholder(ui, rect, key);
            return;
        }
        let is_active = self.workspace.active() == index;
        let mut result = ViewportResult::default();
        let response = match key {
            None => compare::empty_viewport(ui, rect, id, is_active),
            Some(key) => {
                let response = ui.interact(rect, id, egui::Sense::click());
                self.placeholder(ui, rect, Some(key));
                let painter = ui.painter_at(rect);
                if let (Some(meta), Some(study)) = (self.session.series(key), self.session.studies().get(key.study)) {
                    let info = ViewportInfo::of(&self.workspace, index, &self.session, &self.views, meta, study);
                    let color = crate::theme::view_color(ViewKind::Stack);
                    compare::header(ui, &painter, rect, id, &info, color, &mut result);
                }
                if is_active {
                    compare::highlight(ui, &painter, rect);
                }
                response
            }
        };
        if result.toggle_link {
            self.workspace.toggle_link(index);
        }
        if response.clicked() {
            self.activate_viewport(index);
        }
        self.take_drop(ui, rect, id, index);
    }

    /// Viewport `index` as a drop target: a series dropped on it goes there,
    /// and it becomes the active one.
    pub(super) fn take_drop(&mut self, ui: &Ui, rect: Rect, id: Id, index: usize) {
        if let Some(key) = compare::drop_target(ui, rect, id) {
            self.workspace.assign(index, key);
            self.activate_viewport(index);
        }
    }
}
