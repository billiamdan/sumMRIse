# Security and privacy

SumMRIse shows the medical images that hospitals give to patients. It is not a medical
device and must not be used for diagnosis.

## Reporting a vulnerability

Please report security problems privately through GitHub: on the repository page, open
**Security ▸ Report a vulnerability** (a private security advisory). Do not open a public
issue for them.

**Never attach real patient images, crash reports from a disc you are not allowed to share,
or screenshots that show a patient's details.** Describe the problem, the SumMRIse version
(About ▸ Version), the operating system, and — if it is about a file — how to make such a
file synthetically. `cargo xtask fixtures <folder>` writes a synthetic disc to start from.

You can expect a first answer within a week. Fixes go into the next version; the release
notes and the advisory credit the reporter unless they prefer otherwise.

## Supported versions

Only the newest released version receives fixes.

## What SumMRIse promises

- **No network access.** The application opens no network connection. CI fails when an
  HTTP, WebSocket or TLS client crate enters the dependency graph (`deny.toml`).
- **Nothing derived from patient data on disk**, unless you explicitly save or print it:
  no image cache, no thumbnails, no unpacked archives. Settings and the list of recent
  studies (path, study description and date, modalities — never names, IDs or birth dates)
  are stored in the application's settings folder; the list can be switched off.
- **Crash reports hold no patient data.** If SumMRIse stops because of a fault, it writes a
  report to `~/Library/Logs/SumMRIse` (macOS), `%LOCALAPPDATA%\SumMRIse\Crash reports`
  (Windows) or `~/.local/state/summrise/crash-reports` (Linux). It contains the version,
  the system, the time, the place in the source code and the functions that were running —
  never names, IDs, dates of birth, file or folder names, or values read from the images.
  A panic message composed while the program ran is withheld. The five newest reports are
  kept. You decide whether to send one.
- **Nothing on a disc is run.** Viewer software and autorun files on a disc are ignored;
  documents on the disc are shown as text or drawn inside the application; printing uses
  the system's programs by absolute path.
- **Damaged or hostile files** are errors, not crashes: decoding runs behind panic guards,
  nesting depth is limited, and archives are read in memory with size limits.

## Signed packages

Release packages are built by GitHub Actions from a version tag on `main`
(`.github/workflows/release.yml`), each with a SHA-256 checksum in `SHA256SUMS`. Until the
project has an Apple Developer ID and a Windows code-signing certificate, the macOS app is
ad-hoc signed and not notarized and the Windows files are not signed: macOS and Windows
warn when they are first opened, and the release notes say so. Check the checksum of a
download with `shasum -a 256 -c SHA256SUMS` (macOS, Linux) or
`Get-FileHash <file> -Algorithm SHA256` (Windows).
