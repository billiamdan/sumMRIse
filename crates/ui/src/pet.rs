//! PET in the viewer: the colour maps of PET and nuclear-medicine series and
//! the PET drawn over a CT or MR (fusion) — the toolbar menu, the keys C and
//! F, the colour map and the overlay in the render paths, what the viewport
//! says, and the read-out of both values.
//!
//! What is fused and how is view state (`summrise_core::pet`); the session
//! keeps the overlay series loaded (`Session::set_overlay`). The overlay is
//! drawn into the picture of the view, so `placement.rs` zooms, pans, turns
//! and mirrors both series alike, and the read-out samples the overlay at the
//! patient position under the pointer with the function that drew it.

use std::collections::HashMap;
use std::sync::Arc;

use egui::containers::menu::{MenuButton, MenuConfig};
use egui::{
    Align, Button, Color32, ComboBox, Context, DragValue, Key, Label, Layout, Modifiers, Painter, PopupCloseBehavior,
    Rect, RichText, Sense, Slider, Stroke, StrokeKind, Ui, pos2, vec2,
};
use egui_material_icons::icons::ICON_LAYERS;
use summrise_core::dicom::FrameRef;
use summrise_core::dicom::format::number;
use summrise_core::imaging::colormap::{self, Colormap};
use summrise_core::imaging::fusion::{self, PixelGrid};
use summrise_core::imaging::{DVec3, ImagePlane, MprImage, OverlayLook, Window};
use summrise_core::pet::{self as uptake, Fusion, has_colormaps, value_text};
use summrise_core::{OverlayStatus, SeriesData, SeriesKey, Session, ViewState};
use summrise_i18n::tr;

use crate::orient;
use crate::theme;
use crate::viewport::ViewportInput;
use crate::widgets::icon;

/// The series drawn over the displayed one, as the render paths get it.
#[derive(Clone, Copy)]
pub(crate) struct Overlay<'a> {
    pub key: SeriesKey,
    pub data: &'a SeriesData,
}

impl<'a> Overlay<'a> {
    /// What to draw for a view with this state: the overlay's volume and
    /// look, if the view draws this overlay and its colour scale is known.
    fn drawn(self, state: &ViewState) -> Option<(&'a SeriesData, OverlayLook)> {
        let fusion = state.fusion().filter(|f| f.series == self.key)?;
        self.data.volume()?;
        Some((self.data, fusion.look()?))
    }
}

/// A CT or MR the user asked to see under a PET, with the overlay it gets
/// as soon as its view exists.
pub(crate) type Pending = Option<(SeriesKey, Fusion)>;

// ---------------------------------------------------------------------------
// Rendering

/// What a rendered picture owes to the PET display: the colour map, and the
/// overlay with how it is drawn. Part of the render key, so that a change
/// renders the picture again.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Look {
    colormap: Colormap,
    overlay: Option<(SeriesKey, OverlayLook)>,
}

impl Look {
    pub(crate) fn of(state: &ViewState, overlay: Option<Overlay<'_>>) -> Self {
        let overlay = overlay.and_then(|o| Some((o.key, o.drawn(state)?.1)));
        Self { colormap: state.colormap(), overlay }
    }

    /// Whether a PET is drawn into the picture.
    #[cfg(test)]
    pub(crate) fn draws_overlay(&self) -> bool {
        self.overlay.is_some()
    }
}

/// The colour map of the series and the PET drawn over it, applied to the
/// picture of an acquired image: one call in the frame render path.
pub(crate) fn compose_frame(
    rgba: &mut [u8],
    frame: FrameRef<'_>,
    plane: Option<&ImagePlane>,
    state: &ViewState,
    overlay: Option<Overlay<'_>>,
) {
    if let FrameRef::Gray { data, .. } = frame {
        compose(rgba, |i| data.get(i), plane.map(PixelGrid::of_plane), state, overlay);
    }
}

/// The same for a reformatted picture: one call in the reformat render path.
pub(crate) fn compose_reformat(rgba: &mut [u8], image: &MprImage, state: &ViewState, overlay: Option<Overlay<'_>>) {
    compose(rgba, |i| image.values[i], Some(PixelGrid::of_reformat(image)), state, overlay);
}

fn compose(
    rgba: &mut [u8],
    value: impl Fn(usize) -> f32 + Sync,
    grid: Option<PixelGrid>,
    state: &ViewState,
    overlay: Option<Overlay<'_>>,
) {
    colormap::recolour(rgba, value, state.colormap());
    if let (Some(grid), Some((data, look))) = (grid, overlay.and_then(|o| o.drawn(state)))
        && let Some(volume) = data.volume()
    {
        fusion::blend(rgba, &grid, volume, &look);
    }
}

