//! Finding DICOM files on removable media or in a folder.
//!
//! Patient CDs follow DICOM PS3.10/PS3.12: a `DICOMDIR` index at the root
//! lists every image file. When one is present it is authoritative (the
//! disc's viewer software, autorun files etc. are never touched). Otherwise
//! the folder is scanned and files are recognised by the `DICM` magic.
//!
//! A ZIP archive or a disc image of such a disc is searched in the same way,
//! in memory (see [`crate::media`]).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;
use log::{debug, warn};
use summrise_i18n::{tr, tr_n};
use walkdir::WalkDir;

use crate::attrs;
use crate::error::{DicomError, Result};
use crate::media::{ArchiveProblem, Media};
use crate::progress::Progress;

/// How the file list was obtained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoverySource {
    /// Files referenced by one or more DICOMDIR files.
    Dicomdir(Vec<PathBuf>),
    /// Recursive folder scan.
    FolderScan,
    /// A single file was opened.
    SingleFile,
}

#[derive(Debug, Clone)]
pub struct Discovery {
    pub files: Vec<PathBuf>,
    pub source: DiscoverySource,
    pub warnings: Vec<String>,
    /// Where the files are read from. For an archive or a disc image,
    /// `files` are paths inside it.
    pub media: Media,
    /// Why an archive may hold images that are not among `files`: the reason
    /// to give if nothing can be opened.
    pub(crate) blocker: Option<ArchiveProblem>,
}

/// File extensions that are never DICOM (viewer software, documents…).
const SKIP_EXTENSIONS: &[&str] = &[
    "exe", "dll", "msi", "bat", "cmd", "com", "sys", "inf", "ini", "cfg", "txt", "htm", "html", "css", "js", "json",
    "xml", "xsl", "pdf", "png", "jpg", "jpeg", "gif", "bmp", "ico", "icns", "zip", "cab", "jar", "class", "app",
    "dylib", "so", "plist", "lnk", "url", "rtf", "doc", "docx", "chm", "hlp", "manifest", "config", "ttf", "otf",
    "wav", "mp3", "mp4", "avi",
];

const DICOMDIR_SEARCH_DEPTH: usize = 3;

/// Lists the DICOM files to index below `root` (or `root` itself if it is a
/// file). A ZIP archive or an ISO 9660 disc image is searched like the folder
/// it was made from, without being unpacked.
pub fn discover(root: &Path, progress: &Progress) -> Result<Discovery> {
    let meta = fs::metadata(root).map_err(|source| DicomError::Io { path: root.to_owned(), source })?;
    if meta.is_file() {
        if is_dicomdir_name(root) {
            return discover_from_dicomdirs(&Media::folder(), &[root.to_path_buf()], progress).map(|d| match d {
                FromDicomdirs::Found(found) => found,
                FromDicomdirs::Unusable(warnings) => Discovery { warnings, ..empty(DiscoverySource::FolderScan) },
            });
        }
        if let Some(archive) = Media::open_archive(root)? {
            // The root of an archive is the empty path.
            return discover_in(archive, Path::new(""), progress);
        }
        return Ok(Discovery { files: vec![root.to_owned()], ..empty(DiscoverySource::SingleFile) });
    }
    discover_in(Media::folder(), root, progress)
}

/// Searches a folder, or an archive from its root: DICOMDIR first, a scan as
/// the fallback.
fn discover_in(media: Media, root: &Path, progress: &Progress) -> Result<Discovery> {
    progress.begin(tr!("Looking for DICOMDIR"), 0);
    let dicomdirs: Vec<PathBuf> =
        files_below(&media, root, DICOMDIR_SEARCH_DEPTH).filter(|path| is_dicomdir_name(path)).collect();
    let from_dicomdirs = if dicomdirs.is_empty() {
        FromDicomdirs::Unusable(Vec::new())
    } else {
        discover_from_dicomdirs(&media, &dicomdirs, progress)?
    };
    // Why files that are wanted cannot be read from the archive.
    let mut skipped = Vec::new();
    let mut discovery = match from_dicomdirs {
        FromDicomdirs::Found(mut found) => {
            if let Some(archive) = media.archive() {
                found.files.retain(|file| match archive.problem(file) {
                    Some(problem) => {
                        skipped.push(problem.clone());
                        false
                    }
                    None => true,
                });
            }
            found
        }
        FromDicomdirs::Unusable(reasons) => {
            let mut scanned = scan_folder(&media, root, progress, &mut skipped)?;
            // Say why the DICOMDIR was not used, before what the scan found.
            scanned.warnings.splice(0..0, reasons);
            scanned
        }
    };
    if let Some(archive) = media.archive() {
        let report = archive.report(&skipped);
        discovery.warnings.extend(report.warnings);
        discovery.blocker = report.blocker;
    }
    discovery.media = media;
    Ok(discovery)
}

