//! What the user can choose for the contrast of the acquired images: a
//! window, or a contrast table stored in the images (VOI LUT Sequence).
//!
//! A table is contrast state next to the window, not a kind of window
//! (`openspec/changes/display-fidelity/design.md`, D4): `ViewState::lut`
//! says which table the acquired images are shown through, `None` meaning the
//! window. Reformats and slabs always use the window — a table is defined on
//! the stored values of one image, not on resampled values. Choosing a
//! window, dragging or editing W or L switches to the window.

use summrise_imaging::{PresetSource, VoiLut, WindowPreset};

use crate::series::SeriesData;
use crate::view::ViewState;

/// One entry of the contrast menu.
#[derive(Debug, Clone, PartialEq)]
pub enum ContrastChoice {
    /// A window: stored, built-in, automatic or the full range.
    Window(WindowPreset),
    /// Contrast table `index` of the stack shown (`SeriesData::stack_luts`).
    Table { index: usize, name: String },
}

impl ContrastChoice {
    /// The name that identifies the choice (English for a built-in window).
    pub fn name(&self) -> &str {
        match self {
            Self::Window(preset) => &preset.name,
            Self::Table { name, .. } => name,
        }
    }

    /// The name in the menu: a built-in window's in the language of the
    /// interface, a stored window's or table's as the images name it.
    pub fn label(&self) -> &str {
        match self {
            Self::Window(preset) => preset.label(),
            Self::Table { name, .. } => name,
        }
    }
}

/// The contrast an image opens with when it stores `presets` (windows) and
/// `luts` (tables): the first table when it stores no window.
pub(crate) fn default_lut(presets: &[WindowPreset], luts: &[VoiLut]) -> Option<usize> {
    (presets.is_empty() && !luts.is_empty()).then_some(0)
}

impl ViewState {
    /// The contrast menu of stack `stack`: the windows stored in the images,
    /// then the contrast tables stored in them, then the built-in CT windows,
    /// Auto and Full range. Keys 1–9 count through this list, so key 1 is
    /// always the default.
    pub fn contrast_choices(series: &SeriesData, stack: usize) -> Vec<ContrastChoice> {
        let (stored, others): (Vec<WindowPreset>, Vec<WindowPreset>) =
            Self::presets_for(series, stack).into_iter().partition(|p| p.source == PresetSource::Dicom);
        let tables = series
            .stack_luts(stack)
            .iter()
            .enumerate()
            .map(|(index, table)| ContrastChoice::Table { index, name: table.name.clone() });
        stored
            .into_iter()
            .map(ContrastChoice::Window)
            .chain(tables)
            .chain(others.into_iter().map(ContrastChoice::Window))
            .collect()
    }

    /// Applies an entry of the contrast menu.
    pub fn choose_contrast(&mut self, choice: &ContrastChoice) {
        match choice {
            ContrastChoice::Window(preset) => self.apply_preset(preset),
            ContrastChoice::Table { index, .. } => self.apply_lut(*index),
        }
    }

    /// Shows the acquired images through contrast table `index` of the stack
    /// shown. The window is kept for reformats and slabs.
    pub fn apply_lut(&mut self, index: usize) {
        self.lut = Some(index);
    }

    /// The contrast table the acquired images are shown through, if any.
    pub fn active_lut<'a>(&self, series: &'a SeriesData) -> Option<&'a VoiLut> {
        series.stack_luts(self.stack).get(self.lut?)
    }

    /// Whether `choice` is what the acquired images are shown with.
    pub fn is_chosen(&self, choice: &ContrastChoice) -> bool {
        match choice {
            ContrastChoice::Window(preset) => self.lut.is_none() && self.preset.as_deref() == Some(&*preset.name),
            ContrastChoice::Table { index, .. } => self.lut == Some(*index),
        }
    }

    /// The name of the contrast of the acquired images: the table's, else
    /// the window preset's; `None` for a window set by hand.
    pub fn contrast_name<'a>(&'a self, series: &'a SeriesData) -> Option<&'a str> {
        match self.active_lut(series) {
            Some(table) => Some(&table.name),
            None => self.preset.as_deref(),
        }
    }

    /// [`Self::contrast_name`] as shown to the user (see [`Self::preset_label`]).
    pub fn contrast_label<'a>(&'a self, series: &'a SeriesData) -> Option<&'a str> {
        match self.active_lut(series) {
            Some(table) => Some(&table.name),
            None => self.preset_label(series),
        }
    }

    /// The name of the window preset in use as shown to the user: the name
    /// of a window stored in the images as it is (text from the disc, even
    /// where it reads like a built-in name), a built-in window's in the
    /// language of the interface; `None` for a window set by hand. The
    /// preset itself is identified by its English name, so that changing
    /// the language changes nothing else.
    pub fn preset_label<'a>(&'a self, series: &SeriesData) -> Option<&'a str> {
        let name = self.preset.as_deref()?;
        let stored = series.stack_presets(self.stack).iter().any(|preset| preset.name == name);
        Some(if stored { name } else { summrise_imaging::window::builtin_label(name) })
    }
}

