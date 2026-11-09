//! Saving and copying views and exporting series (`openspec/changes/export-and-share`):
//! the menu in the toolbar, the keys, the windows and what they start. Private by
//! default: nothing about the patient is in a picture, a file name or a file's
//! metadata unless the user asks for it for one export, and the choice is never
//! remembered.
//!
//! The viewer reaches all of this through [`Source`] (what is on screen) and
//! [`Export`]. Printing has its own window in `print`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egui::containers::menu::{MenuButton, MenuConfig};
use egui::text_selection::LabelSelectionState;
use egui::{
    Button, ColorImage, ComboBox, Context, Event, FontDefinitions, Id, Key, KeyboardShortcut, Label, Modal,
    ModalResponse, Modifiers, PopupCloseBehavior, Rect, Response, RichText, Ui, Vec2, vec2,
};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::{ICON_CONTENT_COPY, ICON_IOS_SHARE, ICON_PHOTO_LIBRARY, ICON_PRINT, ICON_SAVE};
use summrise_core::dicom::{Series, Study};
use summrise_core::export::{self, ImageFormat, Picture, SeriesExported};
use summrise_core::print::PrintSystem;
use summrise_core::{Job, OverlayStatus, SeriesData, SeriesKey, Session, Settings, Tiles, ViewKind, ViewState, Waker};
use summrise_i18n::{tr, tr_n};

use crate::app::Toasts;
use crate::print::{self, PrintDefaults, PrintWindow};
use crate::theme;
use crate::volume_view::{ShownPicture, VolumeView};
use crate::widgets::{icon, shortcut};

pub(crate) mod annotated;
mod picture;
mod raster;

/// Where the size of the active view is noted (see [`note_view`]).
const VIEW_SIZE: &str = "export: size of the active view";
/// A view that has never been on screen, in points.
const DEFAULT_VIEW_SIZE: Vec2 = vec2(1024.0, 768.0);

/// Called by each view as it is drawn: the picture "with annotations" has the
/// size of the active view on screen.
pub(crate) fn note_view(ctx: &Context, rect: Rect, active: bool) {
    if active {
        ctx.data_mut(|d| d.insert_temp(Id::unique(VIEW_SIZE), rect.size()));
    }
}

/// What is on screen, for the commands to work on.
#[derive(Clone, Copy)]
pub(crate) struct Source<'a> {
    session: &'a Session,
    views: &'a HashMap<SeriesKey, ViewState>,
    view: ViewKind,
    volume: &'a VolumeView,
    tiles: Tiles,
}

impl<'a> Source<'a> {
    /// What the commands work on: the active viewport's series, view and tile grid.
    pub(crate) fn new(
        session: &'a Session,
        views: &'a HashMap<SeriesKey, ViewState>,
        view: ViewKind,
        volume: &'a VolumeView,
        tiles: Tiles,
    ) -> Self {
        Self { session, views, view, volume, tiles }
    }

    fn active(&self) -> Option<(SeriesKey, Arc<SeriesData>, &'a ViewState)> {
        let key = self.session.active_series?;
        let series = self.session.data(key).filter(|series| series.is_viewable())?;
        Some((key, series, self.views.get(&key)?))
    }

    /// A copy of what the commands need, which background jobs can keep.
    pub(crate) fn selected(&self) -> Option<Selected> {
        let (key, series, state) = self.active()?;
        let overlay = self.session.overlay().zip(match self.session.overlay_status() {
            Some(OverlayStatus::Ready(data)) => Some(data),
            _ => None,
        });
        Some(Selected {
            key,
            meta: self.session.series(key)?.clone(),
            study: self.session.studies().get(key.study)?.clone(),
            state: state.clone(),
            view: self.view,
            overlay,
            fusion: crate::pet::shown(self.session, Some(state)),
            picture_3d: self.volume.shown_picture(key, self.view, state),
            tiles: self.tiles,
            series,
        })
    }

    /// Whether there is a picture to save, and a whole series to export.
    fn ready(&self) -> (bool, bool) {
        let Some((key, series, state)) = self.active() else { return (false, false) };
        let image = match self.view {
            ViewKind::Stack => series.frame(state.slot_index(&series)).is_some(),
            ViewKind::Mpr(_) => series.stack_volume(state.stack()).is_some(),
            ViewKind::Volume => self.volume.shown_picture(key, self.view, state).is_some_and(|p| p.is_final()),
        };
        (image, series.is_complete())
    }
}

