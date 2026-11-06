//! Opening ZIP archives and ISO 9660 disc images of the synthetic CD written
//! by `summrise-testkit`: the same index and the same pixels as from the
//! unpacked folder, and a clear answer for what cannot be read. No real
//! patient data involved.

use std::path::{Path, PathBuf};
use std::process::Command;

use summrise_dicom::{
    ArchiveProblem, DicomError, DiscoverySource, MediaKind, Progress, StudyIndex, decode_instance, load_series,
    read_instance, read_metadata, scan, thumbnail,
};
use summrise_imaging::DVec3;
use summrise_testkit::archive::{
    IsoNames, IsoOptions, ZipEntry, ZipMethod, iso_image, iso_image_with, udf_only_image, zip_archive, zip_folder,
};
use summrise_testkit::{CT_MARKER_HU, MARKER_POSITION, PhantomOptions, WrittenStudy, write_phantom_study};

/// Top-level folder of the synthetic ZIP archives.
const TOP: &str = "PHANTOM CD";

/// The synthetic CD as a folder `CD`, next to which archives of it are written.
struct Fixture {
    dir: tempfile::TempDir,
    folder: PathBuf,
    written: WrittenStudy,
}

fn fixture(opts: &PhantomOptions) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let folder = dir.path().join("CD");
    let written = write_phantom_study(&folder, opts).expect("write fixture");
    Fixture { dir, folder, written }
}

impl Fixture {
    fn small() -> Self {
        fixture(&PhantomOptions::small())
    }

    /// Writes `bytes` as a file `name` beside the folder.
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, bytes).expect("write archive");
        path
    }

    fn zip(&self) -> PathBuf {
        self.file("study.zip", &zip_folder(&self.folder, TOP).expect("zip"))
    }

    fn iso(&self, names: IsoNames) -> PathBuf {
        let name = if names == IsoNames::Joliet { "joliet.iso" } else { "level1.iso" };
        self.file(name, &iso_image(&self.folder, names).expect("iso"))
    }

    fn folder_index(&self) -> StudyIndex {
        scan(&self.folder, &Progress::new()).expect("scan the folder")
    }

    /// Every file of the CD as a stored ZIP entry below [`TOP`], named by `name`
    /// from its path in the folder (with `/` between folders).
    fn entries(&self, name: impl Fn(&str) -> String) -> Vec<ZipEntry> {
        let mut files: Vec<PathBuf> = walkdir::WalkDir::new(&self.folder)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(walkdir::DirEntry::into_path)
            .collect();
        files.sort();
        files
            .iter()
            .map(|file| {
                let relative = file.strip_prefix(&self.folder).expect("below the folder");
                let parts: Vec<_> = relative.components().map(|c| c.as_os_str().to_string_lossy()).collect();
                ZipEntry::new(name(&parts.join("/")), std::fs::read(file).expect("read"), ZipMethod::Stored)
            })
            .collect()
    }

    /// The path of an image inside the archives, from its path in the folder.
    fn relative(&self, file: &Path) -> PathBuf {
        file.strip_prefix(&self.folder).expect("below the folder").to_path_buf()
    }
}

fn open(path: &Path) -> StudyIndex {
    scan(path, &Progress::new()).unwrap_or_else(|e| panic!("{} does not open: {e}", path.display()))
}

/// Why `path` cannot be opened.
fn problem(path: &Path) -> ArchiveProblem {
    match scan(path, &Progress::new()) {
        Err(DicomError::Archive { problem, .. }) => problem,
        other => panic!("{}: expected an archive problem, got {other:?}", path.display()),
    }
}

/// Everything the index says about the studies, without where the files are.
fn described(index: &StudyIndex) -> String {
    let mut studies = index.studies.clone();
    for instance in studies.iter_mut().flat_map(|s| &mut s.series).flat_map(|s| &mut s.instances) {
        instance.path = PathBuf::new();
    }
    format!("{studies:#?}\n{:?}\n{:?}", index.stats, index.warnings)
}

/// The paths of all images, in the order of the index.
fn paths(index: &StudyIndex) -> Vec<PathBuf> {
    index.studies.iter().flat_map(|s| &s.series).flat_map(|s| &s.instances).map(|i| i.path.clone()).collect()
}