// ---------------------------------------------------------------------------
// Keeping the session in step

/// Once per frame, before the views are drawn: gives a CT or MR put under a
/// PET its overlay as soon as its view exists, tells the session which
/// series the displayed one draws over it, and fixes the overlay's colour
/// scale when its first image is decoded. Returns the overlay to draw, once
/// it is ready.
pub(crate) fn sync(
    session: &mut Session,
    views: &mut HashMap<SeriesKey, ViewState>,
    pending: &mut Pending,
) -> Option<(SeriesKey, Arc<SeriesData>)> {
    let active = session.active_series;
    if let Some((base, fusion)) = pending.take()
        && Some(base) == active
    {
        match views.get_mut(&base) {
            Some(state) => state.set_fusion(Some(fusion)),
            None => *pending = Some((base, fusion)),
        }
    }
    let mut state = active.and_then(|key| views.get_mut(&key));
    let wanted = match &state {
        Some(state) => state.fusion().map(|f| f.series),
        None => pending.as_ref().map(|(_, fusion)| fusion.series),
    };
    session.set_overlay(wanted);
    let key = session.overlay()?;
    if let (Some(state), Some(data)) = (state.as_mut(), session.data(key)) {
        state.settle_fusion(&data);
    }
    match session.overlay_status()? {
        OverlayStatus::Ready(data) => Some((key, data)),
        OverlayStatus::Loading { .. } | OverlayStatus::Unavailable(_) => None,
    }
}

/// Shows the CT or MR `base` with the PET overlay `fusion`, from the PET.
pub(crate) fn show_over(session: &mut Session, pending: &mut Pending, (base, fusion): (SeriesKey, Fusion)) {
    session.select_series(base);
    *pending = Some((base, fusion));
}

// ---------------------------------------------------------------------------
// What the viewport says

/// What a viewport says about the PET drawn over it.
#[derive(Clone)]
pub(crate) struct Shown {
    /// The annotation: "PET overlay 50 % · Hot iron · SUV 0–5", when it is drawn.
    line: Option<String>,
    /// Loading progress, or that the overlay is unavailable, for a badge on
    /// the picture (shown whether annotations are on or not), and its
    /// explanation: the full reason.
    status: Option<(String, String)>,
}

/// What the viewport of a view with `state` says about its PET overlay.
pub(crate) fn shown(session: &Session, state: Option<&ViewState>) -> Option<Shown> {
    let fusion = state?.fusion()?;
    let status = match session.overlay_status()? {
        OverlayStatus::Loading { loaded, total } => Some((
            tr!("PET loading {loaded} / {total}", loaded = loaded, total = total),
            tr!("The PET is loaded in the background and drawn over the images when all of its images are there.")
                .to_owned(),
        )),
        OverlayStatus::Ready(_) => None,
        OverlayStatus::Unavailable(problem) => {
            Some((tr!("PET overlay unavailable: {why}", why = problem.summary()), problem.reason()))
        }
    };
    let scale = scale_label(session, fusion);
    let line = tr!(
        "PET overlay {opacity} % · {colours} · {scale}",
        opacity = number(f64::from(fusion.opacity) * 100.0, 0),
        colours = fusion.colormap.label(),
        scale = scale
    );
    Some(Shown { line: Some(line), status })
}

/// What a viewport of a multi-series layout says that is not the active one,
/// when a PET was set to be drawn over its series. The session keeps one
/// overlay, that of the active viewport's series, so this viewport shows its
/// images alone, and says so rather than show an overlay that may be out of
/// date.
pub(crate) fn shown_elsewhere(state: Option<&ViewState>) -> Option<Shown> {
    state?.fusion()?;
    let badge = tr!("PET overlay only in the active viewport");
    let hint = tr!("A PET is drawn over one viewport at a time. Click this viewport to see its PET overlay.");
    Some(Shown { line: None, status: Some((badge.to_owned(), hint.to_owned())) })
}

/// The name of the scale of a PET overlay as shown: a window stored in the
/// PET's images under its own name, a built-in one translated, "custom" for
/// one set by hand.
fn scale_label(session: &Session, fusion: &Fusion) -> String {
    let Some(name) = fusion.scale.as_ref().and_then(|(_, name)| name.as_deref()) else {
        return tr!("custom").to_owned();
    };
    let stored = session.data(fusion.series).is_some_and(|data| data.presets().iter().any(|p| p.name == name));
    if stored { name.to_owned() } else { summrise_core::imaging::window::builtin_label(name).to_owned() }
}

