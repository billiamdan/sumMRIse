//! The study viewer: toolbar, series browser, viewport grid, inspector and
//! status bar.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{
    Align, Color32, ComboBox, DragValue, Frame, Key, Layout as ELayout, Margin, Modifiers, Panel, Rect, RichText,
    Slider, Ui, vec2,
};
use egui_material_icons::icons::{
    ICON_BADGE, ICON_CLOSE, ICON_CONTRAST, ICON_FOLDER_OPEN, ICON_INFO, ICON_INVERT_COLORS, ICON_LEFT_PANEL_CLOSE,
    ICON_LEFT_PANEL_OPEN, ICON_PAN_TOOL, ICON_RESTART_ALT, ICON_RIGHT_PANEL_CLOSE, ICON_RIGHT_PANEL_OPEN,
    ICON_SETTINGS, ICON_VISIBILITY, ICON_VISIBILITY_OFF, ICON_WARNING, ICON_ZOOM_IN,
};
use summrise_core::dicom::{DiscoverySource, LoadedSeries, format};
use summrise_core::imaging::{PresetSource, SlabMode};
use summrise_core::{
    Layout, SeriesKey, SeriesState, Session, SessionEvent, Settings, Tool, ViewKind, ViewState, Waker,
};

use crate::app::{Action, Toasts};
use crate::browser::{self, ThumbnailTextures};
use crate::inspector::Inspector;
use crate::render::RenderCache;
use crate::theme;
use crate::viewport::{self, Hover, ViewportInput};
use crate::widgets::icon;

