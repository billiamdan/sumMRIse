//! A study opened from a ZIP archive or an ISO 9660 disc image, at the level
//! of the session: the same images as from the unpacked folder, read by the
//! loader, the previews and the inspector at the same time, and nothing
//! written to disk. Synthetic data only.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use summrise_core::dicom::{InstanceMetadata, MediaKind, Progress, read_metadata, scan};
use summrise_core::{SeriesData, SeriesKey, Session};
use summrise_testkit::archive::{IsoNames, iso_image, zip_folder};
use summrise_testkit::{PhantomOptions, write_phantom_study};

/// Writes the synthetic CD to `dir/CD` and archives of it to `archives`
/// (which may be `dir` itself). Returns the folder and the archives.
fn write_fixtures(dir: &Path, archives: &Path) -> (PathBuf, [PathBuf; 2]) {
    let folder = dir.join("CD");
    write_phantom_study(&folder, &PhantomOptions::small()).expect("write fixture");
    let (zip, iso) = (archives.join("study.zip"), archives.join("study.iso"));
    std::fs::write(&zip, zip_folder(&folder, "PHANTOM CD").expect("zip")).expect("write zip");
    std::fs::write(&iso, iso_image(&folder, IsoNames::Joliet).expect("iso")).expect("write iso");
    (folder, [zip, iso])
}

fn wait_until(session: &mut Session, what: &str, done: impl Fn(&Session) -> bool) {
    let start = Instant::now();
    loop {
        session.poll();
        if done(session) {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Everything the application reads when a study is looked at completely.
struct Loaded {
    kind: MediaKind,
    series: Vec<Arc<SeriesData>>,
    /// (width, height, pixels) of the preview of each series.
    previews: Vec<(u32, u32, Vec<u8>)>,
    /// Metadata of the first image of each series, as the inspector reads it.
    metadata: Vec<InstanceMetadata>,
}

/// Opens `path` and looks at all of it: every image of every series through
/// the loader, every preview, and the metadata of an image of each series.
fn load_everything(path: &Path) -> Loaded {
    let index = scan(path, &Progress::new()).unwrap_or_else(|e| panic!("{} does not open: {e}", path.display()));
    let kind = index.media.kind();
    let keys: Vec<SeriesKey> =
        (0..index.studies[0].series.len()).map(|series| SeriesKey { study: 0, series }).collect();
    let first_images: Vec<PathBuf> = index.studies[0].series.iter().map(|s| s.instances[0].path.clone()).collect();
    let mut session = Session::new(index, 1 << 30, None);
    let mut series = Vec::new();
    for &key in &keys {
        session.select_series(key);
        wait_until(&mut session, "a series", |s| s.loaded(key).is_some());
        series.push(session.loaded(key).expect("loaded"));
    }
    wait_until(&mut session, "the previews", |s| keys.iter().all(|&key| s.thumbnail(key).is_some()));
    let preview = |key: &SeriesKey| session.thumbnail(*key).map(|t| (t.width, t.height, t.rgba.clone()));
    let previews = keys.iter().map(|key| preview(key).expect("preview")).collect();
    let metadata = first_images.iter().map(|path| read_metadata(session.media(), path).expect("metadata")).collect();
    Loaded { kind, series, previews, metadata }
}

fn assert_same(loaded: &Loaded, expected: &Loaded, name: &str) {
    assert_eq!(loaded.series.len(), expected.series.len(), "{name}");
    for (data, reference) in loaded.series.iter().zip(&expected.series) {
        assert_eq!(data.frames(), reference.frames(), "{name}");
        for image in 0..reference.len() {
            assert_eq!(data.frame(image), reference.frame(image), "{name}: image {image}");
        }
        assert_eq!(data.volume().map(|v| v.geometry), reference.volume().map(|v| v.geometry), "{name}");
        assert_eq!(data.volume_issue(), reference.volume_issue(), "{name}");
        assert_eq!(data.value_range(), reference.value_range(), "{name}");
        assert_eq!(data.presets(), reference.presets(), "{name}");
        assert_eq!(data.warnings(), reference.warnings(), "{name}");
        assert_eq!(data.units(), reference.units(), "{name}");
    }
    assert_eq!(loaded.previews, expected.previews, "{name}");
    for (metadata, reference) in loaded.metadata.iter().zip(&expected.metadata) {
        assert_eq!(metadata.dataset, reference.dataset, "{name}");
        assert_eq!(metadata.summary, reference.summary, "{name}");
    }
}

#[test]
fn a_study_from_a_zip_or_an_iso_is_the_study_of_the_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (folder, archives) = write_fixtures(dir.path(), dir.path());
    let expected = load_everything(&folder);
    assert_eq!(expected.kind, MediaKind::Folder);
    // CT (a volume of 24 slices), MR, localizer and the colour capture.
    assert_eq!(expected.series.iter().map(|s| s.len()).collect::<Vec<_>>(), [24, 12, 3, 1]);
    assert!(expected.series[0].volume().is_some());
    for (archive, kind) in archives.iter().zip([MediaKind::Zip, MediaKind::Iso]) {
        let loaded = load_everything(archive);
        assert_eq!(loaded.kind, kind);
        assert_same(&loaded, &expected, &archive.display().to_string());
    }
}

/// The loader thread, the previews and header indexing on rayon, and the
/// inspector all read one archive at the same time.
#[test]
fn everything_reads_one_archive_at_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (folder, archives) = write_fixtures(dir.path(), dir.path());
    let expected = load_everything(&folder);
    let images = scan(&folder, &Progress::new()).expect("scan").stats.images;
    for archive in &archives {
        let index = scan(archive, &Progress::new()).expect("scan");
        let media = index.media.clone();
        let files: Vec<PathBuf> =
            index.studies[0].series.iter().flat_map(|s| &s.instances).map(|i| i.path.clone()).collect();
        std::thread::scope(|scope| {
            // The same archive is indexed again (and again) while it is being read.
            let indexing = scope.spawn(|| {
                (0..3).map(|_| scan(archive, &Progress::new()).expect("scan").stats.images).collect::<Vec<_>>()
            });
            // The inspector goes through every image.
            let inspecting = scope.spawn(|| {
                files.iter().map(|file| read_metadata(&media, file).expect("metadata").attribute_count()).min()
            });
            // And the session loads every series and every preview.
            let loaded = load_everything(archive);
            assert_same(&loaded, &expected, &archive.display().to_string());
            assert_eq!(indexing.join().expect("indexing"), [images; 3]);
            assert!(inspecting.join().expect("inspecting").is_some_and(|attributes| attributes > 30));
        });
    }
}

/// Paths below `root` with the size of each file, sorted.
fn listing(root: &Path) -> Vec<String> {
    fn visit(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read dir").map(|e| e.expect("dir entry")) {
            let path = entry.path();
            let meta = entry.metadata().expect("metadata");
            let relative = path.strip_prefix(root).expect("below the root").display().to_string();
            if meta.is_dir() {
                out.push(format!("{relative}/"));
                visit(&path, root, out);
            } else {
                out.push(format!("{relative} ({} bytes)", meta.len()));
            }
        }
    }
    let mut out = Vec::new();
    visit(root, root, &mut out);
    out.sort();
    out
}

