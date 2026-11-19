## 1. Media (`summrise-dicom`)

- [x] 1.1 `Media` (folder, ZIP archive, ISO 9660 image) in `media.rs`, recognised by content (`DICM` preamble, ZIP signatures, `CD001`, UDF-only, 7z/RAR/gzip/bzip2/xz/Zstandard/tar/DMG); test `the_content_decides_what_a_file_is_not_its_name`
- [x] 1.2 Discovery, DICOMDIR parsing and resolution (case, `;1` and trailing dot tolerated), header indexing, `read_instance`, `load_series`, `thumbnail` and `read_metadata` read through the media; `StudyIndex::media`; tests `zip_and_iso_give_the_index_of_the_unpacked_folder`, `dicomdir_references_resolve_whatever_the_case_of_the_names`
- [x] 1.3 Folder path unchanged: every existing test passes with signature updates only (`cargo xtask ci`)
- [x] 1.4 ZIP through `zip` 9.4 with only stored, deflate and deflate64 reading; encrypted entries and other methods listed with their reason; test `deflate64_entries_are_read`
- [x] 1.5 ISO 9660 reader: primary volume descriptor, Joliet preferred, `;1` stripped, files in several extents read in order; unit tests in `iso9660.rs`, test `a_file_in_several_extents_is_read_from_all_of_them` (fails against a reader that ignores the second extent, checked by a temporary mutation)
- [x] 1.6 Header indexing streams entries up to Pixel Data; test `indexing_reads_the_headers_and_not_the_pixel_data` (less than a quarter of the archive read for 512 × 512 CT slices, the images read only on loading)
- [x] 1.7 Limits for untrusted archives (4 GiB per entry, declared size reserved and enforced, at most 1 000 000 entries, ISO directory size, depth and loop limits, names never paths, `__MACOSX` and hidden entries skipped, changed archive detected); tests `hostile_archives_cannot_make_the_reader_take_more_than_declared`, `an_archive_that_changes_after_opening_is_not_read_with_stale_offsets`, `entry_names_become_plain_relative_paths_or_nothing`
- [x] 1.8 Messages through `tr!`/`tr_n!` for password protection, unsupported compression, nested archives, UDF-only images, other formats and damaged files; unreadable entries skipped and counted; tests `password_protected_archives_say_so`, `unsupported_compression_methods_say_so`, `an_archive_inside_the_archive_is_not_opened`, `damaged_and_foreign_files_say_what_they_are`, `a_truncated_disc_image_opens_as_far_as_it_goes`, `an_archive_without_images_reports_nothing_found`, `messages_name_the_archive_and_the_way_out`

## 2. Application (`summrise-core`, `summrise-ui`, `summrise`)

