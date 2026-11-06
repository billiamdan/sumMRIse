//! Offscreen UI test of opening archives and disc images in the real app:
//! the welcome screen offers it, a dropped ZIP archive or disc image opens
//! like the folder, the recent list brings it back, and an archive that
//! cannot be opened says why. Set `SUMMRISE_SNAPSHOT_DIR` to keep the
//! rendered PNGs for review.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use egui_material_icons::icons::ICON_CLOSE;
use summrise_testkit::archive::{IsoNames, ZipEntry, ZipMethod, iso_image, zip_archive, zip_folder};
use summrise_testkit::{PhantomOptions, write_phantom_study};
use summrise_ui::{AppOptions, SumMRIseApp};

type App<'a> = Harness<'a, SumMRIseApp>;

fn app(open: Option<PathBuf>) -> App<'static> {
    Harness::builder()
        .with_size([1280.0, 800.0])
        .wgpu()
        .build_eframe(move |cc| SumMRIseApp::new(cc, AppOptions { open, ..AppOptions::default() }))
}

/// Runs UI frames until something labelled `label` is on screen.
fn wait_for(harness: &mut App<'_>, label: &str) {
    let start = Instant::now();
    while harness.query_all_by_label_contains(label).next().is_none() {
        assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {label:?}");
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    // A few more frames: thumbnails and the first image arrive.
    for _ in 0..40 {
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn save(harness: &mut App<'_>, name: &str) {
    let image = harness.render().expect("wgpu render");
    assert!(image.width() > 0 && image.height() > 0);
    if let Some(dir) = std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from) {
        std::fs::create_dir_all(&dir).expect("snapshot dir");
        image.save(dir.join(format!("{name}.png"))).expect("save png");
    }
}

/// A file as the windowing system hands it over when it is dropped.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn drop_file(harness: &mut App<'_>, path: &Path) {
    harness.input_mut().dropped_files.push(Arc::new(Dropped(path.to_path_buf())));
    harness.step();
}

/// The synthetic CD in `dir/CD`, and a ZIP archive and a disc image of it.
fn fixtures(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let folder = dir.join("CD");
    let opts = PhantomOptions { ct_size: 128, ct_slices: 40, mr_size: 96, ..PhantomOptions::default() };
    write_phantom_study(&folder, &opts).expect("fixture");
    let (zip, iso) = (dir.join("MRI download.zip"), dir.join("patient-disc.iso"));
    std::fs::write(&zip, zip_folder(&folder, "CD").expect("zip")).expect("write zip");
    std::fs::write(&iso, iso_image(&folder, IsoNames::Joliet).expect("iso")).expect("write iso");
    (folder, zip, iso)
}

#[test]
fn dropped_archives_and_disc_images_open_like_the_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_folder, zip, iso) = fixtures(dir.path());

    // The welcome screen offers the new way in, next to the folder.
    let mut harness = app(None);
    wait_for(&mut harness, "Open folder or disc");
    harness.get_by_label_contains("Open archive or disc image");
    harness.get_by_label_contains("drag a folder, a ZIP archive, a disc image");
    save(&mut harness, "archive-1-welcome");
    harness.get_by_label_contains("Open archive or disc image").hover();
    harness.run_steps(10);
    save(&mut harness, "archive-2-welcome-tooltip");

    // A ZIP archive dropped onto the window opens: the same series as from the folder.
    drop_file(&mut harness, &zip);
    wait_for(&mut harness, "CT CHEST 2.5mm (synthetic)");
    for series in ["T2 TSE SAG (synthetic)", "Localizer 3-plane (synthetic)", "Dose report (synthetic)"] {
        assert!(harness.query_all_by_label(series).next().is_some(), "{series}");
    }
    // The status bar names the archive and says that the DICOMDIR inside is used.
    assert!(harness.query_all_by_label_contains("images · ZIP archive · DICOMDIR").next().is_some());
    save(&mut harness, "archive-3-dropped-zip");

    // A disc image dropped onto the open study replaces it.
    drop_file(&mut harness, &iso);
    wait_for(&mut harness, "CT CHEST 2.5mm (synthetic)");
    assert!(harness.query_all_by_label_contains("images · Disc image · DICOMDIR").next().is_some());
    harness.get_by_label("MPR").click();
    wait_for(&mut harness, "Axial");
    save(&mut harness, "archive-4-dropped-iso-mpr");

    // Back on the welcome screen, both are in the recent list, by the path of the file …
    harness.get_by_label(ICON_CLOSE.codepoint).click();
    wait_for(&mut harness, "Open folder or disc");
    assert!(harness.query_all_by_label_contains("patient-disc.iso").next().is_some());
    assert!(harness.query_all_by_label_contains("MRI download.zip").next().is_some());
    assert_eq!(harness.query_all_by_label("Synthetic phantom study · 2026-01-15 · CT, MR, OT").count(), 2);
    save(&mut harness, "archive-5-recent");
}

#[test]
fn archive_given_on_the_command_line_opens() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_folder, zip, _iso) = fixtures(dir.path());
    let mut harness = app(Some(zip));
    wait_for(&mut harness, "CT CHEST 2.5mm (synthetic)");
    assert!(harness.query_all_by_label("T2 TSE SAG (synthetic)").next().is_some());
}

#[test]
fn archive_that_cannot_be_opened_says_why() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (folder, _zip, _iso) = fixtures(dir.path());
    let image = std::fs::read(folder.join("DICOM/ST000001/SE000001/IM000001")).expect("read");
    let mut locked = ZipEntry::new("CD/DICOM/IM000001", image, ZipMethod::Deflated);
    locked.encrypted = true;
    let path = dir.path().join("protected.zip");
    std::fs::write(&path, zip_archive(&[locked])).expect("write");

    let mut harness = app(None);
    wait_for(&mut harness, "Open folder or disc");
    drop_file(&mut harness, &path);
    wait_for(&mut harness, "protected.zip is protected by a password.");
    // The welcome screen is still there to try something else.
    harness.get_by_label_contains("Open archive or disc image");
    save(&mut harness, "archive-6-password-protected");

    // A disc image in a format that is not read.
    let udf = dir.path().join("dvd.iso");
    std::fs::write(&udf, summrise_testkit::archive::udf_only_image()).expect("write");
    drop_file(&mut harness, &udf);
    wait_for(&mut harness, "dvd.iso is a disc image in UDF format");
    save(&mut harness, "archive-7-udf");
}
