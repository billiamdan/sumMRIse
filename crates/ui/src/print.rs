//! The print window (`openspec/changes/export-and-share`, design D9 to D11): what
//! to print, printer, copies, paper, orientation, images per page and "Include
//! patient details", with a preview of the first page. Pages are composed in
//! the background (`summrise_core::print`) with the fonts of the interface, then
//! go to the print system on standard input, or to a PDF file the user places.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use egui::epaint::text::{FontDefinitions, Fonts, TextOptions, ViewportKey};
use egui::{
    Align, Color32, ColorImage, ComboBox, Context, DragValue, FontId, Grid, Layout, RadioButton, Response, RichText,
    TextureHandle, TextureOptions, Ui, vec2,
};
use summrise_core::dicom::Progress;
use summrise_core::export::{self, Picture};
use summrise_core::print::{
    IMAGES_PER_PAGE, PREVIEW_DPI, PRINT_DPI, Page, PageHeader, PageOrientation, PagePicture, PageSetup, Paper,
    PdfWriter, PrintJob, PrintSystem, Printers, TextBitmap, TextRaster, compose_page,
};
use summrise_core::{Job, Settings, ViewKind, Waker};
use summrise_i18n::{tr, tr_ctx, tr_n};

use crate::app::Toasts;
use crate::export::{
    Chooser, Selected, closes, details_checkbox, dialog, file_name, font_definitions, personal_details_warning,
    small_note, warning_note,
};
use crate::theme;

/// Height of the preview in the window, in points; it fits the smallest window
/// (600 points high) with everything around it.
const PREVIEW_HEIGHT: f32 = 340.0;
/// Width of the fields of the options, in points: with the longest label of any
/// language they fit half of the window.
const FIELD_WIDTH: f32 = 160.0;

/// Text of pages through the fonts of the interface: laid out by egui and copied
/// from its glyph atlas as coverage. It works without a window, on any thread.
pub(crate) struct PageFonts {
    fonts: Fonts,
    options: TextOptions,
}

impl PageFonts {
    pub(crate) fn new(definitions: FontDefinitions) -> Self {
        // Exact shapes for paper: neither hinted nor shifted by fractions of a pixel.
        let options = TextOptions { font_hinting: false, subpixel_binning: false, ..TextOptions::default() };
        Self { fonts: Fonts::new(options, definitions), options }
    }
}

impl TextRaster for PageFonts {
    fn line(&mut self, text: &str, size: f64) -> TextBitmap {
        // Also keeps the atlas from filling up during a long job.
        self.fonts.begin_pass(self.options, ViewportKey::default());
        // One point is one pixel here: the size is given in pixels.
        let galley = self.fonts.with_pixels_per_point(1.0).layout_no_wrap(
            text.to_owned(),
            FontId::proportional(size as f32),
            Color32::WHITE,
        );
        let width = galley.size().x.ceil() as usize + 1;
        let height = galley.size().y.ceil() as usize + 1;
        let mut coverage = vec![0u8; width * height];
        let atlas = self.fonts.texture_atlas().image();
        for row in &galley.rows {
            for glyph in &row.glyphs {
                let uv = glyph.uv_rect;
                if uv.is_nothing() {
                    continue;
                }
                let at = row.pos + glyph.pos.to_vec2() + uv.offset;
                let (left, top) = (at.x.round() as isize, at.y.round() as isize);
                let [u0, v0] = uv.min.map(usize::from);
                let [u1, v1] = uv.max.map(usize::from);
                for (y, v) in (v0..v1).enumerate() {
                    for (x, u) in (u0..u1).enumerate() {
                        let (px, py) = (left + x as isize, top + y as isize);
                        if px < 0 || py < 0 || px as usize >= width || py as usize >= height {
                            continue;
                        }
                        let value = atlas.pixels[v * atlas.size[0] + u].a();
                        let target = &mut coverage[py as usize * width + px as usize];
                        *target = (*target).max(value);
                    }
                }
            }
        }
        TextBitmap { width, height, coverage }
    }
}

/// What the print window keeps for the next time, during this session only.
/// "Include patient details" is not among it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PrintDefaults {
    paper: Paper,
    orientation: PageOrientation,
    per_page: usize,
}

