//! Start screen: open a folder/disc, detected discs, recent studies.

use std::path::PathBuf;

use egui::{Align, Label, Layout, RichText, Sense, TextStyle, Ui, vec2};
use egui_material_icons::icons::{ICON_ALBUM, ICON_CLOSE, ICON_FOLDER_OPEN, ICON_HISTORY, ICON_LOCK};
use summrise_core::RecentStudies;

use crate::app::Action;
use crate::theme;
use crate::widgets::{card, icon, paint_logo, primary_button, section_title};

pub(crate) fn show(ui: &mut Ui, recent: &RecentStudies, media: &[PathBuf], actions: &mut Vec<Action>) {
    let p = theme::palette(ui);
    let width = ui.available_width().min(620.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.set_max_width(width);
            ui.add_space((ui.available_height() * 0.08).clamp(24.0, 90.0));
            let (logo, _) = ui.allocate_exact_size(vec2(84.0, 84.0), Sense::hover());
            paint_logo(ui.painter(), logo);
            ui.add_space(10.0);
            ui.label(RichText::new("SumMRIse").text_style(TextStyle::Name(theme::TITLE.into())).color(p.text));
            ui.label(
                RichText::new("View the MRI and CT scans from your hospital CD, DVD or USB stick")
                    .text_style(TextStyle::Name(theme::SUBTITLE.into()))
                    .color(p.text_weak),
            );
            ui.add_space(22.0);
            if primary_button(ui, &format!("{}  Open folder or disc…", icon(ICON_FOLDER_OPEN)), vec2(280.0, 42.0))
                .on_hover_text(format!("{}+O", if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" }))
                .clicked()
            {
                actions.push(Action::OpenDialog);
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new("…or drag a folder, DICOMDIR or DICOM file onto this window").small().color(p.text_weak),
            );
            ui.add_space(22.0);

            if !media.is_empty() {
                card(ui, |ui| {
                    ui.set_width(width - 28.0);
                    section_title(ui, "Discs and drives with images");
                    for m in media {
                        let name =
                            m.file_name().map_or_else(|| m.display().to_string(), |n| n.to_string_lossy().into_owned());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon(ICON_ALBUM)).size(18.0).color(p.accent));
                            ui.label(RichText::new(&name).strong());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("Open").clicked() {
                                    actions.push(Action::Open(m.clone()));
                                }
                            });
                        });
                    }
                });
                ui.add_space(12.0);
            }

            if !recent.entries.is_empty() {
                card(ui, |ui| {
                    ui.set_width(width - 28.0);
                    ui.horizontal(|ui| {
                        section_title(ui, "Recent");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Clear").clicked() {
                                actions.push(Action::ClearRecent);
                            }
                        });
                    });
                    for entry in &recent.entries {
                        let row = ui.horizontal(|ui| {
                            ui.label(RichText::new(icon(ICON_HISTORY)).size(18.0).color(p.text_weak));
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width() - 40.0);
                                ui.add(Label::new(RichText::new(&entry.label).strong()).truncate());
                                ui.add(
                                    Label::new(
                                        RichText::new(entry.path.display().to_string()).small().color(p.text_weak),
                                    )
                                    .truncate(),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button(icon(ICON_CLOSE)).on_hover_text("Remove from list").clicked() {
                                    actions.push(Action::RemoveRecent(entry.path.clone()));
                                }
                            });
                        });
                        let response =
                            ui.interact(row.response.rect, ui.scope_id().with(("recent", &entry.path)), Sense::click());
                        if response.clicked() {
                            actions.push(Action::Open(entry.path.clone()));
                        }
                        response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        ui.add_space(4.0);
                    }
                });
                ui.add_space(12.0);
            }

            ui.add_space(18.0);
            ui.label(
                RichText::new(format!(
                    "{}  Images are processed only on this computer — nothing is uploaded.",
                    icon(ICON_LOCK)
                ))
                .small()
                .color(p.text_weak),
            );
            ui.add(
                Label::new(
                    RichText::new(
                        "SumMRIse is intended for personal and educational viewing. It is not a medical \
                         device and must not be used for diagnosis — discuss your images with your doctor.",
                    )
                    .small()
                    .color(p.text_weak),
                )
                .wrap(),
            );
            ui.add_space(6.0);
            if ui.link(RichText::new("About SumMRIse").small()).clicked() {
                actions.push(Action::ShowAbout);
            }
            ui.add_space(24.0);
        });
    });
}
