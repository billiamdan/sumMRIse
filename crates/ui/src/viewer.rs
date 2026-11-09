//! The study viewer: toolbar, series browser, viewport grid, inspector and
//! status bar.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{
    Align, Color32, ComboBox, DragValue, Frame, Key, Label, Layout as ELayout, Margin, Modifiers, Panel, Rect,
    RichText, Slider, Ui, vec2,
};
use egui_material_icons::icons::{
    ICON_BADGE, ICON_CLOSE, ICON_CONTRAST, ICON_FOLDER_OPEN, ICON_INFO, ICON_INVERT_COLORS, ICON_LEFT_PANEL_CLOSE,
    ICON_LEFT_PANEL_OPEN, ICON_PAN_TOOL, ICON_RESTART_ALT, ICON_RIGHT_PANEL_CLOSE, ICON_RIGHT_PANEL_OPEN,
    ICON_SETTINGS, ICON_VISIBILITY, ICON_VISIBILITY_OFF, ICON_WARNING, ICON_ZOOM_IN,
};
use summrise_core::dicom::{DiscoverySource, MediaKind, format};
use summrise_core::imaging::{PresetSource, SlabMode};
use summrise_core::{
    ContrastChoice, DisplayOrientation, Layout, OverBudget, SeriesData, SeriesKey, SeriesState, Session, SessionEvent,
    Settings, Tiles, Tool, ViewKind, ViewState, Waker, Workspace,
};
use summrise_i18n::{tr, tr_ctx, tr_n};

use crate::app::{Action, Toasts};
use crate::browser::{self, ThumbnailTextures};
use crate::cine::{self, Cine};
use crate::documents::{self, DocumentPane};
use crate::fidelity;
use crate::guide::{self, PresetHint as _};
use crate::inspector::Inspector;
use crate::orient;
use crate::render::RenderCache;
use crate::theme;
use crate::viewport::{self, Hover, ViewportInput};
use crate::volume_view::{self, VolumeView};
use crate::widgets::{command_key, icon, keep_together, shortcut};

pub(crate) struct Viewer {
    pub session: Session,
    views: HashMap<SeriesKey, ViewState>,
    layout: Layout,
    tool: Tool,
    active_view: ViewKind,
    renders: RenderCache,
    volume_view: VolumeView,
    thumbs: ThumbnailTextures,
    inspector: Inspector,
    hover: Option<(ViewKind, Hover)>,
    slab_thickness: f64,
    /// Playback of images or stacks as a loop (never saved).
    cine: Cine,
    /// A CT or MR asked for under a PET, until its view exists (see `pet`).
    fusion_pending: crate::pet::Pending,
    /// The viewports on screen (see `workspace`, never saved).
    workspace: Workspace,
    /// Where the viewports were drawn in the last frame, altogether and one by one.
    central: Rect,
    cells: Vec<workspace::DrawnCell>,
    /// The series on screen need more memory than the limit.
    over_budget: Option<OverBudget>,
    /// Where cross-reference lines were drawn in the last frame, per viewport (for tests).
    #[cfg(test)]
    cross_lines: HashMap<usize, Vec<[egui::Pos2; 2]>>,
    /// The report or document shown instead of the images, if any.
    documents: DocumentPane,
    /// Saving, copying, exporting and printing (see `export`).
    export: crate::export::Export,
}

impl Viewer {
    pub(crate) fn new(session: Session, settings: &Settings) -> Self {
        Self {
            session,
            views: HashMap::new(),
            layout: settings.default_layout,
            tool: Tool::WindowLevel,
            active_view: ViewKind::Stack,
            renders: RenderCache::default(),
            volume_view: VolumeView::default(),
            thumbs: ThumbnailTextures::default(),
            inspector: Inspector::default(),
            hover: None,
            slab_thickness: 10.0,
            cine: Cine::default(),
            fusion_pending: None,
            workspace: Workspace::default(),
            central: Rect::NOTHING,
            cells: Vec::new(),
            over_budget: None,
            #[cfg(test)]
            cross_lines: HashMap::new(),
            documents: DocumentPane::default(),
            export: crate::export::Export::default(),
        }
    }

    /// Window title suffix.
    pub(crate) fn title(&self) -> String {
        self.session.study().title()
    }

    /// The displayed series, from the moment its first image is decoded.
    fn active_series(&self) -> Option<(SeriesKey, Arc<SeriesData>)> {
        let key = self.session.active_series?;
        Some((key, self.session.data(key).filter(|series| series.is_viewable())?))
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        settings: &mut Settings,
        waker: &Waker,
        actions: &mut Vec<Action>,
        toasts: &mut Toasts,
    ) {
        for event in self.session.poll() {
            match event {
                // The window, zoom and position a series opens with are fixed here, by its
                // first image; images arriving later never touch them.
                SessionEvent::SeriesOpened(key) => {
                    if let Some(series) = self.session.data(key) {
                        self.views.entry(key).or_insert_with(|| ViewState::new(&series));
                    }
                }
                SessionEvent::SeriesReady(key) => {
                    if let Some(series) = self.session.loaded(key) {
                        for w in series.warnings() {
                            toasts.warn(w.clone());
                        }
                        // The reformats are available from now on. A remembered reformat
                        // layout takes over by itself only if the user has not started to
                        // work with the acquired images; otherwise the display stays as
                        // it is until they pick a layout (the saved default is kept).
                        let in_use = self.views.get(&key).is_some_and(|view| !view.is_pristine(&series));
                        if in_use && self.layout.needs_volume() && self.session.active_series == Some(key) {
                            self.layout = Layout::Single;
                        }
                    }
                }
                SessionEvent::SeriesFailed(_, e) => toasts.error(tr!("Could not open series: {reason}", reason = e)),
                SessionEvent::ThumbnailReady(_) => {}
            }
        }
        if self.session.is_busy() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
        }
        // A link in a report goes back to the images, to the single view with that image.
        if self.documents.update(&mut self.session, &mut self.views, waker) {
            self.use_preset(Layout::Single);
        }
        let document_open = self.documents.is_open();
        self.prepare_workspace(toasts);
        let active = self.active_series();
        let has_volume = active.as_ref().is_some_and(|(_, s)| s.volume().is_some_and(|v| v.geometry.dims[2] >= 3));
        let layout = if has_volume { self.layout } else { Layout::Single };
        if !layout.views().contains(&self.active_view) {
            self.active_view = layout.views()[0];
        }
        // While a document is shown nothing acts on the images: ⌘C copies the text of the document.
        if !document_open {
            self.export.keys(ui.ctx());
        }
        let keys_for_images = !document_open && !self.export.is_open();
        let command = crate::compare::keys(ui.ctx(), self.workspace.is_comparing());
        if let Some(command) = command.filter(|_| keys_for_images) {
            self.run_command(command);
        }
        if let Some((key, series)) = active.as_ref().filter(|_| keys_for_images) {
            self.handle_keys(ui, *key, series, settings, has_volume);
        }
        // The window of the stack shown and playback follow the UI clock.
        match active.as_ref().and_then(|(key, series)| Some((*key, series, self.views.get_mut(key)?))) {
            Some((key, series, state)) => {
                state.settle_stack_window(series);
                // The picture stays put while a measurement is drawn on it.
                if state.measurements.is_busy() {
                    self.cine.stop();
                }
                let rate = self.session.series(key).map_or(cine::DEFAULT_RATE, |meta| self.cine.rate(key, meta));
                let now = ui.input(|i| i.time);
                if let Some(due) = self.cine.tick(now, key, layout.views(), series, state, rate) {
                    ui.ctx().request_repaint_after(due);
                }
            }
            None => self.cine.stop(),
        }
        if document_open {
            self.cine.stop();
        }
        // The PET drawn over the series shown, which the session keeps loaded.
        let overlay = crate::pet::sync(&mut self.session, &mut self.views, &mut self.fusion_pending);

