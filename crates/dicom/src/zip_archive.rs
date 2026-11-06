//! ZIP archives: the directory through the `zip` crate, entries read into
//! memory when they are needed. See [`crate::media`].
//!
//! Stored, deflate and deflate64 entries are read (Windows writes deflate64
//! for big files). Encrypted entries and other compression methods are listed
//! with the reason they cannot be read. Nothing is ever extracted.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek};

use zip::read::ZipArchiveMetadata;
use zip::{SUPPORTED_COMPRESSION_METHODS, ZipArchive, ZipReadOptions};

use crate::media::{ArchiveProblem, Entry, Listing, Location, MAX_ENTRIES, entry_path};

/// Reads the central directory of the archive in `file`.
pub(crate) fn list(file: File) -> io::Result<Listing> {
    let directory = ZipArchive::new(BufReader::new(file))?.metadata();
    if directory.len() > MAX_ENTRIES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "the archive lists too many files"));
    }
    let mut entries = Vec::with_capacity(directory.len());
    for index in 0..directory.len() {
        let Ok(entry) = directory.entry(index) else { continue };
        // Folders are implied by the paths of the files; links are never followed.
        if entry.is_dir() || entry.is_symlink() {
            continue;
        }
        let Ok(name) = entry.name() else { continue };
        // Archives made on Windows may separate folders with a backslash.
        let Some(path) = entry_path(name.split(['/', '\\'])) else { continue };
        let method = entry.compression();
        let problem = if entry.encrypted() {
            Some(ArchiveProblem::PasswordProtected)
        } else if !SUPPORTED_COMPRESSION_METHODS.contains(&method) {
            Some(ArchiveProblem::Compression(method.to_string()))
        } else {
            None
        };
        entries.push(Entry::new(path, entry.size(), Location::Zip(index), problem));
    }
    Ok(Listing { entries, zip: Some(directory), udf: false, damaged: false })
}

/// Lets `read` read entry `index` of the archive in `file` from its start.
/// The entry is decompressed as far as `read` reads and no further; the
/// stream fails rather than yield more than the declared size, and verifies
/// the checksum when it is read to its end.
pub(crate) fn with_reader<T>(
    directory: &ZipArchiveMetadata,
    index: usize,
    file: &mut (impl Read + Seek),
    read: impl FnOnce(&mut dyn Read) -> T,
) -> io::Result<T> {
    let mut entry = directory.entry(index)?.with_reader(file, ZipReadOptions::new())?;
    Ok(read(&mut entry))
}
