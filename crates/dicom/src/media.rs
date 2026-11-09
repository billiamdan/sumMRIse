//! Where the files of a study are read from.
//!
//! A study is opened from a folder (a mounted disc, a copy of one, a single
//! file), from a ZIP archive or from an ISO 9660 disc image. [`Media`] is the
//! one way to the files for everything that reads them — discovery, DICOMDIR
//! parsing and resolution, header indexing, pixel data, thumbnails and the
//! metadata inspector — and the index keeps the media it was built from
//! ([`crate::StudyIndex::media`]), so that every later read goes the same way.
//!
//! * **Folder**: paths are file-system paths, and files are opened as they
//!   always were.
//! * **ZIP archive, disc image**: paths are the paths of the entries inside
//!   the archive. Nothing is unpacked to disk. The directory of the archive
//!   is read once and kept in memory; an entry is read from the archive file
//!   into memory when it is needed, and only as far as it is needed (headers
//!   are parsed from a stream that ends before the pixel data).
//!
//! Reading is safe from any number of threads at once and cannot deadlock:
//! the directory never changes after it was read, and every read opens its
//! own handle on the archive file, so no lock is held while reading.
//!
//! Archives are untrusted input. An entry is never read beyond its declared
//! size, never more memory than that size is reserved for it, entries larger
//! than [`MAX_ENTRY_BYTES`] are refused, and the name of an entry is only
//! ever a key into the directory in memory, never a path on the file system.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use dicom_core::Tag;
use dicom_object::file::ReadPreamble;
use dicom_object::{DefaultDicomObject, OpenFileOptions};
use summrise_i18n::{tr, tr_n};
use zip::read::ZipArchiveMetadata;

use crate::discovery::normalise_name;
use crate::error::{DicomError, Result};
use crate::{iso9660, zip_archive};

/// Largest entry read from an archive or a disc image: 4 GiB.
///
/// Enhanced multi-frame objects can exceed 1 GB, so the limit is far above
/// what patient media hold, yet it bounds what a hostile archive can make the
/// application reserve for one entry. It is also the size up to which a file
/// lies in a single extent of an ISO 9660 image.
pub const MAX_ENTRY_BYTES: u64 = 4 << 30;

/// Most files listed from one archive. Patient media hold a few thousand; the
/// limit bounds the memory a hostile directory can take.
pub(crate) const MAX_ENTRIES: usize = 1_000_000;

/// Extensions of files that are archives or disc images themselves. Inside an
/// archive they are not opened, only reported.
const NESTED_EXTENSIONS: &[&str] = &["zip", "iso", "7z", "rar", "tar", "tgz", "gz"];

/// What kind of media a study is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    /// Files on the local file system: a folder, a mounted disc, a single file.
    Folder,
    /// A ZIP archive.
    Zip,
    /// An ISO 9660 disc image.
    Iso,
}

/// Where the files of a study are read from: see the [module](self).
/// Cheap to clone.
#[derive(Clone, Default)]
pub struct Media(Option<Arc<Archive>>);

impl std::fmt::Debug for Media {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Entry names can identify a patient: they are not part of the output.
        match &self.0 {
            None => f.write_str("Folder"),
            Some(archive) => f
                .debug_struct(if archive.kind == MediaKind::Zip { "Zip" } else { "Iso" })
                .field("file", &archive.file)
                .field("entries", &archive.entries.len())
                .finish(),
        }
    }
}

impl Media {
    /// Files on the local file system: a folder, a mounted disc or a single
    /// file. Paths are file-system paths.
    pub fn folder() -> Self {
        Self(None)
    }

