//! PET in the viewer — uptake values, colour maps and the PET drawn over the
//! CT — driven offscreen through the keyboard, the pointer and the toolbar
//! menu. Set `SUMMRISE_SNAPSHOT_DIR` to keep renderings (`pet-*.png`).
//!
//! The data is the PET study of the testkit: the phantom CT with its 300 HU
//! marker at L 40, A 35, S 50 mm and, in the CT's Frame of Reference, a PET
//! with a hot sphere of SUV 5 at the same place.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::{Key, Pos2, Rect, Theme};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use summrise_core::dicom::{DecodedInstance, DicomError, InstanceHeader, Media, Progress, scan};
use summrise_core::imaging::{Colormap, DVec3, Look};
use summrise_core::{
    DicomFiles, ImageSource, Layout, OverlayStatus, SeriesData, SeriesKey, Session, Settings, ViewKind, ViewState,
    Waker,
};
use summrise_testkit::pet::{self as phantom, PetStudyOptions, write_pet_study};
use summrise_testkit::{MARKER_POSITION, PhantomOptions};

use super::Viewer;
use crate::app::{Action, Toasts};
use crate::placement::Placement;
use crate::theme;

const CT: SeriesKey = SeriesKey { study: 0, series: phantom::CT };
const PET: SeriesKey = SeriesKey { study: 0, series: phantom::PET };
const NAC: SeriesKey = SeriesKey { study: 0, series: phantom::PET_NAC };
const MENU: &str = "PET colours and fusion";

/// Everything `Viewer::show` needs.
struct App {
    viewer: Viewer,
    settings: Settings,
    waker: Waker,
    actions: Vec<Action>,
    toasts: Toasts,
}

struct Fixture {
    harness: Harness<'static, App>,
    gate: Arc<PetGate>,
    _dir: tempfile::TempDir,
}

fn snapshot_dir() -> Option<PathBuf> {
    std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from)
}

fn marker() -> DVec3 {
    DVec3::from_array(MARKER_POSITION)
}

/// The PET study with a CT of 96 × 96 × 48 voxels of 2.5 × 2.5 × 4.2 mm.
fn study() -> PetStudyOptions {
    PetStudyOptions {
        ct: PhantomOptions { ct_size: 96, ct_slices: 48, ..PhantomOptions::small() },
        ..Default::default()
    }
}

/// Holds back the files of the PET series until the test opens the gate;
/// everything else is read at once.
#[derive(Default)]
struct PetGate {
    closed: AtomicBool,
}

impl ImageSource for PetGate {
    fn read(&self, media: &Media, header: &InstanceHeader) -> Result<Vec<u8>, DicomError> {
        let pet = header.path.parent().is_some_and(|folder| folder.ends_with("SE000005"));
        while pet && self.closed.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        DicomFiles.read(media, header)
    }

    fn decode(&self, header: &InstanceHeader, bytes: Vec<u8>) -> Result<DecodedInstance, DicomError> {
        DicomFiles.decode(header, bytes)
    }
}

/// The viewer on the PET study with `key` displayed and loaded completely.
fn open(options: &PetStudyOptions, key: SeriesKey, theme: Theme, size: [f32; 2], gate: PetGate) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    write_pet_study(dir.path(), options).expect("fixture");
    let index = scan(dir.path(), &Progress::new()).expect("scan");
    let settings = Settings::default();
    let gate = Arc::new(gate);
    let mut session = Session::with_source(index, settings.memory_budget_bytes(), None, gate.clone());
    session.select_series(key);
    let app = App {
        viewer: Viewer::new(session, &settings),
        settings,
        waker: Arc::new(|| {}),
        actions: Vec::new(),
        toasts: Toasts::default(),
    };
    let mut builder = Harness::builder().with_size(size).with_theme(theme).with_context_setup(theme::install);
    if snapshot_dir().is_some() {
        builder = builder.wgpu();
    }
    let harness = builder.build_ui_state(
        |ui, app: &mut App| app.viewer.show(ui, &mut app.settings, &app.waker, &mut app.actions, &mut app.toasts),
        app,
    );
    let mut fixture = Fixture { harness, gate, _dir: dir };
    fixture.run_until("the series", |f| f.viewer().session.loaded(key).is_some());
    fixture.steps(4);
    fixture
}