/// What a command works on: the series and view on screen when it started.
#[derive(Clone)]
pub(crate) struct Selected {
    pub(crate) key: SeriesKey,
    pub(crate) series: Arc<SeriesData>,
    pub(crate) meta: Series,
    pub(crate) study: Study,
    pub(crate) state: ViewState,
    pub(crate) view: ViewKind,
    /// The PET drawn over the images, once it is ready.
    pub(crate) overlay: Option<(SeriesKey, Arc<SeriesData>)>,
    pub(crate) fusion: Option<crate::pet::Shown>,
    pub(crate) picture_3d: Option<ShownPicture>,
    /// The tile grid on screen: annotated pictures show it; image only is the current image.
    pub(crate) tiles: Tiles,
}

impl Selected {
    /// Whether the picture of the active view may itself show personal details.
    pub(crate) fn may_show_details(&self) -> bool {
        let header = (self.view == ViewKind::Stack)
            .then(|| self.series.frames().get(self.state.slot_index(&self.series)))
            .flatten()
            .and_then(|info| self.meta.instances.get(info.instance));
        match header {
            Some(header) => export::may_show_personal_details(header, &self.meta.modality),
            // A reformat or the 3D view is made from all images.
            None => export::series_may_show_personal_details(&self.meta),
        }
    }

    /// Whether the picture is mirrored, as the view on screen shows it.
    pub(crate) fn is_mirrored(&self) -> bool {
        self.view != ViewKind::Volume && self.state.transform(self.view).orientation.is_mirrored()
    }

    /// The neutral name suggested for the picture of the active view.
    fn suggested_name(&self, format: ImageFormat) -> String {
        let (view, number) = match self.view {
            ViewKind::Stack => (None, Some(self.state.slot_index(&self.series) + 1)),
            ViewKind::Mpr(_) => (Some("MPR"), Some(self.state.slice_index(self.view, &self.series) + 1)),
            ViewKind::Volume => (Some("3D"), None),
        };
        export::image_file_name(self.meta.modality.trim(), self.meta.number, view, number, format)
    }

    /// The picture the clipboard and an image-only file get. The 3D view always
    /// carries its caption, because its picture is computed.
    fn plain_picture(&self, ctx: &Context) -> Option<Picture> {
        let picture = self.image_only()?.picture;
        if self.view != ViewKind::Volume {
            return Some(picture);
        }
        let mut fonts = print::PageFonts::new(font_definitions(ctx));
        Some(picture.with_caption(tr!("Illustration: colours and surfaces are computed"), &mut fonts))
    }
}

/// The fonts of the interface, also those found on the system for text on
/// screen in other scripts, so that pictures read as the screen does.
pub(crate) fn font_definitions(ctx: &Context) -> FontDefinitions {
    ctx.fonts(|fonts| {
        let mut definitions = fonts.definitions().clone();
        for found in fonts.discovered_fonts() {
            definitions.font_data.insert(found.name.clone(), Arc::new(found.data.clone()));
            for family in &found.families {
                definitions.families.entry(family.family.clone()).or_default().push(found.name.clone());
            }
        }
        definitions
    })
}

/// Asks the user where a file goes. Native dialogs, except in tests.
pub(crate) trait Chooser {
    /// Where to save a file, starting from the name `suggested`.
    fn save_file(&self, title: &str, kinds: &str, extensions: &[&str], suggested: &str) -> Option<PathBuf>;
    /// Which folder to export into.
    fn folder(&self, title: &str) -> Option<PathBuf>;
}

struct NativeDialogs;

impl Chooser for NativeDialogs {
    fn save_file(&self, title: &str, kinds: &str, extensions: &[&str], suggested: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).set_file_name(suggested).add_filter(kinds, extensions).save_file()
    }

    fn folder(&self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }
}

/// "Image only" or "With annotations".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Content {
    #[default]
    ImageOnly,
    WithAnnotations,
}

/// What the window "Save image" asks; never remembered.
#[derive(Debug, Clone, Copy, Default)]
struct SaveOptions {
    content: Content,
    format: ImageFormat,
    include_details: bool,
}