/// Set for the child process of the test below: the folder with the archives.
const CHILD: &str = "SUMMRISE_ARCHIVE_PRIVACY_CHILD";

/// Privacy: opening and looking at a whole study from a ZIP archive and from
/// a disc image creates no file — nothing is unpacked, cached or left behind.
///
/// The temporary folder of the process is shared with everything else that
/// runs on the machine, so the check is made in a process of its own (this
/// test, started again) whose temporary folder is a new, empty one.
#[test]
fn opening_archives_writes_nothing_to_disk() {
    if let Some(archives) = std::env::var_os(CHILD).map(PathBuf::from) {
        let temp = std::env::temp_dir();
        assert_eq!(listing(&temp), Vec::<String>::new(), "the temporary folder of this process starts empty");
        let before = listing(&archives);
        assert_eq!(before.len(), 2, "{before:?}");
        for name in ["study.zip", "study.iso"] {
            let loaded = load_everything(&archives.join(name));
            assert_eq!(loaded.series.iter().map(|s| s.len()).collect::<Vec<_>>(), [24, 12, 3, 1], "{name}");
            assert_eq!((loaded.previews.len(), loaded.metadata.len()), (4, 4), "{name}");
        }
        assert_eq!(listing(&temp), Vec::<String>::new(), "nothing was written to the temporary folder");
        assert_eq!(listing(&archives), before, "nothing was written beside the archives");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let (archives, private_temp) = (dir.path().join("archives"), dir.path().join("tmp"));
    std::fs::create_dir_all(&archives).expect("create");
    std::fs::create_dir_all(&private_temp).expect("create");
    write_fixtures(dir.path(), &archives);
    let child = Command::new(std::env::current_exe().expect("this test binary"))
        .args(["--exact", "opening_archives_writes_nothing_to_disk", "--test-threads=1"])
        .env(CHILD, &archives)
        // What `std::env::temp_dir` reads on Unix and on Windows.
        .env("TMPDIR", &private_temp)
        .env("TMP", &private_temp)
        .env("TEMP", &private_temp)
        .output()
        .expect("start the child process");
    let output = format!("{}\n{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
    assert!(child.status.success(), "the check in the child process failed:\n{output}");
    assert!(output.contains("1 passed"), "the child process ran the check:\n{output}");
    assert_eq!(listing(&private_temp), Vec::<String>::new());
    assert_eq!(listing(&archives).len(), 2);
}