fn open_dark(key: SeriesKey) -> Fixture {
    open(&study(), key, Theme::Dark, [1440.0, 900.0], PetGate::default())
}

impl Fixture {
    /// Runs UI frames until `done` (loading runs on threads).
    fn run_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < Duration::from_secs(60), "timed out waiting for {what}");
            self.harness.step();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn run_until_overlay_is_drawn(&mut self) {
        self.run_until("the PET overlay", |f| {
            matches!(f.viewer().session.overlay_status(), Some(OverlayStatus::Ready(_)))
        });
        self.steps(4);
    }

    fn steps(&mut self, n: usize) {
        self.harness.run_steps(n);
    }

    fn viewer(&self) -> &Viewer {
        &self.harness.state().viewer
    }

    fn series(&self, key: SeriesKey) -> Arc<SeriesData> {
        self.viewer().session.loaded(key).expect("the series is loaded")
    }

    fn view(&self, key: SeriesKey) -> &ViewState {
        self.viewer().views.get(&key).expect("the series has a view")
    }

    fn view_mut(&mut self, key: SeriesKey) -> &mut ViewState {
        self.harness.state_mut().viewer.views.get_mut(&key).expect("the series has a view")
    }

    /// Whether something on screen is labelled exactly `label`.
    fn shows(&self, label: &str) -> bool {
        self.harness.query_all_by_label(label).next().is_some()
    }

    /// Whether something on screen has a label containing `text`.
    fn shows_part(&self, text: &str) -> bool {
        self.harness.query_all_by_label_contains(text).next().is_some()
    }

    fn press(&mut self, key: Key) {
        self.harness.key_press(key);
        self.steps(2);
    }

    /// Screen rectangle of the viewport whose label starts with `name`.
    fn viewport(&self, name: &str) -> Rect {
        self.harness
            .query_all_by_label_contains(name)
            .find(|node| {
                let node = node.accesskit_node();
                node.role() == Role::Image && node.label().is_some_and(|label| label.starts_with(name))
            })
            .unwrap_or_else(|| panic!("no viewport named {name:?}"))
            .rect()
    }

    /// Where the patient position `p` is drawn in a view of `key`.
    fn on_screen(&self, key: SeriesKey, kind: ViewKind, viewport: Rect, p: DVec3) -> Pos2 {
        let displayed = &self.viewer().renders.get(key, kind).expect("rendered").displayed;
        let (x, y) = displayed.patient_to_pixel(p).expect("the view has geometry");
        Placement::of(viewport, displayed, &self.view(key).transform(kind)).to_screen(x, y)
    }

    /// Shows the acquired image of `key` nearest to the marker.
    fn go_to_marker(&mut self, key: SeriesKey) {
        let series = self.series(key);
        let distance = |i: &usize| (series.frames()[*i].plane.expect("plane").position.z - marker().z).abs();
        let index = (0..series.len()).min_by(|a, b| distance(a).total_cmp(&distance(b))).expect("images");
        self.view_mut(key).set_slice(ViewKind::Stack, &series, index);
        self.steps(3);
    }

    /// The read-out of the status bar with the pointer at `pos`.
    fn read_out_at(&mut self, pos: Pos2) -> String {
        self.harness.hover_at(pos);
        self.steps(3);
        // A text label keeps its text as the value of its node.
        self.harness
            .query_all_by_label_contains(" mm   ·   ")
            .filter_map(|node| node.accesskit_node().value())
            .next()
            .unwrap_or_else(|| panic!("no read-out with the pointer at {pos:?}"))
    }

    fn snapshot(&mut self, name: &str) {
        if let Some(dir) = snapshot_dir() {
            let image = self.harness.render().expect("wgpu render");
            std::fs::create_dir_all(&dir).expect("snapshot dir");
            image.save(dir.join(format!("{name}.png"))).expect("save png");
        }
    }
}

