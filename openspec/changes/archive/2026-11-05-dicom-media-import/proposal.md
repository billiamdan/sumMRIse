## Why

Discovery relied on folder names containing "Dicom", ignored the DICOMDIR index every
patient CD carries, opened each file up to eight times, read pixel data while indexing,
and grouped "investigations" by directory instead of by Study Instance UID. CDs also
contain viewer software and autorun files that must not be parsed. Fast, standards-based
import is the foundation of everything the user sees.

## What Changes

- Use `DICOMDIR` (PS3.10/PS3.12) as the authoritative file list when present, resolving
  Referenced File IDs case-insensitively (ISO 9660 `;1` suffixes, lower-case mounts);
  fall back to a folder scan when it is unreadable or >10 % of references are missing.
- Folder scan recognises Part-10 files by the `DICM` magic, skips hidden entries and
  known non-DICOM extensions (but never the folder the user chose).
- Index headers only (stop before Pixel Data), in parallel, with panic isolation for
  malformed files; group by Study → Series Instance UID; de-duplicate SOP instances.
- Report statistics and warnings (unreadable files, non-image objects, missing files).
- Choose a sensible default series and detect inserted media with a DICOMDIR.

## Capabilities

### New Capabilities
- `dicom-import`: discovering DICOM files on media/folders and indexing them into studies, series and instances.

### Modified Capabilities
<!-- None. -->

## Impact

`crates/dicom` (`discovery.rs`, `index.rs`), `crates/core` (`session.rs`, `media.rs`).
Replaces `DicomParser::find_dicom_investigations` and the per-tag `open_file` calls.
