//! The documents of a study: the list in the series browser, and the view that
//! takes the place of the images while one is open (`openspec/changes/report-viewer`).
//!
//! Reports are shown exactly as written: nothing here summarises, translates or
//! explains them. Labels around a report are translated; the report never is.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use egui::load::SizedTexture;
use egui::{
    Color32, ColorImage, CornerRadius, Frame, Key, Label, Margin, Modifiers, Rect, RichText, ScrollArea, Sense, Shape,
    TextureHandle, TextureOptions, Ui, Vec2, vec2,
};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::{
    ICON_ARROW_BACK, ICON_ARTICLE, ICON_CHEVRON_LEFT, ICON_CHEVRON_RIGHT, ICON_DESCRIPTION, ICON_FIT_WIDTH,
    ICON_FOLDER_OPEN, ICON_MONITORING, ICON_PICTURE_AS_PDF, ICON_ZOOM_IN, ICON_ZOOM_OUT,
};
use summrise_core::dicom::documents::{
    DoseReport, PlainText, ReportContent, ReportItem, ReportStatus, StructuredReport,
};
use summrise_core::dicom::{DocumentKind, DocumentRef, MediaKind};
use summrise_core::documents::{
    DocumentState, ImageLinks, ImageTarget, MAX_PAGES, OpenDocument, PageImage, PdfPages, PdfProblem, bounded_scale,
    go_to_image, settle_image,
};
use summrise_core::{SeriesKey, Session, Settings, ViewState, Waker};
use summrise_i18n::tr;

use crate::theme;
use crate::widgets::{icon, section_title};

/// Width of the column that reports are read in.
const READING_WIDTH: f32 = 760.0;

/// What a zoom button steps through, in percent.
const ZOOM_STEPS: [u32; 9] = [25, 50, 75, 100, 125, 150, 200, 300, 400];

/// How large the pages of a PDF are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zoom {
    FitWidth,
    Percent(u32),
}

impl Zoom {
    /// Points on screen per point of the page; `shown` is the percentage now on screen.
    fn step(shown: u32, up: bool) -> Self {
        let next = if up {
            ZOOM_STEPS.iter().find(|&&step| step > shown).or(ZOOM_STEPS.last())
        } else {
            ZOOM_STEPS.iter().rev().find(|&&step| step < shown).or(ZOOM_STEPS.first())
        };
        Self::Percent(next.copied().unwrap_or(100))
    }
}

/// A page rasterised, on its way to the screen.
struct Picture {
    texture: TextureHandle,
    /// Pixels per point of the page.
    scale: f32,
    incomplete: bool,
}

/// How to ask the system to show a file in its folder. Replaced in tests, which
/// must not start a file manager.
pub(crate) type Reveal = fn(&Path) -> std::io::Result<()>;

/// The document on screen and what belongs to it.
pub(crate) struct DocumentPane {
    open: Option<OpenDocument>,
    /// A link was activated: the image to go to in the next frame.
    follow: Option<ImageTarget>,
    /// The image a link went to, until its series can show it.
    pending: Option<ImageTarget>,
    links: Option<ImageLinks>,
    zoom: Zoom,
    /// The zoom on screen in percent, which the zoom buttons step from: it is
    /// measured when the page is laid out and shown in the next frame.
    shown_percent: u32,
    pictures: HashMap<usize, Picture>,
    /// Pages that cannot be rasterised, and why.
    failed_pages: HashMap<usize, String>,
    /// The study without images that was opened on its first document: once
    /// per study, so that the user can go back from it.
    opened_first: Option<usize>,
    /// Why "Show in folder" did not work.
    reveal_failed: bool,
    pub(crate) reveal: Reveal,
}

impl Default for DocumentPane {
    fn default() -> Self {
        Self {
            open: None,
            follow: None,
            pending: None,
            links: None,
            zoom: Zoom::FitWidth,
            shown_percent: 100,
            pictures: HashMap::new(),
            failed_pages: HashMap::new(),
            opened_first: None,
            reveal_failed: false,
            reveal: reveal_in_file_manager,
        }
    }
}

