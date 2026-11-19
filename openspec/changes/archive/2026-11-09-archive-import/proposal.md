## Why

Hospitals increasingly hand out a download — a ZIP archive of the disc — or a disc image
instead of a physical disc. SumMRIse opens folders and single DICOM files only, so
patients have to know how to unpack an archive or mount an image before they can look at
their scans, and unpacking leaves a second copy of their data on disk.

## What Changes

- A ZIP archive or an ISO 9660 disc image of a patient disc opens exactly as the unpacked
  folder would: the same studies, series, images and warnings; the DICOMDIR inside is
  used first, a scan is the fallback; viewer software, autorun files and the clutter that
  archives made on a Mac carry are ignored.
- Ways in: a new "Open archive or disc image…" action on the welcome screen with a file
  dialog (⌘Shift+O / Ctrl+Shift+O), dropping such a file onto the window, the
  command-line argument and the recent list.
- The content decides what a file is, not its name: a ZIP archive called `.iso`, or a
  download without any extension, opens all the same.
- Nothing is unpacked: entries are read from the archive into memory when they are
  needed, and only as far as they are needed.
- What cannot be read is explained in plain words: password-protected archives,
  compression methods other than stored, deflate and deflate64, an archive inside the
  archive, disc images with UDF only, other formats (7z, RAR, tar, DMG), damaged or
  incomplete files. Where only some files cannot be read, the rest opens and the skipped
  files are counted in a warning.

Privacy: an archive is read in memory and never unpacked, so opening one leaves no copy
of patient data on disk — no extracted files, no temporary files, no cache. Names of
entries (which can contain a patient's name) are not logged above debug level.

Patient safety: images read from an archive are bit for bit the files in it (ZIP
checksums are verified when an image is loaded), so orientation, geometry and values are
those of the unpacked folder. An archive that changes on disk while a study is open is
not read with stale offsets: reads fail with a message instead of showing other data.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `dicom-import`: adds ZIP archives and ISO 9660 disc images as media, recognition by content, reading in memory, explanations for what is not supported, and limits for untrusted archives.
- `application-shell`: adds the action for opening an archive or a disc image and the other ways to open one.

## Impact

- `crates/dicom`: new `media` module (`Media`: folder, ZIP archive, disc image) through
  which discovery, DICOMDIR resolution, header indexing, pixel data, thumbnails and
  metadata read; a ZIP reader on the `zip` crate and a minimal ISO 9660 reader. The index
  carries its media. `read_instance`, `load_series`, `thumbnail`, `read_metadata`,
  `index_files` and `discovery::referenced_files` take the media as their first argument.
- `crates/core`: the series loader and the thumbnails read through the media of the
  index (`ImageSource::read` and `SeriesLoader::spawn` take it).
- `crates/ui`: welcome-screen action, file dialog, shortcut; the inspector reads through
  the media.
- `crates/testkit`: writers for synthetic ZIP archives and ISO 9660 images.
- New dependency: `zip` (MIT), with only the features for reading stored, deflate and
  deflate64 entries; it brings `deflate64`, `typed-path` and, on macOS, `nix`. `flate2`,
  already in the tree, becomes a direct dependency of the testkit.
- No file-format or settings changes; the recent list stores the path of the archive as
  it stores the path of a folder.
