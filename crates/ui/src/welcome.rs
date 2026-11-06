//! Start screen: open a folder/disc, detected discs, recent studies.

use std::path::PathBuf;

use egui::{Align, Label, Layout, RichText, Sense, TextStyle, Ui, vec2};
use egui_material_icons::icons::{ICON_ALBUM, ICON_CLOSE, ICON_FOLDER_OPEN, ICON_FOLDER_ZIP, ICON_HISTORY, ICON_LOCK};
use summrise_core::RecentStudies;
use summrise_i18n::{tr, tr_ctx};

use crate::app::Action;
use crate::theme;
use crate::widgets::{card, icon, paint_logo, primary_button, section_title, shortcut};

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
                RichText::new(tr!("View the MRI and CT scans from your hospital CD, DVD or USB stick"))
                    .text_style(TextStyle::Name(theme::SUBTITLE.into()))
                    .color(p.text_weak),
            );
            ui.add_space(22.0);
            let folder = format!("{}  {}", icon(ICON_FOLDER_OPEN), tr!("Open folder or disc…"));
            if primary_button(ui, &folder, vec2(280.0, 42.0)).on_hover_text(shortcut("O")).clicked() {
                actions.push(Action::OpenDialog);
            }
            ui.add_space(8.0);
            // A disc that came as a download: opened as it is, without unpacking it.
            let archive = format!("{}  {}", icon(ICON_FOLDER_ZIP), tr!("Open archive or disc image…"));
            if ui
                .add(egui::Button::new(archive).min_size(vec2(280.0, 30.0)))
                .on_hover_text(tr!(
                    "Opens a ZIP archive or an ISO disc image without unpacking it ({shortcut})",
                    shortcut = shortcut(&tr!("Shift+{key}", key = "O"))
                ))
                .clicked()
            {
                actions.push(Action::OpenArchiveDialog);
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(tr!(
                    "…or drag a folder, a ZIP archive, a disc image, a DICOMDIR or a DICOM file onto this window"
                ))
                .small()
                .color(p.text_weak),
            );
            ui.add_space(22.0);

            if !media.is_empty() {
                card(ui, |ui| {
                    ui.set_width(width - 28.0);
                    section_title(ui, tr!("Discs and drives with images"));
                    for m in media {
                        let name =
                            m.file_name().map_or_else(|| m.display().to_string(), |n| n.to_string_lossy().into_owned());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon(ICON_ALBUM)).size(18.0).color(p.accent));
                            ui.label(RichText::new(&name).strong());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button(tr!("Open")).clicked() {
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
                        // Translators: the list of recently opened studies.
                        section_title(ui, tr_ctx!("studies", "Recent"));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // Translators: empties the list of recent studies.
                            if ui.small_button(tr_ctx!("recent list", "Clear")).clicked() {
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
                                if ui.small_button(icon(ICON_CLOSE)).on_hover_text(tr!("Remove from list")).clicked() {
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
                    "{}  {}",
                    icon(ICON_LOCK),
                    tr!("Images are processed only on this computer — nothing is uploaded.")
                ))
                .small()
                .color(p.text_weak),
            );
            ui.add(
                Label::new(
                    // Translators: keep this statement unambiguous and at least as strong as the English.
                    RichText::new(tr!(
                        "SumMRIse is intended for personal and educational viewing. It is not a medical \
                         device and must not be used for diagnosis — discuss your images with your doctor."
                    ))
                    .small()
                    .color(p.text_weak),
                )
                .wrap(),
            );
            ui.add_space(6.0);
            if ui.link(RichText::new(tr!("About SumMRIse")).small()).clicked() {
                actions.push(Action::ShowAbout);
            }
            ui.add_space(24.0);
        });
    });
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    /// The welcome screen with nothing in its lists; the actions it raises are the state.
    fn welcome() -> Harness<'static, Vec<Action>> {
        welcome_in(egui::Theme::Dark)
    }

    fn welcome_in(appearance: egui::Theme) -> Harness<'static, Vec<Action>> {
        let mut builder =
            Harness::builder().with_size([900.0, 520.0]).with_theme(appearance).with_context_setup(theme::install);
        if std::env::var_os("SUMMRISE_SNAPSHOT_DIR").is_some() {
            builder = builder.wgpu();
        }
        builder.build_ui_state(
            |ui, actions: &mut Vec<Action>| {
                let fill = theme::palette(ui).bg;
                egui::Frame::new().fill(fill).inner_margin(24).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    show(ui, &RecentStudies::default(), &[], actions);
                });
            },
            Vec::new(),
        )
    }

    /// The new button in both appearances (rendered to `SUMMRISE_SNAPSHOT_DIR` for review).
    #[test]
    fn archive_action_is_offered_in_dark_and_light_appearance() {
        for (appearance, name) in [(egui::Theme::Dark, "dark"), (egui::Theme::Light, "light")] {
            let mut harness = welcome_in(appearance);
            harness.run_steps(3);
            harness.get_by_label_contains("Open archive or disc image");
            if let Some(dir) = std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(std::path::PathBuf::from) {
                std::fs::create_dir_all(&dir).expect("snapshot dir");
                let image = harness.render().expect("wgpu render");
                image.save(dir.join(format!("archive-welcome-{name}.png"))).expect("save png");
            }
        }
    }

    #[test]
    fn offers_to_open_an_archive_or_a_disc_image_next_to_the_folder() {
        let mut harness = welcome();
        harness.step();
        harness.get_by_label_contains("Open archive or disc image").click();
        harness.run_steps(2);
        assert_eq!(harness.state().as_slice(), [Action::OpenArchiveDialog]);
        // The folder dialog is where it was.
        harness.get_by_label_contains("Open folder or disc").click();
        harness.run_steps(2);
        assert_eq!(harness.state().as_slice(), [Action::OpenArchiveDialog, Action::OpenDialog]);
        // Dropping the file works as well, and the screen says so.
        harness.get_by_label_contains("a ZIP archive, a disc image");
    }

    #[test]
    fn archive_action_explains_itself_and_names_its_shortcut() {
        let mut harness = welcome();
        harness.step();
        harness.get_by_label_contains("Open archive or disc image").hover();
        harness.run_steps(8);
        let tooltip = format!("without unpacking it ({})", shortcut("Shift+O"));
        assert!(harness.query_all_by_label_contains(&tooltip).next().is_some(), "tooltip with {tooltip:?}");
    }
}