/// What continues after the warning about burned-in text.
#[derive(Debug, Clone, Copy)]
enum Warned {
    Save(SaveOptions),
    Copy,
    ExportSeries(ImageFormat),
}

/// The menu and the keys ask for one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    SaveImage,
    CopyImage,
    ExportSeries,
    Print,
}

/// Where the user is.
#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    SaveOptions(Box<Selected>, SaveOptions),
    SeriesOptions(Box<Selected>, ImageFormat),
    Warning(Box<Selected>, Warned),
    /// Some of the files exist already: the folder, the format and their number.
    Overwrite(Box<Selected>, PathBuf, ImageFormat, usize),
    Exporting(Job<SeriesExported>, PathBuf),
    Print(Box<PrintWindow>),
}

/// Saving, copying, exporting and printing.
pub(crate) struct Export {
    stage: Stage,
    request: Option<Command>,
    /// The window just opened: its first button takes the keyboard focus.
    fresh: bool,
    pub(crate) chooser: Arc<dyn Chooser>,
    pub(crate) print_system: PrintSystem,
    print_defaults: PrintDefaults,
}

impl Default for Export {
    fn default() -> Self {
        Self {
            stage: Stage::Idle,
            request: None,
            fresh: false,
            chooser: Arc::new(NativeDialogs),
            print_system: PrintSystem::system(),
            print_defaults: PrintDefaults::default(),
        }
    }
}

impl Drop for Export {
    /// A study closed during an export: nothing goes on behind the user's back.
    fn drop(&mut self) {
        match &self.stage {
            Stage::Exporting(job, _) => job.cancel(),
            Stage::Print(window) => window.cancel_job(),
            _ => {}
        }
    }
}

impl Export {
    /// Whether one of its windows is open: the keys of the viewer rest meanwhile.
    pub(crate) fn is_open(&self) -> bool {
        !matches!(self.stage, Stage::Idle) || self.request.is_some()
    }

