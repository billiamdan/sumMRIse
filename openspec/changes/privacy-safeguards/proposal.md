## Why

Medical images and their headers are protected health information. The old app wrote
the **entire study — pixel data and patient identifiers — as plaintext JSON** to the
user cache directory, logged patient IDs at info level, shipped a settings form with a
pre-filled fake OpenAI key, and contained an unused client designed to send studies to a
third-party LLM. A viewer for patients must be private by default.

## What Changes

- **BREAKING** Remove the JSON study cache (`~/Library/Caches/summrise/dicom_cache.json`
  on macOS); nothing derived from images is written to disk.
- **BREAKING** Remove the `llm` crate and AI settings; the app makes no network
  connections. Any future AI feature must be a separate, opt-in change with
  de-identification and explicit consent (see design.md).
- Patient identifiers are never logged above debug level.
- The recent list stores only folder path, study description/date and modalities, can be
  turned off and cleared.
- Privacy mode hides patient name, ID and birth date in overlays, browser and inspector
  (useful for screenshots).
- Visible "not a medical device" notice on the welcome screen and in About; hardened
  runtime on macOS with no entitlement exceptions.

## Capabilities

### New Capabilities
- `privacy`: handling of patient-identifying information, local-only processing and disclaimers.

### Modified Capabilities
<!-- None. -->

## Impact

Removes `crates/llm`, the cache code in the former `src/main.rs`; affects
`crates/core/src/settings.rs`, `crates/ui`, logging in all crates. Users lose the
(broken) cache — re-opening a disc re-indexes it in seconds with DICOMDIR support.