#[test]
fn zip_and_iso_give_the_index_of_the_unpacked_folder() {
    let f = Fixture::small();
    let folder = f.folder_index();
    assert!(matches!(folder.source, DiscoverySource::Dicomdir(_)));
    assert_eq!(folder.media.kind(), MediaKind::Folder);
    let in_folder: Vec<PathBuf> = paths(&folder).iter().map(|p| f.relative(p)).collect();

    let cases = [
        (f.zip(), MediaKind::Zip, PathBuf::from(TOP)),
        (f.iso(IsoNames::Level1), MediaKind::Iso, PathBuf::new()),
        (f.iso(IsoNames::Joliet), MediaKind::Iso, PathBuf::new()),
    ];
    for (archive, kind, top) in cases {
        let index = open(&archive);
        let name = archive.display();
        // Studies, series, instance UIDs, geometry, counts and warnings.
        assert_eq!(described(&index), described(&folder), "{name}");
        assert_eq!(index.stats.images, 24 + 12 + 3 + 1, "{name}");
        assert!(index.warnings.is_empty(), "{name}: {:?}", index.warnings);
        // The DICOMDIR inside is used, and the clutter around it is not touched.
        assert!(matches!(&index.source, DiscoverySource::Dicomdir(used) if used.len() == 1), "{name}");
        assert_eq!((index.media.kind(), index.media.archive_file()), (kind, Some(archive.as_path())), "{name}");
        assert_eq!(index.root, archive, "{name}");
        // The path of an image is its path inside the archive.
        let expected: Vec<PathBuf> = in_folder.iter().map(|p| top.join(p)).collect();
        assert_eq!(paths(&index), expected, "{name}");
    }
}

#[test]
fn without_a_dicomdir_the_archive_is_scanned_like_the_folder() {
    let f = fixture(&PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() });
    let folder = f.folder_index();
    assert_eq!(folder.source, DiscoverySource::FolderScan);
    for archive in [f.zip(), f.iso(IsoNames::Level1), f.iso(IsoNames::Joliet)] {
        let index = open(&archive);
        assert_eq!(index.source, DiscoverySource::FolderScan, "{}", archive.display());
        assert_eq!(described(&index), described(&folder), "{}", archive.display());
        // Viewer software, autorun files and the resource forks of a Mac are
        // told apart before anything is parsed.
        assert_eq!((index.stats.files, index.stats.unreadable), (40, 0), "{}", archive.display());
    }
}

#[test]
fn series_from_zip_and_iso_are_pixel_identical_to_the_folder() {
    let f = Fixture::small();
    let folder = f.folder_index();
    for archive in [f.zip(), f.iso(IsoNames::Level1), f.iso(IsoNames::Joliet)] {
        let index = open(&archive);
        let name = archive.display();
        for (series, expected) in index.studies[0].series.iter().zip(&folder.studies[0].series) {
            let title = series.title();
            let loaded = load_series(&index.media, series, &Progress::new()).expect("decode from the archive");
            let reference = load_series(&folder.media, expected, &Progress::new()).expect("decode from the folder");
            assert_eq!(loaded.frames, reference.frames, "{name}: {title}");
            for image in 0..reference.len() {
                assert_eq!(loaded.frame(image), reference.frame(image), "{name}: {title}, image {image}");
            }
            assert_eq!(loaded.volume().map(|v| v.geometry), reference.volume().map(|v| v.geometry), "{name}: {title}");
            assert_eq!(loaded.value_range, reference.value_range, "{name}: {title}");
            assert_eq!(loaded.presets, reference.presets, "{name}: {title}");
            assert_eq!(loaded.initial_window, reference.initial_window, "{name}: {title}");
            assert_eq!(loaded.warnings, reference.warnings, "{name}: {title}");

            // File by file, as the viewer loads a series: the very bytes of the file.
            for (header, expected_header) in series.instances.iter().zip(&expected.instances) {
                let bytes = read_instance(&index.media, header).expect("read from the archive");
                assert_eq!(bytes, std::fs::read(&expected_header.path).expect("read the file"), "{name}: {title}");
            }
            let first = read_instance(&index.media, &series.instances[0]).and_then(decode_instance).expect("decode");
            assert_eq!(first.frames.len(), series.instances[0].frames as usize);

            // The preview and the metadata of the inspector.
            let preview = thumbnail(&index.media, series, 64).expect("thumbnail");
            let expected_preview = thumbnail(&folder.media, expected, 64).expect("thumbnail");
            assert_eq!(
                (preview.width, preview.height, &preview.rgba),
                (expected_preview.width, expected_preview.height, &expected_preview.rgba),
                "{name}: {title}"
            );
            let metadata = read_metadata(&index.media, &series.instances[0].path).expect("metadata");
            let expected_metadata = read_metadata(&folder.media, &expected.instances[0].path).expect("metadata");
            assert_eq!(metadata.dataset, expected_metadata.dataset, "{name}: {title}");
            assert_eq!(metadata.summary, expected_metadata.summary, "{name}: {title}");
        }

        // The CT in numbers: 24 axial slices from superior to inferior, the marker
        // sphere at (40, -35, 50) mm (patient left, anterior, superior) reads 300 HU.
        let ct = load_series(&index.media, &index.studies[0].series[0], &Progress::new()).expect("decode CT");
        assert_eq!(ct.units.as_deref(), Some("HU"), "{name}");
        let z: Vec<f64> = ct.frames.iter().map(|f| f.plane.map_or(0.0, |p| p.position.z)).collect();
        assert_eq!(z.len(), 24, "{name}");
        assert!(z.windows(2).all(|w| w[0] > w[1]), "{name}: superior to inferior: {z:?}");
        let volume = ct.volume().expect("the CT is a volume");
        let marker = volume.value_at_patient(DVec3::from_array(MARKER_POSITION)).expect("inside the volume");
        assert!((f64::from(marker) - CT_MARKER_HU).abs() < 1.0, "{name}: marker reads {marker} HU");
    }
}

