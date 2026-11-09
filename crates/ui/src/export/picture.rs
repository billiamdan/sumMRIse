//! "Image only" (design D1): the picture of a view from the data, at its native
//! resolution with square pixels, through the same render path as the screen:
//! the current contrast, inversion, contrast table and colour map, with a PET
//! overlay if one is shown. Scanner graphics are left out, because they can hold
//! text and "image only" promises no added text. Zoom and pan are ignored, but
//! the picture is turned and mirrored like its view, so that it never contradicts
//! the orientation letters of the view it came from.
//!
//! The same pictures, with what is said about them, make up the printed sheets.

use egui::ColorImage;
use summrise_core::dicom::format;
use summrise_core::export::Picture;
use summrise_core::imaging::{EdgeLabels, MprAxis};
use summrise_core::print::{PageHeader, PagePicture};
use summrise_core::{DisplayOrientation, ViewKind, ViewState};
use summrise_i18n::tr;

use super::Selected;
use crate::fidelity;
use crate::orient::{view_name, view_short_name};
use crate::render::{Displayed, render_frame, render_mpr};

/// A picture of the view, with what is known of its geometry.
pub(crate) struct ImageOnly {
    pub picture: Picture,
    pub mm_per_pixel: Option<f64>,
    pub letters: Option<EdgeLabels>,
}

fn to_picture(image: &ColorImage) -> Picture {
    let [width, height] = image.size;
    let rgb = image.pixels.iter().flat_map(|pixel| [pixel.r(), pixel.g(), pixel.b()]).collect();
    Picture { width, height, rgb }
}

/// Gives a rendered image its square pixels, turn and mirror, and its letters.
fn finished(image: &ColorImage, displayed: &Displayed, orientation: DisplayOrientation) -> ImageOnly {
    let spacing = displayed.mm_per_pixel_y().zip(displayed.mm_per_pixel_x()).map(|(row, column)| [row, column]);
    let (picture, mm_per_pixel) = to_picture(image).with_square_pixels(spacing);
    let letters = displayed.screen_axes().map(|(right, down)| {
        let (right, down) = orientation.screen_axes(right, down);
        EdgeLabels::new(right, down)
    });
    ImageOnly { picture: picture.oriented(orientation), mm_per_pixel, letters }
}