/// The command that shows `path` in the file manager. Nothing is executed
/// other than the system's own file manager, and it opens the folder, not the
/// file.
fn reveal_command(path: &Path) -> Command {
    if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg("-R").arg(path);
        command
    } else if cfg!(windows) {
        let mut command = Command::new("explorer");
        command.arg("/select,").arg(path);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    }
}

fn reveal_in_file_manager(path: &Path) -> std::io::Result<()> {
    let mut child = reveal_command(path).spawn()?;
    // Collects the process when it ends; nobody waits for it.
    std::thread::spawn(move || child.wait());
    Ok(())
}

fn kind_icon(kind: DocumentKind) -> MaterialIcon {
    match kind {
        DocumentKind::Report => ICON_DESCRIPTION,
        DocumentKind::DoseReport => ICON_MONITORING,
        DocumentKind::StoredPdf | DocumentKind::PdfFile => ICON_PICTURE_AS_PDF,
        DocumentKind::RtfFile | DocumentKind::TextFile | DocumentKind::HtmlFile => ICON_ARTICLE,
    }
}

impl DocumentPane {
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Back to the images.
    pub(crate) fn close(&mut self) {
        self.open = None;
        self.pictures.clear();
        self.failed_pages.clear();
        self.reveal_failed = false;
    }

    fn open_document(&mut self, session: &Session, document: &DocumentRef, waker: &Waker) {
        if self.open.as_ref().is_some_and(|open| open.reference == *document) {
            return;
        }
        self.close();
        self.zoom = Zoom::FitWidth;
        self.open =
            Some(OpenDocument::open(session.media(), document.clone(), session.active_study, Some(waker.clone())));
    }

    /// Whether `document` is the one on screen.
    fn shows(&self, document: &DocumentRef) -> bool {
        self.open.as_ref().is_some_and(|open| open.reference == *document)
    }

    /// The scale of the picture of `page` on screen, for tests.
    #[cfg(test)]
    pub(crate) fn picture_scale(&self, page: usize) -> Option<f32> {
        self.pictures.get(&page).map(|picture| picture.scale)
    }

    /// Whether no link is waiting for its image, for tests.
    #[cfg(test)]
    pub(crate) fn is_settled(&self) -> bool {
        self.follow.is_none() && self.pending.is_none()
    }

    /// Call once per frame, before anything is drawn: takes over what the
    /// background work has found and carries out a link. Returns whether the
    /// images were asked for by a link, which shows them in the single view.
    pub(crate) fn update(
        &mut self,
        session: &mut Session,
        views: &mut HashMap<SeriesKey, ViewState>,
        waker: &Waker,
    ) -> bool {
        // The documents of another study are in the list now.
        if self.open.as_ref().is_some_and(|open| open.study != session.active_study) {
            self.close();
        }
        // A study without images has its documents to show, and nothing else.
        if session.active_series.is_none() && self.opened_first != Some(session.active_study) {
            self.opened_first = Some(session.active_study);
            let first = session.study().documents.first().or_else(|| session.index().loose_documents.first()).cloned();
            if let Some(first) = first {
                self.open_document(session, &first, waker);
            }
        }
        if let Some(open) = &mut self.open {
            open.poll();
        }
        let followed = self.follow.take();
        if let Some(target) = followed {
            self.close();
            go_to_image(session, target);
            self.pending = Some(target);
        }
        if let Some(target) = self.pending
            && settle_image(session, views, target)
        {
            self.pending = None;
        }
        followed.is_some()
    }