    /// Opens `file` as an archive if its content is a ZIP archive or an ISO
    /// 9660 disc image. The content decides, not the name: ZIP is recognised
    /// by the signature of its first header, ISO 9660 by `CD001` at byte
    /// 32769. `Ok(None)` is any other file (a DICOM file, for instance), to
    /// be read as a plain file; a container that is recognised but cannot be
    /// read is an error that says why.
    pub fn open_archive(file: &Path) -> Result<Option<Self>> {
        let io_error = |source| DicomError::Io { path: file.to_owned(), source };
        let mut handle = File::open(file).map_err(io_error)?;
        let stamp = Stamp::of(&handle).map_err(io_error)?;
        let kind = match sniff(&mut handle, stamp.len, file).map_err(io_error)? {
            Sniffed::Other => return Ok(None),
            Sniffed::Unsupported(problem) => return Err(DicomError::Archive { path: file.to_owned(), problem }),
            Sniffed::Archive(kind) => kind,
        };
        let listing = match kind {
            MediaKind::Iso => iso9660::list(&mut handle, stamp.len),
            _ => zip_archive::list(handle),
        };
        let listing = listing.map_err(|e| match e.kind() {
            // The directory is not what the format says it should be, or the file ends before it does.
            io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof | io::ErrorKind::Unsupported => {
                log::debug!("unreadable archive directory: {e}");
                DicomError::Archive { path: file.to_owned(), problem: ArchiveProblem::Damaged(kind) }
            }
            _ => io_error(e),
        })?;
        Ok(Some(Self(Some(Arc::new(Archive::new(file, kind, stamp, listing))))))
    }

    pub fn kind(&self) -> MediaKind {
        self.0.as_ref().map_or(MediaKind::Folder, |archive| archive.kind)
    }

    /// The archive or disc image the files are in; `None` for a folder.
    pub fn archive_file(&self) -> Option<&Path> {
        self.0.as_ref().map(|archive| archive.file.as_path())
    }

    /// `path` as it is shown to the user: for a folder the path itself, for
    /// an archive the name of the archive file followed by the path inside
    /// it. Not a path that can be opened.
    pub fn display_path(&self, path: &Path) -> PathBuf {
        match &self.0 {
            None => path.to_path_buf(),
            Some(archive) => Path::new(&archive.name()).join(path),
        }
    }

    /// Reads a whole file into memory.
    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        match &self.0 {
            None => std::fs::read(path).map_err(|source| DicomError::Io { path: path.to_owned(), source }),
            Some(archive) => {
                archive.read(path).map_err(|source| DicomError::Io { path: self.display_path(path), source })
            }
        }
    }

    pub(crate) fn archive(&self) -> Option<&Archive> {
        self.0.as_deref()
    }

    /// Size of the file at `path` in bytes; `None` if there is no such file.
    pub(crate) fn size(&self, path: &Path) -> Option<u64> {
        match &self.0 {
            None => std::fs::metadata(path).ok().map(|meta| meta.len()),
            Some(archive) => archive.entry(path).map(|entry| entry.size),
        }
    }

    /// Refuses a file whose sequences nest too deeply for the parser to build
    /// them (see [`crate::nesting`]), before it tries.
    fn check_nesting(&self, path: &Path, read_until: Option<Tag>) -> std::result::Result<(), String> {
        let check = |entry: &mut dyn Read| {
            let mut start = [0u8; 132];
            let filled = read_fully(entry, &mut start).map_err(|e| e.to_string())?;
            let start = &start[..filled];
            let after_preamble = if start.get(128..132) == Some(b"DICM".as_slice()) { &start[128..] } else { start };
            crate::nesting::check(after_preamble.chain(entry), read_until)
        };
        match &self.0 {
            // What cannot be opened is for the parser to report.
            None => File::open(path).map_or(Ok(()), |mut file| check(&mut file)),
            Some(archive) => archive.with_reader(path, check).unwrap_or(Ok(())),
        }
    }

    /// Parses the DICOM file at `path`, all of it or up to (not including)
    /// the attribute `read_until`. The error is the parser's reason.
    pub(crate) fn open_object(
        &self,
        path: &Path,
        read_until: Option<Tag>,
    ) -> std::result::Result<DefaultDicomObject, String> {
        self.check_nesting(path, read_until)?;
        let options = match read_until {
            Some(tag) => OpenFileOptions::new().read_until(tag),
            None => OpenFileOptions::new(),
        };
        match &self.0 {
            None => options.open_file(path).map_err(|e| e.to_string()),
            Some(archive) => {
                archive.with_reader(path, |entry| read_object(entry, options)).map_err(|e| e.to_string())?
            }
        }
    }
}

