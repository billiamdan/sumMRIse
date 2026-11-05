//! Right panel: curated overview and the complete, searchable attribute tree
//! of the image currently displayed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{CollapsingHeader, Label, RichText, ScrollArea, TextEdit, Ui};
use egui_material_icons::icons::ICON_SEARCH;
use summrise_core::dicom::{InstanceMetadata, TagNode, read_metadata};
use summrise_core::{Job, Waker};

use crate::theme;
use crate::widgets::{icon, kv_grid, section_title};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tab {
    #[default]
    Overview,
    Attributes,
}

#[derive(Default)]
pub(crate) struct Inspector {
    tab: Tab,
    filter: String,
    path: Option<PathBuf>,
    job: Option<Job<Result<InstanceMetadata, String>>>,
    data: Option<Arc<InstanceMetadata>>,
    error: Option<String>,
}

impl Inspector {
    fn ensure_loaded(&mut self, path: Option<&Path>, waker: &Waker) {
        if self.path.as_deref() == path {
            return;
        }
        self.path = path.map(Path::to_path_buf);
        if let Some(job) = self.job.take() {
            job.cancel();
        }
        if let Some(p) = path {
            let p = p.to_path_buf();
            self.job = Some(Job::spawn("metadata", Some(waker.clone()), move |_| {
                read_metadata(&p).map_err(|e| e.to_string())
            }));
        } else {
            self.data = None;
        }
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        waker: &Waker,
        path: Option<&Path>,
        display: &[(String, String)],
        hide_patient: bool,
    ) {
        self.ensure_loaded(path, waker);
        if let Some(job) = &mut self.job
            && let Some(result) = job.poll()
        {
            match result {
                Ok(md) => {
                    self.data = Some(Arc::new(md));
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
            self.job = None;
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Overview, "Overview");
            ui.selectable_value(&mut self.tab, Tab::Attributes, "All attributes");
        });
        ui.separator();

        let p = theme::palette(ui);
        if let Some(e) = &self.error {
            ui.label(RichText::new(format!("Metadata unavailable: {e}")).color(p.error));
        }
        match self.tab {
            Tab::Overview => {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if !display.is_empty() {
                        section_title(ui, "Display");
                        kv_grid(ui, "display-grid", display);
                        ui.add_space(6.0);
                    }
                    match &self.data {
                        Some(md) => {
                            for section in &md.summary {
                                if hide_patient && section.title == "Patient" {
                                    continue;
                                }
                                CollapsingHeader::new(RichText::new(section.title).strong())
                                    .id_salt(("summary", section.title))
                                    .default_open(true)
                                    .show(ui, |ui| kv_grid(ui, ("summary-grid", section.title), &section.rows));
                            }
                        }
                        None => {
                            ui.spinner();
                        }
                    }
                });
            }
            Tab::Attributes => {
                ui.add(
                    TextEdit::singleline(&mut self.filter)
                        .hint_text(format!("{}  Search tag, name or value", icon(ICON_SEARCH)))
                        .desired_width(f32::INFINITY),
                );
                let Some(md) = self.data.clone() else {
                    ui.spinner();
                    return;
                };
                ui.label(
                    RichText::new(format!("{} attributes · right-click to copy", md.attribute_count()))
                        .small()
                        .color(p.text_weak),
                );
                let needle = self.filter.trim().to_lowercase();
                ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    section_title(ui, "File meta information");
                    node_rows(ui, &md.file_meta, &needle, hide_patient, "meta");
                    section_title(ui, "Data set");
                    node_rows(ui, &md.dataset, &needle, hide_patient, "ds");
                });
            }
        }
    }
}

fn node_rows(ui: &mut Ui, nodes: &[TagNode], needle: &str, hide_patient: bool, salt: &str) {
    let p = theme::palette(ui);
    for (i, n) in nodes.iter().enumerate().filter(|(_, n)| n.matches(needle)) {
        let hidden = hide_patient && n.group == 0x0010;
        if n.items.is_empty() {
            let value = if hidden { "•••• (hidden)" } else { n.value.as_str() };
            let row = ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(n.tag_label()).monospace().small().color(p.text_weak));
                    ui.label(RichText::new(&n.name).strong().color(p.text)).on_hover_text(format!(
                        "{} · VR {}",
                        n.keyword.as_deref().unwrap_or("private / unknown"),
                        n.vr
                    ));
                    ui.label(RichText::new(&n.vr).small().color(p.text_weak));
                });
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.add(Label::new(RichText::new(value).color(p.text)).wrap().selectable(true));
                });
                ui.add_space(3.0);
            });
            if !hidden {
                row.response.context_menu(|ui| {
                    if ui.button("Copy value").clicked() {
                        ui.ctx().copy_text(n.value.clone());
                        ui.close();
                    }
                    if ui.button("Copy tag and value").clicked() {
                        ui.ctx().copy_text(format!("{} {} = {}", n.tag_label(), n.name, n.value));
                        ui.close();
                    }
                });
            }
        } else {
            let open = (!needle.is_empty()).then_some(true);
            CollapsingHeader::new(
                RichText::new(format!("{}  {}  ({} items)", n.tag_label(), n.name, n.items.len())).color(p.text),
            )
            .id_salt((salt, i, n.group, n.element))
            .open(open)
            .show(ui, |ui| {
                for (k, item) in n.items.iter().enumerate() {
                    CollapsingHeader::new(RichText::new(format!("Item {}", k + 1)).color(p.text_weak))
                        .id_salt((salt, i, k))
                        .open(open)
                        .show(ui, |ui| node_rows(ui, item, needle, hide_patient, &format!("{salt}.{i}.{k}")));
                }
            });
        }
    }
}