    /// The keys: ⌘S, ⇧⌘S, ⌘P and the copy command. To be called whatever has
    /// the keyboard focus: these keys mean nothing else here.
    pub(crate) fn keys(&mut self, ctx: &Context) {
        let pressed = |modifiers: Modifiers, key: Key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)))
        };
        // The longer shortcut first: with Shift held, the shorter one matches as well.
        let mut command = None;
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
            command = Some(Command::ExportSeries);
        } else if pressed(Modifiers::COMMAND, Key::S) {
            command = Some(Command::SaveImage);
        }
        if pressed(Modifiers::COMMAND, Key::P) {
            command = Some(Command::Print);
        }
        // egui turns the copy shortcut into an event of its own. Copying text
        // goes first: a text field in use, or text selected in a label.
        let copy = ctx.input(|i| i.events.iter().any(|event| matches!(event, Event::Copy)));
        if copy && !ctx.egui_wants_keyboard_input() && !ctx.plugin::<LabelSelectionState>().lock().has_selection() {
            command = Some(Command::CopyImage);
        }
        if !self.is_open() {
            self.request = command;
        }
    }

    /// The icon menu in the toolbar.
    pub(crate) fn menu(&mut self, ui: &mut Ui, source: &Source<'_>) {
        let (image_ready, series_ready) = source.ready();
        let config = MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside);
        let (button, _) = MenuButton::new(icon(ICON_IOS_SHARE)).config(config).ui(ui, |ui| {
            ui.set_min_width(220.0);
            let not_ready = tr!("Available when the image on screen has loaded");
            let not_loaded = tr!("Available when all images of the series have loaded");
            let shift_s = tr!("Shift+{key}", key = shortcut("S"));
            let copy_key = shortcut("C");
            let entries = [
                (
                    Command::SaveImage,
                    ICON_SAVE,
                    tr!("Save image…"),
                    shortcut("S"),
                    tr!("Save the view as a PNG or JPEG file ({shortcut})", shortcut = shortcut("S")),
                    image_ready,
                    not_ready,
                ),
                (
                    Command::CopyImage,
                    ICON_CONTENT_COPY,
                    tr!("Copy image"),
                    copy_key.clone(),
                    tr!(
                        "Copy the picture, without patient details, to the clipboard ({shortcut})",
                        shortcut = copy_key
                    ),
                    image_ready,
                    not_ready,
                ),
                (
                    Command::ExportSeries,
                    ICON_PHOTO_LIBRARY,
                    tr!("Export series…"),
                    shift_s.clone(),
                    tr!("Save all images of the series as numbered files ({shortcut})", shortcut = shift_s),
                    series_ready,
                    not_loaded,
                ),
                (
                    Command::Print,
                    ICON_PRINT,
                    tr!("Print…"),
                    shortcut("P"),
                    tr!("Print the image or the series, or save them as a PDF ({shortcut})", shortcut = shortcut("P")),
                    image_ready,
                    not_ready,
                ),
            ];
            for (command, glyph, name, key, hint, enabled, disabled_hint) in entries {
                if menu_entry(ui, glyph, name, key, &hint, enabled, disabled_hint).clicked() {
                    self.request = Some(command);
                    ui.close();
                }
            }
        });
        button
            .accessible_name(tr!("Save, copy or print"))
            .on_hover_text(tr!("Save or copy the image, export the series, or print"));
    }

    /// The windows, and whatever was asked for since the last frame.
    pub(crate) fn show(
        &mut self,
        ctx: &Context,
        source: &Source<'_>,
        settings: &Settings,
        waker: &Waker,
        toasts: &mut Toasts,
    ) {
        if let Some(command) = self.request.take() {
            self.start(command, ctx, source, waker, toasts);
        }
        let stage = std::mem::take(&mut self.stage);
        self.stage = match stage {
            Stage::Idle => Stage::Idle,
            Stage::SaveOptions(selected, options) => self.save_window(ctx, selected, options, settings, toasts),
            Stage::SeriesOptions(selected, format) => self.series_window(ctx, selected, format, waker),
            Stage::Warning(selected, warned) => self.warning(ctx, selected, warned, settings, waker, toasts),
            Stage::Overwrite(selected, folder, format, existing) => {
                self.overwrite_window(ctx, selected, folder, format, existing, waker)
            }
            Stage::Exporting(job, folder) => exporting_window(ctx, job, folder, toasts),
            Stage::Print(mut window) => {
                if window.show(ctx, settings, self.chooser.as_ref(), &self.print_system, waker, toasts) {
                    Stage::Print(window)
                } else {
                    self.print_defaults = window.defaults();
                    Stage::Idle
                }
            }
        };
    }

    fn start(&mut self, command: Command, ctx: &Context, source: &Source<'_>, waker: &Waker, toasts: &mut Toasts) {
        let (image_ready, series_ready) = source.ready();
        let Some(selected) = source.selected().map(Box::new) else { return };
        self.fresh = true;
        self.stage = match command {
            Command::SaveImage if image_ready => Stage::SaveOptions(selected, SaveOptions::default()),
            Command::ExportSeries if series_ready => Stage::SeriesOptions(selected, ImageFormat::default()),
            Command::Print if image_ready => {
                Stage::Print(Box::new(PrintWindow::new(selected, self.print_defaults, &self.print_system, waker)))
            }
            Command::CopyImage if image_ready => {
                if selected.may_show_details() {
                    Stage::Warning(selected, Warned::Copy)
                } else {
                    copy_image(ctx, &selected, toasts);
                    Stage::Idle
                }
            }
            // The keys work anywhere; the menu says why an entry is greyed out.
            Command::SaveImage | Command::CopyImage | Command::Print => {
                toasts.info(tr!("Available when the image on screen has loaded"));
                Stage::Idle
            }
            Command::ExportSeries => {
                toasts.info(tr!("Available when all images of the series have loaded"));
                Stage::Idle
            }
        };
    }

    /// The keyboard focus for the button a window starts on, once.
    fn focus_once(&mut self, response: &Response) {
        if std::mem::take(&mut self.fresh) {
            response.request_focus();
        }
    }

    fn save_window(
        &mut self,
        ctx: &Context,
        selected: Box<Selected>,
        mut options: SaveOptions,
        settings: &Settings,
        toasts: &mut Toasts,
    ) -> Stage {
        let hide = settings.hide_patient_details;
        let is_3d = selected.view == ViewKind::Volume;
        let mirrored = selected.is_mirrored();
        let response = dialog(ctx, "export save window", 430.0, |ui| {
            ui.heading(tr!("Save image"));
            ui.add_space(4.0);
            ui.radio_value(&mut options.content, Content::ImageOnly, tr!("Image only"));
            small_note(
                ui,
                if is_3d {
                    tr!("The 3D picture as computed for the screen, with its caption that it is an illustration.")
                } else {
                    tr!(
                        "The picture itself at its own resolution, in the current contrast, without any text. Zoom and pan are left out; it is turned and flipped like the view."
                    )
                },
            );
            ui.radio_value(&mut options.content, Content::WithAnnotations, tr!("With annotations"));
            small_note(
                ui,
                tr!(
                    "The view as on screen, with orientation letters, scale bar and contrast values, marked as not for diagnostic use."
                ),
            );
            ui.add_space(4.0);
            format_choice(ui, &mut options.format);
            details_checkbox(ui, &mut options.include_details, options.content == Content::WithAnnotations, hide);
            if mirrored && options.content == Content::ImageOnly {
                warning_note(
                    ui,
                    tr!(
                        "This view is mirrored, and the file will be too. An image-only file has no letters to show the true directions."
                    ),
                );
            }
            ui.add_space(6.0);
            buttons(ui, tr!("Save…"))
        });
        let (cancelled, confirmed) = &response.inner;
        self.focus_once(confirmed);
        if confirmed.clicked() {
            return if selected.may_show_details() {
                self.fresh = true;
                Stage::Warning(selected, Warned::Save(options))
            } else {
                save_image(ctx, &selected, options, settings, self.chooser.as_ref(), toasts);
                Stage::Idle
            };
        }
        if cancelled.clicked() || closes(&response) {
            return Stage::Idle;
        }
        Stage::SaveOptions(selected, options)
    }

    fn series_window(
        &mut self,
        ctx: &Context,
        selected: Box<Selected>,
        mut format: ImageFormat,
        waker: &Waker,
    ) -> Stage {
        let count = selected.series.len();
        let mirrored = selected.state.transform(ViewKind::Stack).orientation.is_mirrored();
        let response = dialog(ctx, "export series window", 430.0, |ui| {
            ui.heading(tr!("Export series"));
            ui.add_space(4.0);
            ui.add(
                Label::new(tr_n!(
                    "Saves {n} image as a numbered file, in the current contrast, without any text.",
                    "Saves all {n} images as numbered files, in the current contrast, without any text.",
                    count
                ))
                .wrap(),
            );
            ui.add_space(4.0);
            format_choice(ui, &mut format);
            if mirrored {
                warning_note(
                    ui,
                    tr!(
                        "These images are shown mirrored, and the files will be too. They have no letters to show the true directions."
                    ),
                );
            }
            ui.add_space(6.0);
            buttons(ui, tr!("Choose folder…"))
        });
        let (cancelled, confirmed) = &response.inner;
        self.focus_once(confirmed);
        if confirmed.clicked() {
            return if export::series_may_show_personal_details(&selected.meta) {
                self.fresh = true;
                Stage::Warning(selected, Warned::ExportSeries(format))
            } else {
                self.choose_folder(selected, format, waker)
            };
        }
        if cancelled.clicked() || closes(&response) {
            return Stage::Idle;
        }
        Stage::SeriesOptions(selected, format)
    }

    fn choose_folder(&mut self, selected: Box<Selected>, format: ImageFormat, waker: &Waker) -> Stage {
        let Some(folder) = self.chooser.folder(tr!("Choose a folder for the images")) else {
            return Stage::Idle;
        };
        let existing = export::existing_files(&folder, &series_names(&selected, format)).len();
        if existing > 0 {
            self.fresh = true;
            return Stage::Overwrite(selected, folder, format, existing);
        }
        start_series_export(selected, folder, format, waker)
    }

    fn warning(
        &mut self,
        ctx: &Context,
        selected: Box<Selected>,
        warned: Warned,
        settings: &Settings,
        waker: &Waker,
        toasts: &mut Toasts,
    ) -> Stage {
        let confirm = match warned {
            Warned::Save(_) | Warned::ExportSeries(_) => tr!("Export anyway"),
            Warned::Copy => tr!("Copy anyway"),
        };
        let response = personal_details_warning(ctx, "export warning", confirm);
        let (cancelled, confirmed) = &response.inner;
        // Nothing happens with Enter alone: the button that stops is the first in focus.
        self.focus_once(cancelled);
        if confirmed.clicked() {
            return match warned {
                Warned::Save(options) => {
                    save_image(ctx, &selected, options, settings, self.chooser.as_ref(), toasts);
                    Stage::Idle
                }
                Warned::Copy => {
                    copy_image(ctx, &selected, toasts);
                    Stage::Idle
                }
                Warned::ExportSeries(format) => self.choose_folder(selected, format, waker),
            };
        }
        if cancelled.clicked() || closes(&response) {
            return Stage::Idle;
        }
        Stage::Warning(selected, warned)
    }

    fn overwrite_window(
        &mut self,
        ctx: &Context,
        selected: Box<Selected>,
        folder: PathBuf,
        format: ImageFormat,
        existing: usize,
        waker: &Waker,
    ) -> Stage {
        let response = dialog(ctx, "export overwrite window", 400.0, |ui| {
            ui.heading(tr!("Replace existing files?"));
            ui.add_space(4.0);
            ui.add(
                Label::new(tr_n!(
                    "{n} file with the same name already exists in this folder.",
                    "{n} files with the same names already exist in this folder.",
                    existing
                ))
                .wrap(),
            );
            ui.add_space(6.0);
            buttons(ui, tr!("Replace"))
        });
        let (cancelled, confirmed) = &response.inner;
        self.focus_once(cancelled);
        if confirmed.clicked() {
            return start_series_export(selected, folder, format, waker);
        }
        if cancelled.clicked() || closes(&response) {
            return Stage::Idle;
        }
        Stage::Overwrite(selected, folder, format, existing)
    }
}