/// Annotation lines of a view about the PET display: the colour map of a PET
/// or nuclear-medicine series, the overlay and what its positions rest on.
pub(crate) fn annotation_lines(series: &SeriesData, state: &ViewState, shown: Option<&Shown>) -> Vec<String> {
    let mut lines = Vec::new();
    if has_colormaps(series.modality()) && !series.is_color() {
        lines.push(tr!("Colours: {map}", map = state.colormap().label()));
    }
    if let Some(line) = shown.and_then(|shown| shown.line.as_ref()) {
        lines.push(line.clone());
        lines.push(tr!("Aligned by scanner positions only").to_owned());
    }
    lines
}

/// The window in an annotation: "W 5  L 2.5 SUV" for uptake values (with
/// their decimals), "W 400  L 40 HU" otherwise (as before).
pub(crate) fn window_annotation(units: Option<&str>, window: &Window) -> String {
    let (width, level) = if uptake::is_suv(units) {
        (number(window.width, 2), number(window.center, 2))
    } else {
        (format!("{:.0}", window.width), format!("{:.0}", window.center))
    };
    // Translators: the window on the image: W its width, L its level (as on scanner annotations), then the unit.
    match units {
        Some(unit) => tr!("W {width}  L {level} {unit}", width = width, level = level, unit = unit),
        None => tr!("W {width}  L {level}", width = width, level = level),
    }
}

/// The value range in the inspector: with two decimals for uptake values,
/// as before otherwise.
pub(crate) fn value_range_text(series: &SeriesData) -> String {
    let (lo, hi) = series.value_range();
    let (low, high) = if uptake::is_suv(series.units()) {
        (number(f64::from(lo), 2), number(f64::from(hi), 2))
    } else {
        (lo.to_string(), hi.to_string())
    };
    match series.units() {
        Some(unit) => tr!("{low} … {high} {unit}", low = low, high = high, unit = unit),
        None => tr!("{low} … {high}", low = low, high = high),
    }
}

/// The overlay's loading progress, or that it is unavailable, as a status
/// badge on the picture, whether annotations are shown or not; the full
/// reason shows on hover. Its row is below those of the "Mirrored" badge
/// (row 0) and of the lossy-compression mark (row 1).
pub(crate) fn draw_status(ui: &Ui, painter: &Painter, viewport: Rect, picture: Rect, input: &ViewportInput<'_>) {
    let Some((text, hint)) = input.fusion.and_then(|s| s.status.as_ref()) else { return };
    let id = input.id.with("pet-overlay");
    orient::badge(ui, painter, viewport, picture, id, 2, input.multi_view, text, text, hint);
}

/// The value part of the read-out under the pointer: "SUV 3.1" for a PET,
/// "CT 40 HU · PET SUV 3.1" while a PET is drawn over a CT, "40 HU" alone.
pub(crate) fn readout(session: &Session, series: &SeriesData, value: f32, patient: Option<DVec3>) -> String {
    let base = value_text(series.units(), f64::from(value));
    let Some(OverlayStatus::Ready(overlay)) = session.overlay_status() else { return base };
    let uptake = overlay.volume().zip(patient).and_then(|(volume, p)| fusion::sample_at(volume, p));
    match (uptake, session.active()) {
        (Some(uptake), Some((_, meta))) => tr!(
            "{modality} {value} · PET {uptake}",
            modality = meta.modality.trim(),
            value = base,
            uptake = value_text(overlay.units(), f64::from(uptake))
        ),
        _ => base,
    }
}

// ---------------------------------------------------------------------------
// Keys and the toolbar menu

/// C cycles the colour map of the PET — of a PET or nuclear-medicine series,
/// or of the PET drawn over a CT or MR. F switches the PET overlay of a CT or
/// MR on (the first PET) and off; on a PET it asks to show the PET over the
/// first CT or MR, which is returned.
pub(crate) fn keys(
    ctx: &Context,
    session: &Session,
    key: SeriesKey,
    series: &SeriesData,
    state: &mut ViewState,
) -> Option<(SeriesKey, Fusion)> {
    let pressed = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
    let pet_like = has_colormaps(series.modality()) && !series.is_color();
    if pressed(Key::C) {
        if pet_like {
            state.set_colormap(state.colormap().next());
        } else if let Some(fusion) = state.fusion_mut() {
            fusion.colormap = fusion.colormap.next();
        }
    }
    if pressed(Key::F) {
        if pet_like {
            return session.underlay_candidates(key).first().map(|&base| (base, Fusion::from_view(key, state)));
        }
        let fusion = match state.fusion() {
            Some(_) => None,
            None => session.overlay_candidates(key).first().copied().map(Fusion::new),
        };
        state.set_fusion(fusion);
    }
    None
}