#[test]
fn dicomdir_references_resolve_whatever_the_case_of_the_names() {
    let f = Fixture::small();
    let folder = f.folder_index();
    // The archive holds lower-case names; the DICOMDIR refers to upper-case ones.
    let lower = f.file("lower.zip", &zip_archive(&f.entries(|path| format!("cd/{}", path.to_lowercase()))));
    let index = open(&lower);
    assert!(matches!(index.source, DiscoverySource::Dicomdir(_)));
    assert_eq!(described(&index), described(&folder));
    assert!(paths(&index).iter().all(|p| p.starts_with("cd/dicom/st000001")), "{:?}", paths(&index)[0]);
    // Folders separated by backslashes, as some Windows tools write them.
    let backslashes = f.file("windows.zip", &zip_archive(&f.entries(|path| path.replace('/', "\\"))));
    assert_eq!(described(&open(&backslashes)), described(&folder));
}

#[test]
fn the_content_decides_what_a_file_is_not_its_name() {
    let f = Fixture::small();
    let folder = f.folder_index();
    let zip = zip_folder(&f.folder, TOP).expect("zip");
    let iso = iso_image(&f.folder, IsoNames::Joliet).expect("iso");
    // A ZIP archive called .iso, a disc image called .zip, and both without any extension.
    for (name, bytes, kind) in [
        ("zip-named.iso", &zip, MediaKind::Zip),
        ("iso-named.zip", &iso, MediaKind::Iso),
        ("download", &zip, MediaKind::Zip),
        ("disc image", &iso, MediaKind::Iso),
    ] {
        let index = open(&f.file(name, bytes));
        assert_eq!(index.media.kind(), kind, "{name}");
        assert_eq!(described(&index), described(&folder), "{name}");
    }
    // A DICOM file is a DICOM file whatever it is called.
    let image = std::fs::read(&f.written.series[0].files[0]).expect("read");
    for name in ["image.zip", "image.iso", "image.7z"] {
        let index = open(&f.file(name, &image));
        assert_eq!((index.media.kind(), &index.source), (MediaKind::Folder, &DiscoverySource::SingleFile), "{name}");
        assert_eq!(index.stats.images, 1, "{name}");
    }
    // And a folder is a folder whatever it is called.
    let named_zip = f.dir.path().join("folder.zip");
    write_phantom_study(&named_zip, &PhantomOptions::small()).expect("write fixture");
    assert_eq!(open(&named_zip).media.kind(), MediaKind::Folder);
}

#[test]
fn deflate64_entries_are_read() {
    let f = Fixture::small();
    let mut entries = f.entries(|path| format!("{TOP}/{path}"));
    for entry in &mut entries {
        entry.method = ZipMethod::Deflate64;
    }
    let index = open(&f.file("windows-big.zip", &zip_archive(&entries)));
    assert_eq!(described(&index), described(&f.folder_index()));
    let ct = load_series(&index.media, &index.studies[0].series[0], &Progress::new()).expect("decode CT");
    assert_eq!(ct.len(), 24);
}

/// The entries of the CD with `change` applied to the images of the CT (the
/// files below `SE000001`).
fn with_ct_images(f: &Fixture, change: impl Fn(usize, &mut ZipEntry)) -> Vec<ZipEntry> {
    let mut entries = f.entries(|path| format!("{TOP}/{path}"));
    let ct = entries.iter_mut().filter(|e| e.name.contains("/SE000001/"));
    for (index, entry) in ct.enumerate() {
        change(index, entry);
    }
    entries
}

