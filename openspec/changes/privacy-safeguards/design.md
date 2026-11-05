## Decisions

- **No persistent image cache.** Header indexing via DICOMDIR is fast enough (seconds),
  and an on-disk cache would replicate PHI outside the user's control. If a cache is
  ever needed it must store only non-identifying derived data, be encrypted at rest and
  be clearable — a separate change.
- **No network.** Removing the LLM client removes the only network dependency
  (`reqwest`). macOS entitlements stay empty; a sandboxed Mac App Store build would add
  only `com.apple.security.files.user-selected.read-only` and removable-volume access.
- **Logging policy.** `info` logs contain counts, sizes and folder paths only. Paths can
  reveal names on some CDs; they are user-local and not transmitted.
- **Future AI assistance (deferred).** Acceptable only as opt-in per study, with on-device
  de-identification (PS3.15 Basic Profile), a preview of exactly what is sent, a provider
  chosen by the user, no image upload by default, and an unmistakable "not medical advice"
  framing.

## Risks / Trade-offs

- [Users wanted "AI second opinion" from the old README] → documented as deferred with
  privacy prerequisites rather than silently dropped.