    /// The document view, in place of the images.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &Session, settings: &Settings) {
        let p = theme::palette(ui);
        ui.painter().rect_filled(ui.max_rect(), 0.0, p.bg);
        // Esc goes back, unless it is on its way to a dialog.
        let dialog = ui.ctx().memory(|m| m.top_modal_layer().is_some());
        if !dialog && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.close();
            ui.ctx().request_repaint();
            return;
        }
        if !settings.hide_patient_details
            && let Some(open) = &mut self.open
        {
            // Hidden again as soon as privacy mode is turned on again.
            open.revealed = false;
        }
        let frame = Frame::new().inner_margin(Margin::symmetric(16, 10));
        frame.show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            self.header(ui, session, settings);
            ui.add_space(4.0);
            ui.separator();
            let hidden = settings.hide_patient_details && self.open.as_ref().is_some_and(|open| !open.revealed);
            if hidden {
                self.hidden_notice(ui);
            } else {
                self.body(ui, session);
            }
        });
        // A link or the button was used: the images come in the next frame.
        if self.follow.is_some() || self.open.is_none() {
            ui.ctx().request_repaint();
        }
    }

    fn status_of(&self) -> Option<ReportStatus> {
        match self.open.as_ref()?.state() {
            DocumentState::Report(report) => Some(report.status),
            DocumentState::Dose(dose) => Some(dose.report.status),
            DocumentState::Pdf(pages) => pages.status,
            _ => None,
        }
    }

    fn header(&mut self, ui: &mut Ui, session: &Session, settings: &Settings) {
        let p = theme::palette(ui);
        let Some(open) = &self.open else { return };
        let reference = open.reference.clone();
        let hidden = settings.hide_patient_details && !open.revealed;
        let mut back = false;
        let mut reveal = false;
        ui.horizontal_wrapped(|ui| {
            let name = tr!("Back to images");
            let label = format!("{} {name}", icon(ICON_ARROW_BACK));
            back = ui.button(label).accessible_name(name).on_hover_text(tr!("Back to images (Esc)")).clicked();
            // A file in an archive is in no folder that could be shown.
            if can_reveal(session, &reference) {
                let name = tr!("Show in folder");
                let label = format!("{} {name}", icon(ICON_FOLDER_OPEN));
                let hint = tr!("Opens the folder of this file in your file manager. The file itself is not opened.");
                reveal = ui.button(label).accessible_name(name).on_hover_text(hint).clicked();
            }
        });
        ui.add_space(6.0);
        // In privacy mode a title or the name of a file could identify the patient.
        let title = if hidden { reference.kind.label() } else { reference.display_title() };
        ui.add(Label::new(RichText::new(title).size(18.0).strong()).wrap().selectable(true));
        let mut line = vec![reference.kind.label().to_owned()];
        line.extend(reference.display_date());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(line.join(" · ")).small().color(p.text_weak));
            if !hidden && let Some(status) = self.status_of() {
                let color = if status.is_final() { p.text_weak } else { p.warn };
                ui.label(RichText::new(status.label()).strong().color(color));
            }
        });
        if self.reveal_failed {
            ui.label(RichText::new(tr!("The folder could not be opened.")).small().color(p.error));
        }
        if back {
            self.close();
        } else if reveal {
            self.reveal_file(&reference);
        }
    }

    fn reveal_file(&mut self, reference: &DocumentRef) {
        if let Err(e) = (self.reveal)(&reference.path) {
            log::debug!("showing a file in its folder failed: {e}");
            self.reveal_failed = true;
        }
    }

    fn hidden_notice(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui);
        ui.add_space(40.0);
        let mut show = false;
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(tr!("Hidden in privacy mode — reports contain personal details"))
                    .size(14.0)
                    .color(p.text),
            );
            ui.add_space(8.0);
            show = ui.button(tr!("Show anyway")).clicked();
        });
        if show && let Some(open) = &mut self.open {
            open.revealed = true;
        }
    }

    fn body(&mut self, ui: &mut Ui, session: &Session) {
        let Some(mut open) = self.open.take() else { return };
        let kind = open.kind();
        match open.state_mut() {
            DocumentState::Loading => {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Spinner::new().size(28.0));
                    ui.label(tr!("Loading the document…"));
                });
            }
            DocumentState::Report(report) => self.report_view(ui, session, report),
            DocumentState::Dose(dose) => self.dose_view(ui, session, dose),
            DocumentState::Text(text) => text_view(ui, kind, text),
            DocumentState::Pdf(pages) => self.pdf_view(ui, pages),
            DocumentState::Failed(reason) => {
                let reason = reason.clone();
                failure_view(ui, &reason);
            }
        }
        self.open = Some(open);
    }

    fn links(&mut self, session: &Session) -> &ImageLinks {
        self.links.get_or_insert_with(|| ImageLinks::new(session.index()))
    }

    fn report_view(&mut self, ui: &mut Ui, session: &Session, report: &StructuredReport) {
        ScrollArea::vertical().id_salt("report").auto_shrink([false, false]).show(ui, |ui| {
            reading_column(ui, |ui| {
                scroll_keys(ui);
                self.report_items(ui, session, report);
            });
        });
    }

    fn report_items(&mut self, ui: &mut Ui, session: &Session, report: &StructuredReport) {
        let p = theme::palette(ui);
        if report.items.is_empty() {
            ui.label(RichText::new(tr!("This report holds no text.")).color(p.text_weak));
        }
        for item in &report.items {
            self.report_item(ui, session, item);
        }
        if report.cut_short {
            ui.add_space(8.0);
            let cut = tr!("Part of this report is not shown: it is nested too deeply or is too long.");
            ui.add(Label::new(RichText::new(cut).color(p.warn)).wrap());
        }
    }

    fn report_item(&mut self, ui: &mut Ui, session: &Session, item: &ReportItem) {
        let p = theme::palette(ui);
        // Headings stand out by size; nesting beyond a few levels adds no meaning but takes width.
        let indent = (item.depth.min(6) * 12) as i8;
        Frame::new().inner_margin(Margin { left: indent, ..Margin::ZERO }).show(ui, |ui| match &item.content {
            ReportContent::Heading(text) => {
                ui.add_space(if item.depth == 0 { 10.0 } else { 6.0 });
                let size = if item.depth == 0 { 16.0 } else { 14.0 };
                ui.add(Label::new(RichText::new(text).size(size).strong()).wrap().selectable(true));
            }
            ReportContent::Paragraph { label, text } => {
                if let Some(label) = label {
                    ui.label(RichText::new(label).small().color(p.text_weak));
                }
                ui.add(Label::new(RichText::new(text).color(p.text)).wrap().selectable(true));
            }
            ReportContent::Value { name, value } => {
                // Translators: an item of a report, such as "Diameter: 12 mm". Both come from the report.
                let line = tr!("{name}: {value}", name = name, value = value);
                ui.add(Label::new(RichText::new(line).color(p.text)).wrap().selectable(true));
            }
            ReportContent::Image { name, sop_instance_uid, frame } => {
                let target = sop_instance_uid.as_deref().and_then(|uid| self.links(session).find(uid, *frame));
                ui.horizontal_wrapped(|ui| {
                    if let Some(name) = name {
                        ui.label(RichText::new(name).color(p.text_weak));
                    }
                    match target.and_then(|target| Some((target, session.series(target.series)?.title()))) {
                        Some((target, series)) => {
                            // Translators: a link in a report to an image; {series} is the name of the series.
                            let link = ui.link(tr!("Image in {series}", series = series));
                            if link.clicked() {
                                self.follow = Some(target);
                            }
                        }
                        None => {
                            ui.label(RichText::new(tr!("image not on this disc")).italics().color(p.text_weak));
                        }
                    }
                });
            }
        });
    }

    fn dose_view(&mut self, ui: &mut Ui, session: &Session, dose: &DoseReport) {
        let p = theme::palette(ui);
        ScrollArea::vertical().id_salt("dose report").auto_shrink([false, false]).show(ui, |ui| {
            reading_column(ui, |ui| {
                scroll_keys(ui);
                if dose.events.is_empty() && dose.total_dlp.is_none() {
                    self.report_items(ui, session, &dose.report);
                    return;
                }
                ui.add_space(6.0);
                let note = tr!("These are the values the scanner recorded. SumMRIse calculates nothing from them.");
                ui.add(Label::new(RichText::new(note).small().color(p.text_weak)).wrap());
                ui.add_space(6.0);
                egui::Grid::new("dose table").num_columns(3).striped(true).spacing([24.0, 6.0]).show(ui, |ui| {
                    // Translators: the columns of the table of a dose report. CTDIvol and DLP
                    // (dose-length product) are international abbreviations.
                    ui.label(RichText::new(tr!("Acquisition")).strong());
                    ui.label(RichText::new(tr!("CTDIvol")).strong());
                    ui.label(RichText::new(tr!("DLP")).strong());
                    ui.end_row();
                    let none = tr!("not recorded");
                    for (i, event) in dose.events.iter().enumerate() {
                        let number = tr!("Acquisition {number}", number = i + 1);
                        selectable(ui, event.label.as_deref().unwrap_or(&number));
                        selectable(ui, event.ctdi_vol.as_deref().unwrap_or(none));
                        selectable(ui, event.dlp.as_deref().unwrap_or(none));
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                match &dose.total_dlp {
                    Some(total) => {
                        let line = tr!("Total DLP {value} (as recorded by the scanner)", value = total);
                        ui.add(Label::new(RichText::new(line).strong()).wrap().selectable(true));
                    }
                    None => {
                        ui.label(RichText::new(tr!("The scanner recorded no total.")).color(p.text_weak));
                    }
                }
                ui.add_space(10.0);
                egui::CollapsingHeader::new(tr!("All values in this report")).id_salt("all dose values").show(
                    ui,
                    |ui| {
                        self.report_items(ui, session, &dose.report);
                    },
                );
            });
        });
    }

    fn pdf_view(&mut self, ui: &mut Ui, pages: &mut PdfPages) {
        let p = theme::palette(ui);
        while let Some((page, result)) = pages.poll() {
            match result {
                Ok(image) => {
                    self.failed_pages.remove(&page);
                    self.pictures.insert(page, picture_of(ui, image));
                }
                Err(problem) => {
                    self.failed_pages.insert(page, problem.to_string());
                }
            }
        }
        let current = pages.page();
        self.pictures.retain(|page, _| page.abs_diff(current) <= 2);

        // Page Up and Page Down turn the pages; the arrow keys move within a page.
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::PageDown)) {
            pages.turn(1);
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::PageUp)) {
            pages.turn(-1);
        }
        self.pdf_toolbar(ui, pages);
        let page = pages.page();
        let Some(size) = pages.size(page) else { return };

        if pages.total_pages() > MAX_PAGES {
            let only = tr!(
                "Only the first {count} of {total} pages can be shown.",
                count = MAX_PAGES,
                total = pages.total_pages()
            );
            ui.label(RichText::new(only).small().color(p.warn));
        }
        if let Some(reason) = self.failed_pages.get(&page).cloned() {
            failure_view(ui, &reason);
            return;
        }
        if self.pictures.get(&page).is_some_and(|picture| picture.incomplete) {
            let missing =
                tr!("Part of this page could not be drawn, such as a font or a picture. Something may be missing.");
            ui.add(Label::new(RichText::new(missing).color(p.warn)).wrap());
        }
        ui.add_space(4.0);

        // Points on screen per point of the page.
        let available = (ui.available_width() - 24.0).max(120.0);
        let points = match self.zoom {
            Zoom::FitWidth => available / size.width.max(1.0),
            Zoom::Percent(percent) => percent as f32 / 100.0,
        };
        let percent = (points * 100.0).round() as u32;
        if percent != self.shown_percent {
            self.shown_percent = percent;
            ui.ctx().request_repaint();
        }
        let wanted = points * ui.ctx().pixels_per_point();
        let max_side = u32::try_from(ui.ctx().input(|i| i.max_texture_side)).unwrap_or(2048).max(1);
        match bounded_scale(size, wanted, max_side) {
            Some(scale) => {
                let stale = self.pictures.get(&page).is_none_or(|picture| (picture.scale / scale - 1.0).abs() > 0.02);
                if stale {
                    pages.request(page, scale, max_side);
                }
            }
            None => {
                failure_view(ui, &PdfProblem::Damaged.to_string());
                return;
            }
        }
        if pages.is_rendering() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        let display = vec2(size.width, size.height) * points;
        ScrollArea::both().id_salt(("pdf page", page)).auto_shrink([false, false]).show(ui, |ui| {
            scroll_keys_within_page(ui);
            // In the middle while it fits. A page wider than the view starts at its left edge, because
            // only what lies to the right of the start of the scroll area can be scrolled to.
            let margin = ((ui.available_width() - display.x) / 2.0).max(0.0);
            ui.horizontal_top(|ui| {
                ui.add_space(margin);
                match self.pictures.get(&page) {
                    // Until the sharp picture is there, the last one stands in for it, scaled.
                    Some(picture) => {
                        let image = egui::Image::from_texture(SizedTexture::new(picture.texture.id(), display))
                            .alt_text(tr!("Page {number}", number = page + 1));
                        ui.add(image);
                    }
                    None => {
                        let (rect, _) = ui.allocate_exact_size(display, Sense::hover());
                        ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
                        let spinner = Rect::from_center_size(rect.center_top() + vec2(0.0, 60.0), Vec2::splat(32.0));
                        ui.put(spinner, egui::Spinner::new().size(28.0).color(Color32::DARK_GRAY));
                    }
                }
            });
        });
    }

    /// The bar above a PDF page.
    fn pdf_toolbar(&mut self, ui: &mut Ui, pages: &mut PdfPages) {
        let count = pages.page_count();
        let page = pages.page();
        ui.horizontal_wrapped(|ui| {
            let previous_name = tr!("Previous page");
            let previous = ui
                .add_enabled(page > 0, egui::Button::new(icon(ICON_CHEVRON_LEFT)))
                .accessible_name(previous_name)
                .on_hover_text(tr!("Previous page (Page Up)"));
            if previous.clicked() {
                pages.turn(-1);
            }
            ui.label(tr!("Page {number} of {count}", number = page + 1, count = count));
            let next_name = tr!("Next page");
            let next = ui
                .add_enabled(page + 1 < count, egui::Button::new(icon(ICON_CHEVRON_RIGHT)))
                .accessible_name(next_name)
                .on_hover_text(tr!("Next page (Page Down)"));
            if next.clicked() {
                pages.turn(1);
            }
            ui.separator();
            let shown = self.shown_percent;
            let zoom_out =
                ui.button(icon(ICON_ZOOM_OUT)).accessible_name(tr!("Zoom out")).on_hover_text(tr!("Zoom out"));
            if zoom_out.clicked() {
                self.zoom = Zoom::step(shown, false);
            }
            ui.label(format!("{shown} %"));
            let zoom_in = ui.button(icon(ICON_ZOOM_IN)).accessible_name(tr!("Zoom in")).on_hover_text(tr!("Zoom in"));
            if zoom_in.clicked() {
                self.zoom = Zoom::step(shown, true);
            }
            let fit = ui
                .add(egui::Button::selectable(self.zoom == Zoom::FitWidth, icon(ICON_FIT_WIDTH)))
                .accessible_name(tr!("Fit to width"))
                .on_hover_text(tr!("Fit to width"));
            if fit.clicked() {
                self.zoom = Zoom::FitWidth;
            }
        });
    }
}