/// Parses a DICOM file from a stream as [`OpenFileOptions::open_file`] parses
/// it from a path: the file starts with the 128-byte preamble unless it
/// starts with the `DICM` code itself. (The parser decides this from what one
/// read returns, which for a stream can be less than the preamble.)
fn read_object(entry: &mut dyn Read, options: OpenFileOptions) -> std::result::Result<DefaultDicomObject, String> {
    let mut start = [0u8; 132];
    let filled = read_fully(entry, &mut start).map_err(|e| e.to_string())?;
    let start = &start[..filled];
    let preamble = if start.starts_with(b"DICM") && start.get(128..132) != Some(b"DICM".as_slice()) {
        ReadPreamble::Never
    } else {
        ReadPreamble::Always
    };
    options.read_preamble(preamble).from_reader(start.chain(entry)).map_err(|e| e.to_string())
}

/// Reads until `buf` is full or the stream ends; returns the bytes read.
pub(crate) fn read_fully(reader: &mut (impl Read + ?Sized), buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Why an archive or a disc image, or files in it, cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveProblem {
    /// Protected by a password.
    PasswordProtected,
    /// Compressed with a method other than stored, deflate or deflate64
    /// (the name of the method).
    Compression(String),
    /// Larger than [`MAX_ENTRY_BYTES`].
    TooLarge,
    /// The file ends early or its directory cannot be read.
    Damaged(MediaKind),
    /// Nothing to read but another archive or disc image (its name), which is
    /// not opened automatically.
    Nested(String),
    /// A disc image without a usable ISO 9660 file system (UDF only).
    UdfOnly,
    /// A container format that is not read at all (its name): 7z, RAR, tar…
    Format(&'static str),
}

/// [`MAX_ENTRY_BYTES`] for messages.
fn size_limit() -> String {
    format!("{} GB", MAX_ENTRY_BYTES >> 30)
}

impl ArchiveProblem {
    /// Why nothing could be opened from the archive `file`, and what to do
    /// instead, for the user.
    pub fn message(&self, file: &Path) -> String {
        let name = file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().into_owned());
        match self {
            Self::PasswordProtected => tr!(
                "{name} is protected by a password. Unpack it with the password, then open the unpacked folder.",
                name = name
            ),
            Self::Compression(method) => tr!(
                "{name} is compressed with a method that is not supported ({method}). Unpack it, then open the unpacked folder.",
                name = name,
                method = method
            ),
            Self::TooLarge => tr!(
                "The images in {name} are larger than {limit}, the most that is read from an archive. Unpack it, then open the unpacked folder.",
                name = name,
                limit = size_limit()
            ),
            Self::Damaged(MediaKind::Iso) => {
                tr!("{name} cannot be read as a disc image: it is damaged or incomplete.", name = name)
            }
            Self::Damaged(_) => tr!(
                "{name} cannot be read as a ZIP archive: it is damaged or was not downloaded completely.",
                name = name
            ),
            Self::Nested(inner) => tr!(
                "{name} contains another archive ({inner}), which is not opened automatically. Unpack {name}, then open {inner}.",
                name = name,
                inner = inner
            ),
            Self::UdfOnly => tr!(
                "{name} is a disc image in UDF format, which is not supported. Open it with your system, where it appears as a disc, then open that disc.",
                name = name
            ),
            Self::Format(format) => tr!(
                "{name} is a {format} file, which is not supported. Unpack or open it with your system, then open the resulting folder.",
                name = name,
                format = format
            ),
        }
    }

    /// The notice for `count` files that were skipped for this reason while
    /// the rest of the archive `name` was opened.
    fn warning(&self, count: usize, name: &str) -> String {
        match self {
            Self::PasswordProtected => tr_n!(
                "{n} file in {name} is protected by a password and was skipped",
                "{n} files in {name} are protected by a password and were skipped",
                count,
                name = name
            ),
            Self::Compression(method) => tr_n!(
                "{n} file in {name} is compressed with a method that is not supported ({method}) and was skipped",
                "{n} files in {name} are compressed with a method that is not supported ({method}) and were skipped",
                count,
                name = name,
                method = method
            ),
            Self::TooLarge => tr_n!(
                "{n} file in {name} is larger than {limit} and was skipped",
                "{n} files in {name} are larger than {limit} and were skipped",
                count,
                name = name,
                limit = size_limit()
            ),
            Self::Damaged(_) | Self::Nested(_) | Self::UdfOnly | Self::Format(_) => tr_n!(
                "{n} file in {name} could not be read and was skipped",
                "{n} files in {name} could not be read and were skipped",
                count,
                name = name
            ),
        }
    }

    /// Why one file with this problem cannot be read, as the reason of a
    /// read error.
    fn reason(&self) -> String {
        match self {
            Self::PasswordProtected => tr!("the file is protected by a password").to_owned(),
            Self::Compression(method) => tr!("the compression method {method} is not supported", method = method),
            Self::TooLarge => tr!("the file is larger than {limit}", limit = size_limit()),
            Self::Damaged(_) | Self::Nested(_) | Self::UdfOnly | Self::Format(_) => {
                tr!("the file could not be read").to_owned()
            }
        }
    }
}

/// What an archive could not give, once discovery has taken what it wanted.
pub(crate) struct ArchiveReport {
    /// Notices for the user when the study opens all the same.
    pub(crate) warnings: Vec<String>,
    /// The reason to give when nothing could be opened.
    pub(crate) blocker: Option<ArchiveProblem>,
}

/// One file in an archive.
#[derive(Debug)]
pub(crate) struct Entry {
    path: PathBuf,
    /// Declared size of the content in bytes.
    size: u64,
    location: Location,
    problem: Option<ArchiveProblem>,
}

/// Where the content of an entry is.
#[derive(Debug)]
pub(crate) enum Location {
    /// Index into the central directory of a ZIP archive.
    Zip(usize),
    /// Offset and size of each extent of a file in a disc image, in order.
    Extents(Vec<(u64, u64)>),
}

/// The content of a file in a disc image: its extents one after the other.
struct Extents<'a, R> {
    file: R,
    extents: std::slice::Iter<'a, (u64, u64)>,
    /// Bytes left in the extent being read.
    left: u64,
}

