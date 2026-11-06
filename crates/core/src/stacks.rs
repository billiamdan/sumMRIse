//! The stack shown of a series that holds several sets of images at the same
//! slice positions (`summrise_dicom::Stacks`), and the contrast window of each
//! stack.
//!
//! [`ViewState::slice`] counts inside the current stack; [`ViewState::slot_index`]
//! is the index of the displayed image among all images of the series, which
//! are stack-major. All stacks share one geometry, so changing the stack keeps
//! the image number, the crosshair and the reformat planes. Stacks that differ
//! in contrast (echoes, b-values …) keep a window each, starting from their own
//! default; time points and heart phases share one window, so that a loop
//! through them does not flicker (`openspec/changes/multi-phase-cine/design.md`,
//! D6 and D7).

use summrise_imaging::Window;

use crate::series::SeriesData;
use crate::view::ViewState;

/// A contrast window, the name of the preset it came from (`None`: set by
/// hand) and the contrast table shown instead of it, if any.
pub(crate) type StackWindow = (Window, Option<String>, Option<usize>);

/// The window stack `stack` opens with, once one of its images is decoded:
/// its first stored window, else SUV 0–5 for uptake values, else an
/// automatic one; and its first contrast table when it stores no window.
fn own_default(series: &SeriesData, stack: usize) -> Option<StackWindow> {
    let initial = series.stack_initial_window(stack)?;
    let (window, preset) = crate::pet::opening_window(series, series.stack_presets(stack), initial);
    let lut = crate::contrast::default_lut(series.stack_presets(stack), series.stack_luts(stack));
    Some((window, Some(preset), lut))
}

/// The window shown for a stack whose own default is not known yet: the
/// series' window.
fn provisional(series: &SeriesData) -> StackWindow {
    let (window, preset) = crate::pet::opening_window(series, series.presets(), series.initial_window());
    (window, Some(preset), crate::contrast::default_lut(series.presets(), series.luts()))
}

impl ViewState {
    /// The stack shown (0 for a series that is not split).
    pub fn stack(&self) -> usize {
        self.stack
    }

    /// Index of the displayed acquired image among all images of the series
    /// (`SeriesData::frames`, `image`, `frame`, the loader's displayed image):
    /// `stack × stack_len + slice`. Not the same as [`Self::slice`], which
    /// counts inside the current stack.
    pub fn slot_index(&self, series: &SeriesData) -> usize {
        let len = series.stack_len();
        self.stack.min(series.stack_count().saturating_sub(1)) * len + self.slice.min(len.saturating_sub(1))
    }

    /// Shows stack `stack` (clamped to the stacks of the series) at the same
    /// image number, with the same crosshair and planes. Where the stacks keep
    /// a window each, the stack left keeps its window and the stack chosen
    /// gets its own: the one it had, else its default.
    pub fn set_stack(&mut self, series: &SeriesData, stack: usize) {
        let stack = stack.min(series.stack_count().saturating_sub(1));
        if stack == self.stack {
            return;
        }
        if series.separate_windows() {
            let current: StackWindow = (self.window, self.preset.take(), self.lut.take());
            // A window that is still waiting for the stack's own default is not kept.
            let untouched = self.pending_window.take().is_some_and(|pending| pending == current);
            if !untouched {
                self.stack_windows.insert(self.stack, current);
            }
            let (window, preset, lut) = match self.stack_windows.remove(&stack).or_else(|| own_default(series, stack)) {
                Some(window) => window,
                None => {
                    let window = provisional(series);
                    self.pending_window = Some(window.clone());
                    window
                }
            };
            (self.window, self.preset, self.lut) = (window, preset, lut);
        }
        self.stack = stack;
    }

    /// Moves `delta` stacks on (back when negative), stopping at the first
    /// and the last.
    pub fn step_stack(&mut self, series: &SeriesData, delta: i64) {
        let last = series.stack_count().saturating_sub(1) as i64;
        let next = (self.stack as i64).saturating_add(delta).clamp(0, last);
        self.set_stack(series, next as usize);
    }

