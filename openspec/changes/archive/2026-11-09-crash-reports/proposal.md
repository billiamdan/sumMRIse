## Why

When SumMRIse stops because of a fault in the program, its window disappears without a
word, and nothing is left that would let anyone find the fault. The standard panic
message is no answer: it is printed to standard error, which desktop sessions on Linux
keep in the system journal, and it can quote anything the program held at that moment —
a patient's name, the path of a disc folder named after the patient, a value read from a
file.

## What Changes

- A crash report: when a panic ends the program or one of its threads, a short text file
  is written to the user's log folder (`~/Library/Logs/SumMRIse`,
  `%LOCALAPPDATA%\SumMRIse\Crash reports`, `$XDG_STATE_HOME/summrise/crash-reports`).
  The five newest are kept.
- **Privacy:** the report holds only what is fixed in the program and facts about the
  computer: version, system and processor, time (UTC), the program's own thread name,
  the place in the source code, the functions that were running, and the panic message
  only when it is literal text of the program. A message composed while the program ran
  is withheld, never filtered. No patient names, IDs, dates of birth, paths of opened
  files or folders, or DICOM values. Release builds no longer print the panic message to
  standard error.
- Panics on damaged files that are caught and shown as "could not be decoded" are
  expected and leave no report.
- The user is told in plain words what happened, that the report holds no patient data,
  and where it is: in a message box of the system after the window closed; in the
  window when part of the program stopped and the window lives on; at the next start if
  the message box could not be shown.
- When the window cannot be opened at all (no graphics adapter), a message box says so
  instead of a silent exit.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `privacy`: crash reports are the one file written without the user exporting it, and
  they hold no patient data.
- `application-shell`: the user is told about a crash and where its report is.

## Impact

New `summrise_core::crash` and `summrise_dicom::panics`; the catch sites for damaged data
use `panics::catch`; threads of the program are watched so that a panic passed on by
rayon is reported; the binary installs the hook and shows the message box (rfd); new
interface texts in five languages. No network access, no new crates.