        let p = theme::palette(ui);
        let reformat_blocker = reformat_blocker(self.session.active_series.and_then(|key| self.session.state(key)));
        Panel::top("toolbar")
            .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(10, 6)))
            .show(ui, |ui| self.toolbar(ui, settings, actions, active.as_ref(), has_volume, &reformat_blocker));
        Panel::bottom("status")
            .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(10, 3)))
            .show(ui, |ui| self.status_bar(ui, active.as_ref()));
        if settings.show_browser {
            let shown = self.workspace.is_comparing().then(|| self.workspace.shown_series());
            Panel::left("browser")
                .resizable(true)
                .default_size(270.0)
                .size_range(210.0..=420.0)
                .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(10, 6)))
                .show(ui, |ui| {
                    let (session, thumbs, documents) = (&mut self.session, &mut self.thumbs, &mut self.documents);
                    browser::show(ui, session, settings, thumbs, shown.as_deref(), documents, waker)
                });
        }
        if settings.show_inspector {
            let display = self.display_rows(active.as_ref());
            let path = active.as_ref().and_then(|(key, series)| {
                let state = self.views.get(key)?;
                let info = series.frames().get(state.slot_index(series))?;
                Some(self.session.series(*key)?.instances.get(info.instance)?.path.clone())
            });
            let camera = self.session.active_series.and_then(|key| self.volume_view.shown_axes(key, self.active_view));
            // The guide describes the picture as it is on screen: turned and mirrored like the view.
            let orientation = active
                .as_ref()
                .and_then(|(key, _)| self.views.get(key))
                .map_or(DisplayOrientation::STANDARD, |state| state.transform(self.active_view).orientation);
            let scene = guide::Scene::of(settings, &self.session, &self.renders, self.active_view, orientation)
                .map(|scene| scene.with_camera(camera));
            let uptake = active.as_ref().and_then(|(key, series)| {
                summrise_core::pet::uptake_section(self.session.series(*key)?, series, settings.hide_patient_details)
            });
            Panel::right("inspector")
                .resizable(true)
                .default_size(360.0)
                .size_range(260.0..=640.0)
                .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(10, 6)))
                .show(ui, |ui| {
                    let hide_patient = settings.hide_patient_details;
                    let file = path.as_deref().map(|path| (self.session.media(), path));
                    self.inspector.show(ui, waker, file, &display, hide_patient, scene.as_ref(), uptake.as_ref())
                });
        }
        // Under the images: the stack control and playback.
        if let Some((key, series)) = active.as_ref().filter(|(_, series)| series.len() > 1 && !document_open)
            && let (Some(state), Some(meta)) = (self.views.get_mut(key), self.session.series(*key))
        {
            let (cine, view, now) = (&mut self.cine, self.active_view, ui.input(|i| i.time));
            Panel::bottom("playback")
                .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(10, 4)))
                .show(ui, |ui| cine::bar(ui, cine, *key, meta, series, state, view, now));
        }
        let overlay = overlay.as_ref().map(|(key, data)| crate::pet::Overlay { key: *key, data });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if self.documents.is_open() {
                self.hover = None;
                self.documents.show(ui, &self.session, settings);
            } else {
                self.viewports(ui, settings, active.as_ref(), layout, overlay);
            }
        });
        crate::measure::notice(ui.ctx(), self.tool, settings);
        let shown_tiles = self.shown_tiles();
        let (view, volume) = (self.active_view, &self.volume_view);
        let source = crate::export::Source::new(&self.session, &self.views, view, volume, shown_tiles);
        self.export.show(ui.ctx(), &source, settings, waker, toasts);
    }

    fn toolbar(
        &mut self,
        ui: &mut Ui,
        settings: &mut Settings,
        actions: &mut Vec<Action>,
        active: Option<&(SeriesKey, Arc<SeriesData>)>,
        has_volume: bool,
        reformat_blocker: &str,
    ) {
        // The group on the right (open, close, privacy, annotations, settings,
        // inspector) keeps its place at the right edge; the controls on the left
        // get the rest of the width and wrap onto another row in a narrow window,
        // so nothing overlaps. Both stay in reading order for the keyboard. The
        // right group's width is the one measured in the previous frame.
        let right_id = egui::Id::unique("toolbar right group");
        let right_width = ui.ctx().data(|d| d.get_temp::<f32>(right_id)).unwrap_or(RIGHT_GROUP_WIDTH);
        let mut show_over = None;
        let shown_tiles = self.shown_tiles();
        ui.horizontal_top(|ui| {
            let left_width = (ui.available_width() - right_width - ui.spacing().item_spacing.x).max(0.0);
            let row = vec2(left_width, ui.spacing().interact_size.y);
            let wrapping = ELayout::left_to_right(Align::Center).with_main_wrap(true);
            ui.allocate_ui_with_layout(row, wrapping, |ui| {
                // These controls act on the images, which a document covers.
                if self.documents.is_open() {
                    ui.disable();
                }
                show_over = self.toolbar_controls(ui, settings, active, has_volume, reformat_blocker);
            });
            // One row high, so that its separator does not reach down the rows on the left.
            let row = vec2(ui.available_width(), ui.spacing().interact_size.y);
            let right_to_left = ELayout::right_to_left(Align::Center);
            let right = ui.allocate_ui_with_layout(row, right_to_left, |ui| {
                toolbar_right(ui, settings, actions);
                // Before "Open" in reading order: this group is laid out from the right.
                let (view, volume) = (self.active_view, &self.volume_view);
                let source = crate::export::Source::new(&self.session, &self.views, view, volume, shown_tiles);
                // A document has its own text to copy: these commands are about images.
                let images = !self.documents.is_open();
                ui.add_enabled_ui(images, |ui| self.export.menu(ui, &source));
            });
            let measured = right.response.rect.width();
            if (measured - right_width).abs() > 0.5 {
                ui.ctx().data_mut(|d| d.insert_temp(right_id, measured));
                ui.ctx().request_discard("toolbar layout");
            }
        });
        if let Some(request) = show_over {
            crate::pet::show_over(&mut self.session, &mut self.fusion_pending, request);
        }
    }

    /// The controls on the left of the toolbar: panels, layouts, tools and the
    /// display of the series shown. Returns the CT or MR the user asked to see
    /// a PET over, if any.
    fn toolbar_controls(
        &mut self,
        ui: &mut Ui,
        settings: &mut Settings,
        active: Option<&(SeriesKey, Arc<SeriesData>)>,
        has_volume: bool,
        reformat_blocker: &str,
    ) -> Option<(SeriesKey, summrise_core::pet::Fusion)> {
        let mut show_over = None;
        let browser_icon = if settings.show_browser { ICON_LEFT_PANEL_CLOSE } else { ICON_LEFT_PANEL_OPEN };
        let browser_hint = tr!("Series browser ({shortcut})", shortcut = shortcut("B"));
        if ui.button(icon(browser_icon)).on_hover_text(browser_hint).clicked() {
            settings.show_browser = !settings.show_browser;
        }
        ui.separator();
        for l in Layout::ALL {
            let enabled = !l.needs_volume() || has_volume;
            let chosen = self.layout == l && (enabled || l == Layout::Single) && !self.workspace.is_comparing();
            let resp = ui
                .add_enabled(enabled, egui::Button::selectable(chosen, l.label()))
                .on_disabled_hover_text(reformat_blocker);
            if resp.clicked() {
                self.use_preset(l);
                settings.default_layout = l;
            }
        }
        self.compare_menu(ui);
        ui.separator();
        for (tool, ic) in [(Tool::WindowLevel, ICON_CONTRAST), (Tool::Pan, ICON_PAN_TOOL), (Tool::Zoom, ICON_ZOOM_IN)] {
            let hint = match tool {
                Tool::WindowLevel => {
                    tr!("Contrast: drag left/right for width, up/down for level (or right-drag with any tool)")
                        .to_owned()
                }
                Tool::Pan => tr!("Pan: drag (also middle-drag or Shift-drag)").to_owned(),
                Tool::Zoom => tr!("Zoom: drag up/down (also pinch or {key}-scroll)", key = command_key()),
                // In the menu "Measure", below.
                Tool::Measure(kind) => kind.label().to_owned(),
            };
            if ui
                .add(egui::Button::selectable(self.tool == tool, format!("{} {}", icon(ic), tool.label())))
                .on_hover_text(hint)
                .clicked()
            {
                self.tool = tool;
            }
        }
        let shown_views = if has_volume { self.layout } else { Layout::Single }.views();
        let measured = active.and_then(|(key, _)| self.views.get_mut(key));
        crate::measure::menu(ui, &mut self.tool, measured, shown_views);
        ui.separator();

        if let Some((key, series)) = active
            && let Some(state) = self.views.get_mut(key)
        {
            if !series.is_color() {
                // Windows and the contrast tables stored in the images (display-fidelity D4).
                let choices = ViewState::contrast_choices(series, state.stack());
                keep_together(ui, "window", |ui| {
                    // Translators: the contrast window (its width and level), not a window of the screen.
                    let preset_label = ui.label(RichText::new(tr_ctx!("contrast control", "Window")).small());
                    ComboBox::from_id_salt("preset")
                        // Translators: a contrast window set by hand.
                        .selected_text(
                            state.contrast_label(series).map_or_else(|| tr!("Custom").to_owned(), str::to_owned),
                        )
                        .width(112.0)
                        .show_ui(ui, |ui| {
                            let mut last = None;
                            for (i, choice) in choices.iter().enumerate() {
                                let source = match choice {
                                    ContrastChoice::Window(preset) => Some(preset.source),
                                    ContrastChoice::Table { .. } => None,
                                };
                                if last.is_some() && last != Some(source) {
                                    ui.separator();
                                }
                                last = Some(source);
                                let label = if i < 9 {
                                    format!("{}   {}", choice.label(), i + 1)
                                } else {
                                    choice.label().to_owned()
                                };
                                let entry = ui.selectable_label(state.is_chosen(choice), label);
                                let entry = match choice {
                                    ContrastChoice::Window(preset) => {
                                        let hint = match preset.source {
                                            PresetSource::Dicom => tr!("Stored in the images"),
                                            PresetSource::Clinical if summrise_core::pet::is_suv(series.units()) => {
                                                summrise_i18n::tr!("Standard PET window")
                                            }
                                            PresetSource::Clinical => tr!("Standard CT window"),
                                            PresetSource::Auto => tr!("From image statistics"),
                                            PresetSource::FullRange => tr!("All values"),
                                        };
                                        entry.preset_hint(hint, settings.show_explanations, preset, series.units())
                                    }
                                    ContrastChoice::Table { .. } => {
                                        fidelity::table_hint(entry, settings.show_explanations)
                                    }
                                };
                                if entry.clicked() {
                                    state.choose_contrast(choice);
                                }
                            }
                        })
                        .response
                        .labelled_by(preset_label.id);
                });
                // The limits apply to what the user enters, never to the window that is
                // set: they follow the value range, which narrows from the header
                // estimate to the values present when loading completes, and a stored
                // or chosen window must not change by itself at that moment.
                let (lo, hi) = series.stack_value_range(state.stack());
                let span = f64::from(hi - lo).max(1.0);
                // Uptake values (SUV) need decimals.
                let decimals = summrise_core::pet::decimals(series.units());
                keep_together(ui, "width", |ui| {
                    // Translators: short label of the window width field (contrast), as on scanner annotations.
                    let w_label = ui
                        .label(RichText::new(tr_ctx!("window width", "W")).small())
                        .on_hover_text(tr!("Window width (contrast)"));
                    if ui
                        .add(
                            DragValue::new(&mut state.window.width)
                                .range(1.0..=span * 4.0)
                                .clamp_existing_to_range(false)
                                .speed(span / 500.0)
                                .max_decimals(decimals),
                        )
                        .labelled_by(w_label.id)
                        .changed()
                    {
                        (state.preset, state.lut) = (None, None);
                    }
                });
                keep_together(ui, "level", |ui| {
                    // Translators: short label of the window level field (brightness); not L for the patient's left.
                    let l_label = ui
                        .label(RichText::new(tr_ctx!("window level", "L")).small())
                        .on_hover_text(tr!("Window level (brightness)"));
                    if ui
                        .add(
                            DragValue::new(&mut state.window.center)
                                .range(f64::from(lo) - span..=f64::from(hi) + span)
                                .clamp_existing_to_range(false)
                                .speed(span / 500.0)
                                .max_decimals(decimals),
                        )
                        .labelled_by(l_label.id)
                        .changed()
                    {
                        (state.preset, state.lut) = (None, None);
                    }
                });
            }
            ui.toggle_value(&mut state.invert, icon(ICON_INVERT_COLORS)).on_hover_text(tr!("Invert grey scale (I)"));
            let reset_hint = summrise_i18n::tr!("Reset contrast, zoom, pan, rotation and tilted planes (R)");
            if ui.button(icon(ICON_RESTART_ALT)).on_hover_text(reset_hint).clicked() {
                state.reset(series);
            }
            let views = if has_volume { self.layout } else { Layout::Single }.views();
            orient::menu(ui, state, &mut self.active_view, views);
            volume_view::toolbar(ui, state, series, views);
            show_over = crate::pet::menu(ui, &self.session, *key, series, state);
            if layout_has_mpr(self.layout) && has_volume {
                // No separator: in a narrow window it would be left alone at the end of a row.
                let slab_thickness = &mut self.slab_thickness;
                let mode = keep_together(ui, "slab", |ui| {
                    let mut mode = state.slab.map(|s| s.mode);
                    // Translators: a thick slab of the reformats, shown as one projection.
                    let slab_label = ui.label(RichText::new(tr!("Slab")).small());
                    ComboBox::from_id_salt("slab")
                        .selected_text(mode.map_or(tr!("Thin slice"), SlabMode::label))
                        .width(80.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut mode, None, tr!("Thin slice"));
                            for m in SlabMode::ALL {
                                ui.selectable_value(&mut mode, Some(m), m.label());
                            }
                        })
                        .response
                        .labelled_by(slab_label.id)
                        .on_hover_text(tr!("Thick-slab projection for reformatted views"));
                    if mode.is_some() {
                        ui.spacing_mut().slider_width = 80.0;
                        ui.add(Slider::new(slab_thickness, 2.0..=100.0).suffix(" mm").max_decimals(0))
                            .labelled_by(slab_label.id);
                    }
                    mode
                });
                state.set_slab(mode, self.slab_thickness);
            }
        }
        show_over
    }

    fn viewports(
        &mut self,
        ui: &mut Ui,
        settings: &Settings,
        active: Option<&(SeriesKey, Arc<SeriesData>)>,
        layout: Layout,
        overlay: Option<crate::pet::Overlay<'_>>,
    ) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, Color32::from_gray(8));
        self.central = rect;
        self.hover = None;
        let comparing = self.workspace.is_comparing();
        if !comparing && active.is_none() {
            self.placeholder(ui, rect, self.session.active_series);
            return;
        }
        // One series in every viewport of a preset, or the series of a multi-series grid.
        let active_key = if comparing { self.session.active_series } else { active.map(|(key, _)| *key) };
        self.workspace.show_preset(layout, active_key, self.active_view);
        // Linked viewports follow what the keys and playback did before this frame.
        self.workspace.follow(&self.session, &mut self.views);
        let rects = match self.workspace.grid() {
            Some(multi_series) => crate::compare::cells(rect, multi_series),
            None => grid(rect, self.workspace.viewports().len()),
        };
        let multi_view = rects.len() > 1;
        self.cells.clear();
        let mut tile_pictures = Vec::new();
        for (i, &r) in rects.iter().enumerate() {
            let Some(&viewport) = self.workspace.viewports().get(i) else { continue };
            let kind = viewport.view;
            let id = ui.scope_id().with(("viewport", kind, i));
            let opened = viewport.series.and_then(|key| Some((key, self.session.data(key)?)));
            let Some((key, series)) = opened.filter(|(_, series)| series.is_viewable()) else {
                self.cells.push(workspace::DrawnCell { rect: r, tiles: Tiles::ONE });
                self.waiting_viewport(ui, r, id, i, viewport.series);
                continue;
            };
            let (Some(meta), Some(study)) = (self.session.series(key), self.session.studies().get(key.study)) else {
                continue;
            };
            let info = comparing
                .then(|| crate::compare::ViewportInfo::of(&self.workspace, i, &self.session, &self.views, meta, study));
            // The overlay of the active viewport's series; a viewport beside it says so if it has one.
            let is_active_series = Some(key) == active_key;
            let overlay = overlay.filter(|_| is_active_series);
            let fusion = if is_active_series {
                crate::pet::shown(&self.session, self.views.get(&key))
            } else {
                crate::pet::shown_elsewhere(self.views.get(&key))
            };
            let state = self.views.entry(key).or_insert_with(|| ViewState::new(&series));
            state.settle_stack_window(&series);
            let smooth = settings.smooth_interpolation;
            self.renders.update(ui.ctx(), key, kind, &series, state, smooth, overlay);
            // The following tiles of a tile grid: the next images of the image set shown.
            let tiles = crate::tiles::shown(viewport.tiles, r, multi_view, kind, &series);
            let (first, count) = (state.slice_index(kind, &series), series.stack_len());
            let slots: Vec<Option<usize>> =
                tiles.images(first, count).map(|image| image.map(|image| state.stack() * count + image)).collect();
            for &slot in slots.iter().skip(1).flatten() {
                self.renders.update_tile(ui.ctx(), key, slot, &series, state, smooth, overlay);
                tile_pictures.push((key, slot));
            }
            let pictures = slots.iter().enumerate().map(|(tile, slot)| match tile {
                0 => self.renders.get(key, kind),
                _ => slot.and_then(|slot| self.renders.get_tile(key, slot)),
            });
            let grid_input = (tiles != Tiles::ONE).then(|| crate::tiles::TileInput {
                tiles,
                first,
                count,
                pictures: pictures.collect(),
            });
            let input = ViewportInput {
                id,
                key,
                kind,
                series: &series,
                meta,
                study,
                rendered: self.renders.get(key, kind),
                tool: self.tool,
                settings,
                is_active: if comparing { self.workspace.active() == i } else { self.active_view == kind },
                multi_view,
                show_details: i == 0 || comparing,
                fusion: fusion.as_ref(),
                tiles: grid_input.as_ref(),
                compare: info.as_ref(),
            };
            let out = if kind == ViewKind::Volume {
                // The volume of the stack on screen (stack 0 for a series that is not split).
                let volume = series.stack_volume(state.stack());
                self.volume_view.show(ui, r, &input, key, state, volume)
            } else {
                viewport::show(ui, r, &input, state)
            };
            self.cells.push(workspace::DrawnCell { rect: r, tiles });
            if let Some(image) = out.open_image {
                // A tile opened: one image again, that one.
                state.set_slice(kind, &series, image);
                self.workspace.set_tiles(i, Tiles::ONE);
                self.activate_viewport(i);
            }
            if out.activated {
                self.activate_viewport(i);
            }
            if let Some(h) = out.hover {
                self.hover = Some((kind, h));
                // In a preset the view under the pointer is the active one; of several series
                // that takes a click, a scroll or a drag: the toolbar and the inspector stay put.
                if !comparing {
                    self.active_view = kind;
                }
            }
            for gesture in out.gestures {
                self.workspace.follow_gesture(i, gesture, &self.session, &mut self.views);
            }
            if out.toggle_link {
                self.workspace.toggle_link(i);
            }
            #[cfg(test)]
            self.cross_lines.insert(i, out.cross_lines);
            if comparing {
                self.take_drop(ui, r, id, i);
            }
        }
        self.renders.retain_tiles(&tile_pictures);
        // And what the pointer did in this frame.
        if self.workspace.follow(&self.session, &mut self.views) {
            ui.ctx().request_repaint();
        }
        // Loading concentrates on the images around the one on screen, or on
        // the one playback waits for.
        for key in self.workspace.shown_series() {
            if let (Some(series), Some(state)) = (self.session.data(key), self.views.get(&key)) {
                let displayed = self.cine.awaited(key).unwrap_or_else(|| state.slot_index(&series));
                self.session.set_displayed(key, displayed);
            }
        }
    }

    /// Shown until the first image of series `active` is decoded.
    fn placeholder(&self, ui: &mut Ui, rect: Rect, active: Option<SeriesKey>) {
        let p = theme::palette(ui);
        let (text, busy) = match active.and_then(|key| self.session.state(key)) {
            Some(SeriesState::Loading(loading)) => {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                let series = &loading.data;
                // The image the series opens on, or the one it was left on.
                let shown = active
                    .and_then(|key| self.views.get(&key))
                    .map_or(series.initial_index(), |view| view.slice_index(ViewKind::Stack, series));
                let count = series.stack_len();
                (tr!("Loading image {number} / {count}", number = shown + 1, count = count), true)
            }
            Some(SeriesState::Failed(e)) => (tr!("This series could not be displayed.\n{reason}", reason = e), false),
            _ if self.session.study().series.is_empty() => {
                (tr!("This study has no images on this disc.").to_owned(), false)
            }
            _ => (tr!("Select a series").to_owned(), false),
        };
        let center = rect.center();
        if busy {
            let spinner = Rect::from_center_size(center - vec2(0.0, 24.0), vec2(32.0, 32.0));
            ui.put(spinner, egui::Spinner::new().size(28.0));
        }
        let message = Rect::from_center_size(center + vec2(0.0, 24.0), vec2((rect.width() - 40.0).max(120.0), 48.0));
        ui.put(message, egui::Label::new(RichText::new(text).size(14.0).color(p.text_weak)).selectable(false));
    }

    fn status_bar(&mut self, ui: &mut Ui, active: Option<&(SeriesKey, Arc<SeriesData>)>) {
        let p = theme::palette(ui);
        // The group on the right keeps its place; the text on the left gets the rest of
        // the width and is shortened ("…", in full on hover) where it does not fit. The
        // right group's width is the one measured in the previous frame.
        let right_id = egui::Id::unique("status bar right group");
        let right_width = ui.ctx().data(|d| d.get_temp::<f32>(right_id)).unwrap_or(0.0);
        ui.horizontal(|ui| {
            let left_width = (ui.available_width() - right_width - ui.spacing().item_spacing.x).max(0.0);
            // What lies under the pointer is of the series there, in a multi-series layout not always the active one.
            let hovered = self.hover.and_then(|(_, h)| Some((h, self.session.data(h.series)?)));
            let text = match &hovered {
                Some((h, series)) => {
                    let mut parts = vec![format!("x {} y {}", h.pixel.0, h.pixel.1)];
                    if let Some(number) = h.image {
                        // Which tile of a grid: the image the values are of.
                        parts.insert(0, tr!("Image {number}", number = number));
                    }
                    if let Some(pt) = h.patient {
                        parts.push(format!(
                            "{} {:.1}  {} {:.1}  {} {:.1} mm",
                            if pt.x >= 0.0 { "L" } else { "R" },
                            pt.x.abs(),
                            if pt.y >= 0.0 { "P" } else { "A" },
                            pt.y.abs(),
                            if pt.z >= 0.0 { "S" } else { "I" },
                            pt.z.abs()
                        ));
                    }
                    if let Some(v) = h.value {
                        // "40 HU", "SUV 3.1", or both values while a PET is drawn over a CT.
                        parts.push(if Some(h.series) == self.session.active_series {
                            crate::pet::readout(&self.session, series, v, h.patient)
                        } else {
                            summrise_core::pet::value_text(series.units(), f64::from(v))
                        });
                    } else if h.outside {
                        parts.push(summrise_i18n::tr!("Outside the scanned area").to_owned());
                    }
                    RichText::new(parts.join("   ·   ")).small().monospace().color(p.text)
                }
                _ if self.documents.is_open() => RichText::new(documents::status_hint()).small().color(p.text_weak),
                _ if self.active_view == ViewKind::Volume => {
                    RichText::new(volume_view::status_hint()).small().color(p.text_weak)
                }
                _ if self.tool.is_measuring() => {
                    RichText::new(crate::measure::status_hint(self.tool)).small().color(p.text_weak)
                }
                _ => RichText::new(tr!(
                    "Scroll: change image · Drag: contrast · Pinch/{key}-scroll: zoom · Double-click: fit",
                    key = command_key()
                ))
                .small()
                .color(p.text_weak),
            };
            ui.scope(|ui| {
                ui.set_max_width(left_width);
                ui.add(Label::new(text).truncate());
            });
            let right = ui.with_layout(ELayout::right_to_left(Align::Center), |ui| {
                let index = self.session.index();
                let source = match (self.session.media().kind(), &index.source) {
                    (MediaKind::Zip, DiscoverySource::Dicomdir(_)) => summrise_i18n::tr!("ZIP archive · DICOMDIR"),
                    (MediaKind::Zip, _) => summrise_i18n::tr!("ZIP archive"),
                    (MediaKind::Iso, DiscoverySource::Dicomdir(_)) => summrise_i18n::tr!("Disc image · DICOMDIR"),
                    (MediaKind::Iso, _) => summrise_i18n::tr!("Disc image"),
                    (MediaKind::Folder, DiscoverySource::Dicomdir(_)) => "DICOMDIR",
                    (MediaKind::Folder, DiscoverySource::FolderScan) => tr!("Folder scan"),
                    (MediaKind::Folder, DiscoverySource::SingleFile) => tr!("Single file"),
                };
                let images =
                    tr_n!("{n} image · {source}", "{n} images · {source}", index.stats.images, source = source);
                ui.label(RichText::new(images).small().color(p.text_weak))
                    .on_hover_text(index.root.display().to_string());
                if !index.warnings.is_empty() {
                    let warn = ui.label(
                        RichText::new(format!("{} {}", icon(ICON_WARNING), index.warnings.len())).small().color(p.warn),
                    );
                    warn.on_hover_text(index.warnings.join("\n"));
                }
                if let Some(over) = self.over_budget {
                    // The series on screen stay loaded over the memory limit, and it says so.
                    let text = format!("{} {}", icon(ICON_WARNING), tr!("Over the memory limit"));
                    let notice = ui.label(RichText::new(text).small().color(p.warn));
                    notice.on_hover_text(workspace::memory_notice(over));
                }
                if let Some((_, series)) = active
                    && let Some(issue) = series.volume_issue()
                    && series.len() > 2
                    && !series.is_color()
                {
                    // Translators: the reformats (MPR) are not available for this series.
                    let off = format!("{} {}", icon(ICON_INFO), tr!("MPR off"));
                    ui.label(RichText::new(off).small().color(p.text_weak)).on_hover_text(issue.to_string());
                }
                if let Some((key, series)) = active
                    && let Some(state) = self.views.get_mut(key)
                {
                    fidelity::graphics_toggle(ui, series, state);
                }
                crate::measure::status_notice(ui, self.tool, self.active_view);
            });
            let measured = right.response.rect.width();
            if (measured - right_width).abs() > 0.5 {
                ui.ctx().data_mut(|d| d.insert_temp(right_id, measured));
                ui.ctx().request_discard("status bar layout");
            }
        });
    }

    fn display_rows(&self, active: Option<&(SeriesKey, Arc<SeriesData>)>) -> Vec<(String, String)> {
        let Some((key, series)) = active else { return Vec::new() };
        let Some(state) = self.views.get(key) else {
            return Vec::new();
        };
        let complete = series.is_complete();
        let mut rows = Vec::new();
        let image = tr!("{number} of {count}", number = state.slice + 1, count = series.stack_len());
        // Translators: the rows of the "Display" section of the image information.
        rows.push((tr_ctx!("display", "Image").into(), image));
        if let Some(stacks) = series.stacks() {
            rows.push((summrise_i18n::tr!("Image set").to_owned(), stacks.label(state.stack())));
            rows.push((summrise_i18n::tr!("Image sets").to_owned(), stacks.summary()));
        }
        if !complete {
            let loaded =
                tr_n!("{loaded} of {n} image", "{loaded} of {n} images", series.len(), loaded = series.loaded());
            rows.push((tr_ctx!("display", "Loading").into(), loaded));
        }
        if !series.is_color() {
            // A contrast table replaces the window of the acquired images (reformats show theirs).
            if let Some(table) = state.active_lut(series) {
                rows.push((summrise_i18n::tr!("Contrast table").to_owned(), table.name.clone()));
            } else {
                let (width, level) = (format::number(state.window.width, 1), format::number(state.window.center, 1));
                // Translators: a window set by hand, shown in brackets after its values.
                let preset = state.preset_label(series).unwrap_or(tr!("custom"));
                // Translators: the window in use: W its width, L its level, then its name.
                let window = match series.units() {
                    Some(unit) => tr!(
                        "W {width} / L {level} {unit} ({preset})",
                        width = width,
                        level = level,
                        unit = unit,
                        preset = preset
                    ),
                    None => tr!("W {width} / L {level} ({preset})", width = width, level = level, preset = preset),
                };
                rows.push((tr_ctx!("contrast control", "Window").into(), window));
            }
            // The values present are only known once every image is decoded.
            if complete {
                rows.push((tr!("Value range").into(), crate::pet::value_range_text(series)));
            }
        }
        if let Some(v) = series.volume() {
            let s = v.geometry.spacing();
            let [columns, rows_, slices] = v.geometry.dims;
            let voxels = tr!("{columns} × {rows} × {slices} voxels", columns = columns, rows = rows_, slices = slices);
            // Translators: the images as one 3D block of voxels (not the loudness of sound).
            rows.push((tr!("Volume").into(), voxels));
            rows.push((tr!("Voxel size").into(), format!("{:.2} × {:.2} × {:.2} mm", s.x, s.y, s.z)));
            let tilt = v.geometry.shear_degrees();
            if tilt > 0.1 {
                let corrected = tr!("{degrees}° (corrected in reformats)", degrees = format!("{tilt:.1}"));
                rows.push((tr!("Gantry tilt").into(), corrected));
            }
        } else if let Some(issue) = series.volume_issue() {
            rows.push((tr!("Reformatting").into(), tr!("Unavailable — {reason}", reason = issue)));
        } else if !complete {
            rows.push((tr!("Reformatting").into(), tr!("Available when all images are loaded").into()));
        }
        // Translators: the computer memory the decoded images of the series take.
        rows.push((tr!("Memory").into(), format_bytes(series.byte_size())));
        rows
    }

    fn handle_keys(
        &mut self,
        ui: &mut Ui,
        key: SeriesKey,
        series: &SeriesData,
        settings: &mut Settings,
        has_volume: bool,
    ) {
        let ctx = ui.ctx().clone();
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let kind = self.active_view;
        let page = self.page_step();
        let Some(state) = self.views.get_mut(&key) else { return };
        // Turn, flip and tilt first: the bindings below ignore Alt and Shift.
        orient::keys(&ctx, state, kind);
        volume_view::keys(&ctx, state, series, kind);
        crate::measure::keys(&ctx, &mut self.tool, state, kind);
        let pressed = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        let step = |state: &mut ViewState, d: i64| state.scroll(kind, series, d);
        // Left and Right change the stack of a series with several; Up and Down the image.
        let stacks = series.stack_count() > 1;
        if pressed(Key::ArrowUp) || (!stacks && pressed(Key::ArrowLeft)) {
            step(state, -1);
        }
        if pressed(Key::ArrowDown) || (!stacks && pressed(Key::ArrowRight)) {
            step(state, 1);
        }
        if stacks && pressed(Key::ArrowLeft) {
            state.step_stack(series, -1);
        }
        if stacks && pressed(Key::ArrowRight) {
            state.step_stack(series, 1);
        }
        // Space plays and pauses, unless a focused widget takes it (as a click).
        if ctx.memory(|m| m.focused().is_none()) && pressed(Key::Space) {
            self.cine.toggle(key, series, state, kind, ctx.input(|i| i.time));
        }
        // Ten images, or a whole tile grid.
        if pressed(Key::PageUp) {
            step(state, -page);
        }
        if pressed(Key::PageDown) {
            step(state, page);
        }
        if pressed(Key::Home) {
            state.set_slice(kind, series, 0);
        }
        if pressed(Key::End) {
            let n = state.slice_count(kind, series);
            state.set_slice(kind, series, n.saturating_sub(1));
        }
        if pressed(Key::R) {
            state.reset(series);
        }
        if pressed(Key::I) {
            state.invert = !state.invert;
        }
        if pressed(Key::O) {
            settings.show_overlays = !settings.show_overlays;
        }
        if pressed(Key::G) {
            state.show_graphics = !state.show_graphics;
        }
        if pressed(Key::M) && has_volume {
            self.layout = if self.layout == Layout::Single { Layout::Mpr } else { Layout::Single };
            self.workspace.leave_grid();
        }
        let digits =
            [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        if let Some(n) = digits.iter().position(|&k| pressed(k))
            && let Some(choice) = ViewState::contrast_choices(series, state.stack()).get(n)
        {
            state.choose_contrast(choice);
        }
        if pressed(Key::A)
            && let Some(preset) =
                ViewState::presets_for(series, state.stack()).into_iter().find(|p| p.source == PresetSource::Auto)
        {
            state.apply_preset(&preset);
        }
        // C: colours of the PET; F: the PET overlay (from a PET: the CT or MR under it).
        if let Some(request) = crate::pet::keys(&ctx, &self.session, key, series, state) {
            crate::pet::show_over(&mut self.session, &mut self.fusion_pending, request);
        }
    }
}