#[test]
fn password_protected_archives_say_so() {
    let f = Fixture::small();
    // Everything is encrypted: nothing can be opened, and the message says why.
    let mut entries = f.entries(|path| format!("{TOP}/{path}"));
    for entry in &mut entries {
        entry.encrypted = true;
    }
    let locked = f.file("locked.zip", &zip_archive(&entries));
    assert_eq!(problem(&locked), ArchiveProblem::PasswordProtected);
    let message = scan(&locked, &Progress::new()).expect_err("locked").to_string();
    assert!(message.starts_with("locked.zip is protected by a password."), "{message}");

    // One image of the CT is encrypted: it is skipped and counted, the rest opens.
    let entries = with_ct_images(&f, |index, entry| entry.encrypted = index == 5);
    let index = open(&f.file("partly.zip", &zip_archive(&entries)));
    assert_eq!(index.warnings, ["1 file in partly.zip is protected by a password and was skipped"]);
    assert_eq!(index.studies[0].series[0].instances.len(), 23);
    assert_eq!(index.stats.images, 39);
    // The same without a DICOMDIR to say which files are images.
    let mut entries = with_ct_images(&f, |index, entry| entry.encrypted = index < 2);
    entries.retain(|e| !e.name.ends_with("DICOMDIR"));
    let index = open(&f.file("scanned.zip", &zip_archive(&entries)));
    assert_eq!(index.source, DiscoverySource::FolderScan);
    assert_eq!(index.warnings, ["2 files in scanned.zip are protected by a password and were skipped"]);
    assert_eq!(index.studies[0].series[0].instances.len(), 22);
}

#[test]
fn unsupported_compression_methods_say_so() {
    let f = Fixture::small();
    // LZMA (method 14) throughout.
    let mut entries = f.entries(|path| format!("{TOP}/{path}"));
    for entry in &mut entries {
        entry.method = ZipMethod::Other(14);
    }
    let lzma = f.file("lzma.zip", &zip_archive(&entries));
    assert_eq!(problem(&lzma), ArchiveProblem::Compression("Lzma".to_owned()));
    let message = scan(&lzma, &Progress::new()).expect_err("lzma").to_string();
    assert!(message.contains("a method that is not supported (Lzma)"), "{message}");

    // Three images of the CT are compressed with bzip2 (method 12).
    let entries = with_ct_images(&f, |index, entry| {
        if index < 3 {
            entry.method = ZipMethod::Other(12);
        }
    });
    let index = open(&f.file("mixed.zip", &zip_archive(&entries)));
    assert_eq!(
        index.warnings,
        ["3 files in mixed.zip are compressed with a method that is not supported (Bzip2) and were skipped"]
    );
    assert_eq!(index.studies[0].series[0].instances.len(), 21);
}

#[test]
fn an_archive_inside_the_archive_is_not_opened() {
    let f = Fixture::small();
    let disc = iso_image(&f.folder, IsoNames::Level1).expect("iso");
    // A download that wraps the disc image: nothing to open, and the message says what to do.
    let wrapped = [
        ZipEntry::new("download/README.TXT", b"Your images".as_slice(), ZipMethod::Stored),
        ZipEntry::new("download/disc.iso", disc.clone(), ZipMethod::Deflated),
    ];
    let wrapper = f.file("wrapper.zip", &zip_archive(&wrapped));
    assert_eq!(problem(&wrapper), ArchiveProblem::Nested("disc.iso".to_owned()));
    let message = scan(&wrapper, &Progress::new()).expect_err("nested").to_string();
    assert!(message.contains("Unpack wrapper.zip, then open disc.iso"), "{message}");

    // Images and another archive: the images open, the archive is pointed out.
    let mut entries = f.entries(|path| format!("{TOP}/{path}"));
    entries.push(ZipEntry::new(format!("{TOP}/MORE/older-study.zip"), zip_archive(&[]), ZipMethod::Stored));
    let index = open(&f.file("both.zip", &zip_archive(&entries)));
    assert_eq!(index.stats.images, 40);
    assert_eq!(index.warnings, ["both.zip contains another archive, which was not opened: older-study.zip"]);
}

