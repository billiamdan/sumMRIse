## Context

`summrise_dicom::scan(path)` finds files on the file system (a DICOMDIR within three
levels, else a recursive `walkdir` scan), parses their headers in parallel on rayon with
`OpenFileOptions::read_until(PixelData).open_file(path)`, and returns a `StudyIndex`
whose `InstanceHeader::path` values are file-system paths. Everything later reads those
paths directly: the series loader thread (`std::fs::read`), the thumbnails (rayon inside
a background stream), the metadata inspector (a background job) and `load_series`.
Nothing knew about containers. The privacy rules forbid unpacking an archive to disk.

## Goals / Non-Goals

**Goals:**
- A ZIP archive or an ISO 9660 image of a patient disc opens exactly as the unpacked
  folder: same index, same warnings, same pixels.
- Nothing derived from patient data is written to disk.
- Hostile or broken archives cannot crash the application, make it allocate without
  bound or read outside the archive, and every refusal is explained.
- The folder path behaves exactly as before.

**Non-Goals:**
- Passwords, nested archives, tar/7z/RAR/DMG, UDF, split archives, creating archives
  (all explained to the user instead).
- Rock Ridge names (DICOM file IDs are ISO 9660 level 1 names anyway).
- Caching decompressed entries between reads.

## Decisions

### D1 — One `Media` value, carried by the index (binding)
`summrise_dicom::media::Media` is the only way to the files for discovery, DICOMDIR
parsing and resolution, header indexing, `read_instance`, `load_series`, `thumbnail` and
`read_metadata`. It is `Media(Option<Arc<Archive>>)`: `None` is a folder and runs exactly
the code that ran before (`walkdir`, `open_file`, `std::fs::read`); `Some` is a ZIP
archive or disc image with its directory in memory. `StudyIndex::media` keeps the media
the index was built from, and every read function takes `&Media` as its first argument,
so a later read cannot go to the wrong place. `Media` is cheap to clone, `Send + Sync`.

Alternatives considered:
- A `trait Media` with three implementations behind `dyn` — rejected: the folder would
  go through new code for no benefit, and the two archive kinds share everything but
  "where does an entry start".
- The media stored in every `InstanceHeader` or `Series` — rejected: larger headers, and
  struct literals in other changes would have to fill it.
- A process-wide "current archive" — rejected: hidden state, wrong as soon as a second
  study is opened while the first is still being read.

### D2 — Paths stay `PathBuf` (binding)
For a folder, `InstanceHeader::path` is what it was. For an archive it is the path of the
entry inside the archive (`PHANTOM CD/DICOM/ST000001/SE000001/IM000001`), and
`StudyIndex::root` is the archive file. Where a path is shown (read errors, DICOMDIR
warnings), `Media::display_path` puts the archive file name and the inner path together:
`study.zip/PHANTOM CD/DICOM/…`. It is never opened.

Alternatives considered: a `study.zip!/inner` string, or an enum path type — rejected:
the first looks like a path that can be opened, the second changes every user of
`InstanceHeader::path`.

### D3 — Privacy: memory only (binding)
An archive keeps in memory its directory (names, sizes, offsets) and, per read, the
entry being read; no temporary file, no extraction, no cache. The test runs the whole
flow (index, every image through the session loader, every preview, the inspector's
metadata) from a ZIP and from an ISO in a child process whose `TMPDIR`/`TMP`/`TEMP` is a
new empty folder, and compares recursive listings of that folder and of the folder
holding the archives before and after. The test was checked against a deliberately broken
reader that writes one file to `std::env::temp_dir()`: it failed, as it should.
Entry names can contain a patient's name, so `warn!` lines name the archive file, and
entry names appear in logs at debug level only.

### D4 — Headers without pixel data (binding)
`Media::open_object(path, Some(PixelData))` parses with
`OpenFileOptions::read_until(PixelData).from_reader(stream)`, where the stream is the
entry: a `seek` plus bounded reads for stored ZIP entries and ISO extents, and the `zip`
crate's lazy inflater for deflated entries, which only inflates what the parser asks for.
The parser decides about the 128-byte preamble from what one read returns, which for a
stream can be less than 132 bytes, so `open_object` reads the first 132 bytes itself and
chooses as `open_file` does. A test indexes an archive of 512 × 512 CT slices and checks
that less than a quarter of the archive (and less than 48 KiB per header) was read.