/// Why the reformat layouts are disabled, as their tooltip: the loading
/// progress while a stack that will form a volume is still loading, else the
/// reason the images cannot be reformatted.
fn reformat_blocker(state: Option<&SeriesState>) -> String {
    let series = state.and_then(SeriesState::data);
    match (state, series.and_then(|s| s.volume_issue())) {
        (_, Some(issue)) => tr!("Multiplanar reformatting unavailable: {reason}", reason = issue),
        (Some(SeriesState::Loading(loading)), None) => {
            tr!("Loading {loaded} / {total}", loaded = loading.data.loaded(), total = loading.data.len())
        }
        _ => tr!("Multiplanar reformatting unavailable: Needs a stack of at least 3 parallel slices").to_owned(),
    }
}

fn format_bytes(bytes: usize) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / f64::from(1u32 << 30)),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / f64::from(1u32 << 20)),
        b => format!("{:.0} KB", (b as f64 / 1024.0).ceil()),
    }
}

/// Width of the right-hand toolbar group before it is first measured.
const RIGHT_GROUP_WIDTH: f32 = 280.0;

/// The group at the right edge of the toolbar, laid out from the right:
/// inspector, settings, annotations, privacy, then close and open.
fn toolbar_right(ui: &mut Ui, settings: &mut Settings, actions: &mut Vec<Action>) {
    let inspector_icon = if settings.show_inspector { ICON_RIGHT_PANEL_CLOSE } else { ICON_RIGHT_PANEL_OPEN };
    let inspector_hint = tr!("Image information ({shortcut})", shortcut = shortcut("I"));
    if ui.button(icon(inspector_icon)).on_hover_text(inspector_hint).clicked() {
        settings.show_inspector = !settings.show_inspector;
    }
    if ui.button(icon(ICON_SETTINGS)).on_hover_text(tr!("Settings")).clicked() {
        actions.push(Action::ShowSettings);
    }
    let overlay_icon = if settings.show_overlays { ICON_VISIBILITY } else { ICON_VISIBILITY_OFF };
    if ui.button(icon(overlay_icon)).on_hover_text(tr!("Show/hide annotations (O)")).clicked() {
        settings.show_overlays = !settings.show_overlays;
    }
    ui.toggle_value(&mut settings.hide_patient_details, icon(ICON_BADGE))
        .on_hover_text(tr!("Privacy: hide patient name, ID and birth date"));
    ui.separator();
    if ui.button(icon(ICON_CLOSE)).on_hover_text(tr!("Close study")).clicked() {
        actions.push(Action::CloseStudy);
    }
    if ui
        .button(format!("{} {}", icon(ICON_FOLDER_OPEN), tr!("Open")))
        .on_hover_text(tr!("Open another folder or disc ({shortcut})", shortcut = shortcut("O")))
        .clicked()
    {
        actions.push(Action::OpenDialog);
    }
}

