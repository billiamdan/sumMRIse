//! A tour of the screens of the app in one language, on a system said to
//! prefer that language ("System language" is the default setting). Shared
//! by the glyph test (every language, nothing saved) and the snapshot helper
//! for translators (one language, saved as PNGs). Labels are looked up with
//! the macros of the application, so the tour finds them in any language.
//! The active language is global: callers hold `summrise_i18n::with_catalogue`.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::{Key, Modifiers, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use summrise_i18n::{tr, tr_ctx};
use summrise_testkit::pet::{PetStudyOptions, write_pet_study};
use summrise_testkit::{PhantomOptions, write_phantom_study};
use summrise_ui::{AppOptions, SumMRIseApp};

pub(crate) type App = Harness<'static, SumMRIseApp>;

/// The CT of the phantom and of the PET study.
pub(crate) const CT: &str = "CT CHEST 2.5mm (synthetic)";
/// The series of the stacks study with three time points.
pub(crate) const DYNAMIC: &str = "DYNAMIC (synthetic)";

/// The app in a window of `size` points on a system that prefers
/// `language`, opening `open`; drawn through wgpu when `render`.
pub(crate) fn app(language: &str, open: Option<PathBuf>, size: [f32; 2], render: bool) -> App {
    let mut builder = Harness::builder().with_size(size);
    if render {
        builder = builder.wgpu();
    }
    let system_languages = vec![language.to_owned()];
    let mut app = builder.build_eframe(move |cc| SumMRIseApp::new(cc, AppOptions { open, system_languages }));
    // The first frame installs the language.
    app.step();
    app
}