### D5 — Hostile archives (binding; the limit is chosen here)
- **4 GiB per entry** (`MAX_ENTRY_BYTES`): larger entries are listed as unreadable,
  skipped and counted ("larger than 4 GB"). Enhanced multi-frame objects can exceed 1 GB,
  so the limit is well above real files; it equals the largest entry a ZIP without
  ZIP64 and a single ISO 9660 extent can hold, and bounds what one entry can make the
  application allocate.
- Memory is reserved for an entry's declared size and no more; the read must end exactly
  there ("ends early" / "longer than the archive says"), and reading one byte past it
  makes a ZIP entry verify its CRC-32. The `zip` crate also fails rather than inflate
  beyond the declared size.
- Entry names are keys into the in-memory directory and nothing else. Names with `..`,
  a root, a drive prefix, NUL or a separator inside a component are not listed; hidden
  entries and `__MACOSX` are skipped as the folder scan skips hidden files; ZIP symlinks
  and folder entries are not listed.
- At most 1 000 000 entries; ISO directories of at most 16 MiB, 32 levels deep, each read
  once (loops through crafted records are cut), every offset checked against the image
  size; at most 64 volume descriptors are looked at.
- The archive's size and modification time are recorded when it is opened; a read that
  finds them changed fails ("has changed since it was opened") instead of reading other
  data at stale offsets.

### D6 — Concurrency: no lock at all (binding; the strategy is chosen here)
The directory is immutable after opening and shared through `Arc`; every read opens its
own handle on the archive file (one `open` and `fstat` per read) and keeps no shared
state but an atomic byte counter. Header indexing on rayon, the loader thread, the
previews and the inspector therefore read in parallel, and a deadlock is impossible
because no lock is ever taken. The cost of an `open` per file is small next to decoding.

Alternatives considered:
- One shared handle behind a mutex (serialising) — rejected: header indexing would run
  one file at a time, and a large image being read would hold back previews and the
  inspector.
- Memory-mapping the archive — rejected: `unsafe` in the dependency and a crash (SIGBUS)
  when a mapped file on a removable medium disappears.

### D7 — ZIP through the `zip` crate (binding)
`zip` 9.4 (MIT) with `default-features = false` and
`features = ["deflate-flate2-zlib-rs", "deflate64"]`: reading stored, deflate (through
`flate2` with its pure-Rust `zlib-rs` back-end, already in the dependency tree) and
deflate64 entries; no AES, bzip2, LZMA, Zstandard, PPMd, time or writing support. It
brings `deflate64` (MIT, `forbid(unsafe_code)`), `typed-path` (MIT/Apache-2.0) and, on
macOS and the BSDs, `nix` (MIT), which the crate uses for extracting, which is not used
here. Encrypted entries and other methods are recognised from the central directory and
listed with their reason before any data is read.

### D8 — ISO 9660: a reader of our own
The reader (`iso9660.rs`, about 300 lines, no `unsafe`) reads the volume descriptors,
prefers the Joliet tree when a supplementary descriptor with a Joliet escape sequence
exists, walks the directory records, drops `;1` and the dot of extension-less names, and
reads files from their extent, or from their extents one after the other when a file is
recorded in several. Interleaved files (a CD-i legacy) are listed as unreadable. UDF
volume descriptors are only recognised, so that a UDF-only image can be explained.

Crates looked at on crates.io (`cargo info`): `iso9660` 0.1.1 (describes itself as
incomplete), `cdfs` 0.2.3, `hadris-iso` 3.0.0-rc.3 (a release candidate; writing on by
default), `iso9660_simple` 0.2.7 and `isomage` 2.1.0 (a reader for many formats). Rejected
in favour of the small reader: the format subset needed is small, and our own code
applies exactly the limits of D5 (offset checks, loop cuts, size caps) to an untrusted
image. It is tested against images from the testkit's writer (whose output libarchive
reads back correctly) and from `hdiutil makehybrid`. A further dependency was not worth
it.