/// A PET opens with the inverted grey scale and in SUV; C cycles the colour
/// maps, the uptake presets are offered, and reset restores the defaults.
#[test]
fn a_pet_opens_inverted_in_suv_and_c_cycles_the_colour_maps() {
    let mut f = open_dark(PET);
    assert_eq!(f.view(PET).colormap(), Colormap::InvertedGrey);
    assert_eq!(f.series(PET).units(), Some("SUV"));
    f.go_to_marker(PET);
    let viewport = f.viewport("Image ");
    let at_marker = f.on_screen(PET, ViewKind::Stack, viewport, marker());
    let read_out = f.read_out_at(at_marker);
    assert!(read_out.ends_with("·   SUV 5"), "the hot sphere reads SUV 5: {read_out}");
    // The uptake presets, in SUV; the W and L fields and annotations follow.
    f.press(Key::Num2);
    let window = f.view(PET).window;
    assert_eq!((window.low(), window.high(), f.view(PET).preset.as_deref()), (0.0, 7.0, Some("SUV 0–7")));
    f.press(Key::Num1);
    assert_eq!(f.view(PET).preset.as_deref(), Some("SUV 0–5"));
    assert!(f.shows(MENU), "the colour menu is offered for a PET");
    f.snapshot("pet-1-pet-inverted-grey");

    for (map, name) in [
        (Colormap::HotIron, "pet-2-pet-hot-iron"),
        (Colormap::Rainbow, "pet-3-pet-rainbow"),
        (Colormap::Grey, "pet-4-pet-grey"),
    ] {
        f.press(Key::C);
        assert_eq!(f.view(PET).colormap(), map);
        f.snapshot(name);
    }
    f.press(Key::C);
    assert_eq!(f.view(PET).colormap(), Colormap::InvertedGrey, "round to the start");
    // The colour map is the user's: reset restores the inverted grey scale.
    f.press(Key::C);
    assert!(!f.view(PET).is_pristine(&f.series(PET)));
    f.press(Key::R);
    assert_eq!(f.view(PET).colormap(), Colormap::InvertedGrey);

    // The menu lists the four maps and, from the PET, the CT to show it over.
    f.harness.get_by_label(MENU).click();
    f.steps(3);
    for name in ["Grey", "Inverted grey", "Hot iron", "Rainbow", "Show over CT CHEST 2.5mm (synthetic)"] {
        assert!(f.shows(name), "{name}");
    }
    assert!(f.harness.unnamed_widgets().is_empty(), "{:?}", f.harness.unnamed_widgets());
}

/// F draws the PET over the CT once it has loaded (with visible progress);
/// the read-out shows both values, in the acquired view and in the MPR
/// layout, and F or R switches the overlay off.
#[test]
fn f_draws_the_pet_over_the_ct_and_the_read_out_shows_both_values() {
    let gate = PetGate::default();
    gate.closed.store(true, Ordering::SeqCst);
    let mut f = open(&study(), CT, Theme::Dark, [1440.0, 900.0], gate);
    assert!(f.view(CT).fusion().is_none());
    assert!(f.shows(MENU), "a PET of the CT's Frame of Reference is offered");
    f.press(Key::F);
    assert_eq!(f.view(CT).fusion().map(|fusion| fusion.series), Some(PET));
    // The PET is held back: its progress shows in the viewport.
    f.steps(3);
    assert!(f.shows("PET loading 0 / 50"), "visible progress");
    f.snapshot("pet-5-ct-pet-loading");
    // A status, not an annotation: it stays when the annotations are hidden.
    f.press(Key::O);
    assert!(!f.harness.state().settings.show_overlays && f.shows("PET loading 0 / 50"));
    f.press(Key::O);
    f.gate.closed.store(false, Ordering::SeqCst);
    f.run_until_overlay_is_drawn();
    assert!(!f.shows_part("PET loading"));
    assert_eq!(f.viewer().session.active_series, Some(CT), "the CT is still the series displayed");

    f.go_to_marker(CT);
    let viewport = f.viewport("Image ");
    let at_marker = f.on_screen(CT, ViewKind::Stack, viewport, marker());
    let read_out = f.read_out_at(at_marker);
    assert!(read_out.ends_with("·   CT 300 HU · PET SUV 5"), "both values: {read_out}");
    f.snapshot("pet-6-ct-with-pet-overlay");

    // Turned a quarter turn and mirrored: the PET is part of the picture and turns with it.
    f.press(Key::CloseBracket);
    f.press(Key::H);
    assert!(f.view(CT).transform(ViewKind::Stack).orientation.is_mirrored());
    let at_marker = f.on_screen(CT, ViewKind::Stack, viewport, marker());
    let read_out = f.read_out_at(at_marker);
    assert!(read_out.ends_with("·   CT 300 HU · PET SUV 5"), "turned and mirrored: {read_out}");
    f.snapshot("pet-6b-ct-with-pet-overlay-turned-mirrored");
    f.press(Key::H);
    f.press(Key::OpenBracket);

    // The reformats: the crosshair on the marker, the PET over every view.
    f.press(Key::M);
    let series = f.series(CT);
    f.view_mut(CT).set_cursor(marker(), &series);
    f.steps(4);
    assert_eq!(f.viewer().layout, Layout::Mpr);
    let coronal = f.viewport("Coronal");
    let at_marker = f.on_screen(CT, ViewKind::Mpr(summrise_core::imaging::MprAxis::Coronal), coronal, marker());
    let read_out = f.read_out_at(at_marker);
    assert!(read_out.contains("CT 300 HU · PET SUV 5"), "both values in the coronal reformat: {read_out}");
    f.snapshot("pet-7-ct-with-pet-overlay-mpr");

    // F switches it off; R too.
    f.press(Key::F);
    assert!(f.view(CT).fusion().is_none());
    f.steps(2);
    assert_eq!(f.viewer().session.overlay(), None);
    f.press(Key::F);
    assert!(f.view(CT).fusion().is_some());
    f.press(Key::R);
    assert!(f.view(CT).fusion().is_none(), "reset switches the overlay off");
}