#[cfg(test)]
mod tests {
    use summrise_dicom::{Media, Progress, StudyIndex, decode_instance, plan_series, read_instance, scan};
    use summrise_imaging::{LookupTable, Window};
    use summrise_testkit::PhantomOptions;
    use summrise_testkit::fidelity::{self as fx, write_fidelity_study};

    use super::*;
    use crate::series::testing::{load, phantom};
    use crate::view::ViewKind;

    fn study() -> (tempfile::TempDir, StudyIndex) {
        let dir = tempfile::tempdir().expect("tempdir");
        write_fidelity_study(dir.path()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        (dir, index)
    }

    fn names(choices: &[ContrastChoice]) -> Vec<&str> {
        choices.iter().map(ContrastChoice::name).collect()
    }

    /// Task 4.2 — an image with two tables and no window opens with the
    /// first table; the tables come first in the menu.
    #[test]
    fn the_first_table_is_the_default_without_a_stored_window() {
        let (_dir, index) = study();
        let series = load(&index.studies[0].series[fx::VOI_LUT]);
        assert_eq!(series.luts().len(), 2);
        let mut state = ViewState::new(&series);
        assert_eq!(state.lut, Some(0));
        assert_eq!(state.active_lut(&series).map(|t| t.name.as_str()), Some("Normal"));
        assert_eq!(state.contrast_name(&series), Some("Normal"));
        assert_eq!(state.preset.as_deref(), Some("Auto"), "the window kept for reformats is the automatic one");
        assert!(state.is_pristine(&series));
        let choices = ViewState::contrast_choices(&series, 0);
        assert_eq!(names(&choices), ["Normal", "Harder", "Auto", "Full range"]);
        assert!(state.is_chosen(&choices[0]) && !state.is_chosen(&choices[1]) && !state.is_chosen(&choices[2]));

        // The second table, then a window: the window replaces the table.
        state.choose_contrast(&choices[1]);
        assert_eq!(state.contrast_name(&series), Some("Harder"));
        assert!(!state.is_pristine(&series));
        state.choose_contrast(&choices[2]);
        assert_eq!((state.lut, state.contrast_name(&series)), (None, Some("Auto")));
        // Reset: the default table again.
        state.reset(&series);
        assert_eq!(state.lut, Some(0));
        assert!(state.is_pristine(&series));
    }

    /// Adjusting by hand switches to an ordinary window, labelled custom.
    #[test]
    fn dragging_leaves_the_table_for_a_window() {
        let (_dir, index) = study();
        let series = load(&index.studies[0].series[fx::VOI_LUT]);
        let mut state = ViewState::new(&series);
        let window = state.window;
        state.drag_window(20.0, 0.0, &series);
        assert_eq!(state.lut, None);
        assert_eq!(state.preset, None);
        assert_eq!(state.contrast_name(&series), None, "\"Custom window\"");
        assert!(state.window.width > window.width, "the drag changed the window it started from");
        // The A key's preset (Auto) is a window too.
        state.apply_lut(1);
        let auto = ViewState::presets(&series).into_iter().find(|p| p.source == PresetSource::Auto).expect("auto");
        state.apply_preset(&auto);
        assert_eq!(state.lut, None);
    }

    /// A stored window stays the default; tables follow the stored windows.
    #[test]
    fn stored_windows_come_first_and_stay_the_default() {
        let (_dir, cd) = phantom(&PhantomOptions::small());
        let ct = load(&cd.studies[0].series[0]);
        let state = ViewState::new(&ct);
        assert_eq!(state.lut, None);
        let choices = ViewState::contrast_choices(&ct, 0);
        assert_eq!(&names(&choices)[..3], ["Mediastinum", "Lung", "Brain"]);
        assert_eq!(choices.len(), ViewState::presets(&ct).len(), "no tables: the windows as before");
        assert!(state.is_chosen(&choices[0]));
    }

    /// The scanner-graphics toggle is view state: on by default, part of
    /// pristine and reset.
    #[test]
    fn scanner_graphics_are_shown_until_the_user_hides_them() {
        let (_dir, index) = study();
        let series = load(&index.studies[0].series[fx::OVERLAY]);
        assert!(series.has_graphics());
        assert_eq!(series.graphics(0).len(), 1);
        let mut state = ViewState::new(&series);
        assert!(state.show_graphics && state.is_pristine(&series));
        state.show_graphics = false;
        assert!(!state.is_pristine(&series));
        state.reset(&series);
        assert!(state.show_graphics);
        let plain = load(&index.studies[0].series[fx::LOSSY]);
        assert!(!plain.has_graphics() && plain.graphics(0).is_empty());
    }

    /// Padding is left out of the value range, the automatic window and the
    /// reformats of the series store.
    #[test]
    fn padding_is_left_out_of_the_series_statistics() {
        let (_dir, index) = study();
        let series = load(&index.studies[0].series[fx::PADDED_CT]);
        assert_eq!(series.value_range(), (-1000.0, 700.0));
        assert_eq!(series.padding(0), Some(summrise_imaging::Padding::new(-3024.0, -3024.0)));
        let state = ViewState::new(&series);
        let auto = ViewState::presets(&series).into_iter().find(|p| p.source == PresetSource::Auto).expect("auto");
        assert!(auto.window.low() >= -1000.0, "{:?}", auto.window);
        let full = ViewState::presets(&series).into_iter().find(|p| p.source == PresetSource::FullRange).expect("full");
        assert_eq!(full.window, Window::from_range(-1000.0, 700.0));
        let volume = series.volume().expect("volume");
        assert_eq!(volume.padding(1), series.padding(1));
        assert_eq!(state.slice_count(ViewKind::Mpr(summrise_imaging::MprAxis::Coronal), &series), 64);
    }

    /// Where image sets keep a window each, they keep a table choice each:
    /// echo 2 stores a table and no window, echo 1 a window.
    #[test]
    fn image_sets_keep_a_table_choice_each() {
        let dir = tempfile::tempdir().expect("tempdir");
        summrise_testkit::stacks::write_stacks_study(dir.path()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        let meta = &index.studies[0].series[summrise_testkit::stacks::DUAL_ECHO];
        let series = SeriesData::new(plan_series(meta));
        let data = (0..4096).map(|i| (i / 2) as u16).collect();
        let table = VoiLut { name: "Echo 2 table".to_owned(), table: LookupTable::new(0, 12, data).expect("table") };
        for (file, header) in meta.instances.iter().enumerate() {
            let mut decoded = read_instance(&Media::folder(), header).and_then(decode_instance).expect("decode");
            if header.instance_number.is_some_and(|n| n % 2 == 0) {
                decoded.presets.clear();
                decoded.extras.voi_luts = vec![table.clone()];
            }
            series.fill(file, Ok(decoded), 6);
        }
        series.finish();
        assert!(series.separate_windows());
        let mut state = ViewState::new(&series);
        assert_eq!((state.lut, state.window.width), (None, 4000.0), "echo 1 opens with its stored window");
        state.step_stack(&series, 1);
        assert_eq!(state.contrast_name(&series), Some("Echo 2 table"), "echo 2 opens with its own table");
        assert_eq!(names(&ViewState::contrast_choices(&series, 1))[0], "Echo 2 table");
        state.step_stack(&series, -1);
        assert_eq!((state.lut, state.window.width), (None, 4000.0));
        state.step_stack(&series, 1);
        assert_eq!(state.lut, Some(0));
        // A window set by hand on echo 2 stays with echo 2.
        state.drag_window(10.0, 0.0, &series);
        state.step_stack(&series, -1);
        state.step_stack(&series, 1);
        assert_eq!(state.lut, None, "echo 2 keeps the window the user set");
        assert!(!state.is_pristine(&series));
        state.reset(&series);
        assert_eq!((state.stack(), state.lut), (1, Some(0)), "reset: echo 2's own table");
    }
}
