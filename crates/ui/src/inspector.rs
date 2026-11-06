//! Right panel: curated overview and the complete, searchable attribute tree
//! of the image currently displayed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{CollapsingHeader, Label, RichText, ScrollArea, TextEdit, Ui};
use egui_material_icons::icons::ICON_SEARCH;
use summrise_core::dicom::{InstanceMetadata, Media, SummarySection, TagNode, read_metadata};
use summrise_core::{Job, Waker};
use summrise_i18n::{tr, tr_n};

use crate::guide;
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
    /// The language the metadata was read in: its labels are read again in
    /// another one.
    language: &'static str,
    /// The plain-language guide shown above the overview.
    guide: guide::Panel,
}

impl Inspector {
    fn ensure_loaded(&mut self, file: Option<(&Media, &Path)>, waker: &Waker) {
        let path = file.map(|(_, path)| path);
        let language = summrise_i18n::active_language();
        if self.path.as_deref() == path && self.language == language {
            return;
        }
        self.path = path.map(Path::to_path_buf);
        self.language = language;
        if let Some(job) = self.job.take() {
            job.cancel();
        }
        if let Some((media, p)) = file {
            let (media, p) = (media.clone(), p.to_path_buf());
            self.job = Some(Job::spawn("metadata", Some(waker.clone()), move |_| {
                read_metadata(&media, &p).map_err(|e| e.to_string())
            }));
        } else {
            self.data = None;
        }
    }

    /// `file` is the file of the displayed image, with the media of the open
    /// study it is read from. `uptake` is the section on the uptake values of
    /// a PET series (`summrise_core::pet::uptake_section`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        waker: &Waker,
        file: Option<(&Media, &Path)>,
        display: &[(String, String)],
        hide_patient: bool,
        scene: Option<&guide::Scene<'_>>,
        uptake: Option<&SummarySection>,
    ) {
        self.ensure_loaded(file, waker);
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
            ui.selectable_value(&mut self.tab, Tab::Overview, tr!("Overview"));
            ui.selectable_value(&mut self.tab, Tab::Attributes, tr!("All attributes"));
        });
        ui.separator();

        let p = theme::palette(ui);
        if let Some(e) = &self.error {
            ui.label(RichText::new(tr!("Metadata unavailable: {reason}", reason = e)).color(p.error));
        }
        match self.tab {
            Tab::Overview => {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if let Some(scene) = scene {
                        self.guide.show(ui, scene);
                    }
                    if !display.is_empty() {
                        section_title(ui, tr!("Display"));
                        kv_grid(ui, "display-grid", display);
                        ui.add_space(6.0);
                    }
                    // What the values of a PET series are shown as, and what that rests on.
                    if let Some(section) = uptake {
                        CollapsingHeader::new(RichText::new(section.title).strong())
                            .id_salt("uptake")
                            .default_open(true)
                            .show(ui, |ui| guide::summary_grid(ui, "uptake-grid", &section.rows, scene.is_some()));
                    }
                    match &self.data {
                        Some(md) => {
                            for section in &md.summary {
                                // By the key, which is the same in every language.
                                if hide_patient && section.key == "Patient" {
                                    continue;
                                }
                                CollapsingHeader::new(RichText::new(section.title).strong())
                                    .id_salt(("summary", section.key))
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        let id = ("summary-grid", section.key);
                                        guide::summary_grid(ui, id, &section.rows, scene.is_some());
                                    });
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
                        .hint_text(format!("{}  {}", icon(ICON_SEARCH), tr!("Search tag, name or value")))
                        .desired_width(f32::INFINITY),
                );
                let Some(md) = self.data.clone() else {
                    ui.spinner();
                    return;
                };
                ui.label(
                    RichText::new(tr_n!(
                        "{n} attribute · right-click to copy",
                        "{n} attributes · right-click to copy",
                        md.attribute_count()
                    ))
                    .small()
                    .color(p.text_weak),
                );
                let needle = self.filter.trim().to_lowercase();
                ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    // Translators: the two parts of a DICOM file, as the DICOM standard calls them.
                    section_title(ui, tr!("File meta information"));
                    node_rows(ui, &md.file_meta, &needle, hide_patient, "meta");
                    section_title(ui, tr!("Data set"));
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
            // Translators: shown in privacy mode instead of a patient detail.
            let value = if hidden { tr!("•••• (hidden)") } else { n.value.as_str() };
            let row = ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(n.tag_label()).monospace().small().color(p.text_weak));
                    ui.label(RichText::new(&n.name).strong().color(p.text)).on_hover_text(format!(
                        "{} · VR {}",
                        n.keyword.as_deref().unwrap_or(tr!("private / unknown")),
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
                    if ui.button(tr!("Copy value")).clicked() {
                        ui.ctx().copy_text(n.value.clone());
                        ui.close();
                    }
                    if ui.button(tr!("Copy tag and value")).clicked() {
                        ui.ctx().copy_text(format!("{} {} = {}", n.tag_label(), n.name, n.value));
                        ui.close();
                    }
                });
            }
        } else {
            let open = (!needle.is_empty()).then_some(true);
            let (tag, name, items) = (n.tag_label(), &n.name, n.items.len());
            // Translators: a DICOM sequence: its tag, its name and the number of its items.
            let title = tr_n!("{tag}  {name}  ({n} item)", "{tag}  {name}  ({n} items)", items, tag = tag, name = name);
            CollapsingHeader::new(RichText::new(title).color(p.text))
                .id_salt((salt, i, n.group, n.element))
                .open(open)
                .show(ui, |ui| {
                    for (k, item) in n.items.iter().enumerate() {
                        CollapsingHeader::new(RichText::new(tr!("Item {number}", number = k + 1)).color(p.text_weak))
                            .id_salt((salt, i, k))
                            .open(open)
                            .show(ui, |ui| node_rows(ui, item, needle, hide_patient, &format!("{salt}.{i}.{k}")));
                    }
                });
        }
    }
}
