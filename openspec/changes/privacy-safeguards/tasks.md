## 1. Removal

- [x] 1.1 Delete the JSON study cache and its 24-hour logic
- [x] 1.2 Delete `crates/llm`, AI settings and the fake API key; no `reqwest` in the graph
- [x] 1.3 Remove info-level logging of patient IDs and study details

## 2. Safeguards

- [x] 2.1 PHI-minimal `recent_label`, toggle and clear in Settings and welcome screen
- [x] 2.2 Privacy mode across overlays, browser and inspector
- [x] 2.3 Disclaimer and local-processing notice on welcome screen and About
- [x] 2.4 Hardened runtime with empty entitlements (macOS)
- [x] 2.5 `.gitignore` blocks DICOM data; tests use synthetic fixtures only

## 3. Verification

- [x] 3.1 Test: recent label contains no patient identifiers
- [ ] 3.2 Automated check that no network symbols are linked (e.g. inspect binary imports in CI)
- [ ] 3.3 Privacy review of log output with real data (de-identified samples)