fn menu_entry(
    ui: &mut Ui,
    glyph: MaterialIcon,
    name: &str,
    key: String,
    hint: &str,
    enabled: bool,
    disabled_hint: &str,
) -> Response {
    let button = Button::new(format!("{} {name}", icon(glyph))).shortcut_text(key);
    ui.add_enabled(enabled, button).accessible_name(name).on_hover_text(hint).on_disabled_hover_text(disabled_hint)
}

/// A modal window `width` points wide.
pub(crate) fn dialog<R>(
    ctx: &Context,
    id: &'static str,
    width: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> ModalResponse<R> {
    Modal::new(Id::unique(id)).show(ctx, |ui| {
        ui.set_width(width);
        add(ui)
    })
}

/// Whether Esc closes the window. A stray click beside it does not: options set
/// or a job running should not be lost to that.
pub(crate) fn closes<R>(response: &ModalResponse<R>) -> bool {
    response.should_close() && !response.backdrop_response.clicked()
}

/// "Cancel" and the button that goes on, at the right.
pub(crate) fn buttons(ui: &mut Ui, confirm: &str) -> (Response, Response) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let confirmed = ui.button(confirm);
        let cancelled = ui.button(tr!("Cancel"));
        (cancelled, confirmed)
    })
    .inner
}