pub(crate) struct Viewer {
    pub session: Session,
    views: HashMap<SeriesKey, ViewState>,
    layout: Layout,
    tool: Tool,
    active_view: ViewKind,
    renders: RenderCache,
    thumbs: ThumbnailTextures,
    inspector: Inspector,
    hover: Option<(ViewKind, Hover)>,
    slab_thickness: f64,
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
            thumbs: ThumbnailTextures::default(),
            inspector: Inspector::default(),
            hover: None,
            slab_thickness: 10.0,
        }
    }

    /// Window title suffix.
    pub(crate) fn title(&self) -> String {
        self.session.study().title()
    }

    fn active_series(&self) -> Option<(SeriesKey, Arc<LoadedSeries>)> {
        let key = self.session.active_series?;
        Some((key, self.session.loaded(key)?))
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
                SessionEvent::SeriesReady(key) => {
                    if let Some(series) = self.session.loaded(key) {
                        self.views.entry(key).or_insert_with(|| ViewState::new(&series));
                        for w in &series.warnings {
                            toasts.warn(w.clone());
                        }
                    }
                }
                SessionEvent::SeriesFailed(_, e) => toasts.error(format!("Could not open series: {e}")),
                SessionEvent::ThumbnailReady(_) => {}
            }
        }
        if self.session.is_busy() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
        }
        self.renders.retain_series(self.session.active_series);
        let active = self.active_series();
        let has_volume = active.as_ref().is_some_and(|(_, s)| s.volume().is_some_and(|v| v.geometry.dims[2] >= 3));
        let layout = if has_volume { self.layout } else { Layout::Single };
        if !layout.views().contains(&self.active_view) {
            self.active_view = layout.views()[0];
        }
        if let Some((key, series)) = &active {
            self.handle_keys(ui, *key, series, settings, has_volume);
        }

        let p = theme::palette(ui);
        Panel::top("toolbar")
            .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(10, 6)))
            .show(ui, |ui| self.toolbar(ui, settings, actions, active.as_ref(), has_volume));
        Panel::bottom("status")
            .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(10, 3)))
            .show(ui, |ui| self.status_bar(ui, active.as_ref()));
        if settings.show_browser {
            Panel::left("browser")
                .resizable(true)
                .default_size(270.0)
                .size_range(210.0..=420.0)
                .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(10, 6)))
                .show(ui, |ui| browser::show(ui, &mut self.session, settings, &mut self.thumbs));
        }
        if settings.show_inspector {
            let display = self.display_rows(active.as_ref());
            let path = active.as_ref().and_then(|(key, series)| {
                let state = self.views.get(key)?;
                let info = series.frames.get(state.slice)?;
                Some(self.session.series(*key)?.instances.get(info.instance)?.path.clone())
            });
            Panel::right("inspector")
                .resizable(true)
                .default_size(360.0)
                .size_range(260.0..=640.0)
                .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(10, 6)))
                .show(ui, |ui| {
                    self.inspector.show(ui, waker, path.as_deref(), &display, settings.hide_patient_details)
                });
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.viewports(ui, settings, active.as_ref(), layout));
    }

    fn toolbar(
        &mut self,
        ui: &mut Ui,
        settings: &mut Settings,
        actions: &mut Vec<Action>,
        active: Option<&(SeriesKey, Arc<LoadedSeries>)>,
        has_volume: bool,
    ) {
        ui.horizontal(|ui| {
            let browser_icon = if settings.show_browser { ICON_LEFT_PANEL_CLOSE } else { ICON_LEFT_PANEL_OPEN };
            if ui.button(icon(browser_icon)).on_hover_text("Series browser (⌘B)").clicked() {
                settings.show_browser = !settings.show_browser;
            }
            ui.separator();
            for l in Layout::ALL {
                let enabled = !l.needs_volume() || has_volume;
                let reason = active
                    .and_then(|(_, s)| s.volume_issue.as_ref().map(ToString::to_string))
                    .unwrap_or_else(|| "Needs a stack of at least 3 parallel slices".into());
                let resp = ui
                    .add_enabled(
                        enabled,
                        egui::Button::selectable(self.layout == l && (enabled || l == Layout::Single), l.label()),
                    )
                    .on_disabled_hover_text(format!("Multiplanar reformatting unavailable: {reason}"));
                if resp.clicked() {
                    self.layout = l;
                    settings.default_layout = l;
                }
            }
            ui.separator();
            for (tool, ic) in
                [(Tool::WindowLevel, ICON_CONTRAST), (Tool::Pan, ICON_PAN_TOOL), (Tool::Zoom, ICON_ZOOM_IN)]
            {
                let hint = match tool {
                    Tool::WindowLevel => "Contrast: drag ↔ width, ↕ level (also right-drag)",
                    Tool::Pan => "Pan: drag (also middle-drag or Shift-drag)",
                    Tool::Zoom => "Zoom: drag ↕ (also pinch or ⌘-scroll)",
                };
                if ui
                    .add(egui::Button::selectable(self.tool == tool, format!("{} {}", icon(ic), tool.label())))
                    .on_hover_text(hint)
                    .clicked()
                {
                    self.tool = tool;
                }
            }
            ui.separator();

            if let Some((key, series)) = active
                && let Some(state) = self.views.get_mut(key)
            {
                if !series.is_color {
                    let presets = ViewState::presets(series);
                    let preset_label = ui.label(RichText::new("Window").small());
                    ComboBox::from_id_salt("preset")
                        .selected_text(state.preset.clone().unwrap_or_else(|| "Custom".into()))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            let mut last = None;
                            for (i, preset) in presets.iter().enumerate() {
                                if last.is_some() && last != Some(preset.source) {
                                    ui.separator();
                                }
                                last = Some(preset.source);
                                let hint = match preset.source {
                                    PresetSource::Dicom => "Stored in the images",
                                    PresetSource::Clinical => "Standard CT window",
                                    PresetSource::Auto => "From image statistics",
                                    PresetSource::FullRange => "All values",
                                };
                                let label = if i < 9 {
                                    format!("{}   {}", preset.name, i + 1)
                                } else {
                                    preset.name.to_string()
                                };
                                if ui
                                    .selectable_label(state.preset.as_deref() == Some(&preset.name), label)
                                    .on_hover_text(hint)
                                    .clicked()
                                {
                                    state.apply_preset(preset);
                                }
                            }
                        })
                        .response
                        .labelled_by(preset_label.id);
                    let (lo, hi) = series.value_range;
                    let span = f64::from(hi - lo).max(1.0);
                    let w_label = ui.label(RichText::new("W").small()).on_hover_text("Window width (contrast)");
                    if ui
                        .add(
                            DragValue::new(&mut state.window.width)
                                .range(1.0..=span * 4.0)
                                .speed(span / 500.0)
                                .max_decimals(0),
                        )
                        .labelled_by(w_label.id)
                        .changed()
                    {
                        state.preset = None;
                    }
                    let l_label = ui.label(RichText::new("L").small()).on_hover_text("Window level (brightness)");
                    if ui
                        .add(
                            DragValue::new(&mut state.window.center)
                                .range(f64::from(lo) - span..=f64::from(hi) + span)
                                .speed(span / 500.0)
                                .max_decimals(0),
                        )
                        .labelled_by(l_label.id)
                        .changed()
                    {
                        state.preset = None;
                    }
                }
                ui.toggle_value(&mut state.invert, icon(ICON_INVERT_COLORS)).on_hover_text("Invert grey scale (I)");
                if ui.button(icon(ICON_RESTART_ALT)).on_hover_text("Reset contrast, zoom and pan (R)").clicked() {
                    state.reset(series);
                }
                if layout_has_mpr(self.layout) && has_volume {
                    ui.separator();
                    let mut mode = state.slab.map(|s| s.mode);
                    let slab_label = ui.label(RichText::new("Slab").small());
                    ComboBox::from_id_salt("slab")
                        .selected_text(mode.map_or("Thin slice", SlabMode::label))
                        .width(96.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut mode, None, "Thin slice");
                            for m in SlabMode::ALL {
                                ui.selectable_value(&mut mode, Some(m), m.label());
                            }
                        })
                        .response
                        .labelled_by(slab_label.id)
                        .on_hover_text("Thick-slab projection for reformatted views");
                    if mode.is_some() {
                        ui.add(Slider::new(&mut self.slab_thickness, 2.0..=100.0).suffix(" mm").max_decimals(0))
                            .labelled_by(slab_label.id);
                    }
                    state.set_slab(mode, self.slab_thickness);
                }
            }

            ui.with_layout(ELayout::right_to_left(Align::Center), |ui| {
                let inspector_icon =
                    if settings.show_inspector { ICON_RIGHT_PANEL_CLOSE } else { ICON_RIGHT_PANEL_OPEN };
                if ui.button(icon(inspector_icon)).on_hover_text("Image information (⌘I)").clicked() {
                    settings.show_inspector = !settings.show_inspector;
                }
                if ui.button(icon(ICON_SETTINGS)).on_hover_text("Settings").clicked() {
                    actions.push(Action::ShowSettings);
                }
                let overlay_icon = if settings.show_overlays { ICON_VISIBILITY } else { ICON_VISIBILITY_OFF };
                if ui.button(icon(overlay_icon)).on_hover_text("Show/hide annotations (O)").clicked() {
                    settings.show_overlays = !settings.show_overlays;
                }
                ui.toggle_value(&mut settings.hide_patient_details, icon(ICON_BADGE))
                    .on_hover_text("Privacy: hide patient name, ID and birth date");
                ui.separator();
                if ui.button(icon(ICON_CLOSE)).on_hover_text("Close study").clicked() {
                    actions.push(Action::CloseStudy);
                }
                if ui
                    .button(format!("{} Open", icon(ICON_FOLDER_OPEN)))
                    .on_hover_text("Open another folder or disc (⌘O)")
                    .clicked()
                {
                    actions.push(Action::OpenDialog);
                }
            });
        });
    }

    fn viewports(
        &mut self,
        ui: &mut Ui,
        settings: &Settings,
        active: Option<&(SeriesKey, Arc<LoadedSeries>)>,
        layout: Layout,
    ) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, Color32::from_gray(8));
        let Some((key, series)) = active else {
            self.placeholder(ui, rect);
            return;
        };
        let (key, series) = (*key, series.clone());
        let Some(meta) = self.session.series(key).cloned() else {
            return;
        };
        let study = self.session.studies()[key.study].clone();
        let views = layout.views();
        let rects = grid(rect, views.len());
        let state = self.views.entry(key).or_insert_with(|| ViewState::new(&series));
        self.hover = None;
        for (i, (&kind, &r)) in views.iter().zip(&rects).enumerate() {
            self.renders.update(ui.ctx(), key, kind, &series, state, settings.smooth_interpolation);
            let input = ViewportInput {
                id: ui.scope_id().with(("viewport", kind)),
                kind,
                series: &series,
                meta: &meta,
                study: &study,
                rendered: self.renders.get(key, kind),
                tool: self.tool,
                settings,
                is_active: self.active_view == kind,
                multi_view: views.len() > 1,
                show_details: i == 0,
            };
            let out = viewport::show(ui, r, &input, state);
            if out.activated {
                self.active_view = kind;
            }
            if let Some(h) = out.hover {
                self.hover = Some((kind, h));
                self.active_view = kind;
            }
        }
    }

    fn placeholder(&self, ui: &mut Ui, rect: Rect) {
        let p = theme::palette(ui);
        let text = match self.session.active_series.and_then(|k| self.session.state(k)) {
            Some(SeriesState::Loading(job)) => {
                let s = job.progress();
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                format!("{} {}/{}…", s.stage, s.done, s.total)
            }
            Some(SeriesState::Failed(e)) => format!("This series could not be displayed.\n{e}"),
            _ => "Select a series".to_owned(),
        };
        let center = rect.center();
        ui.put(Rect::from_center_size(center - vec2(0.0, 24.0), vec2(32.0, 32.0)), egui::Spinner::new().size(28.0));
        ui.painter().text(
            center + vec2(0.0, 14.0),
            egui::Align2::CENTER_TOP,
            text,
            egui::FontId::proportional(14.0),
            p.text_weak,
        );
    }

    fn status_bar(&self, ui: &mut Ui, active: Option<&(SeriesKey, Arc<LoadedSeries>)>) {
        let p = theme::palette(ui);
        ui.horizontal(|ui| {
            match (&self.hover, active) {
                (Some((_, h)), Some((_, series))) => {
                    let mut parts = vec![format!("x {} y {}", h.pixel.0, h.pixel.1)];
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
                        parts.push(format!(
                            "{} {}",
                            format::number(f64::from(v), 1),
                            series.units.as_deref().unwrap_or("")
                        ));
                    }
                    ui.label(RichText::new(parts.join("   ·   ")).small().monospace().color(p.text));
                }
                _ => {
                    ui.label(
                        RichText::new(
                            "Scroll: change image · Drag: contrast · Pinch/⌘-scroll: zoom · Double-click: fit",
                        )
                        .small()
                        .color(p.text_weak),
                    );
                }
            }
            ui.with_layout(ELayout::right_to_left(Align::Center), |ui| {
                let index = self.session.index();
                let source = match &index.source {
                    DiscoverySource::Dicomdir(_) => "DICOMDIR",
                    DiscoverySource::FolderScan => "Folder scan",
                    DiscoverySource::SingleFile => "Single file",
                };
                ui.label(RichText::new(format!("{} images · {source}", index.stats.images)).small().color(p.text_weak))
                    .on_hover_text(index.root.display().to_string());
                if !index.warnings.is_empty() {
                    let warn = ui.label(
                        RichText::new(format!("{} {}", icon(ICON_WARNING), index.warnings.len())).small().color(p.warn),
                    );
                    warn.on_hover_text(index.warnings.join("\n"));
                }
                if let Some((_, series)) = active
                    && let Some(issue) = &series.volume_issue
                    && series.len() > 2
                    && !series.is_color
                {
                    ui.label(RichText::new(format!("{} MPR off", icon(ICON_INFO))).small().color(p.text_weak))
                        .on_hover_text(issue.to_string());
                }
            });
        });
    }

    fn display_rows(&self, active: Option<&(SeriesKey, Arc<LoadedSeries>)>) -> Vec<(String, String)> {
        let Some((key, series)) = active else { return Vec::new() };
        let Some(state) = self.views.get(key) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        rows.push(("Image".into(), format!("{} of {}", state.slice + 1, series.len())));
        if !series.is_color {
            let units = series.units.as_deref().map(|u| format!(" {u}")).unwrap_or_default();
            rows.push((
                "Window".into(),
                format!(
                    "W {} / L {}{units} ({})",
                    format::number(state.window.width, 1),
                    format::number(state.window.center, 1),
                    state.preset.as_deref().unwrap_or("custom")
                ),
            ));
            rows.push(("Value range".into(), format!("{} … {}{units}", series.value_range.0, series.value_range.1)));
        }
        if let Some(v) = series.volume() {
            let s = v.geometry.spacing();
            rows.push((
                "Volume".into(),
                format!("{} × {} × {} voxels", v.geometry.dims[0], v.geometry.dims[1], v.geometry.dims[2]),
            ));
            rows.push(("Voxel size".into(), format!("{:.2} × {:.2} × {:.2} mm", s.x, s.y, s.z)));
            let tilt = v.geometry.shear_degrees();
            if tilt > 0.1 {
                rows.push(("Gantry tilt".into(), format!("{tilt:.1}° (corrected in reformats)")));
            }
        } else if let Some(issue) = &series.volume_issue {
            rows.push(("Reformatting".into(), format!("Unavailable — {issue}")));
        }
        rows.push(("Memory".into(), format_bytes(series.byte_size())));
        rows
    }

    fn handle_keys(
        &mut self,
        ui: &mut Ui,
        key: SeriesKey,
        series: &LoadedSeries,
        settings: &mut Settings,
        has_volume: bool,
    ) {
        let ctx = ui.ctx().clone();
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let kind = self.active_view;
        let Some(state) = self.views.get_mut(&key) else { return };
        let pressed = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        let step = |state: &mut ViewState, d: i64| state.scroll(kind, series, d);
        if pressed(Key::ArrowUp) || pressed(Key::ArrowLeft) {
            step(state, -1);
        }
        if pressed(Key::ArrowDown) || pressed(Key::ArrowRight) {
            step(state, 1);
        }
        if pressed(Key::PageUp) {
            step(state, -10);
        }
        if pressed(Key::PageDown) {
            step(state, 10);
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
        if pressed(Key::M) && has_volume {
            self.layout = if self.layout == Layout::Single { Layout::Mpr } else { Layout::Single };
        }
        let digits =
            [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        if let Some(n) = digits.iter().position(|&k| pressed(k))
            && let Some(preset) = ViewState::presets(series).get(n)
        {
            state.apply_preset(preset);
        }
        if pressed(Key::A)
            && let Some(preset) = ViewState::presets(series).into_iter().find(|p| p.source == PresetSource::Auto)
        {
            state.apply_preset(&preset);
        }
    }
}

fn format_bytes(bytes: usize) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / f64::from(1u32 << 30)),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / f64::from(1u32 << 20)),
        b => format!("{:.0} KB", (b as f64 / 1024.0).ceil()),
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