    /// Gives the stack shown its own default window as soon as that is known
    /// (when the first image of the stack is decoded), unless the user has
    /// changed the window meanwhile. Until then the stack shows only loading
    /// placeholders. Call once per frame.
    pub fn settle_stack_window(&mut self, series: &SeriesData) {
        let Some(pending) = &self.pending_window else { return };
        if pending.0 != self.window || pending.1 != self.preset || pending.2 != self.lut {
            self.pending_window = None;
            return;
        }
        if let Some((window, preset, lut)) = own_default(series, self.stack) {
            (self.window, self.preset, self.lut) = (window, preset, lut);
            self.pending_window = None;
        }
    }

    /// For [`ViewState::new`]: stacks that keep a window each open with the
    /// window of the stack shown, provisionally the series' window.
    pub(crate) fn open_stack_window(&mut self, series: &SeriesData) {
        if !series.separate_windows() {
            return;
        }
        match own_default(series, self.stack) {
            Some((window, preset, lut)) => (self.window, self.preset, self.lut) = (window, preset, lut),
            None => self.pending_window = Some((self.window, self.preset.clone(), self.lut)),
        }
    }

    /// For [`ViewState::is_pristine`]: whether every stack not shown still has
    /// its default window.
    pub(crate) fn stack_windows_pristine(&self, series: &SeriesData) -> bool {
        self.stack_windows.iter().all(|(&stack, window)| own_default(series, stack).as_ref() == Some(window))
    }