/// Weak text under an option.
pub(crate) fn small_note(ui: &mut Ui, text: &str) {
    let weak = theme::palette(ui).text_weak;
    ui.add(Label::new(RichText::new(text).small().color(weak)).wrap());
}

/// Something the user must not miss.
pub(crate) fn warning_note(ui: &mut Ui, text: &str) {
    let warn = theme::palette(ui).warn;
    ui.add(Label::new(RichText::new(text).color(warn)).wrap());
}

fn format_choice(ui: &mut Ui, format: &mut ImageFormat) {
    ui.horizontal(|ui| {
        // Translators: the file format of a saved image.
        let label = ui.label(tr!("Format"));
        ComboBox::from_id_salt("export format")
            .selected_text(format.label())
            .show_ui(ui, |ui| {
                for candidate in ImageFormat::ALL {
                    ui.selectable_value(format, candidate, candidate.label());
                }
            })
            .response
            .labelled_by(label.id);
    });
}

/// "Include patient details": off by default and never remembered; in privacy
/// mode it stays off, because that mode hides these details everywhere.
pub(crate) fn details_checkbox(ui: &mut Ui, include: &mut bool, applies: bool, privacy_mode: bool) {
    if privacy_mode || !applies {
        *include = false;
    }
    let response = ui
        .add_enabled(applies && !privacy_mode, egui::Checkbox::new(include, tr!("Include patient details")))
        .on_hover_text(tr!("Adds the patient's name, ID and birth date, for this time only"));
    if privacy_mode {
        response.on_disabled_hover_text(tr!("Privacy mode is on: patient details stay hidden"));
    } else {
        response.on_disabled_hover_text(tr!("An image-only file has no text"));
    }
}

