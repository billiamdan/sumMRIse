## Context

Moved from `macos-distribution` (2.4 Intel check, 2.5 Developer ID + notarization),
`ci-quality-gates` (2.4 GitHub Releases) and `privacy-safeguards` (3.2 network check).
`cargo xtask bundle-macos` already supports `--sign` and `--notarize`; CI already imports
a signing certificate when secrets exist.

## Decisions

- **Notarize with an App Store Connect API key** (`notarytool --key`) in CI rather than an
  Apple ID password; keep the keychain-profile path for local releases.
- **Publish with the `gh` CLI** in a job gated on version tags, avoiding another
  third-party action; checksums with `shasum -a 256`.
- **Network gate via cargo-deny bans** (`reqwest`, `hyper`, `ureq`, `curl`, `isahc`,
  `surf`, `attohttpc`) instead of inspecting binary imports, which is noisy because system
  UI frameworks link networking libraries themselves.

## Risks / Trade-offs

- [Secrets in CI] → restrict the release job to tags on `main`, use environment
  protection rules and short-lived API keys.
- [Notarization outages] → the job retries and can be re-run; unsigned artefacts remain
  available from the workflow run.