    /// For [`ViewState::reset`]: every stack back to its default window, the
    /// stack shown included; the stack shown stays.
    pub(crate) fn reset_stack_windows(&mut self, series: &SeriesData) {
        self.stack_windows.clear();
        self.pending_window = None;
        if series.separate_windows() {
            self.open_stack_window(series);
            if self.pending_window.is_some() {
                let window = provisional(series);
                (self.window, self.preset, self.lut) = window.clone();
                self.pending_window = Some(window);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use summrise_dicom::{Media, Progress, StudyIndex, decode_instance, plan_series, read_instance, scan};
    use summrise_imaging::{DVec3, MprAxis, reslice_plane};
    use summrise_testkit::stacks::{self as phantoms, write_stacks_study};
    use summrise_testkit::{MARKER_POSITION, PhantomOptions};

    use super::*;
    use crate::series::testing::{load, phantom};
    use crate::view::ViewKind;

    fn stacks_study() -> (tempfile::TempDir, StudyIndex) {
        let dir = tempfile::tempdir().expect("tempdir");
        write_stacks_study(dir.path()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        (dir, index)
    }

    fn marker() -> DVec3 {
        DVec3::from_array(MARKER_POSITION)
    }

    /// Height (S, mm) of the acquired image on screen.
    fn height(state: &ViewState, series: &SeriesData) -> f64 {
        series.frames()[state.slot_index(series)].plane.expect("plane").position.z
    }

    fn window(state: &ViewState) -> (f64, f64) {
        (state.window.width, state.window.center)
    }

    #[test]
    fn a_split_series_has_one_volume_per_stack() {
        let (_dir, index) = stacks_study();
        let series = load(&index.studies[0].series[phantoms::DYNAMIC]);
        assert_eq!((series.stack_count(), series.stack_len(), series.len()), (3, 12, 36));
        assert_eq!(series.stack_label(2).as_deref(), Some("Time point 3 / 3"));
        assert!(!series.separate_windows(), "time points share one window");
        assert_eq!(series.initial_index(), 6, "the middle image of the first stack");
        for stack in 0..3 {
            let volume = series.stack_volume(stack).expect("a volume per stack");
            assert_eq!(volume.geometry.dims, [48, 48, 12]);
            assert_eq!(Some(&volume.geometry), series.geometry(), "one geometry");
            let value = phantoms::DYNAMIC_MARKER[stack] as f32;
            assert_eq!(volume.value_at_patient(marker()), Some(value), "time point {}", stack + 1);
        }
        assert!(series.stack_volume(3).is_none());
        let first = series.volume().expect("volume");
        assert!(std::ptr::eq(first, series.stack_volume(0).expect("stack 0")), "volume() is stack 0's");
        // The series that is not split has one stack of all its images.
        let (_d, standard) = phantom(&PhantomOptions::small());
        let ct = load(&standard.studies[0].series[0]);
        assert_eq!((ct.stack_count(), ct.stack_len(), ct.stack_label(0)), (1, 24, None));
        assert!(std::ptr::eq(ct.volume().expect("volume"), ct.stack_volume(0).expect("stack 0")));
    }

    /// Task 3.2 — scrolling stays inside the stack; changing the stack keeps
    /// the image number, the position and the crosshair.
    #[test]
    fn scrolling_stays_in_the_stack_and_a_new_stack_keeps_the_image() {
        let (_dir, index) = stacks_study();
        let series = load(&index.studies[0].series[phantoms::DYNAMIC]);
        let mut state = ViewState::new(&series);
        assert_eq!((state.stack(), state.slice, state.slot_index(&series)), (0, 6, 6));
        assert_eq!(height(&state, &series), 50.0, "image 7 / 12 at S 50 mm");
        assert_eq!(state.slice_count(ViewKind::Stack, &series), 12);
        let cursor = state.cursor;

        state.step_stack(&series, 1);
        assert_eq!((state.stack(), state.slice, state.slot_index(&series)), (1, 6, 18));
        assert_eq!(height(&state, &series), 50.0);
        assert_eq!(state.cursor, cursor, "the crosshair is shared");
        assert!(!state.is_pristine(&series), "choosing another stack is the user's doing");

        // Scrolling past the end stays in the stack.
        state.scroll(ViewKind::Stack, &series, 100);
        assert_eq!((state.slice, state.slot_index(&series)), (11, 23));
        assert_eq!(height(&state, &series), 75.0);
        state.set_slice(ViewKind::Stack, &series, 0);
        assert_eq!((state.slice, state.slot_index(&series), height(&state, &series)), (0, 12, 20.0));
        // Steps stop at the first and the last stack.
        state.step_stack(&series, 5);
        assert_eq!(state.stack(), 2);
        state.step_stack(&series, -7);
        assert_eq!(state.stack(), 0);
        state.set_slice(ViewKind::Stack, &series, 6);
        assert!(state.is_pristine(&series), "back where the series opened");
    }

    /// Task 3.4 — the reformats show the current stack: the marker at L 40,
    /// A 35, S 50 mm reads 900 in time point 2 and 1200 in time point 3.
    #[test]
    fn the_reformats_show_the_stack_chosen() {
        let (_dir, index) = stacks_study();
        let series = load(&index.studies[0].series[phantoms::DYNAMIC]);
        let mut state = ViewState::new(&series);
        state.set_cursor(marker(), &series);
        assert_eq!(state.cursor, marker(), "the marker lies on a voxel centre");
        let at_crosshair = |state: &ViewState, axis: MprAxis| {
            let volume = series.stack_volume(state.stack()).expect("volume");
            let img = reslice_plane(volume, &state.plane_basis(axis), state.mpr_position(axis), state.slab, 768);
            let (x, y) = img.patient_to_pixel(state.cursor);
            img.value_at(x.round() as usize, y.round() as usize)
        };
        for axis in MprAxis::ALL {
            assert_eq!(at_crosshair(&state, axis), Some(600.0), "time point 1, {axis:?}");
        }
        state.set_stack(&series, 1);
        assert_eq!(state.cursor, marker());
        for axis in MprAxis::ALL {
            assert_eq!(at_crosshair(&state, axis), Some(900.0), "time point 2, {axis:?}");
        }
        state.step_stack(&series, 1);
        assert_eq!(at_crosshair(&state, MprAxis::Axial), Some(1200.0), "time point 3");
        // Slice counts and scrolling of the reformats use the stack's volume.
        assert_eq!(state.slice_count(ViewKind::Mpr(MprAxis::Axial), &series), 12);
        state.scroll(ViewKind::Mpr(MprAxis::Axial), &series, 1);
        assert_eq!((state.stack(), state.slice), (2, 7), "the acquired image follows the reformat");
    }

    /// Task 3.3 — echoes keep a window each, starting from their own stored
    /// window; reset restores every stack's default and keeps the stack.
    #[test]
    fn echoes_keep_a_window_each() {
        let (_dir, index) = stacks_study();
        let series = load(&index.studies[0].series[phantoms::DUAL_ECHO]);
        assert!(series.separate_windows());
        let mut state = ViewState::new(&series);
        assert_eq!(window(&state), (4000.0, 2000.0), "echo 1 (TE 15 ms) opens with its stored window");
        assert_eq!(state.preset.as_deref(), Some("Window 1"));
        assert!(state.is_pristine(&series));

        state.step_stack(&series, 1);
        assert_eq!(window(&state), (700.0, 350.0), "echo 2 (TE 90 ms) with its own");
        state.window = Window::new(200.0, 500.0);
        state.preset = None;
        state.step_stack(&series, -1);
        assert_eq!(window(&state), (4000.0, 2000.0));
        assert!(!state.is_pristine(&series), "echo 2 keeps a window the user set");
        state.step_stack(&series, 1);
        assert_eq!(window(&state), (500.0, 200.0), "and finds it again");
        assert_eq!(state.preset, None);

        // Presets, the automatic window and the full range follow the stack.
        let presets = ViewState::presets_for(&series, 1);
        assert_eq!((presets[0].window.width, presets[0].window.center), (700.0, 350.0));
        let full = presets.iter().find(|p| p.name == "Full range").expect("complete");
        assert!(full.window.high() <= 600.0, "echo 2 reaches 600 at most: {:?}", full.window);
        let echo1 = ViewState::presets_for(&series, 0);
        assert!(echo1.iter().any(|p| p.name == "Full range" && p.window.high() >= 3500.0));
        let auto = presets.iter().find(|p| p.name == "Auto").expect("auto");
        assert!(auto.window.high() < 1000.0, "from echo 2's images only: {:?}", auto.window);
        // Dragging is scaled to echo 2's range (0 to 600): a fine step.
        let before = state.window.width;
        state.drag_window(10.0, 0.0, &series);
        assert!((state.window.width - before).abs() < 10.0, "{} → {}", before, state.window.width);

        // Reset: echo 2 back to its default and still shown; echo 1 to its own.
        state.step_stack(&series, -1);
        state.drag_window(50.0, 20.0, &series);
        state.step_stack(&series, 1);
        state.reset(&series);
        assert_eq!((state.stack(), window(&state)), (1, (700.0, 350.0)));
        state.step_stack(&series, -1);
        assert_eq!(window(&state), (4000.0, 2000.0));
        assert!(state.is_pristine(&series));
    }

    /// Task 3.3 — time points share one window: no flicker between them.
    #[test]
    fn time_points_share_one_window() {
        let (_dir, index) = stacks_study();
        let series = load(&index.studies[0].series[phantoms::DYNAMIC]);
        let mut state = ViewState::new(&series);
        state.window = Window::new(400.0, 800.0);
        state.set_stack(&series, 2);
        assert_eq!(window(&state), (800.0, 400.0));
        assert_eq!(ViewState::presets_for(&series, 2), ViewState::presets(&series));
    }

    /// Task 3.5 — the guide says in one statement what the stacks are, after
    /// the plane of the slices; series that are not split get none.
    #[test]
    fn the_guide_says_what_the_stacks_are() {
        let (_dir, index) = stacks_study();
        let study = &index.studies[0];
        let statement = |series: usize| {
            let guide = crate::guide::describe_series(&study.series[series]);
            let found = guide.statements.iter().position(|s| s.text.starts_with("This series holds"));
            found.map(|i| (i, guide.statements[i].text.clone(), guide.statements[i].basis.clone()))
        };
        let (place, text, basis) = statement(phantoms::DYNAMIC).expect("a statement");
        assert_eq!(text, "This series holds 3 time points of the same slices.");
        assert_eq!(basis, "Recorded in the images: temporal position.");
        assert_eq!(place, 2, "after what the series is and the plane of its slices");
        let text = |series: usize| statement(series).map(|(_, text, _)| text);
        assert_eq!(
            text(phantoms::DUAL_ECHO).as_deref(),
            Some("This series holds 2 echoes of the same slices: images taken at different echo times.")
        );
        assert_eq!(
            text(phantoms::DIFFUSION).as_deref(),
            Some("This series holds 2 diffusion weightings (b-values) of the same slices.")
        );
        let (_, text_of_parts, basis_of_parts) = statement(phantoms::REPEATED).expect("a statement");
        assert_eq!(text_of_parts, "This series holds 3 sets of images of the same slices.");
        assert!(basis_of_parts.contains("nothing recorded in them tells the sets apart"));
        assert_eq!(text(phantoms::INCOMPLETE), None);
        assert_eq!(text(phantoms::CINE), None);
    }

    /// A stack chosen before any of its images is decoded gets its own window
    /// when its first image arrives, unless the user has set one.
    #[test]
    fn a_stack_gets_its_own_window_when_its_first_image_arrives() {
        let (_dir, index) = stacks_study();
        let meta = &index.studies[0].series[phantoms::DUAL_ECHO];
        let fill = |series: &SeriesData, echo: usize, slice: u32| {
            let file =
                meta.instances.iter().position(|h| h.instance_number == Some((slice * 2) as i32 + echo as i32 + 1));
            let header = &meta.instances[file.expect("file")];
            let result = read_instance(&Media::folder(), header).and_then(decode_instance).map_err(|e| e.to_string());
            series.fill(file.expect("file"), result, 6);
        };
        let series = SeriesData::new(plan_series(meta));
        fill(&series, 0, 6);
        let mut state = ViewState::new(&series);
        assert_eq!(window(&state), (4000.0, 2000.0));
        // Echo 2 has no decoded image yet: the series' window for now.
        state.set_stack(&series, 1);
        assert_eq!(window(&state), (4000.0, 2000.0));
        state.settle_stack_window(&series);
        assert_eq!(window(&state), (4000.0, 2000.0), "still nothing of echo 2");
        fill(&series, 1, 6);
        state.settle_stack_window(&series);
        assert_eq!(window(&state), (700.0, 350.0), "echo 2's own window, once its first image is decoded");
        assert_eq!(state.preset.as_deref(), Some("Window 1"));

        // A window the user set meanwhile stays.
        let series = SeriesData::new(plan_series(meta));
        fill(&series, 0, 6);
        let mut state = ViewState::new(&series);
        state.set_stack(&series, 1);
        state.window = Window::new(100.0, 300.0);
        fill(&series, 1, 6);
        state.settle_stack_window(&series);
        assert_eq!(window(&state), (300.0, 100.0));
        // Leaving a stack still on its provisional window does not keep that window.
        let series = SeriesData::new(plan_series(meta));
        fill(&series, 0, 6);
        let mut state = ViewState::new(&series);
        state.set_stack(&series, 1);
        state.set_stack(&series, 0);
        fill(&series, 1, 6);
        state.set_stack(&series, 1);
        assert_eq!(window(&state), (700.0, 350.0));
    }
}