/// The toolbar menu "PET colours and fusion", shown for a PET or
/// nuclear-medicine series and for a series that a PET can be drawn over. It
/// stays open while its entries are used. Returns a CT or MR to show the PET
/// over (chosen from the PET), with the overlay it gets.
pub(crate) fn menu(
    ui: &mut Ui,
    session: &Session,
    key: SeriesKey,
    series: &SeriesData,
    state: &mut ViewState,
) -> Option<(SeriesKey, Fusion)> {
    let pet_like = has_colormaps(series.modality()) && !series.is_color();
    let overlays = session.overlay_candidates(key);
    let bases = if pet_like { session.underlay_candidates(key) } else { Vec::new() };
    if !pet_like && overlays.is_empty() {
        return None;
    }
    let mut request = None;
    let config = MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside);
    let (button, _) = MenuButton::new(icon(ICON_LAYERS)).config(config).ui(ui, |ui| {
        ui.set_min_width(270.0);
        ui.set_max_width(360.0);
        if pet_like {
            heading(ui, tr!("Colour map (C)"));
            let mut map = state.colormap();
            colormap_entries(ui, &mut map);
            state.set_colormap(map);
        }
        if !bases.is_empty() {
            ui.separator();
            // Translators: a PET drawn in colour over a CT or MR of the same study (image fusion).
            heading(ui, tr!("Fusion"));
            request = base_entries(ui, session, key, state, &bases);
        }
        if !overlays.is_empty() {
            overlay_entries(ui, session, state, &overlays);
        }
    });
    button
        .accessible_name(tr!("PET colours and fusion"))
        .on_hover_text(tr!("Colours of a PET, and a PET drawn over a CT or MR (C, F)"));
    request
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().strong().color(theme::palette(ui).text_weak));
}

/// A note in the menu, wrapped, in the weak colour.
fn note(ui: &mut Ui, text: &str) {
    let weak = theme::palette(ui).text_weak;
    // Left-aligned: a menu stretches its entries, which would space the words out.
    ui.with_layout(Layout::top_down(Align::Min), |ui| {
        ui.add(Label::new(RichText::new(text).small().color(weak)).wrap());
    });
}

/// What each colour map shows, for its tooltip.
fn colormap_hint(map: Colormap) -> &'static str {
    match map {
        Colormap::Grey => tr!("Low values dark, high values bright"),
        Colormap::InvertedGrey => tr!("Low values bright, high values dark: the usual way to show PET"),
        Colormap::HotIron => tr!("From black through red, orange and yellow to white"),
        Colormap::Rainbow => tr!("From black through blue, cyan, green and yellow to red"),
    }
}

/// A strip of the colours of `map`, from the low end to the high end.
fn swatch(ui: &mut Ui, map: Colormap) {
    let (rect, _) = ui.allocate_exact_size(vec2(44.0, 12.0), Sense::hover());
    let steps = 22;
    for step in 0..steps {
        let [r, g, b] = map.at(f64::from(step) / f64::from(steps - 1));
        let left = rect.left() + rect.width() * step as f32 / steps as f32;
        let right = rect.left() + rect.width() * (step + 1) as f32 / steps as f32;
        let piece = Rect::from_min_max(pos2(left, rect.top()), pos2(right, rect.bottom()));
        ui.painter().rect_filled(piece, 0.0, Color32::from_rgb(r, g, b));
    }
    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, theme::palette(ui).border), StrokeKind::Outside);
}

/// One entry per colour map, each with a strip of its colours.
fn colormap_entries(ui: &mut Ui, map: &mut Colormap) {
    for candidate in Colormap::ALL {
        ui.horizontal(|ui| {
            swatch(ui, candidate);
            ui.selectable_value(map, candidate, candidate.label()).on_hover_text(colormap_hint(candidate));
        });
    }
}

/// From a PET: "Show over …" for every CT or MR of its Frame of Reference.
fn base_entries(
    ui: &mut Ui,
    session: &Session,
    pet: SeriesKey,
    state: &ViewState,
    bases: &[SeriesKey],
) -> Option<(SeriesKey, Fusion)> {
    let mut request = None;
    for (n, &base) in bases.iter().enumerate() {
        let title = session.series(base).map(|s| s.title()).unwrap_or_default();
        let name = tr!("Show over {series}", series = title);
        let button = Button::new(name.as_str()).shortcut_text(if n == 0 { "F" } else { "" });
        let hint = tr!("Opens {series} with this PET drawn over it", series = title);
        if ui.add(button).accessible_name(name.as_str()).on_hover_text(hint).clicked() {
            request = Some((base, Fusion::from_view(pet, state)));
            ui.close();
        }
    }
    note(ui, positions_note());
    request
}