/// Runs frames for `duration`.
pub(crate) fn settle(app: &mut App, duration: Duration) {
    let start = Instant::now();
    while start.elapsed() < duration {
        app.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Runs frames until a node whose label contains `label` is on screen,
/// then a few more (thumbnails, the first image).
pub(crate) fn wait_for(app: &mut App, label: &str) {
    let start = Instant::now();
    while app.query_all_by_label_contains(label).next().is_none() {
        assert!(start.elapsed() < Duration::from_secs(60), "timed out waiting for {label:?}");
        app.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    settle(app, Duration::from_millis(300));
}

/// Runs frames until the reformat layouts can be chosen: the shown series is loaded.
pub(crate) fn wait_until_loaded(app: &mut App) {
    let mpr = tr_ctx!("layout", "MPR");
    let start = Instant::now();
    loop {
        let enabled = app.query_all_by_label(mpr).next().is_some_and(|node| !node.accesskit_node().is_disabled());
        if enabled {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "timed out loading the series");
        app.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    settle(app, Duration::from_millis(400));
}

pub(crate) fn click(app: &mut App, label: &str) {
    app.query_all_by_label(label).next().unwrap_or_else(|| panic!("nothing labelled {label:?}")).click();
    settle(app, Duration::from_millis(300));
}

/// Opens the combo box labelled `label` and picks `entry`.
pub(crate) fn choose(app: &mut App, label: &str, entry: &str) {
    app.query_all(by().label(label).role(Role::ComboBox))
        .next()
        .unwrap_or_else(|| panic!("no list labelled {label:?}"))
        .click();
    settle(app, Duration::from_millis(200));
    click(app, entry);
}

/// The widgets of the toolbar (everything above the views) and of the status
/// bar (the text in the bottom 28 points) with their label (or value) and
/// rectangle.
fn toolbar_widgets(app: &App) -> Vec<(String, Rect)> {
    let views_top = app
        .query_all(by().role(Role::Image))
        .filter(|node| node.accesskit_node().bounding_box().is_some())
        .map(|node| node.rect().top())
        .fold(f32::INFINITY, f32::min);
    let bottom = app
        .query_all(by())
        .filter(|node| node.accesskit_node().bounding_box().is_some())
        .map(|node| node.rect().bottom())
        .fold(0.0, f32::max);
    app.query_all(by())
        .filter(|node| node.accesskit_node().bounding_box().is_some())
        .filter_map(|node| {
            let rect = node.rect();
            let ak = node.accesskit_node();
            let interesting = !matches!(ak.role(), Role::Window | Role::GenericContainer | Role::Unknown);
            let status =
                matches!(ak.role(), Role::Label | Role::Button | Role::CheckBox) && rect.top() >= bottom - 28.0;
            let label = ak.label().or_else(|| ak.value()).unwrap_or_default();
            (interesting && rect.is_positive() && (rect.bottom() <= views_top || status)).then_some((label, rect))
        })
        .collect()
}

/// What does not fit in the toolbar or the status bar of a window `width`
/// points wide: a control or text outside the window or two on top of each
/// other.
pub(crate) fn toolbar_problems(app: &App, width: f32, what: &str) -> Vec<String> {
    let widgets = toolbar_widgets(app);
    let mut problems = Vec::new();
    for (label, rect) in &widgets {
        if rect.left() < -0.5 || rect.right() > width + 0.5 {
            problems.push(format!("{what}: {label:?} is not inside the window ({rect:?})"));
        }
    }
    for (i, (a_label, a)) in widgets.iter().enumerate() {
        for (b_label, b) in &widgets[i + 1..] {
            let common = a.intersect(*b);
            let nested = a.contains_rect(*b) || b.contains_rect(*a);
            if common.is_positive() && common.area() >= 1.0 && !nested {
                problems.push(format!("{what}: {a_label:?} and {b_label:?} overlap ({a:?}, {b:?})"));
            }
        }
    }
    problems
}

/// The tour: the synthetic studies it opens, where it saves its pictures,
/// and what did not fit.
pub(crate) struct Tour {
    pub(crate) language: String,
    snapshots: Option<PathBuf>,
    pub(crate) problems: Vec<String>,
    data: tempfile::TempDir,
}

impl Tour {
    /// A tour in `language`, saving `<language>-<screen>.png` to `snapshots`
    /// when given (then everything is drawn through wgpu).
    pub(crate) fn new(language: &str, snapshots: Option<PathBuf>) -> Self {
        let data = tempfile::tempdir().expect("tempdir");
        Self { language: language.to_owned(), snapshots, problems: Vec::new(), data }
    }

    fn app(&self, open: Option<PathBuf>, size: [f32; 2]) -> App {
        app(&self.language, open, size, self.snapshots.is_some())
    }

    fn save(&self, app: &mut App, name: &str) {
        if let Some(dir) = &self.snapshots {
            let image = app.render().expect("wgpu render");
            std::fs::create_dir_all(dir).expect("snapshot dir");
            image.save(dir.join(format!("{}-{name}.png", self.language))).expect("save png");
        }
    }

    fn study(&self, name: &str, write: impl FnOnce(&Path)) -> PathBuf {
        let dir = self.data.path().join(name);
        if !dir.exists() {
            std::fs::create_dir_all(&dir).expect("study dir");
            write(&dir);
        }
        dir
    }

    fn phantom(&self) -> PathBuf {
        // As in the smoke test for pictures; small for a check only.
        let opts = if self.snapshots.is_some() {
            PhantomOptions { ct_size: 128, ct_slices: 60, mr_size: 96, ..PhantomOptions::default() }
        } else {
            PhantomOptions::small()
        };
        self.study("phantom", |dir| write_phantom_study(dir, &opts).map(drop).expect("fixture"))
    }

    fn pet(&self) -> PathBuf {
        self.study("pet", |dir| {
            let ct = PhantomOptions { ct_size: 96, ct_slices: 48, ..PhantomOptions::small() };
            write_pet_study(dir, &PetStudyOptions { ct, ..Default::default() }).expect("fixture");
        })
    }

    /// The main screens: the welcome screen, the settings window, the About
    /// dialog and the viewer with the CT (acquired axial images and the
    /// reformats) with the inspector and the guide.
    pub(crate) fn main_screens(&mut self) {
        let mut app = self.app(None, [1280.0, 800.0]);
        wait_for(&mut app, tr!("Open folder or disc…"));
        self.save(&mut app, "01-welcome");
        app.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
        wait_for(&mut app, tr!("Restore defaults"));
        self.save(&mut app, "02-settings");
        // The display check takes the place of the settings window.
        click(&mut app, tr!("Check the display…"));
        wait_for(&mut app, tr!("Display check"));
        self.save(&mut app, "03-display-check");
        app.key_press(Key::Escape);
        settle(&mut app, Duration::from_millis(200));
        app.key_press(Key::F1);
        wait_for(&mut app, tr!("Not a medical device. Not for diagnostic use."));
        self.save(&mut app, "04-about");

        let mut app = self.app(Some(self.phantom()), [1440.0, 900.0]);
        wait_for(&mut app, CT);
        wait_until_loaded(&mut app);
        self.save(&mut app, "05-viewer-ct-axial");
        self.problems.extend(toolbar_problems(&app, 1440.0, "CT at 1440 pt"));
        click(&mut app, tr_ctx!("layout", "MPR"));
        settle(&mut app, Duration::from_millis(500));
        self.save(&mut app, "06-viewer-ct-mpr");
    }

    /// Everything else a translator needs to see: the toolbar at 1440, 1280
    /// and 960 points with reformats and a thick slab and with the 3D view,
    /// the PET menu, an archive that cannot be opened and the playback bar of
    /// a series with several time points.
    pub(crate) fn other_screens(&mut self) {
        let pet = self.pet();
        for width in [1440.0f32, 1280.0, 960.0] {
            let mut app = self.app(Some(pet.clone()), [width, 800.0]);
            wait_for(&mut app, CT);
            click(&mut app, CT);
            wait_until_loaded(&mut app);
            click(&mut app, tr_ctx!("layout", "MPR"));
            choose(&mut app, tr!("Slab"), tr!("MIP"));
            settle(&mut app, Duration::from_millis(400));
            self.problems.extend(toolbar_problems(&app, width, &format!("reformats with a slab at {width} pt")));
            self.save(&mut app, &format!("07-toolbar-{width}-mpr-slab"));
            if width == 1440.0 {
                click(&mut app, tr!("PET colours and fusion"));
                self.save(&mut app, "08-pet-menu");
                app.key_press(Key::Escape);
                settle(&mut app, Duration::from_millis(200));
            }
            click(&mut app, tr!("3D"));
            settle(&mut app, Duration::from_millis(1500));
            self.problems.extend(toolbar_problems(&app, width, &format!("3D view at {width} pt")));
            self.save(&mut app, &format!("09-toolbar-{width}-3d"));
        }

        let udf = self.data.path().join("dvd.iso");
        std::fs::write(&udf, summrise_testkit::archive::udf_only_image()).expect("write");
        let mut app = self.app(Some(udf), [1280.0, 800.0]);
        // The welcome screen is back, with the notification that says why.
        wait_for(&mut app, tr!("Open folder or disc…"));
        assert!(app.query_all_by_label_contains("dvd.iso").next().is_some(), "the archive error is shown");
        self.save(&mut app, "10-archive-error");

        let stacks = self.study("stacks", |dir| {
            summrise_testkit::stacks::write_stacks_study(dir).expect("fixture");
        });
        let mut app = self.app(Some(stacks), [1440.0, 900.0]);
        wait_for(&mut app, DYNAMIC);
        click(&mut app, DYNAMIC);
        wait_for(&mut app, tr!("Play"));
        settle(&mut app, Duration::from_millis(500));
        self.save(&mut app, "11-playback-bar");
    }
}