/// The files below `root`, at most `max_depth` levels down and without hidden
/// entries: of a folder, or of an archive (whose root is the empty path).
fn files_below<'a>(media: &'a Media, root: &Path, max_depth: usize) -> Box<dyn Iterator<Item = PathBuf> + 'a> {
    match media.archive() {
        Some(archive) => Box::new(
            archive.files().filter(move |entry| entry.depth() <= max_depth).map(|entry| entry.path().to_path_buf()),
        ),
        None => Box::new(
            WalkDir::new(root)
                .max_depth(max_depth)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| e.depth() == 0 || !is_hidden(e.path()))
                .filter_map(std::result::Result::ok)
                .filter(|e| e.file_type().is_file())
                .map(walkdir::DirEntry::into_path),
        ),
    }
}

fn empty(source: DiscoverySource) -> Discovery {
    Discovery { files: Vec::new(), source, warnings: Vec::new(), media: Media::folder(), blocker: None }
}

fn is_dicomdir_name(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy())
        .is_some_and(|n| n.eq_ignore_ascii_case("DICOMDIR") || n.eq_ignore_ascii_case("DICOMDIR;1"))
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.') && n != ".")
}

/// What the DICOMDIR indexes of a disc gave.
enum FromDicomdirs {
    /// The files they list.
    Found(Discovery),
    /// They are unusable and the folder must be scanned instead; the warnings
    /// say why, for the user.
    Unusable(Vec<String>),
}

/// Uses DICOMDIR indexes, or says why they are unusable so that the caller
/// falls back to a folder scan.
fn discover_from_dicomdirs(media: &Media, dicomdirs: &[PathBuf], progress: &Progress) -> Result<FromDicomdirs> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut warnings = Vec::new();
    let mut used = Vec::new();
    for dicomdir in dicomdirs {
        progress.begin(tr!("Reading DICOMDIR"), 0);
        // The names of the entries of an archive can identify the patient: the
        // log names the archive instead.
        let logged = media.archive_file().unwrap_or(dicomdir);
        match referenced_files(media, dicomdir) {
            Ok((resolved, missing)) => {
                let total = resolved.len() + missing;
                if total == 0 {
                    warnings.push(tr!("{path} lists no images", path = media.display_path(dicomdir).display()));
                    continue;
                }
                if missing * 10 > total {
                    warn!("DICOMDIR {} references {missing}/{total} missing files", logged.display());
                    warnings.push(tr_n!(
                        "{n} of the {total} files that DICOMDIR lists is missing; the folder was scanned instead",
                        "{n} of the {total} files that DICOMDIR lists are missing; the folder was scanned instead",
                        missing,
                        total = total
                    ));
                    return Ok(FromDicomdirs::Unusable(warnings));
                }
                if missing > 0 {
                    warnings.push(tr_n!(
                        "{n} file listed in DICOMDIR is missing",
                        "{n} files listed in DICOMDIR are missing",
                        missing
                    ));
                }
                for f in resolved {
                    if seen.insert(f.clone()) {
                        files.push(f);
                    }
                }
                used.push(dicomdir.clone());
            }
            Err(e) => {
                if media.archive().is_some() {
                    warn!("ignoring an unreadable DICOMDIR in {}", logged.display());
                    debug!("unreadable DICOMDIR: {e}");
                } else {
                    warn!("ignoring unreadable DICOMDIR {}: {e}", dicomdir.display());
                }
                warnings.push(tr!("DICOMDIR could not be read ({reason}); the folder was scanned instead", reason = e));
                return Ok(FromDicomdirs::Unusable(warnings));
            }
        }
    }
    if files.is_empty() {
        return Ok(FromDicomdirs::Unusable(warnings));
    }
    Ok(FromDicomdirs::Found(Discovery { files, warnings, ..empty(DiscoverySource::Dicomdir(used)) }))
}