impl<R: Read + Seek> Read for Extents<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.left == 0 {
            let Some(&(offset, size)) = self.extents.next() else { return Ok(0) };
            self.file.seek(SeekFrom::Start(offset))?;
            self.left = size;
        }
        let wanted = buf.len().min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let read = self.file.read(&mut buf[..wanted])?;
        if read == 0 && wanted > 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, tr!("the file ends early")));
        }
        self.left -= read as u64;
        Ok(read)
    }
}

impl Entry {
    pub(crate) fn new(path: PathBuf, size: u64, location: Location, problem: Option<ArchiveProblem>) -> Self {
        let problem = problem.or((size > MAX_ENTRY_BYTES).then_some(ArchiveProblem::TooLarge));
        Self { path, size, location, problem }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Number of path components: 1 for a file at the root of the archive.
    pub(crate) fn depth(&self) -> usize {
        self.path.components().count()
    }
}

/// The path of an archive entry from the components of its name, or `None`
/// for an entry that is not listed: hidden ones (as the folder scan skips
/// them), the `__MACOSX` folder of archives made on a Mac, and names that are
/// not plain relative paths. The path is a key into the directory of the
/// archive and nothing else.
pub(crate) fn entry_path<'a>(components: impl IntoIterator<Item = &'a str>) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for component in components {
        if component.is_empty() || component == "." {
            continue;
        }
        if component.starts_with('.') || component == "__MACOSX" || component.contains(['/', '\\', '\0']) {
            return None;
        }
        // One normal component on this platform: not a drive, a root or a parent.
        let mut parts = Path::new(component).components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(part)), None) if part == OsStr::new(component) => path.push(component),
            _ => return None,
        }
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

/// What the reader of a format found in an archive.
pub(crate) struct Listing {
    pub(crate) entries: Vec<Entry>,
    /// Central directory of a ZIP archive.
    pub(crate) zip: Option<Arc<ZipArchiveMetadata>>,
    /// A disc image that also carries a UDF file system, which may hold more
    /// than its ISO 9660 one.
    pub(crate) udf: bool,
    /// Part of the directory could not be read.
    pub(crate) damaged: bool,
}

/// Size and modification time of the archive file when its directory was
/// read. The offsets in the directory are only valid for that file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    fn of(file: &File) -> io::Result<Self> {
        let meta = file.metadata()?;
        Ok(Self { len: meta.len(), modified: meta.modified().ok() })
    }
}

/// The archive file, counting the bytes read from it.
struct Counted<'a> {
    file: File,
    read: &'a AtomicU64,
}