/// The warning before a picture leaves that may show personal details itself:
/// "Cancel" and `confirm`.
pub(crate) fn personal_details_warning(
    ctx: &Context,
    id: &'static str,
    confirm: &str,
) -> ModalResponse<(Response, Response)> {
    dialog(ctx, id, 430.0, |ui| {
        ui.heading(tr!("The picture itself may show personal details"));
        ui.add_space(4.0);
        ui.add(
            Label::new(tr!(
                "Names or other personal details can be part of this picture, as in a scanned page or an ultrasound image. SumMRIse cannot remove them."
            ))
            .wrap(),
        );
        ui.add_space(6.0);
        buttons(ui, confirm)
    })
}

/// Saves the picture of the active view where the user chooses, and nowhere else.
fn save_image(
    ctx: &Context,
    selected: &Selected,
    options: SaveOptions,
    settings: &Settings,
    chooser: &dyn Chooser,
    toasts: &mut Toasts,
) {
    let suggested = selected.suggested_name(options.format);
    let extensions: &[&str] = match options.format {
        ImageFormat::Png => &["png"],
        ImageFormat::Jpeg => &["jpg", "jpeg"],
    };
    let title = tr!("Save image");
    let Some(path) = chooser.save_file(title, options.format.label(), extensions, &suggested) else { return };
    let path = export::with_extension(path, options.format);
    let picture = match options.content {
        Content::ImageOnly => selected.plain_picture(ctx),
        Content::WithAnnotations => {
            let size = ctx.data(|d| d.get_temp::<Vec2>(Id::unique(VIEW_SIZE))).unwrap_or(DEFAULT_VIEW_SIZE);
            annotated::render(ctx, selected, settings, options.include_details && !settings.hide_patient_details, size)
        }
    };
    let Some(picture) = picture else {
        toasts.error(tr!("Could not save the image: the picture is not available."));
        return;
    };
    let name = file_name(&path);
    match export::encode(&picture, options.format).and_then(|bytes| export::write_file(&path, &name, &bytes)) {
        Ok(()) => toasts.info(tr!("Saved {name}", name = name)),
        Err(e) => toasts.error(tr!("Could not save the image: {reason}", reason = e)),
    }
}

/// The last part of `path`, to say what was written: never its folder, which
/// a screenshot of a notification could show.
pub(crate) fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// Places the image-only picture of the active view on the clipboard: never
/// anything about the patient beyond what the pixels themselves show.
fn copy_image(ctx: &Context, selected: &Selected, toasts: &mut Toasts) {
    let Some(picture) = selected.plain_picture(ctx) else {
        toasts.error(tr!("Could not copy the image: the picture is not available."));
        return;
    };
    ctx.copy_image(ColorImage::from_rgb([picture.width, picture.height], &picture.rgb));
    // The clipboard is reached after this frame, and it does not report back. Where it can
    // refuse images (Wayland without the data-control protocol), the message says what to do.
    toasts.info(if cfg!(target_os = "linux") {
        tr!("Image copied. If it does not paste, the clipboard of this system does not take images: use Save image… instead.")
    } else {
        tr!("Image copied to the clipboard.")
    });
}

fn series_names(selected: &Selected, format: ImageFormat) -> Vec<String> {
    export::series_file_names(selected.meta.modality.trim(), selected.meta.number, selected.series.len(), format)
}

/// Exports every image of the series, in display order, across image sets.
fn start_series_export(selected: Box<Selected>, folder: PathBuf, format: ImageFormat, waker: &Waker) -> Stage {
    let names = series_names(&selected, format);
    let target = folder.clone();
    let job = Job::spawn("export-series", Some(waker.clone()), move |progress| {
        export::export_series(&target, &names, format, progress, |slot| selected.acquired(slot).map(|i| i.picture))
    });
    Stage::Exporting(job, folder)
}