fn can_reveal(session: &Session, reference: &DocumentRef) -> bool {
    reference.kind.is_file() && session.media().kind() == MediaKind::Folder
}

/// A texture for a rasterised page.
fn picture_of(ui: &Ui, image: PageImage) -> Picture {
    let size = [image.width as usize, image.height as usize];
    let pixels = ColorImage::from_rgba_premultiplied(size, &image.rgba);
    let texture = ui.ctx().load_texture(format!("document-page-{}", image.page), pixels, TextureOptions::LINEAR);
    Picture { texture, scale: image.scale, incomplete: image.incomplete }
}

fn selectable(ui: &mut Ui, text: &str) {
    ui.add(Label::new(text).selectable(true));
}

/// A column of readable width in the middle of what is available.
fn reading_column(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let width = ui.available_width().min(READING_WIDTH);
    let margin = ((ui.available_width() - width) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_width(width);
            add(ui);
        });
    });
}

/// Page Up, Page Down and the arrow keys move through what is in the scroll
/// area around `ui`.
fn scroll_keys(ui: &mut Ui) {
    let page = ui.clip_rect().height() * 0.9;
    let (up, down) =
        ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::PageUp), i.consume_key(Modifiers::NONE, Key::PageDown)));
    if up {
        ui.scroll_with_delta(vec2(0.0, page));
    }
    if down {
        ui.scroll_with_delta(vec2(0.0, -page));
    }
    scroll_keys_within_page(ui);
}