- [x] 2.1 Series loader and thumbnails read through the media of the index (`ImageSource::read`, `SeriesLoader::spawn`); test `a_study_from_a_zip_or_an_iso_is_the_study_of_the_folder` (every image, preview and metadata equal to the folder's)
- [x] 2.2 Inspector reads metadata through the media (visible in snapshot `archive-3-dropped-zip.png`: overview read from the ZIP)
- [x] 2.3 Welcome screen: "Open archive or disc image…" below "Open folder or disc…", tooltip with the shortcut, drop hint mentions archives; tests `offers_to_open_an_archive_or_a_disc_image_next_to_the_folder`, `archive_action_explains_itself_and_names_its_shortcut`, `archive_action_is_offered_in_dark_and_light_appearance`
- [x] 2.4 Shortcut ⌘Shift+O / Ctrl+Shift+O opens only the archive dialog, ⌘O / Ctrl+O only the folder dialog; test `open_shortcuts_ask_for_one_dialog_each`
- [x] 2.5 A dropped ZIP and a dropped disc image open, both appear in the recent list with their path, the command-line path (`AppOptions::open`, as `main.rs` passes it) opens an archive, and an archive that cannot be opened is explained; offscreen tests in `crates/ui/tests/archive.rs`
- 2.6 Moved to `field-validation` 5.2 when this change was archived: The native file dialog with its `zip`/`iso` filter — not verified: the `rfd` dialog cannot be opened by the offscreen tests, and the application window cannot be operated on this machine
- 2.7 Moved to `field-validation` 5.3 when this change was archived: The `summrise` binary started with an archive argument — not verified: the application is not run here; the same path is covered through `AppOptions::open` (2.5)

## 3. Testkit

- [x] 3.1 ZIP writer (stored, deflate, deflate64, other method numbers, encryption flag, declared sizes, ZIP64 sizes, data descriptors, one top-level folder, `__MACOSX` and `.DS_Store` clutter); Info-ZIP `unzip -t` reports "No errors detected" for the packed phantom disc (test `zip_of_the_synthetic_cd_passes_the_test_of_unzip`)
- [x] 3.2 ISO 9660 writer (level 1 upper-case names with `;1`, Joliet variant, a file in two extents, UDF-only image); libarchive (`tar -tf`, `tar -xOf`) lists every file and extracts identical bytes (test `iso_of_the_synthetic_cd_is_read_by_libarchive`)

## 4. Verification

- [x] 4.1 Index from ZIP and ISO equals the folder's (studies, series, instance UIDs, geometry, counts, warnings), with and without DICOMDIR; tests `zip_and_iso_give_the_index_of_the_unpacked_folder`, `without_a_dicomdir_the_archive_is_scanned_like_the_folder`
- [x] 4.2 Series from ZIP and ISO pixel-identical to the folder's; CT marker at (40, −35, 50) mm reads 300 HU, slices superior to inferior; test `series_from_zip_and_iso_are_pixel_identical_to_the_folder`
- [x] 4.3 Privacy: opening and fully loading a study from a ZIP and from an ISO creates no file, checked in a child process with its own empty temporary folder and beside the archives; the test fails against a reader that writes one temporary file (checked by a temporary mutation); test `opening_archives_writes_nothing_to_disk`
- [x] 4.4 Concurrency: indexing, loading, previews and inspector reading one archive at once; tests `reading_from_many_threads_at_once_gives_the_same_as_reading_alone`, `everything_reads_one_archive_at_once`
- [x] 4.5 Tests with threads or timing run 20 times without a failure: `summrise-dicom --test archive` (21 tests), `summrise-core --test archive` (3), `summrise-core --lib` with the loader tests (39), `summrise-ui --test archive` (3) and the new `summrise-ui` unit tests (4), 100 runs in all
- [x] 4.6 Cross-check with the tools of macOS (tests skip where a tool is missing): `ditto -c -k --keepParent` (resource forks as `CD/._DICOMDIR`) and with `--sequesterRsrc` (`__MACOSX/`, as the Finder's "Compress"), `zip -r`, `hdiutil makehybrid -iso -joliet`, `-iso` and `-iso -joliet -udf` all open with the index and pixels of the folder; `hdiutil makehybrid -udf` is explained as UDF-only; tests `archive_made_by_ditto_opens_like_the_folder`, `archive_made_by_zip_opens_like_the_folder`, `image_made_by_hdiutil_opens_like_the_folder`
- [x] 4.7 `mkisofs`/`genisoimage`/`xorrisofs` images: `image_made_by_mkisofs_opens_like_the_folder` passes with `xorrisofs` (here, from Homebrew, and on the Linux CI runner, which installs `xorriso`; CI sets `SUMMRISE_REQUIRE_TOOLS`, so a missing tool fails the test instead of skipping it)
- 4.8 Moved to `field-validation` 5.4 when this change was archived: Archives made on Windows (Explorer's deflate64 for large files) and real hospital downloads — not verified on this machine; tracked with `field-validation`
- [x] 4.9 Snapshots reviewed (dark and light welcome screen, tooltip, dropped ZIP, dropped ISO in MPR, recent list, error notifications)
- [x] 4.10 `cargo xtask ci` passes; `openspec validate --all --strict` passes
- [x] 4.11 `cargo deny check` (advisories, bans, licences and sources ok, in CI and locally)