/// Parses a DICOMDIR and resolves its Referenced File IDs relative to the
/// DICOMDIR's folder. Returns (existing files, number of missing files).
pub fn referenced_files(media: &Media, dicomdir: &Path) -> Result<(Vec<PathBuf>, usize)> {
    let obj = media
        .open_object(dicomdir, None)
        .map_err(|reason| DicomError::Parse { path: media.display_path(dicomdir), reason })?;
    let base = dicomdir.parent().unwrap_or(Path::new("."));
    let records = obj.get(tags::DIRECTORY_RECORD_SEQUENCE).and_then(|e| e.items()).unwrap_or(&[]);
    let mut resolver = CaseInsensitiveResolver::default();
    let mut found = Vec::new();
    let mut missing = 0;
    for record in records {
        // Record In-use Flag 0x0000 marks deleted records.
        if attrs::uint(record, tags::RECORD_IN_USE_FLAG) == Some(0) {
            continue;
        }
        let components = attrs::texts(record, tags::REFERENCED_FILE_ID);
        if components.is_empty() {
            continue;
        }
        let kind = attrs::text(record, tags::DIRECTORY_RECORD_TYPE).unwrap_or_default();
        // Only image-bearing records; reports, presentation states etc. are skipped.
        if !matches!(kind.as_str(), "IMAGE" | "" | "RT DOSE" | "STEREOMETRIC" | "WAVEFORM") {
            debug!("skipping DICOMDIR record type {kind}");
            continue;
        }
        match resolver.resolve(media, base, &components) {
            Some(path) => found.push(path),
            None => missing += 1,
        }
    }
    Ok((found, missing))
}

/// Resolves DICOMDIR paths on case-sensitive file systems (ISO 9660 discs
/// may be mounted with lower-case names or with `;1` version suffixes).
#[derive(Default)]
struct CaseInsensitiveResolver {
    listings: HashMap<PathBuf, HashMap<String, PathBuf>>,
}

impl CaseInsensitiveResolver {
    fn resolve(&mut self, media: &Media, base: &Path, components: &[String]) -> Option<PathBuf> {
        // An archive has its directory in memory, with the names normalised in the same way.
        if let Some(archive) = media.archive() {
            return archive.resolve(base, components);
        }
        let direct: PathBuf = components.iter().fold(base.to_path_buf(), |p, c| p.join(c));
        if direct.is_file() {
            return Some(direct);
        }
        let mut current = base.to_path_buf();
        for component in components {
            let listing = self.listings.entry(current.clone()).or_insert_with(|| {
                fs::read_dir(&current)
                    .map(|rd| {
                        rd.filter_map(std::result::Result::ok)
                            .map(|e| (normalise_name(&e.file_name().to_string_lossy()), e.path()))
                            .collect()
                    })
                    .unwrap_or_default()
            });
            current = listing.get(&normalise_name(component))?.clone();
        }
        current.is_file().then_some(current)
    }
}

pub(crate) fn normalise_name(name: &str) -> String {
    name.trim().trim_end_matches(";1").trim_end_matches('.').to_ascii_lowercase()
}

/// Recursive scan recognising DICOM Part 10 files by their `DICM` magic.
/// Why files of an archive that might be images cannot be read is noted in
/// `skipped`.
fn scan_folder(
    media: &Media,
    root: &Path,
    progress: &Progress,
    skipped: &mut Vec<ArchiveProblem>,
) -> Result<Discovery> {
    progress.begin(if media.archive().is_some() { tr!("Scanning the archive") } else { tr!("Scanning folder") }, 0);
    let mut files = Vec::new();
    for path in files_below(media, root, usize::MAX) {
        if progress.is_cancelled() {
            return Err(DicomError::Cancelled);
        }
        if is_dicomdir_name(&path) {
            continue;
        }
        let skip = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .is_some_and(|ext| SKIP_EXTENSIONS.contains(&ext.as_str()));
        if skip {
            continue;
        }
        let is_dicom = match media.archive() {
            None => has_dicm_magic(&path),
            Some(archive) => match archive.problem(&path) {
                Some(problem) => {
                    skipped.push(problem.clone());
                    false
                }
                None => archive.has_dicm_magic(&path),
            },
        };
        if is_dicom {
            files.push(path);
            progress.advance();
        }
    }
    Ok(Discovery { files, ..empty(DiscoverySource::FolderScan) })
}

/// Checks for the 128-byte preamble followed by `DICM` (PS3.10 7.1).
pub fn has_dicm_magic(path: &Path) -> bool {
    let mut buf = [0u8; 132];
    fs::File::open(path).and_then(|mut f| f.read_exact(&mut buf)).is_ok() && &buf[128..132] == b"DICM"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_iso9660_names() {
        assert_eq!(normalise_name("IM000001;1"), "im000001");
        assert_eq!(normalise_name("DICOM"), "dicom");
        assert_eq!(normalise_name("FILE."), "file");
    }

    #[test]
    fn recognises_dicomdir_names() {
        assert!(is_dicomdir_name(Path::new("/media/cd/DICOMDIR")));
        assert!(is_dicomdir_name(Path::new("/media/cd/dicomdir")));
        assert!(!is_dicomdir_name(Path::new("/media/cd/DICOMDIR.bak")));
    }
}