impl Default for PrintDefaults {
    fn default() -> Self {
        Self { paper: Paper::A4, orientation: PageOrientation::Portrait, per_page: 9 }
    }
}

/// The options that change the pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageOptions {
    pub(crate) whole_series: bool,
    pub(crate) paper: Paper,
    pub(crate) orientation: PageOrientation,
    pub(crate) per_page: usize,
    pub(crate) include_details: bool,
}

/// What makes the pages of a job. Can be sent to a thread.
pub(crate) struct Plan {
    selected: Arc<Selected>,
    whole_series: bool,
    pub(crate) setup: PageSetup,
    header: PageHeader,
    smooth: bool,
    definitions: FontDefinitions,
}

impl Plan {
    pub(crate) fn new(selected: &Arc<Selected>, options: PageOptions, dpi: u32, smooth: bool, ctx: &Context) -> Self {
        // One image on a page of its own when printing the image on screen.
        let per_page = if options.whole_series { options.per_page } else { 1 };
        Self {
            selected: selected.clone(),
            whole_series: options.whole_series,
            setup: PageSetup { paper: options.paper, orientation: options.orientation, per_page, dpi },
            header: selected.page_header(options.include_details),
            smooth,
            definitions: font_definitions(ctx),
        }
    }

    fn images(&self) -> usize {
        if self.whole_series { self.selected.series.len() } else { 1 }
    }

    pub(crate) fn pages(&self) -> usize {
        self.setup.page_count(self.images())
    }

    pub(crate) fn page(&self, page: usize, fonts: &mut dyn TextRaster) -> Page {
        let pictures: Vec<PagePicture> = self
            .setup
            .page_images(page, self.images())
            .map(|i| if self.whole_series { self.selected.acquired_on_page(i) } else { self.selected.active_on_page() })
            .collect();
        compose_page(&self.setup, &self.header, page, self.pages(), &pictures, self.smooth, fonts)
    }

    /// All pages as a PDF in memory; `None` when cancelled.
    fn pdf(&self, progress: &Progress) -> Result<Option<Vec<u8>>, String> {
        let mut fonts = PageFonts::new(self.definitions.clone());
        let mut writer = PdfWriter::new();
        let pages = self.pages();
        progress.begin(tr!("Composing pages"), pages);
        for page in 0..pages {
            if progress.is_cancelled() {
                return Ok(None);
            }
            let composed = self.page(page, &mut fonts);
            writer.add_page(&composed.image, self.setup.dpi).map_err(|e| e.to_string())?;
            progress.advance();
        }
        Ok(Some(writer.finish()))
    }
}

/// What a press of a button in the window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Print,
    SavePdf,
    SaveAndOpen,
}

/// How a job in the background ended.
enum Outcome {
    Sent(Option<String>),
    Saved(String),
    SavedAndOpened(String),
    /// Saved, but the application for PDFs could not be started.
    SavedNotOpened(String, String),
    Cancelled,
    Failed(String),
}

/// The printers, as far as known.
enum PrinterList {
    Looking(Job<Result<Printers, String>>),
    Known(Printers),
    Failed(String),
    /// No print system on this computer.
    None,
}

/// The preview of the first page.
#[derive(Default)]
struct Preview {
    /// What is being composed.
    job: Option<(PageOptions, Job<ColorImage>)>,
    /// What is shown.
    shown: Option<(PageOptions, TextureHandle)>,
}

pub(crate) struct PrintWindow {
    selected: Arc<Selected>,
    options: PageOptions,
    copies: u32,
    printer: Option<String>,
    printers: PrinterList,
    preview: Preview,
    job: Option<Job<Outcome>>,
    /// A press waiting for the warning about burned-in text to be answered.
    warning: Option<Action>,
    /// The last failure; the window stays open and "Save as PDF…" is there.
    error: Option<String>,
    /// The window just opened: the button that prints takes the keyboard focus.
    fresh: bool,
    fresh_warning: bool,
}