impl Selected {
    fn overlay(&self) -> Option<crate::pet::Overlay<'_>> {
        self.overlay.as_ref().map(|(key, data)| crate::pet::Overlay { key: *key, data })
    }

    /// The state to render with: no scanner graphics.
    fn plain_state(&self) -> ViewState {
        let mut state = self.state.clone();
        state.show_graphics = false;
        state
    }

    /// Acquired image `slot` of the series, by its place among all images
    /// (image sets one after the other), with the window of its image set.
    pub(crate) fn acquired(&self, slot: usize) -> Option<ImageOnly> {
        let mut state = self.plain_state();
        let (stack, slice) = (slot / self.series.stack_len().max(1), slot % self.series.stack_len().max(1));
        state.set_stack(&self.series, stack);
        state.slice = slice;
        let (image, displayed) = render_frame(&self.series, &state, self.overlay())?;
        let orientation = self.state.transform(ViewKind::Stack).orientation;
        Some(finished(&image?, &displayed, orientation))
    }

    fn reformat(&self, axis: MprAxis) -> Option<ImageOnly> {
        let (image, displayed) = render_mpr(&self.series, &self.state, axis, self.overlay())?;
        let orientation = self.state.transform(ViewKind::Mpr(axis)).orientation;
        Some(finished(&image?, &displayed, orientation))
    }

    /// The picture of the active view. The 3D view's own picture has no
    /// native resolution: it is what is computed for the screen.
    pub(crate) fn image_only(&self) -> Option<ImageOnly> {
        match self.view {
            ViewKind::Stack => self.acquired(self.state.slot_index(&self.series)),
            ViewKind::Mpr(axis) => self.reformat(axis),
            ViewKind::Volume => {
                let shown = self.picture_3d.as_ref()?;
                Some(ImageOnly { picture: shown.picture()?, mm_per_pixel: None, letters: Some(shown.letters()) })
            }
        }
    }

    /// What every page of a print job says at its top. The patient's details
    /// only when the user asked for them for this job.
    pub(crate) fn page_header(&self, include_details: bool) -> PageHeader {
        let date = self.study.date.as_deref().and_then(format::date);
        let study = match date {
            Some(date) => format!("{} · {date}", self.study.title()),
            None => self.study.title(),
        };
        let series = match self.meta.number {
            Some(number) => tr!("S{number}: {title}", number = number, title = self.meta.title()),
            None => self.meta.title(),
        };
        let mut patient = Vec::new();
        if include_details {
            let p = &self.study.patient;
            patient.push(p.name.as_deref().map(format::person_name).unwrap_or_else(|| tr!("Unknown patient").into()));
            if let Some(id) = &p.id {
                patient.push(tr!("ID {id}", id = id));
            }
            if let Some(date) = p.birth_date.as_deref().and_then(format::date) {
                patient.push(tr!("Born {date}", date = date));
            }
        }
        PageHeader { study, series, patient }
    }

    /// Acquired image `slot` as it goes on a sheet. An image that could not be
    /// decoded says so at its place.
    pub(crate) fn acquired_on_page(&self, slot: usize) -> PagePicture {
        let series = &self.series;
        let (stack, slice) = (slot / series.stack_len().max(1), slot % series.stack_len().max(1));
        let mut state = self.state.clone();
        state.set_stack(series, stack);
        state.slice = slice;
        let image = self.acquired(slot);
        let number = tr!("Image {number} / {count}", number = slice + 1, count = series.stack_len());
        let mut notes = Vec::new();
        if image.is_none() {
            notes.push(tr!("Image {number} could not be decoded", number = slice + 1));
        }
        notes.extend(series.stack_label(stack));
        if let Some(lossy) = fidelity::lossy_label(ViewKind::Stack, series, &self.meta, &state) {
            notes.push(lossy);
        }
        self.finish_notes(&state, ViewKind::Stack, &mut notes);
        let window = window_text(self, &state, true);
        page_picture(image, number, window, notes)
    }

    /// The image of the active view as it goes on a sheet.
    pub(crate) fn active_on_page(&self) -> PagePicture {
        match self.view {
            ViewKind::Stack => self.acquired_on_page(self.state.slot_index(&self.series)),
            ViewKind::Mpr(axis) => {
                let (kind, state) = (ViewKind::Mpr(axis), &self.state);
                let number = tr!(
                    "{view} {number} / {count}",
                    view = view_short_name(kind, state),
                    number = state.slice_index(kind, &self.series) + 1,
                    count = state.slice_count(kind, &self.series)
                );
                // Translators: a reformat is an image computed through the volume in another plane; {view} names it.
                let mut notes = vec![tr!("Reformat: {view}", view = view_name(kind, state))];
                if let Some(slab) = state.slab {
                    let thickness = format!("{:.0}", slab.thickness_mm);
                    notes.push(tr!("{mode} slab {thickness} mm", mode = slab.mode.label(), thickness = thickness));
                }
                if let Some(lossy) = fidelity::lossy_label(kind, &self.series, &self.meta, state) {
                    notes.push(lossy);
                }
                notes.extend(self.series.stack_label(state.stack()));
                self.finish_notes(state, kind, &mut notes);
                page_picture(self.reformat(axis), number, window_text(self, state, false), notes)
            }
            ViewKind::Volume => {
                let image = self.image_only();
                let state = &self.state;
                let number = tr!("3D view: {look}", look = state.look.label());
                let notes = vec![tr!("Illustration: colours and surfaces are computed").to_owned()];
                let window = (state.look.follows_window() && !self.series.is_color())
                    .then(|| crate::pet::window_annotation(self.series.units(), &state.window));
                page_picture(image, number, window, notes)
            }
        }
    }

    /// The notes every picture has in common: mirrored, inverted, colours and
    /// a PET drawn over it.
    fn finish_notes(&self, state: &ViewState, kind: ViewKind, notes: &mut Vec<String>) {
        if state.transform(kind).orientation.is_mirrored() {
            notes.insert(0, tr!("Mirrored").to_owned());
        }
        if state.invert {
            notes.push(tr!("Inverted").to_owned());
        }
        notes.extend(crate::pet::annotation_lines(&self.series, state, self.fusion.as_ref()));
    }
}

/// The contrast of a picture in words; none for a colour image. A contrast
/// table applies to the acquired images only.
fn window_text(selected: &Selected, state: &ViewState, acquired: bool) -> Option<String> {
    if selected.series.is_color() {
        return None;
    }
    if acquired && let Some(table) = state.active_lut(&selected.series) {
        return Some(tr!("Contrast table: {name}", name = table.name));
    }
    Some(crate::pet::window_annotation(selected.series.units(), &state.window))
}

fn page_picture(image: Option<ImageOnly>, number: String, window: Option<String>, notes: Vec<String>) -> PagePicture {
    let (picture, mm_per_pixel, letters) = match image {
        Some(image) => (image.picture, image.mm_per_pixel, image.letters),
        None => (Picture::black(64, 64), None, None),
    };
    PagePicture { picture, mm_per_pixel, letters, number, window, notes }
}