fn scroll_keys_within_page(ui: &mut Ui) {
    const LINE: f32 = 40.0;
    let (up, down) = ui
        .input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown)));
    if up {
        ui.scroll_with_delta(vec2(0.0, LINE));
    }
    if down {
        ui.scroll_with_delta(vec2(0.0, -LINE));
    }
}

/// Plain text, line by line: a label per line keeps a long document cheap to
/// draw, because lines outside the view are not painted.
fn text_view(ui: &mut Ui, kind: DocumentKind, text: &PlainText) {
    let p = theme::palette(ui);
    ScrollArea::vertical().id_salt("text document").auto_shrink([false, false]).show(ui, |ui| {
        reading_column(ui, |ui| {
            scroll_keys(ui);
            if let Some(encoding) = text.assumed_encoding {
                let note = tr!(
                    "This file does not say which character set it uses. It is shown as {encoding}, so some letters may be wrong.",
                    encoding = encoding
                );
                ui.add(Label::new(RichText::new(note).small().color(p.warn)).wrap());
                ui.add_space(6.0);
            }
            if text.text.trim().is_empty() {
                ui.label(RichText::new(tr!("This document holds no text.")).color(p.text_weak));
            }
            // A text file may rely on its columns; RTF and HTML say where their lines end.
            let monospace = kind == DocumentKind::TextFile;
            for line in text.text.split('\n') {
                if line.trim().is_empty() {
                    ui.add_space(8.0);
                    continue;
                }
                let line = RichText::new(line).color(p.text);
                let line = if monospace { line.monospace() } else { line };
                ui.add(Label::new(line).wrap().selectable(true));
            }
            if text.truncated {
                ui.add_space(8.0);
                let cut = tr!("This document is longer than can be shown; the rest is cut off.");
                ui.add(Label::new(RichText::new(cut).color(p.warn)).wrap());
            }
        });
    });
}