impl PrintWindow {
    pub(crate) fn new(selected: Box<Selected>, defaults: PrintDefaults, system: &PrintSystem, waker: &Waker) -> Self {
        let printers = if system.is_available() {
            let system = system.clone();
            PrinterList::Looking(Job::spawn("printers", Some(waker.clone()), move |_| {
                system.printers().map_err(|e| e.to_string())
            }))
        } else {
            PrinterList::None
        };
        Self {
            selected: Arc::from(selected),
            options: PageOptions {
                whole_series: false,
                paper: defaults.paper,
                orientation: defaults.orientation,
                per_page: defaults.per_page,
                include_details: false,
            },
            copies: 1,
            printer: None,
            printers,
            preview: Preview::default(),
            job: None,
            warning: None,
            error: None,
            fresh: true,
            fresh_warning: false,
        }
    }

    /// Stops the job in the background, if there is one.
    pub(crate) fn cancel_job(&self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
    }

    /// What the window keeps for the next time.
    pub(crate) fn defaults(&self) -> PrintDefaults {
        PrintDefaults {
            paper: self.options.paper,
            orientation: self.options.orientation,
            per_page: self.options.per_page,
        }
    }

    fn poll_printers(&mut self) {
        let PrinterList::Looking(job) = &mut self.printers else { return };
        match job.poll() {
            Some(Ok(printers)) => {
                self.printer = printers.default.clone().or_else(|| printers.names.first().cloned());
                self.printers = PrinterList::Known(printers);
            }
            Some(Err(reason)) => self.printers = PrinterList::Failed(reason),
            // A fault stopped the search: the default printer can still be tried.
            None if job.is_finished() => {
                self.printers = PrinterList::Failed(tr!("the job stopped unexpectedly").to_owned());
            }
            None => {}
        }
    }

    /// Whether there is somewhere to print to: a printer from the list, or the
    /// system's default where the list could not be read.
    fn may_print(&self) -> bool {
        self.printer.is_some() || matches!(self.printers, PrinterList::Failed(_))
    }

    fn may_show_details(&self) -> bool {
        if self.options.whole_series {
            export::series_may_show_personal_details(&self.selected.meta)
        } else {
            self.selected.may_show_details()
        }
    }

    /// The window, for a frame. False: it is closed.
    pub(crate) fn show(
        &mut self,
        ctx: &Context,
        settings: &Settings,
        chooser: &dyn Chooser,
        system: &PrintSystem,
        waker: &Waker,
        toasts: &mut Toasts,
    ) -> bool {
        self.poll_printers();
        self.update_preview(ctx, settings, waker);
        if let Some(job) = &mut self.job
            && let Some(outcome) = job.poll()
        {
            self.job = None;
            match outcome {
                Outcome::Sent(printer) => {
                    toasts.info(match printer {
                        Some(printer) => tr!("Sent to {printer}.", printer = printer),
                        None => tr!("Sent to the printer.").to_owned(),
                    });
                    return false;
                }
                Outcome::Saved(name) => {
                    toasts.info(tr!("Saved {name}", name = name));
                    return false;
                }
                Outcome::SavedAndOpened(name) => {
                    toasts
                        .info(tr!("Saved {name}. Printing continues in the application that opened it.", name = name));
                    return false;
                }
                Outcome::SavedNotOpened(name, reason) => {
                    self.error =
                        Some(tr!("Saved {name}, but it could not be opened: {reason}", name = name, reason = reason));
                }
                Outcome::Cancelled => {}
                Outcome::Failed(reason) => self.error = Some(tr!("Could not print: {reason}", reason = reason)),
            }
        } else if self.job.as_ref().is_some_and(Job::is_finished) {
            self.job = None;
            self.error = Some(tr!("Could not print: {reason}", reason = tr!("the job stopped unexpectedly")));
        }
        let busy = self.job.is_some();
        if busy || self.preview.job.is_some() || matches!(self.printers, PrinterList::Looking(_)) {
            ctx.request_repaint_after(Duration::from_millis(60));
        }

        let available = system.is_available();
        let response = dialog(ctx, "print window", 690.0, |ui| self.contents(ui, settings, available, busy));
        let (action, cancelled) = response.inner;
        if let Some(action) = action {
            if self.may_show_details() {
                self.warning = Some(action);
                self.fresh_warning = true;
            } else {
                self.run(action, ctx, settings, chooser, system, waker);
            }
        }
        if let Some(pending) = self.warning {
            let response = personal_details_warning(ctx, "print warning", tr!("Print anyway"));
            let (no, yes) = &response.inner;
            if std::mem::take(&mut self.fresh_warning) {
                no.request_focus();
            }
            if yes.clicked() {
                self.warning = None;
                self.run(pending, ctx, settings, chooser, system, waker);
            } else if no.clicked() || closes(&response) {
                self.warning = None;
            }
            return true;
        }
        if busy {
            if cancelled || closes(&response) {
                self.cancel_job();
            }
            return true;
        }
        !(cancelled || closes(&response))
    }