impl Read for Counted<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.file.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

impl Seek for Counted<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}

/// The directory of a ZIP archive or a disc image, and the way to the
/// content of its entries.
pub(crate) struct Archive {
    file: PathBuf,
    kind: MediaKind,
    stamp: Stamp,
    /// The files, in the order of the archive.
    entries: Vec<Entry>,
    by_path: HashMap<PathBuf, usize>,
    /// By path with every name normalised as DICOMDIR references are.
    by_normalised: HashMap<String, usize>,
    zip: Option<Arc<ZipArchiveMetadata>>,
    /// Names of archives and disc images inside.
    nested: Vec<String>,
    udf: bool,
    damaged: bool,
    /// Bytes read from the archive file for its entries.
    read: AtomicU64,
}

/// The key of [`Archive::by_normalised`] for the names `components`.
fn normalised_key<'a>(components: impl Iterator<Item = &'a str>) -> String {
    components.map(normalise_name).collect::<Vec<_>>().join("/")
}

fn normalised_path(path: &Path) -> Vec<String> {
    path.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect()
}

impl Archive {
    fn new(file: &Path, kind: MediaKind, stamp: Stamp, listing: Listing) -> Self {
        let mut entries: Vec<Entry> = Vec::with_capacity(listing.entries.len());
        let mut by_path = HashMap::with_capacity(listing.entries.len());
        let mut by_normalised = HashMap::with_capacity(listing.entries.len());
        let mut nested = Vec::new();
        for entry in listing.entries {
            // A name that is there twice: the first entry is the file.
            if by_path.contains_key(&entry.path) {
                continue;
            }
            let extension = entry.path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
            if extension.is_some_and(|e| NESTED_EXTENSIONS.contains(&e.as_str()))
                && let Some(name) = entry.path.file_name()
            {
                nested.push(name.to_string_lossy().into_owned());
            }
            let index = entries.len();
            by_path.insert(entry.path.clone(), index);
            let names = normalised_path(&entry.path);
            by_normalised.entry(normalised_key(names.iter().map(String::as_str))).or_insert(index);
            entries.push(entry);
        }
        Self {
            file: file.to_path_buf(),
            kind,
            stamp,
            entries,
            by_path,
            by_normalised,
            zip: listing.zip,
            nested,
            udf: listing.udf,
            damaged: listing.damaged,
            read: AtomicU64::new(0),
        }
    }