/// A PET that cannot form a volume is not drawn, and the viewport and the
/// menu say why.
#[test]
fn an_overlay_without_a_volume_is_unavailable_and_says_why() {
    let options = PetStudyOptions { missing_slice: true, ..study() };
    let mut f = open(&options, CT, Theme::Dark, [1440.0, 900.0], PetGate::default());
    f.press(Key::F);
    f.steps(3);
    // A badge on the picture says that and, briefly, why; its tooltip and the menu say it in full.
    assert!(f.shows("PET overlay unavailable: no regular volume"), "the viewport says why");
    assert!(f.viewer().session.state(PET).is_none(), "nothing of the PET is loaded");
    f.snapshot("pet-8-overlay-unavailable");
    f.harness.get_by_label("PET overlay unavailable: no regular volume").hover();
    f.steps(8);
    let reason = "The PET series does not form a volume: slices are not evenly spaced";
    assert!(f.shows_part(reason), "the tooltip");
    f.harness.get_by_label(MENU).click();
    f.steps(3);
    assert!(f.shows_part(&format!("PET overlay unavailable: {reason}")), "and the menu");
    assert!(f.shows_part("nothing is aligned afterwards"), "the note on positions, also before the PET is loaded");
    assert!(f.harness.unnamed_widgets().is_empty(), "{:?}", f.harness.unnamed_widgets());
    f.snapshot("pet-9-overlay-unavailable-menu");
}

/// From the PET, F shows it over the CT with the PET's colour scale.
#[test]
fn from_the_pet_f_shows_it_over_the_ct() {
    let mut f = open_dark(PET);
    f.press(Key::Num2);
    f.press(Key::C);
    assert_eq!(f.view(PET).colormap(), Colormap::HotIron);
    f.press(Key::F);
    f.run_until("the CT with the PET over it", |f| {
        f.viewer().session.active_series == Some(CT)
            && f.viewer().views.get(&CT).is_some_and(|view| view.fusion().is_some())
            && matches!(f.viewer().session.overlay_status(), Some(OverlayStatus::Ready(_)))
    });
    let fusion = f.view(CT).fusion().expect("fusion").clone();
    assert_eq!((fusion.series, fusion.colormap), (PET, Colormap::HotIron));
    let (window, name) = fusion.scale.expect("the PET's colour scale");
    assert_eq!((window.high(), name.as_deref()), (7.0, Some("SUV 0–7")));
}