#[test]
fn damaged_and_foreign_files_say_what_they_are() {
    let f = Fixture::small();
    let zip = zip_folder(&f.folder, TOP).expect("zip");
    let iso = iso_image(&f.folder, IsoNames::Joliet).expect("iso");
    let garbage: Vec<u8> = (0..100_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();

    // A download that was interrupted: the directory at the end of the archive is missing.
    let cut = f.file("cut.zip", &zip[..zip.len() / 2]);
    assert_eq!(problem(&cut), ArchiveProblem::Damaged(MediaKind::Zip));
    let message = scan(&cut, &Progress::new()).expect_err("cut").to_string();
    assert!(message.contains("damaged or was not downloaded completely"), "{message}");
    // Not an archive at all, whatever the name says.
    assert_eq!(problem(&f.file("garbage.zip", &garbage)), ArchiveProblem::Damaged(MediaKind::Zip));
    assert_eq!(problem(&f.file("garbage.iso", &garbage)), ArchiveProblem::Damaged(MediaKind::Iso));
    assert_eq!(problem(&f.file("empty.iso", &[])), ArchiveProblem::Damaged(MediaKind::Iso));
    // A disc image that ends before its directories do.
    assert_eq!(problem(&f.file("short.iso", &iso[..18 * 2048])), ArchiveProblem::Damaged(MediaKind::Iso));

    // UDF without ISO 9660, as some DVDs are written.
    let udf = f.file("dvd.iso", &udf_only_image());
    assert_eq!(problem(&udf), ArchiveProblem::UdfOnly);
    let message = scan(&udf, &Progress::new()).expect_err("udf").to_string();
    assert!(message.starts_with("dvd.iso is a disc image in UDF format"), "{message}");

    // Other containers, by their signature or, failing that, by their name.
    let seven_zip = [b"7z\xBC\xAF\x27\x1C".as_slice(), &garbage].concat();
    assert_eq!(problem(&f.file("renamed.zip", &seven_zip)), ArchiveProblem::Format("7z"));
    assert_eq!(problem(&f.file("study.rar", &garbage)), ArchiveProblem::Format("RAR"));
    assert_eq!(problem(&f.file("study.dmg", &garbage)), ArchiveProblem::Format("DMG"));
    let mut tar = vec![0u8; 1024];
    tar[257..262].copy_from_slice(b"ustar");
    assert_eq!(problem(&f.file("study", &tar)), ArchiveProblem::Format("tar"));
    let message = scan(&f.file("study.tgz", &[0x1F, 0x8B, 8, 0]), &Progress::new()).expect_err("gzip").to_string();
    assert!(message.starts_with("study.tgz is a gzip file, which is not supported."), "{message}");

    // Any other file is what it always was: a file without images.
    let message = scan(&f.file("notes.bin", &garbage), &Progress::new()).expect_err("no images").to_string();
    assert!(message.starts_with("no DICOM images were found in"), "{message}");
}

#[test]
fn an_archive_without_images_reports_nothing_found() {
    let f = Fixture::small();
    let documents = [
        ZipEntry::new("letter/", [], ZipMethod::Stored),
        ZipEntry::new("letter/README.TXT", b"No images here".as_slice(), ZipMethod::Deflated),
        ZipEntry::new("letter/scan", vec![0x42u8; 4000], ZipMethod::Deflated),
    ];
    for (name, bytes) in [("documents.zip", zip_archive(&documents)), ("empty.zip", zip_archive(&[]))] {
        let path = f.file(name, &bytes);
        let error = scan(&path, &Progress::new()).expect_err("nothing to find");
        assert!(matches!(error, DicomError::NothingFound(_)), "{name}: {error:?}");
        assert_eq!(error.to_string(), format!("no DICOM images were found in {}", path.display()));
    }
    // The same for a disc image.
    let empty = tempfile::tempdir().expect("tempdir");
    std::fs::write(empty.path().join("README.TXT"), "No images here").expect("write");
    let image = f.file("documents.iso", &iso_image(empty.path(), IsoNames::Joliet).expect("iso"));
    assert!(matches!(scan(&image, &Progress::new()), Err(DicomError::NothingFound(_))));
}

#[test]
fn a_truncated_disc_image_opens_as_far_as_it_goes() {
    let f = Fixture::small();
    let iso = iso_image(&f.folder, IsoNames::Level1).expect("iso");
    // The directories are at the start of the image, the files follow: three quarters of it.
    let index = open(&f.file("partial.iso", &iso[..iso.len() * 3 / 4]));
    let missing = 40 - index.stats.images;
    assert!(missing > 1 && index.stats.images > 0, "{} images", index.stats.images);
    assert_eq!(
        index.warnings,
        [
            "Part of partial.iso could not be read: the disc image is damaged or incomplete".to_owned(),
            format!("{missing} files in partial.iso could not be read and were skipped")
        ]
    );
    // What is there is read correctly.
    let folder = f.folder_index();
    let image = &index.studies[0].series[0].instances[0];
    let same = folder.studies[0].series[0].instances.iter().find(|i| i.sop_instance_uid == image.sop_instance_uid);
    assert_eq!(
        read_instance(&index.media, image).expect("read"),
        std::fs::read(&same.expect("the same image").path).expect("read")
    );
}

#[test]
fn hostile_archives_cannot_make_the_reader_take_more_than_declared() {
    let f = Fixture::small();
    let folder = f.folder_index();

    // An image that claims to be 5 GiB: refused, counted, never read.
    let entries = with_ct_images(&f, |index, entry| {
        if index == 0 {
            entry.declared_size = Some(5 << 30);
        }
    });
    let index = open(&f.file("huge.zip", &zip_archive(&entries)));
    assert_eq!(index.warnings, ["1 file in huge.zip is larger than 4 GB and was skipped"]);
    assert_eq!(index.studies[0].series[0].instances.len(), 23);
    let huge = PathBuf::from(format!("{TOP}/DICOM/ST000001/SE000001/IM000001"));
    assert!(matches!(index.media.read(&huge), Err(DicomError::Io { .. })));

    // An image that is longer than the archive says, stored and deflated: reading it
    // is an error, not more data than declared. (Whether its header can still be
    // indexed depends on how much one read returns; it is an image or an unreadable file.)
    for method in [ZipMethod::Stored, ZipMethod::Deflated] {
        let entries = with_ct_images(&f, |index, entry| {
            if index == 0 {
                (entry.method, entry.declared_size) = (method, Some(entry.data.len() as u64 - 1000));
            }
        });
        let index = open(&f.file("longer.zip", &zip_archive(&entries)));
        assert_eq!(index.stats.images + index.stats.unreadable, 40, "{method:?}");
        let error = index.media.read(&huge).expect_err("longer than declared");
        assert!(matches!(error, DicomError::Io { .. }), "{method:?}: {error}");
        // The message names the archive and the file in it.
        let shown = Path::new("longer.zip").join(&huge);
        assert!(error.to_string().starts_with(&format!("cannot read {}: ", shown.display())), "{method:?}: {error}");
    }
    // And one that is shorter than it says.
    let entries = with_ct_images(&f, |index, entry| {
        if index == 0 {
            entry.declared_size = Some(entry.data.len() as u64 + 1_000_000);
        }
    });
    let index = open(&f.file("shorter.zip", &zip_archive(&entries)));
    assert!(index.media.read(&huge).is_err());
    let other = &index.studies[0].series[0].instances[5];
    assert!(read_instance(&index.media, other).is_ok(), "the other images are not affected");

    // Names that would leave the archive, were they used as paths, are not entries at all.
    let image = std::fs::read(&f.written.series[0].files[0]).expect("read");
    let mut entries = f.entries(|path| format!("{TOP}/{path}"));
    entries.retain(|e| !e.name.ends_with("DICOMDIR"));
    entries.push(ZipEntry::new("../../outside/IM000001", image.clone(), ZipMethod::Stored));
    entries.push(ZipEntry::new("/tmp/absolute/IM000002", image.clone(), ZipMethod::Stored));
    entries.push(ZipEntry::new(format!("{TOP}/DICOM/../../IM000003"), image, ZipMethod::Stored));
    let index = open(&f.file("climbing.zip", &zip_archive(&entries)));
    // The two that climb are no entries; the absolute name is one below the root of
    // the archive (a second copy of an image that is there already).
    assert_eq!((index.stats.files, index.stats.images, index.stats.duplicates), (41, 40, 1));
    let counts = |index: &StudyIndex| index.studies[0].series.iter().map(|s| s.instances.len()).collect::<Vec<_>>();
    assert_eq!(counts(&index), counts(&folder));
    assert!(paths(&index).iter().all(|path| path.starts_with(TOP)), "{:?}", paths(&index));
}

#[test]
fn a_file_in_several_extents_is_read_from_all_of_them() {
    let f = Fixture::small();
    let folder = f.folder_index();
    // IM000003 of the CT and of the MR are recorded in two extents, the second part first.
    let options = IsoOptions { names: IsoNames::Joliet, in_two_extents: Some("IM000003".to_owned()) };
    let index = open(&f.file("split.iso", &iso_image_with(&f.folder, &options).expect("iso")));
    assert_eq!(described(&index), described(&folder));
    for (series, expected) in index.studies[0].series.iter().zip(&folder.studies[0].series) {
        for (header, expected_header) in series.instances.iter().zip(&expected.instances) {
            let bytes = read_instance(&index.media, header).expect("read from the image");
            assert_eq!(bytes, std::fs::read(&expected_header.path).expect("read"), "{}", header.path.display());
        }
    }
    let ct = load_series(&index.media, &index.studies[0].series[0], &Progress::new()).expect("decode CT");
    let reference = load_series(&folder.media, &folder.studies[0].series[0], &Progress::new()).expect("decode CT");
    assert!((0..24).all(|image| ct.frame(image) == reference.frame(image)));
}

#[test]
fn an_archive_that_changes_after_opening_is_not_read_with_stale_offsets() {
    let f = Fixture::small();
    let zip = f.zip();
    let index = open(&zip);
    let image = &index.studies[0].series[0].instances[0];
    assert!(read_instance(&index.media, image).is_ok());
    // Another download replaces the file while the study is open.
    let mut other = zip_folder(&f.folder, "OTHER").expect("zip");
    other.extend_from_slice(&[0; 64]);
    std::fs::write(&zip, other).expect("replace");
    let error = read_instance(&index.media, image).expect_err("the archive has changed");
    assert!(error.to_string().contains("has changed since it was opened"), "{error}");
}

#[test]
fn cancellation_stops_scanning_an_archive() {
    let f = fixture(&PhantomOptions { write_dicomdir: false, ..PhantomOptions::small() });
    for archive in [f.zip(), f.iso(IsoNames::Level1)] {
        let progress = Progress::new();
        progress.cancel();
        assert!(matches!(scan(&archive, &progress), Err(DicomError::Cancelled)));
    }
}

/// Indexing, loading, previews and the inspector all read at the same time.
#[test]
fn reading_from_many_threads_at_once_gives_the_same_as_reading_alone() {
    let f = Fixture::small();
    let folder = f.folder_index();
    let expected: Vec<Vec<u8>> =
        folder.studies[0].series[0].instances.iter().map(|i| std::fs::read(&i.path).expect("read")).collect();
    for archive in [f.zip(), f.iso(IsoNames::Joliet)] {
        let index = open(&archive);
        let ct = &index.studies[0].series[0];
        std::thread::scope(|scope| {
            // The loader: every image of the CT, three times over.
            for _ in 0..3 {
                scope.spawn(|| {
                    for (header, bytes) in ct.instances.iter().zip(&expected) {
                        assert_eq!(&read_instance(&index.media, header).expect("read"), bytes);
                    }
                });
            }
            // Header indexing on the rayon pool.
            scope.spawn(|| assert_eq!(described(&open(&archive)), described(&folder)));
            scope.spawn(|| assert_eq!(described(&open(&archive)), described(&folder)));
            // Previews of every series, and the inspector.
            scope.spawn(|| {
                for series in &index.studies[0].series {
                    thumbnail(&index.media, series, 64).expect("thumbnail");
                }
            });
            scope.spawn(|| {
                for header in &ct.instances {
                    let metadata = read_metadata(&index.media, &header.path).expect("metadata");
                    assert!(metadata.attribute_count() > 40);
                }
            });
            // A whole series decoded in parallel.
            scope.spawn(|| assert_eq!(load_series(&index.media, ct, &Progress::new()).expect("decode").len(), 24));
        });
    }
}

// ---------------------------------------------------------------------------
// Archives and images made by the tools of the operating system

#[allow(clippy::print_stderr)]
fn skipped(tool: &str) {
    eprintln!("skipped: `{tool}` is not available on this machine");
}

/// Runs `tool` with `args`; `false` if the tool is not installed.
fn run(tool: &str, args: &[&std::ffi::OsStr], cwd: &Path) -> bool {
    match Command::new(tool).args(args).current_dir(cwd).output() {
        Ok(output) => {
            assert!(output.status.success(), "{tool} failed: {}", String::from_utf8_lossy(&output.stderr));
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => panic!("{tool} could not be started: {e}"),
    }
}

/// The archive `made` of the fixture opens as the folder does, pixels included.
fn assert_opens_like_the_folder(f: &Fixture, made: &Path, kind: MediaKind) {
    let folder = f.folder_index();
    let index = open(made);
    assert_eq!(index.media.kind(), kind, "{}", made.display());
    assert!(matches!(index.source, DiscoverySource::Dicomdir(_)), "{}", made.display());
    assert_eq!(described(&index), described(&folder), "{}", made.display());
    for (series, expected) in index.studies[0].series.iter().zip(&folder.studies[0].series) {
        let loaded = load_series(&index.media, series, &Progress::new()).expect("decode from the archive");
        let reference = load_series(&folder.media, expected, &Progress::new()).expect("decode from the folder");
        for image in 0..reference.len() {
            assert_eq!(loaded.frame(image), reference.frame(image), "{}: {}", made.display(), series.title());
        }
    }
}

#[test]
fn archive_made_by_ditto_opens_like_the_folder() {
    let f = fixture(&PhantomOptions { ct_size: 128, ..PhantomOptions::small() });
    // A file with an extended attribute, as downloaded files have: ditto then stores
    // its resource fork as a hidden `._DICOMDIR` beside it or, as the Finder's
    // "Compress" does, in a `__MACOSX` folder.
    let dicomdir = f.folder.join("DICOMDIR");
    let attribute = ["-w", "com.summrise.test", "1"].map(std::ffi::OsStr::new);
    let with_attribute = run("xattr", &[&attribute[..], &[dicomdir.as_os_str()]].concat(), f.dir.path());
    for (name, options, clutter) in [
        ("ditto.zip", &["-c", "-k", "--keepParent"][..], b"CD/._DICOMDIR".as_slice()),
        ("finder.zip", &["-c", "-k", "--keepParent", "--sequesterRsrc"][..], b"__MACOSX/CD/._DICOMDIR".as_slice()),
    ] {
        let made = f.dir.path().join(name);
        let options: Vec<&std::ffi::OsStr> = options.iter().map(std::ffi::OsStr::new).collect();
        if !run("ditto", &[&options[..], &[f.folder.as_os_str(), made.as_os_str()]].concat(), f.dir.path()) {
            return skipped("ditto");
        }
        assert_opens_like_the_folder(&f, &made, MediaKind::Zip);
        assert!(paths(&open(&made))[0].starts_with("CD/DICOM"), "{name}: the parent folder is kept");
        if with_attribute {
            let bytes = std::fs::read(&made).expect("read");
            assert!(
                bytes.windows(clutter.len()).any(|w| w == clutter),
                "{name}: the clutter of a Mac is in the archive"
            );
        }
    }
}

#[test]
fn archive_made_by_zip_opens_like_the_folder() {
    let f = fixture(&PhantomOptions { ct_size: 128, ..PhantomOptions::small() });
    let made = f.dir.path().join("zip.zip");
    let args = ["-r", "-q"].map(std::ffi::OsStr::new);
    let args = [&args[..], &[made.as_os_str(), std::ffi::OsStr::new("CD")]].concat();
    if !run("zip", &args, f.dir.path()) {
        return skipped("zip");
    }
    assert_opens_like_the_folder(&f, &made, MediaKind::Zip);
}

#[test]
fn image_made_by_hdiutil_opens_like_the_folder() {
    let f = fixture(&PhantomOptions { ct_size: 128, ..PhantomOptions::small() });
    // ISO 9660 with Joliet names, ISO 9660 alone, and both together with UDF (a "bridge" disc).
    for (name, file_systems) in [
        ("joliet.iso", &["-iso", "-joliet"][..]),
        ("plain.iso", &["-iso"][..]),
        ("bridge.iso", &["-iso", "-joliet", "-udf"][..]),
    ] {
        let made = f.dir.path().join(name);
        let args: Vec<&std::ffi::OsStr> = ["makehybrid", "-quiet"]
            .iter()
            .chain(file_systems)
            .map(std::ffi::OsStr::new)
            .chain([std::ffi::OsStr::new("-o"), made.as_os_str(), f.folder.as_os_str()])
            .collect();
        if !run("hdiutil", &args, f.dir.path()) {
            return skipped("hdiutil");
        }
        assert_opens_like_the_folder(&f, &made, MediaKind::Iso);
    }
    // UDF alone is told apart and explained.
    let made = f.dir.path().join("udf.iso");
    let args = ["makehybrid", "-quiet", "-udf", "-o"].map(std::ffi::OsStr::new);
    assert!(run("hdiutil", &[&args[..], &[made.as_os_str(), f.folder.as_os_str()]].concat(), f.dir.path()));
    assert_eq!(problem(&made), ArchiveProblem::UdfOnly);
}

#[test]
fn image_made_by_mkisofs_opens_like_the_folder() {
    let f = fixture(&PhantomOptions { ct_size: 128, ..PhantomOptions::small() });
    let made = f.dir.path().join("mkisofs.iso");
    let args = ["-quiet", "-J", "-o"].map(std::ffi::OsStr::new);
    let args = [&args[..], &[made.as_os_str(), f.folder.as_os_str()]].concat();
    // The same tool under the names it has on different systems.
    if !["mkisofs", "genisoimage", "xorrisofs"].iter().any(|tool| run(tool, &args, f.dir.path())) {
        return skipped("mkisofs");
    }
    assert_opens_like_the_folder(&f, &made, MediaKind::Iso);
}