/// The document cannot be displayed. The application goes on as before.
fn failure_view(ui: &mut Ui, reason: &str) {
    let p = theme::palette(ui);
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(tr!("This document could not be displayed.")).size(14.0).strong().color(p.text));
        ui.add_space(4.0);
        ui.add(Label::new(RichText::new(reason).color(p.text_weak)).wrap());
    });
}

/// The hint in the status bar while a document is on screen.
pub(crate) fn status_hint() -> &'static str {
    tr!("Esc: back to the images · Page Up/Down: move through the document")
}

/// The documents of the active study and the report files of the medium, in the
/// series browser; nothing at all when there are none. Their rows take the
/// keyboard focus with Tab and open with Enter or Space.
pub(crate) fn browser_section(
    ui: &mut Ui,
    session: &Session,
    settings: &Settings,
    pane: &mut DocumentPane,
    waker: &Waker,
) {
    let documents = &session.study().documents;
    let files = &session.index().loose_documents;
    if documents.is_empty() && files.is_empty() {
        return;
    }
    let p = theme::palette(ui);
    section_title(ui, tr!("Documents"));
    let mut chosen = None;
    let height = (ui.available_height() * 0.4).max(80.0);
    ScrollArea::vertical().id_salt("documents").max_height(height).auto_shrink([false, true]).show(ui, |ui| {
        for (i, document) in documents.iter().enumerate() {
            if document_row(ui, session, settings, pane, document, i) {
                chosen = Some(document);
            }
        }
        if !files.is_empty() {
            if !documents.is_empty() {
                ui.add_space(2.0);
            }
            ui.label(RichText::new(tr!("Other files on the disc")).small().color(p.text_weak));
            for (i, document) in files.iter().enumerate() {
                if document_row(ui, session, settings, pane, document, documents.len() + i) {
                    chosen = Some(document);
                }
            }
        }
    });
    if let Some(document) = chosen {
        pane.open_document(session, document, waker);
    }
    ui.add_space(2.0);
    ui.separator();
}