    /// Name of the archive file.
    fn name(&self) -> String {
        self.file.file_name().map_or_else(|| self.file.display().to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// The files in the archive, without hidden ones, in its own order.
    pub(crate) fn files(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    fn entry(&self, path: &Path) -> Option<&Entry> {
        self.by_path.get(path).map(|&index| &self.entries[index])
    }

    /// Why the file at `path` cannot be read, if it is known that it cannot.
    pub(crate) fn problem(&self, path: &Path) -> Option<&ArchiveProblem> {
        self.entry(path)?.problem.as_ref()
    }

    /// The file a DICOMDIR means by the names `components` below the folder
    /// `base`: upper or lower case, with or without the `;1` and the dot of
    /// ISO 9660 names.
    pub(crate) fn resolve(&self, base: &Path, components: &[String]) -> Option<PathBuf> {
        let base = normalised_path(base);
        let key = normalised_key(base.iter().chain(components).map(String::as_str));
        self.by_normalised.get(&key).map(|&index| self.entries[index].path.clone())
    }

    /// Bytes read from the archive file for its entries so far.
    pub(crate) fn bytes_read(&self) -> u64 {
        self.read.load(Ordering::Relaxed)
    }

    /// Size of the archive file.
    pub(crate) fn len(&self) -> u64 {
        self.stamp.len
    }

    /// A handle of its own for one read, so that reads never wait for each
    /// other.
    fn open(&self) -> io::Result<BufReader<Counted<'_>>> {
        let file = File::open(&self.file)?;
        if Stamp::of(&file)? != self.stamp {
            return Err(io::Error::other(tr!("the file has changed since it was opened")));
        }
        Ok(BufReader::new(Counted { file, read: &self.read }))
    }

    /// Lets `read` read the file at `path` from its start, as far as it
    /// likes. The stream ends at the declared size of the entry at the
    /// latest. Nothing but what `read` asks for is read or decompressed.
    pub(crate) fn with_reader<T>(&self, path: &Path, read: impl FnOnce(&mut dyn Read) -> T) -> io::Result<T> {
        let not_found = || io::Error::new(io::ErrorKind::NotFound, tr!("there is no such file in the archive"));
        let entry = self.entry(path).ok_or_else(not_found)?;
        if let Some(problem) = &entry.problem {
            return Err(io::Error::new(io::ErrorKind::Unsupported, problem.reason()));
        }
        let mut file = self.open()?;
        match (&entry.location, &self.zip) {
            (Location::Extents(extents), _) => Ok(read(&mut Extents { file, extents: extents.iter(), left: 0 })),
            (Location::Zip(index), Some(directory)) => zip_archive::with_reader(directory, *index, &mut file, read),
            (Location::Zip(_), None) => Err(not_found()),
        }
    }

    /// Reads the whole file at `path`. Memory is reserved for its declared
    /// size and no more; a file that is shorter or longer than declared is
    /// an error.
    pub(crate) fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let size = self.entry(path).map_or(0, |entry| entry.size);
        let too_large = || io::Error::new(io::ErrorKind::OutOfMemory, tr!("there is not enough memory for the file"));
        let size = usize::try_from(size).map_err(|_| too_large())?;
        self.with_reader(path, |entry| {
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(size).map_err(|_| too_large())?;
            let mut chunk = vec![0u8; 64 << 10];
            while bytes.len() < size {
                let wanted = chunk.len().min(size - bytes.len());
                match entry.read(&mut chunk[..wanted]) {
                    Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, tr!("the file ends early"))),
                    Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            }
            // The entry has to end here. Asking for more lets a ZIP entry
            // verify its checksum and shows data beyond the declared size.
            match read_fully(entry, &mut chunk[..1])? {
                0 => Ok(bytes),
                _ => Err(io::Error::new(io::ErrorKind::InvalidData, tr!("the file is longer than the archive says"))),
            }
        })?
    }

    /// Checks for the 128-byte preamble followed by `DICM` (PS3.10 7.1).
    pub(crate) fn has_dicm_magic(&self, path: &Path) -> bool {
        if self.entry(path).is_none_or(|entry| entry.size < 132) {
            return false;
        }
        let magic = |entry: &mut dyn Read| {
            let mut buf = [0u8; 132];
            entry.read_exact(&mut buf).is_ok() && &buf[128..132] == b"DICM"
        };
        self.with_reader(path, magic).unwrap_or(false)
    }

    /// What the archive could not give. `skipped` are the problems of the
    /// files that discovery wanted but that cannot be read.
    pub(crate) fn report(&self, skipped: &[ArchiveProblem]) -> ArchiveReport {
        let name = self.name();
        let mut counts: Vec<(&ArchiveProblem, usize)> = Vec::new();
        for problem in skipped {
            match counts.iter_mut().find(|(counted, _)| *counted == problem) {
                Some((_, count)) => *count += 1,
                None => counts.push((problem, 1)),
            }
        }
        // The cause first, then what it cost.
        let mut warnings = Vec::new();
        if self.damaged {
            warnings
                .push(tr!("Part of {name} could not be read: the disc image is damaged or incomplete", name = name));
        }
        warnings.extend(counts.iter().map(|(problem, count)| problem.warning(*count, &name)));
        if !self.nested.is_empty() {
            let mut inner = self.nested.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            if self.nested.len() > 3 {
                inner.push_str(", …");
            }
            warnings.push(tr_n!(
                "{name} contains another archive, which was not opened: {inner}",
                "{name} contains {n} other archives, which were not opened: {inner}",
                self.nested.len(),
                name = name,
                inner = inner
            ));
        }
        // The most frequent reason first; of equally frequent ones the first met.
        let most_frequent = counts.iter().fold(None, |best: Option<&(&ArchiveProblem, usize)>, entry| match best {
            Some(best) if best.1 >= entry.1 => Some(best),
            _ => Some(entry),
        });
        let blocker = most_frequent
            .map(|(problem, _)| (*problem).clone())
            .or_else(|| self.damaged.then_some(ArchiveProblem::Damaged(self.kind)))
            .or_else(|| self.nested.first().map(|inner| ArchiveProblem::Nested(inner.clone())))
            .or_else(|| self.udf.then_some(ArchiveProblem::UdfOnly));
        ArchiveReport { warnings, blocker }
    }
}