/// The note on what fusion rests on.
fn positions_note() -> &'static str {
    tr!(
        "The PET is placed by the positions the scanner recorded; nothing is aligned afterwards. Movement or breathing between the two scans can shift it against the anatomy."
    )
}

/// On a CT or MR: which PET is drawn over it, and how.
fn overlay_entries(ui: &mut Ui, session: &Session, state: &mut ViewState, overlays: &[SeriesKey]) {
    heading(ui, tr!("PET overlay (F)"));
    let current = state.fusion().map(|f| f.series);
    let mut choice = current;
    ui.selectable_value(&mut choice, None, tr!("No PET overlay")).on_hover_text(tr!("Show these images alone"));
    for &pet in overlays {
        let title = session.series(pet).map(|s| s.title()).unwrap_or_default();
        let hint = tr!("Draw {series} over these images", series = title);
        ui.selectable_value(&mut choice, Some(pet), title).on_hover_text(hint);
    }
    if choice != current {
        state.set_fusion(choice.map(Fusion::new));
    }
    if let Some(fusion) = state.fusion_mut() {
        ui.separator();
        overlay_settings(ui, session, fusion);
    }
    note(ui, positions_note());
}

/// How the PET is drawn: its status, opacity, colour map, colour scale and
/// threshold (the last two once its first image is decoded).
fn overlay_settings(ui: &mut Ui, session: &Session, fusion: &mut Fusion) {
    match session.overlay_status() {
        Some(OverlayStatus::Loading { loaded, total }) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr!("PET loading {loaded} / {total}", loaded = loaded, total = total));
            });
        }
        Some(OverlayStatus::Unavailable(problem)) => {
            let warn = theme::palette(ui).warn;
            let text = tr!("PET overlay unavailable: {reason}", reason = problem.reason());
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.add(Label::new(RichText::new(text).color(warn)).wrap());
            });
        }
        Some(OverlayStatus::Ready(_)) | None => {}
    }

    let mut percent = f64::from(fusion.opacity) * 100.0;
    let opacity = Slider::new(&mut percent, 0.0..=100.0).suffix(" %").max_decimals(0).text(tr!("Opacity"));
    if ui.add(opacity).on_hover_text(tr!("0 % shows the images alone, 100 % covers them")).changed() {
        fusion.opacity = (percent / 100.0) as f32;
    }

    ui.horizontal(|ui| {
        let label = ui.label(tr!("Colours"));
        let mut map = fusion.colormap;
        ComboBox::from_id_salt("pet-overlay-colours")
            .selected_text(map.label())
            .width(120.0)
            .show_ui(ui, |ui| colormap_entries(ui, &mut map))
            .response
            .labelled_by(label.id)
            .on_hover_text(tr!("Colour map of the PET drawn over the images (C)"));
        fusion.colormap = map;
    });

    let Some(data) = session.data(fusion.series).filter(|data| data.is_viewable()) else { return };
    ui.horizontal(|ui| {
        // Translators: the range of PET values that the colours are spread over.
        let label = ui.label(tr!("Scale"));
        let selected = scale_label(session, fusion);
        ComboBox::from_id_salt("pet-overlay-scale")
            .selected_text(selected)
            .width(120.0)
            .show_ui(ui, |ui| {
                for preset in ViewState::presets(&data) {
                    let chosen = fusion.scale.as_ref().is_some_and(|(_, name)| name.as_deref() == Some(&preset.name));
                    if ui.selectable_label(chosen, preset.label()).clicked() {
                        fusion.scale = Some((preset.window, Some(preset.name.to_string())));
                    }
                }
            })
            .response
            .labelled_by(label.id)
            .on_hover_text(tr!("The values the colours of the PET are spread over"));
    });
    ui.horizontal(|ui| {
        let label = ui.label(tr!("Hide below"));
        let units = data.units();
        let (lo, hi) = data.value_range();
        let mut threshold = fusion.threshold().unwrap_or(f64::from(lo));
        let span = f64::from(hi - lo).max(1e-3);
        let field = DragValue::new(&mut threshold)
            .range(f64::from(lo)..=f64::from(hi))
            .clamp_existing_to_range(false)
            .speed(span / 500.0)
            .max_decimals(uptake::decimals(units).max(1))
            .suffix(units.map(|u| format!(" {u}")).unwrap_or_default());
        let response = ui.add(field).labelled_by(label.id);
        if response.on_hover_text(tr!("PET values below this are not drawn, so the images show through")).changed() {
            fusion.threshold = Some(threshold);
        }
    });
}