### D9 — The content decides (brief), the extension only explains
In this order: the DICOM preamble and `DICM` (a DICOM file may carry a ZIP-like
preamble, and stays a file); a ZIP local header, empty-archive or split-archive
signature; `CD001` at byte 32 769, the identifier of the first volume descriptor, or of
a later one after UDF's `BEA01` (UDF `NSR02`/`NSR03` without any `CD001` is "UDF only");
signatures of 7z, RAR, gzip, bzip2, xz and Zstandard, tar's
`ustar` at byte 257 and the `koly` trailer of a DMG. Only then the extension: `.zip` is
still tried as a ZIP archive (its directory is at the end, after any prefix) and is
"damaged" if that fails; `.iso` is a damaged image; `.7z`, `.rar`, `.tar`, `.gz`, `.dmg`
and similar are "not supported"; anything else stays a single file, as before.

### D10 — What is opened and what is said
Inside an archive the folder rules apply: DICOMDIR within three levels of the root,
else a scan with the same extension and `DICM` checks. Archives and disc images inside
(by extension) are never opened; they are named in a warning, or in the error when
nothing else is there. Files that discovery wants but cannot read are skipped and
counted in a warning per reason ("1 file in partly.zip is protected by a password and
was skipped"), next to today's count of unparseable files. When no image could be
indexed, the most frequent reason among those files becomes the error; failing that a
damaged directory, a nested archive or a UDF-only image; failing that the usual "no DICOM
images were found". A disc image that is cut off also gets "Part of … could not be read:
the disc image is damaged or incomplete". All new messages go through `tr!`/`tr_n!`.

### D11 — The ways in (UI)
- Welcome screen: a secondary button "Open archive or disc image…" below the primary
  "Open folder or disc…" (the folder stays the main way), with a tooltip and the
  shortcut ⌘Shift+O / Ctrl+Shift+O. The longer shortcut is checked first, so that it
  never also opens the folder dialog.
- The file dialog offers `zip` and `iso` (in both cases, because GTK filters are
  case-sensitive). A download without an extension can be dropped or passed on the
  command line, where the content decides.
- Drop, command line and the recent list needed no change: they pass a path to `scan`.
  The recent list stores the archive's path as it stores a folder's.
- The viewer toolbar is unchanged: it is already full at 1280 px with the inspector open,
  and `viewer.rs` is shared with other changes. The shortcut and drop work in the viewer.

## Risks / Trade-offs

- [An archive on slow media] → every read opens the file again; on an optical disc or a
  network share this adds latency per image. The loader's single reader thread and
  read-ahead limit apply unchanged.
- [Deflated entries are inflated twice] → the header is inflated during indexing and the
  whole entry again when the image is loaded; the first part is a few KiB.
- [Deflate64 coverage] → decoding is tested with a synthetic deflate64 archive made of
  stored blocks (valid deflate64, but it does not exercise the 64 KiB window); archives
  written by Windows Explorer were not available on this Mac.
- [Third-party wording] → I/O and `zip` errors (for example "Invalid checksum") appear
  untranslated inside "cannot read …" messages, as dicom-rs messages do today.
- [Memory for a large entry] → an image entry is held in memory while it is decoded, as a
  file read from a folder is; at most 4 GiB per entry by D5.

## Migration Plan

API changes for code outside this change (no file-format or settings change):
- `StudyIndex` has a new field `media: Media`.
- `read_instance`, `load_series`, `thumbnail`, `read_metadata`, `index_files` and
  `discovery::referenced_files` take `&Media` as their first argument
  (`Media::folder()` for files on disk).
- `core::ImageSource::read(&self, media, header)` and
  `SeriesLoader::spawn(series, media, data, source, decoders, displayed, waker)`.
- `DicomError::Archive { path, problem }` is a new error variant.
