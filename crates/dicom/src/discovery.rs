//! Finding DICOM files on removable media or in a folder.
//!
//! Patient CDs follow DICOM PS3.10/PS3.12: a `DICOMDIR` index at the root
//! lists every image file. When one is present it is authoritative (the
//! disc's viewer software, autorun files etc. are never touched). Otherwise
//! the folder is scanned and files are recognised by the `DICM` magic.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use dicom_dictionary_std::tags;
use log::{debug, warn};
use walkdir::WalkDir;

use crate::attrs;
use crate::error::{DicomError, Result};
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
}

/// File extensions that are never DICOM (viewer software, documents…).
const SKIP_EXTENSIONS: &[&str] = &[
    "exe", "dll", "msi", "bat", "cmd", "com", "sys", "inf", "ini", "cfg", "txt", "htm", "html", "css", "js", "json",
    "xml", "xsl", "pdf", "png", "jpg", "jpeg", "gif", "bmp", "ico", "icns", "zip", "cab", "jar", "class", "app",
    "dylib", "so", "plist", "lnk", "url", "rtf", "doc", "docx", "chm", "hlp", "manifest", "config", "ttf", "otf",
    "wav", "mp3", "mp4", "avi",
];

const DICOMDIR_SEARCH_DEPTH: usize = 3;

/// Lists the DICOM files to index below `root` (or `root` itself if it is a file).
pub fn discover(root: &Path, progress: &Progress) -> Result<Discovery> {
    let meta = fs::metadata(root).map_err(|source| DicomError::Io { path: root.to_owned(), source })?;
    if meta.is_file() {
        if is_dicomdir_name(root) {
            return discover_from_dicomdirs(&[root.to_path_buf()], progress)
                .map(|d| d.unwrap_or_else(|| empty(DiscoverySource::FolderScan)));
        }
        return Ok(Discovery { files: vec![root.to_owned()], source: DiscoverySource::SingleFile, warnings: vec![] });
    }

    progress.begin("Looking for DICOMDIR", 0);
    let dicomdirs: Vec<PathBuf> = WalkDir::new(root)
        .max_depth(DICOMDIR_SEARCH_DEPTH)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e.path()))
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file() && is_dicomdir_name(e.path()))
        .map(walkdir::DirEntry::into_path)
        .collect();
    if !dicomdirs.is_empty()
        && let Some(found) = discover_from_dicomdirs(&dicomdirs, progress)?
    {
        return Ok(found);
    }
    scan_folder(root, progress)
}

fn empty(source: DiscoverySource) -> Discovery {
    Discovery { files: Vec::new(), source, warnings: Vec::new() }
}

fn is_dicomdir_name(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy())
        .is_some_and(|n| n.eq_ignore_ascii_case("DICOMDIR") || n.eq_ignore_ascii_case("DICOMDIR;1"))
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.') && n != ".")
}

/// Uses DICOMDIR indexes. Returns `None` when they are unusable so the
/// caller falls back to a folder scan.
fn discover_from_dicomdirs(dicomdirs: &[PathBuf], progress: &Progress) -> Result<Option<Discovery>> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut warnings = Vec::new();
    let mut used = Vec::new();
    for dicomdir in dicomdirs {
        progress.begin("Reading DICOMDIR", 0);
        match referenced_files(dicomdir) {
            Ok((resolved, missing)) => {
                let total = resolved.len() + missing;
                if total == 0 {
                    warnings.push(format!("{} lists no images", dicomdir.display()));
                    continue;
                }
                if missing * 10 > total {
                    warn!("DICOMDIR {} references {missing}/{total} missing files", dicomdir.display());
                    warnings.push(format!(
                        "DICOMDIR references {missing} of {total} files that are missing; the folder was scanned instead"
                    ));
                    return Ok(None);
                }
                if missing > 0 {
                    warnings.push(format!("{missing} files listed in DICOMDIR are missing"));
                }
                for f in resolved {
                    if seen.insert(f.clone()) {
                        files.push(f);
                    }
                }
                used.push(dicomdir.clone());
            }
            Err(e) => {
                warn!("ignoring unreadable DICOMDIR {}: {e}", dicomdir.display());
                warnings.push(format!("DICOMDIR could not be read ({e}); the folder was scanned instead"));
                return Ok(None);
            }
        }
    }
    if files.is_empty() {
        return Ok(None);
    }
    Ok(Some(Discovery { files, source: DiscoverySource::Dicomdir(used), warnings }))
}

/// Parses a DICOMDIR and resolves its Referenced File IDs relative to the
/// DICOMDIR's folder. Returns (existing files, number of missing files).
pub fn referenced_files(dicomdir: &Path) -> Result<(Vec<PathBuf>, usize)> {
    let obj = dicom_object::open_file(dicomdir)
        .map_err(|e| DicomError::Parse { path: dicomdir.to_owned(), reason: e.to_string() })?;
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
        match resolver.resolve(base, &components) {
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
    fn resolve(&mut self, base: &Path, components: &[String]) -> Option<PathBuf> {
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

fn normalise_name(name: &str) -> String {
    name.trim().trim_end_matches(";1").trim_end_matches('.').to_ascii_lowercase()
}

/// Recursive scan recognising DICOM Part 10 files by their `DICM` magic.
fn scan_folder(root: &Path, progress: &Progress) -> Result<Discovery> {
    progress.begin("Scanning folder", 0);
    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e.path()))
        .filter_map(std::result::Result::ok)
    {
        if progress.is_cancelled() {
            return Err(DicomError::Cancelled);
        }
        if !entry.file_type().is_file() || is_dicomdir_name(entry.path()) {
            continue;
        }
        let path = entry.path();
        let skip = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .is_some_and(|ext| SKIP_EXTENSIONS.contains(&ext.as_str()));
        if !skip && has_dicm_magic(path) {
            files.push(path.to_path_buf());
            progress.advance();
        }
    }
    Ok(Discovery { files, source: DiscoverySource::FolderScan, warnings: Vec::new() })
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
