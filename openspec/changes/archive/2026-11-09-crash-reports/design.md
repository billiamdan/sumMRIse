## Context

Every panic in a Rust program runs the panic hook first, whether or not something
catches it later. SumMRIse catches panics on purpose where damaged files meet library
code (header reading, decoding, documents); those must not look like crashes. rayon runs
parallel work on worker threads and passes a worker's panic on to the thread that started
the work with `resume_unwind`, which does not run the hook again. winit catches a panic in
its event handlers on macOS and Windows and resumes it after the event loop has ended, so
it reaches `main`.

## Decisions

- **D1 Report only what is fixed in the program.** The message is kept only when its
  payload is a `&'static str` (`panic!("text")`, `unwrap()` on `None`); everything composed
  at run time (`format!`, `expect`, the `Debug` of an error) is withheld. Thread names are
  kept only when they are the program's own (`main`, `summrise-<lowercase>`). Source paths
  are cut to the crate's folder. *Rejected:* filtering messages for names, IDs and paths —
  a filter can only find what it knows to look for, and a patient's name can be any
  word. *Rejected:* recent log lines — they hold paths.
- **D2 Expected panics are marked.** `summrise_dicom::panics::catch` counts, per thread,
  the catches that are running; the hook leaves a panic alone while the count is above
  zero. It lives in the lowest crate that catches panics. *Rejected:* writing a report
  for every panic — a damaged disc would fill the folder and the user would be told about
  crashes that did not happen.
- **D3 Panics on rayon workers are held.** The hook keeps the report of a worker's panic;
  `catch` drops it when the panic reaches it; where a thread of the program ends (watched
  with `crash::watch`, the decoder pools' `panic_handler`) or the window ends (`main`), it is
  written. *Rejected:* turning off rayon in dicom-pixeldata — it slows decoding for every
  user.
- **D4 Telling the user.** After a fault that closed the window, `main` shows a message box
  (rfd); only when the user has closed it is the report marked as told (`unseen.txt` holds
  its name otherwise, and the next start shows it in the window). A fault on another
  thread leaves the window running; the window shows the notice at once. *Rejected:*
  restarting the program — it could run into the same fault on the same disc.
- **D5 Folder.** The usual per-user log folder of each system, from the environment
  (`HOME`, `LOCALAPPDATA`, `XDG_STATE_HOME`), absolute paths only. At most five reports.

## Risks / Trade-offs

- [The report says less] → the source location and backtrace identify a fault in a given
  version; that is what a developer needs, and the message of a literal panic is kept.
- [A held report can be dropped by an unrelated catch on another thread at the same
  moment] → rare; the report of a crash on the window's thread is never held.
- [No zenity on a Linux desktop] → rfd cannot show the message box; the next start tells.
