//! Left panel: study header and the series list with thumbnails.

use std::collections::HashMap;

use egui::{
    Color32, ColorImage, CornerRadius, Label, ProgressBar, RichText, ScrollArea, Sense, Shape, TextureHandle,
    TextureOptions, Ui, vec2,
};
use egui_material_icons::icons::{ICON_ERROR, ICON_WARNING};
use summrise_core::dicom::{Study, format};
use summrise_core::{SeriesKey, SeriesState, Session, Settings};

use crate::theme;
use crate::widgets::{chip, icon, section_title};

#[derive(Default)]
pub(crate) struct ThumbnailTextures {
    map: HashMap<SeriesKey, TextureHandle>,
}

impl ThumbnailTextures {
    fn get(&mut self, ui: &Ui, session: &Session, key: SeriesKey) -> Option<TextureHandle> {
        if let Some(t) = self.map.get(&key) {
            return Some(t.clone());
        }
        let thumb = session.thumbnail(key)?;
        let image = ColorImage::from_rgba_unmultiplied([thumb.width as usize, thumb.height as usize], &thumb.rgba);
        let tex = ui.ctx().load_texture(format!("thumb-{}-{}", key.study, key.series), image, TextureOptions::LINEAR);
        self.map.insert(key, tex.clone());
        Some(tex)
    }
}

fn study_label(study: &Study) -> String {
    let date = study.date.as_deref().and_then(format::date).unwrap_or_else(|| "Undated".into());
    format!("{date} · {}", study.title())
}

pub(crate) fn modality_color(modality: &str) -> Color32 {
    match modality {
        "CT" => Color32::from_rgb(0x5E, 0xB0, 0xFF),
        "MR" => Color32::from_rgb(0xB0, 0x8C, 0xFF),
        "PT" | "NM" => Color32::from_rgb(0xFF, 0x9F, 0x5A),
        "US" => Color32::from_rgb(0x4C, 0xD6, 0xC2),
        "CR" | "DX" | "MG" | "XA" | "RF" => Color32::from_rgb(0xE0, 0xC0, 0x60),
        _ => Color32::from_rgb(0x9A, 0xA3, 0xB2),
    }
}

/// Draws the browser. Selecting a study or series updates the session.
pub(crate) fn show(ui: &mut Ui, session: &mut Session, settings: &Settings, thumbs: &mut ThumbnailTextures) {
    let p = theme::palette(ui);
    ui.add_space(6.0);

    if session.studies().len() > 1 {
        let current = session.active_study;
        let mut pick = None;
        egui::ComboBox::from_id_salt("study-picker")
            .width(ui.available_width())
            .selected_text(study_label(&session.studies()[current]))
            .show_ui(ui, |ui| {
                for (i, s) in session.studies().iter().enumerate() {
                    if ui.selectable_label(i == current, study_label(s)).clicked() {
                        pick = Some(i);
                    }
                }
            });
        if let Some(i) = pick {
            session.select_study(i);
        }
        ui.add_space(4.0);
    }

    let study = session.study().clone();
    ui.add(Label::new(RichText::new(study.title()).size(15.0).strong().color(p.text)).wrap());
    let mut meta = Vec::new();
    if let Some(d) = study.date.as_deref().and_then(format::date) {
        meta.push(d);
    }
    meta.push(format!("{} series · {} images", study.series.len(), study.image_count()));
    ui.label(RichText::new(meta.join(" · ")).small().color(p.text_weak));
    if !settings.hide_patient_details {
        let pt = &study.patient;
        let mut who = vec![pt.name.as_deref().map(format::person_name).unwrap_or_else(|| "Unknown patient".into())];
        who.extend(pt.age.as_deref().and_then(format::age));
        who.extend(pt.sex.as_deref().map(format::sex));
        ui.label(RichText::new(who.join(" · ")).small().color(p.text_weak));
    }
    ui.horizontal_wrapped(|ui| {
        for m in study.modalities() {
            chip(ui, m, modality_color(m));
        }
    });
    ui.add_space(4.0);
    ui.separator();
    section_title(ui, "Series");

    let active_study = session.active_study;
    let mut clicked = None;
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (i, series) in study.series.iter().enumerate() {
            let key = SeriesKey { study: active_study, series: i };
            let selected = session.active_series == Some(key);
            let bg = ui.painter().add(Shape::Noop);
            let inner = ui.horizontal(|ui| {
                ui.add_space(4.0);
                let (rect, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius::same(6), Color32::BLACK);
                if let Some(tex) = thumbs.get(ui, session, key) {
                    let size = tex.size_vec2();
                    let scale = (rect.width() / size.x).min(rect.height() / size.y);
                    let r = egui::Rect::from_center_size(rect.center(), size * scale);
                    ui.painter().image(
                        tex.id(),
                        r,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.add(Label::new(RichText::new(series.title()).strong()).truncate());
                    let mut line = Vec::new();
                    if let Some(n) = series.number {
                        line.push(format!("#{n}"));
                    }
                    line.push(series.modality.clone());
                    if let Some(plane) = series.plane() {
                        line.push(plane.label().to_owned());
                    } else if series.has_mixed_planes() {
                        line.push("Mixed planes".into());
                    }
                    ui.label(RichText::new(line.join(" · ")).small().color(p.text_weak));
                    let dims = series.dimensions().map(|(w, h)| format!(" · {w}×{h}")).unwrap_or_default();
                    // While a series loads (or waits, partly loaded), the count shows how far it is.
                    let loading = match session.state(key) {
                        Some(SeriesState::Loading(loading)) => Some((loading.data.loaded(), loading.is_paused())),
                        _ => None,
                    };
                    let total = series.frame_count();
                    let count = loading.map_or_else(|| total.to_string(), |(loaded, _)| format!("{loaded} / {total}"));
                    ui.label(RichText::new(format!("{count} images{dims}")).small().color(p.text_weak));
                    let unsupported = series.unsupported_transfer_syntaxes();
                    if !unsupported.is_empty() {
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                icon(ICON_WARNING),
                                format::transfer_syntax_name(&unsupported[0])
                            ))
                            .small()
                            .color(p.warn),
                        )
                        .on_hover_text("This compression is not supported by this build");
                    }
                    match session.state(key) {
                        Some(SeriesState::Loading(_)) => {
                            let (loaded, paused) = loading.unwrap_or_default();
                            let hint = if paused {
                                format!("Paused at {loaded} / {total} — select the series to continue loading")
                            } else {
                                format!("Loading {loaded} / {total}")
                            };
                            ui.add(
                                ProgressBar::new(loaded as f32 / total.max(1) as f32)
                                    .desired_height(4.0)
                                    .desired_width(ui.available_width() - 8.0),
                            )
                            .on_hover_text(hint);
                        }
                        Some(SeriesState::Failed(e)) => {
                            ui.label(
                                RichText::new(format!("{} Could not decode", icon(ICON_ERROR))).small().color(p.error),
                            )
                            .on_hover_text(e);
                        }
                        _ => {}
                    }
                });
            });
            let rect = inner.response.rect.expand2(vec2(2.0, 4.0));
            let response = ui.interact(rect, ui.scope_id().with(("series", i)), Sense::click());
            let fill = if selected {
                p.accent.gamma_multiply(0.25)
            } else if response.hovered() {
                p.surface_hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().set(bg, Shape::rect_filled(rect, CornerRadius::same(8), fill));
            if response.clicked() {
                clicked = Some(key);
            }
            response.on_hover_cursor(egui::CursorIcon::PointingHand);
            ui.add_space(6.0);
        }
    });
    if let Some(key) = clicked {
        session.select_series(key);
    }
}
