## 1. Maths

- [x] 1.1 `Window::apply` for LINEAR, LINEAR_EXACT, SIGMOID with PS3.3 boundary tests
- [x] 1.2 `CT_PRESETS` table and tests (Brain preset on HU values)
- [x] 1.3 `auto_window` with background exclusion and flat-image fallback
- [x] 1.4 Parallel grey → RGBA conversion with NaN-as-background and inversion

## 2. Behaviour

- [x] 2.1 Initial window from DICOM presets (named by explanation) or auto
- [x] 2.2 Preset menu grouped by source; keys 1–9, A
- [x] 2.3 Drag adjustment scaled to value range; right-drag always adjusts contrast
- [x] 2.4 Numeric W/L fields; Custom marking; invert; reset

## 3. Verification

- [x] 3.1 Unit tests in `summrise-imaging` and `summrise-core`

## Follow-ups (moved out of this change)

- 3.2 Usability check with non-expert users (are presets understandable?) → `field-validation`
