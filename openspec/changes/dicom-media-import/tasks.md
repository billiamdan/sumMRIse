## 1. Discovery

- [x] 1.1 DICOMDIR search (depth ≤ 3), parsing of Directory Record Sequence, skip inactive records
- [x] 1.2 Case-insensitive / ISO 9660 path resolution with cached directory listings
- [x] 1.3 Fallback to folder scan with warning (>10 % missing or unreadable DICOMDIR)
- [x] 1.4 Folder scan by `DICM` magic, extension skip-list, hidden-entry filter that never filters the root

## 2. Indexing

- [x] 2.1 Header-only parsing (`read_until(PixelData)`) in parallel with `catch_unwind`
- [x] 2.2 Study/Series/Instance model incl. per-frame geometry from Enhanced functional groups
- [x] 2.3 Deduplication, non-image counting, ordering, warnings and statistics
- [x] 2.4 Progress stages and cancellation

## 3. Application integration

- [x] 3.1 Default-series heuristic in `Session::default_series`
- [x] 3.2 `MediaWatcher` (background polling of mount points)

## 4. Verification

- [x] 4.1 Integration tests with synthetic CD: DICOMDIR used, clutter ignored, scan fallback, grouping, UTF-8, cancellation, empty folder
- [ ] 4.2 Field test with real hospital CDs from several vendors (needs consented, de-identified samples)