/// What a file is, by its content.
enum Sniffed {
    Archive(MediaKind),
    /// A container that is recognised but not read.
    Unsupported(ArchiveProblem),
    /// Anything else, a DICOM file for instance.
    Other,
}

/// Signatures of containers that are not read, at the start of the file.
const OTHER_FORMATS: &[(&[u8], &str)] = &[
    (b"7z\xBC\xAF\x27\x1C", "7z"),
    (b"Rar!\x1A\x07", "RAR"),
    (b"\x1F\x8B", "gzip"),
    (b"BZh", "bzip2"),
    (b"\xFD7zXZ\x00", "xz"),
    (b"\x28\xB5\x2F\xFD", "Zstandard"),
];

/// Extensions of containers that are not read.
const OTHER_EXTENSIONS: &[(&str, &str)] = &[
    ("7z", "7z"),
    ("rar", "RAR"),
    ("tar", "tar"),
    ("gz", "gzip"),
    ("tgz", "gzip"),
    ("bz2", "bzip2"),
    ("xz", "xz"),
    ("zst", "Zstandard"),
    ("dmg", "DMG"),
];

/// Tells what `file` (of `len` bytes, called `path`) is. The content decides;
/// the extension only tells what a file that is nothing known was meant to be.
fn sniff(file: &mut File, len: u64, path: &Path) -> io::Result<Sniffed> {
    let mut head = [0u8; 512];
    let filled = read_fully(file, &mut head)?;
    let head = &head[..filled];
    // A DICOM file is one whatever it is called and whatever its preamble holds.
    if head.get(128..132) == Some(b"DICM".as_slice()) {
        return Ok(Sniffed::Other);
    }
    // A local file header, the end of an empty archive, or the mark of a split one.
    if [b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"].iter().any(|signature| head.starts_with(*signature)) {
        return Ok(Sniffed::Archive(MediaKind::Zip));
    }
    match iso9660::probe(file, len)? {
        iso9660::Volume::Iso9660 => return Ok(Sniffed::Archive(MediaKind::Iso)),
        iso9660::Volume::UdfOnly => return Ok(Sniffed::Unsupported(ArchiveProblem::UdfOnly)),
        iso9660::Volume::None => {}
    }
    if let Some((_, format)) = OTHER_FORMATS.iter().find(|(signature, _)| head.starts_with(signature)) {
        return Ok(Sniffed::Unsupported(ArchiveProblem::Format(format)));
    }
    if head.get(257..262) == Some(b"ustar".as_slice()) {
        return Ok(Sniffed::Unsupported(ArchiveProblem::Format("tar")));
    }
    // The trailer of a DMG disk image.
    if len >= 512 {
        let mut trailer = [0u8; 4];
        file.seek(SeekFrom::Start(len - 512))?;
        if read_fully(file, &mut trailer)? == 4 && &trailer == b"koly" {
            return Ok(Sniffed::Unsupported(ArchiveProblem::Format("DMG")));
        }
    }
    let extension = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    Ok(match extension.as_deref() {
        // A ZIP archive may begin with other data; its directory is at the end.
        Some("zip") => Sniffed::Archive(MediaKind::Zip),
        Some("iso") => Sniffed::Unsupported(ArchiveProblem::Damaged(MediaKind::Iso)),
        Some(extension) => match OTHER_EXTENSIONS.iter().find(|(known, _)| *known == extension) {
            Some((_, format)) => Sniffed::Unsupported(ArchiveProblem::Format(format)),
            None => Sniffed::Other,
        },
        None => Sniffed::Other,
    })
}

#[cfg(test)]
mod tests {
    use summrise_testkit::archive::{IsoNames, iso_image, zip_folder};
    use summrise_testkit::{PhantomOptions, write_phantom_study};

    use super::*;
    use crate::decode::load_series;
    use crate::index::scan;
    use crate::progress::Progress;