/// The progress of a series export, with Cancel; when it is done, what it did.
fn exporting_window(ctx: &Context, mut job: Job<SeriesExported>, folder: PathBuf, toasts: &mut Toasts) -> Stage {
    if let Some(done) = job.poll() {
        report_series_export(&done, &folder, toasts);
        return Stage::Idle;
    }
    if job.is_finished() {
        toasts.error(tr!("The export stopped unexpectedly."));
        return Stage::Idle;
    }
    ctx.request_repaint_after(Duration::from_millis(60));
    let snapshot = job.progress();
    let response = dialog(ctx, "export progress window", 380.0, |ui| {
        ui.heading(if snapshot.stage.is_empty() { tr!("Exporting images") } else { &snapshot.stage });
        ui.add_space(4.0);
        ui.add(
            egui::ProgressBar::new(snapshot.fraction().unwrap_or(0.0))
                .text(format!("{} / {}", snapshot.done, snapshot.total)),
        );
        ui.add_space(6.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.button(tr!("Cancel"))).inner
    });
    if response.inner.clicked() || closes(&response) {
        job.cancel();
    }
    Stage::Exporting(job, folder)
}

fn report_series_export(done: &SeriesExported, folder: &Path, toasts: &mut Toasts) {
    log::debug!("exported {} of {} images", done.written, done.total);
    let (written, total) = (done.written, done.total);
    if let Some(error) = &done.error {
        toasts.error(tr_n!(
            "{done} of {n} image was exported: {reason}",
            "{done} of {n} images were exported: {reason}",
            total,
            done = written,
            reason = error
        ));
    } else if done.cancelled {
        toasts.info(tr_n!(
            "{done} of {n} image was exported.",
            "{done} of {n} images were exported.",
            total,
            done = written
        ));
    } else if done.missing > 0 {
        toasts.warn(tr_n!(
            "{done} of {n} image was exported. The rest could not be decoded.",
            "{done} of {n} images were exported. The rest could not be decoded.",
            total,
            done = written
        ));
    } else {
        toasts.info(tr_n!(
            "{n} image exported to {folder}.",
            "{n} images exported to {folder}.",
            written,
            folder = file_name(folder)
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use summrise_core::Job;

    use super::*;

    /// Spec "Cancelling": Cancel stops the job, the files written remain and a
    /// message states how many. The job here waits for Cancel, so that it cannot
    /// finish before the button is pressed.
    #[test]
    fn cancelling_an_export_says_how_much_was_exported() {
        let job = Job::spawn("test", None, |progress| {
            progress.begin("Exporting images", 60);
            let start = Instant::now();
            while !progress.is_cancelled() {
                assert!(start.elapsed() < Duration::from_secs(60), "never cancelled");
                std::thread::sleep(Duration::from_millis(1));
            }
            SeriesExported { written: 20, total: 60, cancelled: true, ..SeriesExported::default() }
        });
        struct State {
            stage: Stage,
            toasts: Toasts,
        }
        let state = State { stage: Stage::Exporting(job, PathBuf::from("/chosen")), toasts: Toasts::default() };
        let mut harness = Harness::new_ui_state(
            |ui, state: &mut State| {
                if let Stage::Exporting(job, folder) = std::mem::take(&mut state.stage) {
                    state.stage = exporting_window(ui.ctx(), job, folder, &mut state.toasts);
                }
            },
            state,
        );
        harness.run_steps(2);
        assert!(harness.query_all_by_label("Cancel").next().is_some(), "the progress window has Cancel");
        assert!(harness.state().toasts.texts().is_empty(), "nothing to report yet");
        harness.get_by_label("Cancel").click();
        let start = Instant::now();
        while !matches!(harness.state().stage, Stage::Idle) {
            assert!(start.elapsed() < Duration::from_secs(60), "the job did not stop");
            harness.step();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(harness.state().toasts.texts(), ["20 of 60 images were exported."]);
    }

    /// Whatever is reported of an export without trouble names no more than the folder.
    #[test]
    fn a_finished_export_says_where_by_the_name_of_the_folder() {
        let done = SeriesExported { written: 60, total: 60, ..SeriesExported::default() };
        let mut toasts = Toasts::default();
        report_series_export(&done, Path::new("/home/someone/Scans"), &mut toasts);
        assert_eq!(toasts.texts(), ["60 images exported to Scans."]);
        let missing = SeriesExported { written: 59, total: 60, missing: 1, ..SeriesExported::default() };
        let mut toasts = Toasts::default();
        report_series_export(&missing, Path::new("/x"), &mut toasts);
        assert_eq!(toasts.texts(), ["59 of 60 images were exported. The rest could not be decoded."]);
    }
}