/// One row of the list. Returns whether it was activated.
fn document_row(
    ui: &mut Ui,
    session: &Session,
    settings: &Settings,
    pane: &DocumentPane,
    document: &DocumentRef,
    index: usize,
) -> bool {
    let p = theme::palette(ui);
    let hide = settings.hide_patient_details;
    // In privacy mode a title or the name of a file could identify the patient.
    let title = if hide { document.kind.label() } else { document.display_title() };
    let mut line = vec![document.kind.label().to_owned()];
    line.extend(document.display_date());
    let line = line.join(" · ");
    let bg = ui.painter().add(Shape::Noop);
    let inner = ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(icon(kind_icon(document.kind))).size(18.0).color(p.text_weak));
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(Label::new(RichText::new(title).strong()).truncate());
            ui.add(Label::new(RichText::new(line.as_str()).small().color(p.text_weak)).truncate());
            if document.too_large {
                ui.label(RichText::new(tr!("too large to display")).small().color(p.warn));
            }
        });
    });
    let rect = inner.response.rect.expand2(vec2(2.0, 3.0));
    let response = ui.interact(rect, ui.scope_id().with(("document", index)), Sense::click());
    let name = format!("{title}, {line}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    let fill = if pane.shows(document) {
        p.accent.gamma_multiply(0.25)
    } else if response.hovered() || response.has_focus() {
        p.surface_hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().set(bg, Shape::rect_filled(rect, CornerRadius::same(8), fill));
    if response.has_focus() {
        ui.painter().rect_stroke(rect, 8, egui::Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
    }
    let clicked = response.clicked();
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if !hide {
        response.on_hover_text(session.media().display_path(&document.path).display().to_string());
    }
    ui.add_space(4.0);
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zoom_buttons_step_through_the_stops() {
        assert_eq!(Zoom::step(100, true), Zoom::Percent(125));
        assert_eq!(Zoom::step(100, false), Zoom::Percent(75));
        // From a width that fits the window, the next stop in that direction.
        assert_eq!(Zoom::step(113, true), Zoom::Percent(125));
        assert_eq!(Zoom::step(113, false), Zoom::Percent(100));
        // The ends stay the ends.
        assert_eq!(Zoom::step(400, true), Zoom::Percent(400));
        assert_eq!(Zoom::step(25, false), Zoom::Percent(25));
        assert_eq!(Zoom::step(900, false), Zoom::Percent(400));
        assert_eq!(Zoom::step(10, true), Zoom::Percent(25));
    }

    /// The file manager is asked to show the folder; the file itself is never opened.
    #[test]
    fn show_in_folder_asks_the_file_manager_for_the_folder() {
        let path = Path::new("/media/cd/REPORT.PDF");
        let command = reveal_command(path);
        let program = command.get_program().to_string_lossy().into_owned();
        let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        if cfg!(target_os = "macos") {
            assert_eq!(program, "open");
            assert_eq!(args, ["-R", "/media/cd/REPORT.PDF"], "-R reveals the file and does not open it");
        } else if cfg!(windows) {
            assert_eq!(program, "explorer");
            assert_eq!(args[0], "/select,");
        } else {
            assert_eq!(program, "xdg-open");
            assert_eq!(args, ["/media/cd"], "the folder, not the file");
        }
    }
}