    /// Design decision 4: header indexing reads up to the pixel data, not the
    /// whole entry, from stored entries, deflated entries and extents alike.
    #[test]
    fn indexing_reads_the_headers_and_not_the_pixel_data() {
        // Four CT slices of 512 × 512 pixels: half a megabyte each, of which the header is 1.5 kB.
        let opts = PhantomOptions {
            ct_size: 512,
            ct_slices: 4,
            mr_slices: 2,
            include_localizer: false,
            include_secondary_capture: false,
            ..PhantomOptions::small()
        };
        for write_dicomdir in [true, false] {
            let dir = tempfile::tempdir().expect("tempdir");
            let folder = dir.path().join("CD");
            write_phantom_study(&folder, &PhantomOptions { write_dicomdir, ..opts.clone() }).expect("fixture");
            let archives = [
                ("study.zip", zip_folder(&folder, "CD").expect("zip")),
                ("study.iso", iso_image(&folder, IsoNames::Joliet).expect("iso")),
            ];
            for (name, bytes) in archives {
                let path = dir.path().join(name);
                std::fs::write(&path, bytes).expect("write");
                let index = scan(&path, &Progress::new()).expect("scan");
                assert_eq!(index.stats.images, 6, "{name}");
                let archive = index.media.archive().expect("an archive");
                let (indexed, len) = (archive.bytes_read(), archive.len());
                assert!(len > 1_000_000, "{name}: {len} bytes");
                assert!(indexed < len / 4, "{name}: {indexed} of {len} bytes were read for the index");
                assert!(indexed < 6 * (48 << 10), "{name}: {indexed} bytes for six headers");

                // Loading the series reads the images themselves.
                let ct = load_series(&index.media, &index.studies[0].series[0], &Progress::new()).expect("decode");
                assert_eq!(ct.len(), 4, "{name}");
                assert!(archive.bytes_read() - indexed > len / 2, "{name}: {} bytes", archive.bytes_read());
            }
        }
    }

    #[test]
    fn entry_names_become_plain_relative_paths_or_nothing() {
        let path = |name: &str| entry_path(name.split(['/', '\\']));
        assert_eq!(path("DICOM/ST000001/IM000001"), Some(PathBuf::from("DICOM").join("ST000001").join("IM000001")));
        // Windows separators, a leading slash and "." mean nothing.
        assert_eq!(path("DICOM\\IM000001"), Some(PathBuf::from("DICOM").join("IM000001")));
        assert_eq!(path("/./DICOM//IM000001"), Some(PathBuf::from("DICOM").join("IM000001")));
        // Hidden entries and the resource forks of archives made on a Mac are not listed.
        assert_eq!(path("__MACOSX/CD/._IM000001"), None);
        assert_eq!(path("CD/.DS_Store"), None);
        assert_eq!(path(".hidden/IM000001"), None);
        // Names that would leave the archive if they were used as paths are not listed either.
        assert_eq!(path("../../etc/passwd"), None);
        assert_eq!(path("CD/../../IM000001"), None);
        assert_eq!(path("CD/IM\0"), None);
        assert_eq!(path(""), None);
        assert_eq!(path("CD/"), Some(PathBuf::from("CD")));
    }

    #[test]
    fn messages_name_the_archive_and_the_way_out() {
        let file = Path::new("/downloads/study.zip");
        let message = ArchiveProblem::PasswordProtected.message(file);
        assert!(message.starts_with("study.zip is protected by a password"), "{message}");
        assert!(!message.contains("/downloads"), "the file name is enough: {message}");
        let message = ArchiveProblem::Nested("disc.iso".to_owned()).message(file);
        assert!(message.contains("Unpack study.zip, then open disc.iso"), "{message}");
        let message = ArchiveProblem::TooLarge.message(file);
        assert!(message.contains("larger than 4 GB"), "{message}");
        assert!(ArchiveProblem::Damaged(MediaKind::Zip).message(file).contains("ZIP archive"));
        assert!(ArchiveProblem::Damaged(MediaKind::Iso).message(Path::new("cd.iso")).contains("disc image"));
        // One file or several.
        assert_eq!(
            ArchiveProblem::PasswordProtected.warning(1, "study.zip"),
            "1 file in study.zip is protected by a password and was skipped"
        );
        assert_eq!(
            ArchiveProblem::Compression("Lzma".to_owned()).warning(3, "study.zip"),
            "3 files in study.zip are compressed with a method that is not supported (Lzma) and were skipped"
        );
    }
}