fn layout_has_mpr(layout: Layout) -> bool {
    layout.views().iter().any(|v| matches!(v, ViewKind::Mpr(_)))
}

/// Splits `rect` into `n` viewport cells with a thin gutter.
fn grid(rect: Rect, n: usize) -> Vec<Rect> {
    let gap = 2.0;
    let cell = |r: Rect, cols: usize, rows: usize, i: usize| {
        let w = (r.width() - gap * (cols - 1) as f32) / cols as f32;
        let h = (r.height() - gap * (rows - 1) as f32) / rows as f32;
        let (c, row) = (i % cols, i / cols);
        Rect::from_min_size(r.min + vec2(c as f32 * (w + gap), row as f32 * (h + gap)), vec2(w, h))
    };
    match n {
        0 | 1 => vec![rect],
        3 if rect.width() / rect.height() > 1.6 => (0..3).map(|i| cell(rect, 3, 1, i)).collect(),
        3 => {
            // One large view on the left, two stacked on the right.
            let left = Rect::from_min_size(rect.min, vec2(rect.width() * 0.6 - gap / 2.0, rect.height()));
            let right = Rect::from_min_max(egui::pos2(left.right() + gap, rect.top()), rect.max);
            vec![left, cell(right, 1, 2, 0), cell(right, 1, 2, 1)]
        }
        _ => (0..n).map(|i| cell(rect, 2, n.div_ceil(2), i)).collect(),
    }
}

