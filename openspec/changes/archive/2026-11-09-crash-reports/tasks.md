## 1. Reports

- [x] 1.1 `summrise_core::crash`: panic hook, report holding only fixed program text (version, system, UTC time, own thread name, source location, backtrace, literal message), five kept, the name of a fatal one kept for the next start (unit tests)
- [x] 1.2 `summrise_dicom::panics::catch` at the catch sites for damaged data (header reading, decoding, thumbnails, documents, image source steps): expected panics leave no report
- [x] 1.3 rayon: a worker's panic is held, dropped by `catch`, written by `crash::watch` (job, stream, loader and media threads), by the decoder pools' panic handler and by `main`
- [x] 1.4 End-to-end test with the real hook (`crates/core/tests/crash_report.rs`): messages composed from the testkit patient's name, ID, birth date, folder path and the `Debug` text of the scanned study; a literal message; a thread named after the patient; damaged data; rayon caught and not caught; a fatal report told once

## 2. Telling the user

- [x] 2.1 Message box after a fault on the window's thread, and when the window cannot be opened (`main`, rfd); `SUMMRISE_TEST_CRASH=window|background` fails on purpose to try it
- [x] 2.2 Notice in the window for a fault on a background thread and for the crash of the previous run, with "Copy path" (UI tests `crates/ui/tests/crash_notice.rs`)
- [x] 2.3 Texts in German, Spanish, French and Russian (machine translations, as all others)

## 3. Documentation

- [x] 3.1 README (privacy), `SECURITY.md`, DICOM conformance statement section 7

## 4. Verification on real systems

Moved to `field-validation` (4.2, 4.3): they need the real window on each system.