/// The menu on the CT with the PET drawn over it, and the PET's own menu
/// with its colour strips, in both themes; every control in them has a name.
#[test]
fn the_fusion_menu_in_both_themes() {
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        let mut f = open(&study(), CT, theme, [1440.0, 900.0], PetGate::default());
        f.press(Key::F);
        f.run_until_overlay_is_drawn();
        f.go_to_marker(CT);
        f.harness.get_by_label(MENU).click();
        f.steps(3);
        for label in ["No PET overlay", "PET WB FDG AC (synthetic)", "PET WB FDG NAC (synthetic)", "Opacity"] {
            assert!(f.shows_part(label), "{label}");
        }
        assert!(f.harness.unnamed_widgets().is_empty(), "{:?}", f.harness.unnamed_widgets());
        f.snapshot(&format!("pet-10-ct-menu-{name}"));

        // The PET itself (loaded already as the overlay): its colour maps.
        f.press(Key::Escape);
        f.harness.state_mut().viewer.session.select_series(PET);
        f.run_until("the PET", |f| f.viewer().views.contains_key(&PET) && f.viewer().session.overlay().is_none());
        f.steps(3);
        f.harness.get_by_label(MENU).click();
        f.steps(3);
        assert!(f.shows("Hot iron") && f.shows("Show over CT CHEST 2.5mm (synthetic)"));
        assert!(f.harness.unnamed_widgets().is_empty(), "{:?}", f.harness.unnamed_widgets());
        f.snapshot(&format!("pet-11-pet-menu-{name}"));
    }
}

/// The inspector lists the inputs of the SUV, hides the weight in privacy
/// mode, and names the input that stops the calculation.
#[test]
fn the_inspector_lists_the_inputs_and_names_what_stops_the_calculation() {
    let mut f = open(&study(), PET, Theme::Dark, [1440.0, 1400.0], PetGate::default());
    f.run_until("the uptake section", |f| f.shows("Uptake values"));
    for value in ["70 kg", "370 MBq", "08:25:13", "2026-01-15 10:15:00", "6586.2 s (109.8 min)", "START", "BQML"] {
        assert!(f.shows(value), "{value}");
    }
    assert!(f.shows("About Values shown"), "SUV has its explanation");
    f.snapshot("pet-12-inspector");

    // Privacy mode hides the weight.
    f.harness.state_mut().settings.hide_patient_details = true;
    f.steps(3);
    assert!(f.shows("Hidden (privacy mode)") && !f.shows("70 kg"));
    f.harness.state_mut().settings.hide_patient_details = false;

    // The PET without attenuation correction: Bq/ml, and why.
    f.harness.state_mut().viewer.session.select_series(NAC);
    f.run_until("the PET without attenuation correction", |f| f.viewer().session.loaded(NAC).is_some());
    f.steps(4);
    assert!(f.shows("Bq/ml as stored; SUV not computed"));
    assert!(f.shows("The images are not corrected for attenuation (the weakening of the signal inside the body)."));
    f.snapshot("pet-13-inspector-refused");
}

/// The 3D view of the PET: its maximum intensity projection is shown in the
/// colour map of the slices (inverted grey at first), says so, and follows C.
#[test]
fn the_3d_projection_of_the_pet_is_shown_in_its_colour_map() {
    let mut f = open_dark(PET);
    let pet = f.series(PET);
    f.harness.get_by_label("3D").click();
    f.steps(3);
    f.view_mut(PET).set_look(Look::Mip, &pet);
    f.run_until("the 3D picture", |f| f.viewer().volume_view.shown_colormap().is_some());
    f.steps(4);
    assert!(f.shows("3D view: MIP"));
    assert_eq!(f.viewer().volume_view.shown_colormap(), Some(Colormap::InvertedGrey));
    let colours = || crate::volume_view::tests::colours_line(&pet, f.view(PET));
    assert_eq!(colours().as_deref(), Some("Colours: Inverted grey"));
    f.snapshot("pet-14-3d-projection");
    f.press(Key::C);
    f.run_until("the next colour map", |f| f.viewer().volume_view.shown_colormap() == Some(Colormap::HotIron));
    let colours = crate::volume_view::tests::colours_line(&pet, f.view(PET));
    assert_eq!(colours.as_deref(), Some("Colours: Hot iron"));
    // The CT has no colour maps: no such line.
    f.harness.state_mut().viewer.session.select_series(CT);
    f.run_until("the CT", |f| f.viewer().session.loaded(CT).is_some() && f.viewer().views.contains_key(&CT));
    let ct = f.series(CT);
    f.view_mut(CT).set_look(Look::Mip, &ct);
    assert_eq!(crate::volume_view::tests::colours_line(&ct, f.view(CT)), None);
}