    /// Everything in the window. Returns what was pressed.
    fn contents(&mut self, ui: &mut Ui, settings: &Settings, available: bool, busy: bool) -> (Option<Action>, bool) {
        // Translators: the title of the print window; "Print" on its button is an instruction.
        ui.heading(tr_ctx!("print window", "Print"));
        ui.add_space(4.0);
        ui.columns_const::<2, _>(|[left, right]| {
            // Left-aligned: columns stretch their entries, which would space the words out.
            left.with_layout(Layout::top_down(Align::Min), |ui| self.options_column(ui, settings, available, busy));
            self.preview_column(right);
        });
        ui.add_space(6.0);
        self.action_row(ui, available, busy)
    }

    fn options_column(&mut self, ui: &mut Ui, settings: &Settings, available: bool, busy: bool) {
        ui.add_enabled_ui(!busy, |ui| {
            let series_loaded = self.selected.series.is_complete();
            let count = self.selected.series.len();
            ui.radio_value(&mut self.options.whole_series, false, tr!("This image"));
            let series = RadioButton::new(
                self.options.whole_series,
                tr_n!("Whole series ({n} image)", "Whole series ({n} images)", count),
            );
            if ui.add_enabled(series_loaded, series).clicked() {
                self.options.whole_series = true;
            }
            if !series_loaded {
                small_note(ui, tr!("The whole series can be printed when it has loaded."));
            }
            ui.add_space(4.0);
            Grid::new("print options").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                if available {
                    self.printer_row(ui);
                    let label = ui.label(tr!("Copies"));
                    ui.add(DragValue::new(&mut self.copies).range(1..=99)).labelled_by(label.id);
                    ui.end_row();
                }
                self.page_rows(ui);
            });
            ui.add_space(4.0);
            details_checkbox(ui, &mut self.options.include_details, true, settings.hide_patient_details);
            ui.add_space(4.0);
            let mirrored = if self.options.whole_series {
                self.selected.state.transform(ViewKind::Stack).orientation.is_mirrored()
            } else {
                self.selected.is_mirrored()
            };
            if mirrored {
                warning_note(ui, tr!("This view is mirrored, and the print will be too. The letters give the true directions."));
            }
            small_note(ui, tr!("Printed images are not at true size. The scale bar gives the scale."));
            if available {
                small_note(ui, tr!("The system's print queue may keep a copy until it has printed."));
            } else {
                small_note(
                    ui,
                    tr!("This computer has no print system SumMRIse can use. Printing continues in the application that opens the PDF."),
                );
            }
        });
    }

    fn printer_row(&mut self, ui: &mut Ui) {
        let label = ui.label(tr!("Printer"));
        match &self.printers {
            PrinterList::Looking(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    field_text(ui, tr!("Looking for printers…"));
                });
            }
            PrinterList::Known(printers) if !printers.names.is_empty() => {
                let names = printers.names.clone();
                let shown = self.printer.clone().unwrap_or_default();
                ComboBox::from_id_salt("print printer")
                    .selected_text(shown)
                    .truncate()
                    .width(FIELD_WIDTH)
                    .show_ui(ui, |ui| {
                        for name in names {
                            let value = Some(name.clone());
                            ui.selectable_value(&mut self.printer, value, name);
                        }
                    })
                    .response
                    .labelled_by(label.id);
            }
            PrinterList::Known(_) => {
                field_text(ui, tr!("No printer is set up on this computer."));
            }
            PrinterList::Failed(reason) => {
                // `lp` may still know where to print.
                field_text(ui, tr!("The system's default printer"))
                    .on_hover_text(tr!("The list of printers is not available: {reason}", reason = reason));
            }
            PrinterList::None => {}
        }
        ui.end_row();
    }

    fn page_rows(&mut self, ui: &mut Ui) {
        let label = ui.label(tr!("Paper"));
        ComboBox::from_id_salt("print paper")
            .selected_text(self.options.paper.label())
            .width(FIELD_WIDTH)
            .show_ui(ui, |ui| {
                for paper in Paper::ALL {
                    ui.selectable_value(&mut self.options.paper, paper, paper.label());
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();

        // Translators: how a printed page lies: upright or on its side.
        let label = ui.label(tr!("Page"));
        ComboBox::from_id_salt("print orientation")
            .selected_text(self.options.orientation.label())
            .width(FIELD_WIDTH)
            .show_ui(ui, |ui| {
                for orientation in PageOrientation::ALL {
                    ui.selectable_value(&mut self.options.orientation, orientation, orientation.label());
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();

        let label = ui.label(tr!("Images per page"));
        ui.add_enabled_ui(self.options.whole_series, |ui| {
            let shown = if self.options.whole_series { self.options.per_page } else { 1 };
            ComboBox::from_id_salt("print per page")
                .selected_text(shown.to_string())
                .width(FIELD_WIDTH)
                .show_ui(ui, |ui| {
                    for per_page in IMAGES_PER_PAGE {
                        ui.selectable_value(&mut self.options.per_page, per_page, per_page.to_string());
                    }
                })
                .response
                .labelled_by(label.id);
        });
        ui.end_row();
    }

    fn pages(&self) -> usize {
        let per_page = if self.options.whole_series { self.options.per_page } else { 1 };
        let images = if self.options.whole_series { self.selected.series.len() } else { 1 };
        images.div_ceil(per_page.max(1))
    }

    fn preview_column(&mut self, ui: &mut Ui) {
        let weak = theme::palette(ui).text_weak;
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(tr!("Page {page} of {pages}", page = 1, pages = self.pages())).small().color(weak));
            let (paper_width, paper_height) = PageSetup {
                paper: self.options.paper,
                orientation: self.options.orientation,
                per_page: 1,
                dpi: PREVIEW_DPI,
            }
            .pixels();
            // Fitted into the column: a page on its side is limited by the width.
            let scale = (ui.available_width() / paper_width as f32).min(PREVIEW_HEIGHT / paper_height as f32);
            let size = vec2(paper_width as f32, paper_height as f32) * scale;
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
            ui.painter().rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, theme::palette(ui).border),
                egui::StrokeKind::Outside,
            );
            let ready = self.preview.job.is_none();
            if let Some((_, texture)) = &self.preview.shown {
                let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().image(texture.id(), rect, whole, Color32::WHITE);
            }
            if !ready {
                ui.put(egui::Rect::from_center_size(rect.center(), vec2(28.0, 28.0)), egui::Spinner::new());
            }
            let description = if ready { tr!("Preview of the first page") } else { tr!("Composing the preview…") };
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, description));
        });
    }

    fn action_row(&mut self, ui: &mut Ui, available: bool, busy: bool) -> (Option<Action>, bool) {
        if let Some(error) = &self.error {
            let color = theme::palette(ui).error;
            ui.add(egui::Label::new(RichText::new(error.as_str()).color(color)).wrap());
        }
        if let Some(job) = &self.job {
            let snapshot = job.progress();
            ui.add(
                egui::ProgressBar::new(snapshot.fraction().unwrap_or(0.0))
                    .text(format!("{} {} / {}", snapshot.stage, snapshot.done, snapshot.total)),
            );
        }
        let primary = if available { tr!("Print") } else { tr!("Save PDF and open…") };
        let enabled = !busy && (!available || self.may_print());
        let (cancelled, primary, save) = ui
            .with_layout(Layout::right_to_left(Align::Center), |ui| {
                let primary: Response = ui.add_enabled(enabled, egui::Button::new(primary));
                let save = ui.add_enabled(!busy, egui::Button::new(tr!("Save as PDF…")));
                (ui.button(tr!("Cancel")), primary, save)
            })
            .inner;
        // The button in focus when the window opens: "Print", once it is known whether
        // there is a printer. It never takes the focus from where the user has gone.
        if self.fresh && !matches!(self.printers, PrinterList::Looking(_)) {
            self.fresh = false;
            if ui.ctx().memory(|m| m.focused().is_none()) {
                if enabled { primary.request_focus() } else { save.request_focus() }
            }
        }
        let action = if primary.clicked() {
            Some(if available { Action::Print } else { Action::SaveAndOpen })
        } else if save.clicked() {
            Some(Action::SavePdf)
        } else {
            None
        };
        (action, cancelled.clicked())
    }

    /// Keeps the preview in step with the options: the first page is composed
    /// in the background, in a lower resolution.
    fn update_preview(&mut self, ctx: &Context, settings: &Settings, waker: &Waker) {
        if let Some((options, job)) = &mut self.preview.job
            && let Some(image) = job.poll()
        {
            let texture = ctx.load_texture("print-preview", image, TextureOptions::LINEAR);
            self.preview.shown = Some((*options, texture));
            self.preview.job = None;
        }
        let wanted = self.options;
        let up_to_date = self.preview.job.as_ref().is_some_and(|(options, _)| *options == wanted)
            || (self.preview.job.is_none()
                && self.preview.shown.as_ref().is_some_and(|(options, _)| *options == wanted));
        if up_to_date {
            return;
        }
        if let Some((_, job)) = self.preview.job.take() {
            job.cancel();
        }
        let plan = Plan::new(&self.selected, wanted, PREVIEW_DPI, settings.smooth_interpolation, ctx);
        let job = Job::spawn("print-preview", Some(waker.clone()), move |_| {
            let mut fonts = PageFonts::new(plan.definitions.clone());
            to_image(&plan.page(0, &mut fonts).image)
        });
        self.preview.job = Some((wanted, job));
    }

    /// Starts what `action` asks for. No file is written for "Print".
    fn run(
        &mut self,
        action: Action,
        ctx: &Context,
        settings: &Settings,
        chooser: &dyn Chooser,
        system: &PrintSystem,
        waker: &Waker,
    ) {
        self.error = None;
        let target = match action {
            Action::Print => None,
            Action::SavePdf | Action::SaveAndOpen => {
                let meta = &self.selected.meta;
                let suggested = export::pdf_file_name(meta.modality.trim(), meta.number);
                let title = if action == Action::SavePdf { tr!("Save as PDF…") } else { tr!("Save PDF and open…") };
                let Some(path) = chooser.save_file(title, "PDF", &["pdf"], &suggested) else { return };
                Some(with_pdf_extension(path))
            }
        };
        let plan = Plan::new(&self.selected, self.options, PRINT_DPI, settings.smooth_interpolation, ctx);
        let system = system.clone();
        let printer = self.printer.clone();
        let copies = self.copies;
        let paper = self.options.paper;
        self.job = Some(Job::spawn("print", Some(waker.clone()), move |progress| {
            let pdf = match plan.pdf(progress) {
                Ok(Some(pdf)) => pdf,
                Ok(None) => return Outcome::Cancelled,
                Err(reason) => return Outcome::Failed(reason),
            };
            let Some(path) = target else {
                let job = PrintJob { printer: printer.clone(), copies, paper };
                return match system.send(&pdf, &job) {
                    Ok(()) => Outcome::Sent(printer),
                    Err(e) => Outcome::Failed(e.to_string()),
                };
            };
            let name = file_name(&path);
            if let Err(e) = export::write_file(&path, &name, &pdf) {
                return Outcome::Failed(e.to_string());
            }
            if action == Action::SavePdf {
                return Outcome::Saved(name);
            }
            match system.open(&path) {
                Ok(()) => Outcome::SavedAndOpened(name),
                Err(e) => Outcome::SavedNotOpened(name, e.to_string()),
            }
        }));
    }
}

/// Text in the place of a field, wrapped to its width.
fn field_text(ui: &mut Ui, text: &str) -> Response {
    ui.scope(|ui| {
        ui.set_max_width(FIELD_WIDTH);
        ui.add(egui::Label::new(text).wrap())
    })
    .inner
}

fn with_pdf_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".pdf");
    PathBuf::from(name)
}

fn to_image(picture: &Picture) -> ColorImage {
    ColorImage::from_rgb([picture.width, picture.height], &picture.rgb)
}