mod workspace;

#[cfg(test)]
mod compare_tests;
#[cfg(test)]
mod documents_tests;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod fidelity_tests;
#[cfg(test)]
mod measure_tests;
#[cfg(test)]
mod oblique_tests;
#[cfg(test)]
mod pet_tests;
#[cfg(test)]
mod print_tests;
#[cfg(test)]
mod stacks_tests;
#[cfg(test)]
mod toolbar_tests;
#[cfg(test)]
mod volume_tests;

/// The viewer while a series loads, driven frame by frame with an image
/// source that the tests let through one file at a time. Set
/// `SUMMRISE_SNAPSHOT_DIR` to keep renderings of the loading states.
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use summrise_core::dicom::{DecodedInstance, DicomError, InstanceHeader, Media, Progress, scan};
    use summrise_core::{DicomFiles, ImageSource};
    use summrise_testkit::{PhantomOptions, write_phantom_study};

    use super::*;

    const CT: SeriesKey = SeriesKey { study: 0, series: 0 };

    /// An artificially slow image source: every read waits until the test
    /// lets it through.
    #[derive(Default)]
    struct SlowSource {
        allowed: AtomicUsize,
        started: AtomicUsize,
        /// Instance number of a CT image that cannot be decoded.
        corrupt: Option<i32>,
    }

    impl SlowSource {
        fn allow(&self, n: usize) {
            self.allowed.fetch_add(n, Ordering::SeqCst);
        }

        fn allow_all(&self) {
            self.allowed.store(usize::MAX, Ordering::SeqCst);
        }

        fn started(&self) -> usize {
            self.started.load(Ordering::SeqCst)
        }
    }

    impl ImageSource for SlowSource {
        fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError> {
            let turn = self.started.fetch_add(1, Ordering::SeqCst);
            while turn >= self.allowed.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            DicomFiles.read(media, header)
        }

        fn decode(&self, header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError> {
            let ct = header.path.parent().is_some_and(|folder| folder.ends_with("SE000001"));
            if ct && self.corrupt.is_some() && header.instance_number == self.corrupt {
                return Err(DicomError::Image("corrupt pixel data".to_owned()));
            }
            DicomFiles.decode(header, bytes)
        }
    }

    /// Everything `Viewer::show` needs.
    struct App {
        viewer: Viewer,
        settings: Settings,
        waker: Waker,
        actions: Vec<Action>,
        toasts: Toasts,
    }

    /// The viewer on the synthetic CD (the 24-image CT is selected).
    struct Fixture {
        harness: Harness<'static, App>,
        slow: Arc<SlowSource>,
        _dir: tempfile::TempDir,
    }

    fn snapshot_dir() -> Option<PathBuf> {
        std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from)
    }

    fn open(slow: SlowSource, settings: Settings) -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        write_phantom_study(dir.path(), &PhantomOptions::small()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        let slow = Arc::new(slow);
        let session = Session::with_source(index, settings.memory_budget_bytes(), None, slow.clone());
        let app = App {
            viewer: Viewer::new(session, &settings),
            settings,
            waker: Arc::new(|| {}),
            actions: Vec::new(),
            toasts: Toasts::default(),
        };
        let mut builder = Harness::builder().with_size([1280.0, 800.0]).with_context_setup(theme::install);
        if snapshot_dir().is_some() {
            builder = builder.wgpu();
        }
        let harness = builder.build_ui_state(
            |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
            app,
        );
        Fixture { harness, slow, _dir: dir }
    }

    impl Fixture {
        /// Runs UI frames until `done`.
        fn run_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
            let start = Instant::now();
            loop {
                self.harness.step();
                if done(self) {
                    return;
                }
                assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        /// Runs UI frames until exactly `loaded` images are in and the loader waits for the test.
        fn run_until_loaded(&mut self, loaded: usize) {
            self.run_until("images", |f| f.series().loaded() == loaded && f.slow.started() == loaded + 1);
            self.harness.step();
        }

        fn run_until_complete(&mut self) {
            self.slow.allow_all();
            self.run_until("the whole series", |f| f.viewer().session.loaded(CT).is_some());
            self.harness.step();
        }

        fn viewer(&self) -> &Viewer {
            &self.harness.state().viewer
        }

        fn series(&self) -> Arc<SeriesData> {
            self.viewer().session.data(CT).expect("the CT is selected")
        }

        fn view(&self) -> &ViewState {
            self.viewer().views.get(&CT).expect("the CT is on screen")
        }

        /// Whether something on screen is labelled exactly `label`.
        fn shows(&self, label: &str) -> bool {
            self.harness.query_all_by_label(label).next().is_some()
        }

        fn is_enabled(&self, button: &str) -> bool {
            !self.harness.get_by_label(button).accesskit_node().is_disabled()
        }

        fn press(&mut self, key: Key) {
            self.harness.key_press(key);
            self.harness.step();
            self.harness.step();
        }

        fn snapshot(&mut self, name: &str) {
            if let Some(dir) = snapshot_dir() {
                let image = self.harness.render().expect("wgpu render");
                std::fs::create_dir_all(&dir).expect("snapshot dir");
                image.save(dir.join(format!("{name}.png"))).expect("save png");
            }
        }
    }

    /// Task 3.1 — an image that is not loaded shows as a placeholder with
    /// its number, and gives way to the image as soon as it is decoded.
    #[test]
    fn unloaded_images_show_a_placeholder_until_they_arrive() {
        let mut f = open(SlowSource::default(), Settings::default());
        // Nothing decoded yet: the image the series opens on is announced, not a blank viewport.
        f.harness.step();
        assert!(f.shows("Loading image 13 / 24"));
        f.snapshot("loading-1-before-first-image");

        // One image is decoded: it is on screen, with the progress.
        f.slow.allow(1);
        f.run_until_loaded(1);
        assert!(f.shows("Image 13 / 24, Loading 1 / 24"));
        f.snapshot("loading-2-first-image");

        // The user goes to the last image, which is not loaded: a placeholder with its number.
        f.press(Key::End);
        assert!(f.shows("Loading image 24 / 24, Loading 1 / 24"));
        assert!(!f.shows("Image 13 / 24, Loading 1 / 24"), "never another image in its place");
        assert_eq!(f.view().slice, 23);
        f.snapshot("loading-3-placeholder");

        // The read in progress finishes, then the displayed image is read: it replaces the placeholder.
        f.slow.allow(2);
        f.run_until_loaded(3);
        assert!(f.series().frame(23).is_some(), "the displayed image was loaded next");
        assert!(f.shows("Image 24 / 24, Loading 3 / 24"));

        // The rest arrives; the user is still where they were.
        f.run_until_complete();
        assert!(f.shows("Image 24 / 24"));
        assert_eq!(f.view().slice, 23);
        f.snapshot("loading-4-complete");
    }

    /// Task 3.2 — the reformat layouts are disabled with "Loading n / N"
    /// until the series is complete and enabled by themselves afterwards.
    #[test]
    fn reformat_layouts_are_enabled_when_the_series_is_complete() {
        let mut f = open(SlowSource::default(), Settings::default());
        f.slow.allow(5);
        f.run_until_loaded(5);
        assert!(f.is_enabled("Images") && !f.is_enabled("MPR") && !f.is_enabled("2 × 2"));
        assert_eq!(reformat_blocker(f.viewer().session.state(CT)), "Loading 5 / 24");
        // The reason is the tooltip of the disabled button.
        f.harness.get_by_label("MPR").hover();
        f.harness.run_steps(8);
        assert!(f.shows("Loading 5 / 24"), "tooltip of the disabled MPR button");
        f.snapshot("loading-5-reformat-tooltip");
        // M (toggle MPR) does nothing yet.
        f.press(Key::M);
        assert!(f.shows("Image 13 / 24, Loading 5 / 24"));

        // The last image arrives: the layouts are available without selecting the series again.
        f.run_until_complete();
        assert!(f.is_enabled("MPR") && f.is_enabled("2 × 2"));
        f.harness.get_by_label("MPR").click();
        f.harness.run_steps(3);
        assert!(f.shows("Axial 12 / 24") && f.shows("Coronal 25 / 48") && f.shows("Sagittal 25 / 48"));
    }

    #[test]
    fn remembered_reformat_layout_applies_when_loading_completes() {
        // The user had the MPR layout on last time.
        let mut f = open(SlowSource::default(), Settings { default_layout: Layout::Mpr, ..Settings::default() });
        f.slow.allow(3);
        f.run_until_loaded(3);
        assert!(f.shows("Image 13 / 24, Loading 3 / 24"), "acquired images while loading");
        let viewed = f.series().frames()[f.view().slice].plane.expect("plane").position.z;
        f.run_until_complete();
        // The reformats appear on the plane of the image that was being viewed, not beside it.
        assert!(f.shows("Axial 12 / 24"), "reformats once the volume exists");
        let axial = f.view().mpr_position(summrise_core::imaging::MprAxis::Axial);
        assert!((axial - viewed).abs() < 1e-6, "axial reformat at {axial} mm, viewed image at {viewed} mm");
        assert_eq!(f.view().slice, 12);
    }

    #[test]
    fn remembered_reformat_layout_does_not_take_over_a_view_in_use() {
        let mut f = open(SlowSource::default(), Settings { default_layout: Layout::Mpr, ..Settings::default() });
        f.slow.allow(3);
        f.run_until_loaded(3);
        // The user starts reading the acquired images while the series loads.
        f.press(Key::PageDown);
        assert!(f.shows("Loading image 23 / 24, Loading 3 / 24"));
        f.run_until_complete();
        // Nothing changes under their hands: same view, same image; the reformats are on offer.
        assert!(f.shows("Image 23 / 24"));
        assert!(!f.shows("Axial 2 / 24"));
        assert_eq!(f.view().slice, 22);
        assert!(f.is_enabled("MPR"));
        assert_eq!(f.harness.state().settings.default_layout, Layout::Mpr, "the saved preference is untouched");
        // Asked for, the reformats open on the plane that was being viewed.
        f.harness.get_by_label("MPR").click();
        f.harness.run_steps(3);
        assert!(f.shows("Axial 2 / 24"));
    }

    /// The W and L fields never rewrite a window by themselves, in particular
    /// not when the value range narrows from the header estimate to the
    /// values actually present as loading completes.
    #[test]
    fn wide_window_set_while_loading_is_not_clamped_at_completion() {
        let mut f = open(SlowSource::default(), Settings::default());
        f.slow.allow(3);
        f.run_until_loaded(3);
        // Inside the limits of the 12-bit estimate (-1024 to 3071 HU), outside those of the
        // values present (-1000 to 700 HU).
        let view = f.harness.state_mut().viewer.views.get_mut(&CT).expect("view");
        view.window = summrise_core::imaging::Window::new(2500.0, 8000.0);
        view.preset = Some("Wide".to_owned());
        f.harness.step();
        f.harness.step();
        assert_eq!((f.view().window.width, f.view().window.center), (8000.0, 2500.0));

        f.run_until_complete();
        f.harness.step();
        assert_eq!((f.view().window.width, f.view().window.center), (8000.0, 2500.0));
        assert_eq!(f.view().preset.as_deref(), Some("Wide"));
    }

    /// Task 4.3 at the level of the viewer: what the user sets while a series
    /// loads — W 1500 / L −600, position, zoom — is unchanged at completion.
    #[test]
    fn window_position_and_zoom_set_while_loading_are_kept() {
        let mut f = open(SlowSource::default(), Settings::default());
        f.slow.allow(3);
        f.run_until_loaded(3);
        assert!(f.shows("W 400 / L 40 HU (Mediastinum)"), "opens with the first image's stored window");

        // The user picks the second window (lung), pages down and zooms in.
        f.press(Key::Num2);
        f.press(Key::PageDown);
        let viewer = &mut f.harness.state_mut().viewer;
        viewer.views.get_mut(&CT).expect("view").transform_mut(ViewKind::Stack).zoom_about(2.0, [30.0, -20.0]);
        f.harness.step();
        assert!(f.shows("W 1500 / L -600 HU (Lung)"));
        let (window, slice, transform) = (f.view().window, f.view().slice, f.view().transform(ViewKind::Stack));
        assert_eq!((window.width, window.center, slice, transform.zoom), (1500.0, -600.0, 22, 2.0));

        // The remaining images arrive.
        f.run_until_complete();
        assert!(f.shows("W 1500 / L -600 HU (Lung)"));
        assert_eq!(f.view().window, window);
        assert_eq!(f.view().slice, slice);
        assert_eq!(f.view().transform(ViewKind::Stack), transform);
        assert_eq!(f.view().preset.as_deref(), Some("Lung"));
    }

    /// Task 3.3 — inspector and read-outs work with a partly loaded series.
    #[test]
    fn inspector_describes_a_partly_loaded_series() {
        let mut f = open(SlowSource::default(), Settings::default());
        f.slow.allow(4);
        f.run_until_loaded(4);
        let active = (CT, f.series());
        let rows = f.viewer().display_rows(Some(&active));
        let row = |label: &str| rows.iter().find(|(l, _)| l == label).map(|(_, value)| value.as_str());
        assert_eq!(row("Image"), Some("13 of 24"));
        assert_eq!(row("Loading"), Some("4 of 24 images"));
        assert_eq!(row("Window"), Some("W 400 / L 40 HU (Mediastinum)"));
        assert_eq!(row("Value range"), None, "not known before every image is decoded");
        assert_eq!(row("Reformatting"), Some("Available when all images are loaded"));
        assert_eq!(row("Volume"), None);
        assert_eq!(row("Memory"), Some("18 KB"), "four 48 × 48 images of 16 bits");
        // The metadata of the displayed image is read from its file as usual.
        f.run_until("metadata", |f| f.shows("CT CHEST 2.5mm (synthetic)") && f.shows("SYNTHETIC PHANTOM"));
        // The browser shows how far the series is.
        assert!(f.harness.query_all_by_label_contains("4 / 24 images").next().is_some());

        f.run_until_complete();
        let rows = f.viewer().display_rows(Some(&active));
        let row = |label: &str| rows.iter().find(|(l, _)| l == label).map(|(_, value)| value.as_str());
        assert_eq!(row("Loading"), None);
        assert_eq!(row("Value range"), Some("-1000 … 700 HU"));
        assert_eq!(row("Volume"), Some("48 × 48 × 24 voxels"));
        assert_eq!(row("Reformatting"), None);
        assert!(f.harness.query_all_by_label_contains("24 images").next().is_some());
    }

    /// Task 3.1 — an image that failed to decode says so at its position.
    #[test]
    fn undecodable_image_says_so_at_its_position() {
        let mut f = open(SlowSource { corrupt: Some(17), ..SlowSource::default() }, Settings::default());
        f.run_until_complete();
        // Image 17 cannot be decoded; its neighbours display normally.
        f.press(Key::ArrowDown);
        f.press(Key::ArrowDown);
        f.press(Key::ArrowDown);
        assert!(f.shows("Image 16 / 24"));
        f.press(Key::ArrowDown);
        assert!(f.shows("Image 17 could not be decoded"));
        f.snapshot("loading-6-failed-image");
        f.press(Key::ArrowDown);
        assert!(f.shows("Image 18 / 24"));
        // A hole in the stack: no reformats, and the reason says why.
        assert!(!f.is_enabled("MPR"));
        assert!(reformat_blocker(f.viewer().session.state(CT)).contains("not evenly spaced"));
    }
}
