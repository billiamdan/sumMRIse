## 1. Acquisition parameters in the index (`summrise-dicom`)

- [x] 1.1 `Acquisition` (new `acquisition.rs`) read from the classic attributes and the Enhanced multi-frame functional groups (macro in the shared group, values flat in the shared group, macro of the first frame), with Scanning Sequence derived for Enhanced MR; unit tests with classic CT and MR, Enhanced MR, per-frame and flat values, and absent, empty and malformed values (`cargo test -p summrise-dicom acquisition`)
- [x] 1.2 `Series::acquisition`, filled by one added call in `index.rs::read_header`; the values of every synthetic series are checked (`index_keeps_acquisition_parameters_of_each_series`)
- [x] 1.3 Summary rows carry a stable key, the DICOM keyword of the attribute they show (`Matrix` and `FieldOfView` for the two rows that combine attributes) (`summary_rows_carry_stable_keys`, `row_keys_are_dictionary_keywords`)
- [x] 1.4 Testkit: the CT phantom records a contrast agent and the MR phantom a protocol name; the existing tests pass unchanged

## 2. Guide logic (`summrise-core::guide`)

- [x] 2.1 Kind of series by the rules of D3: CT, MR, localizer by image type and by name, dose report, secondary capture, PET, nuclear medicine, ultrasound, X-ray, mammogram, fluoroscopy, unrecognised modality; whole words only (`localizers_and_stored_pictures_are_recognised`, `other_modalities_are_named_and_unknown_ones_are_not_guessed`)
- [x] 2.2 MR weighting by the rules of D4 with concrete values: spin echo TR 500 / TE 12 ms → likely T1; TR 4000 / TE 100 → likely T2; TR 2500 / TE 20 → likely proton density; TR 9000 / TE 120 / TI 2500 → likely T2 FLAIR; TR 4000 / TE 40 / TI 150 → likely STIR; b-value 1000 → diffusion-weighted; gradient echo TE 20 ms / flip 20° → likely T2*; "t1_mprage_sag" → T1 as named; contradictory or missing values → cannot tell (`cargo test -p summrise-core guide`)
- [x] 2.3 Description with a basis for every statement (kind, plane, weighting, MR 2D or 3D, slice thickness, CT kernel, contrast agent); CT with a kernel, 1.25 mm slices and a contrast agent; the four synthetic series (CT chest, MR sagittal, localizer, dose report) get sensible descriptions (`ct_series_states_thickness_kernel_and_contrast_agent`, `mr_acquisition_type_says_slice_by_slice_or_block`, `synthetic_series_are_described_sensibly`)
- [x] 2.4 Parameter explanations looked up by key; every key of the synthetic overviews that should be explained is, and no patient row is (`every_parameter_explanation_is_short_and_plain`, `overview_rows_of_the_synthetic_images_are_explained`)
- [x] 2.5 Preset explanations by source and by width and level; every built-in CT window has its own (`every_preset_says_what_the_window_is_for`)
- [x] 2.6 Viewing direction for axial, coronal, sagittal, mirrored, rotated and oblique planes (`viewing_direction_follows_the_screen_axes`)
- [x] 2.7 Body region mapping for the common Body Part Examined values, spelling variants and unknown values (`body_parts_map_to_regions_and_unknown_values_to_none`)
- [x] 2.8 Scanned block from the indexed headers and the placement of an image in it (`scanned_block_and_placement_come_from_the_headers`)
- [x] 2.9 Every explanation and statement is non-empty, has at most two sentences and contains no word of the interpretive list (`every_statement_about_a_kind_a_weighting_or_a_plane_is_plain`, `fixed_texts_and_direction_sentences_are_plain`)

## 3. Interface (`summrise-ui`)

- [x] 3.1 One line under each series title in the browser (`chest_ct_is_described_in_the_browser_the_card_and_the_figure`; snapshot `guide-1-chest-ct.png`)
- [x] 3.2 "About this series" card: headline, statements with their basis, the fixed line, the footnote where settings are used (same test; snapshots `guide-1-chest-ct.png`, `guide-1-chest-ct-light.png`, `guide-6-stored-picture.png`, `guide-7-sagittal-mr.png`)
- [x] 3.3 Info marks on the explained rows, shown on hover and on keyboard focus, none on patient rows (`parameter_explanations_show_on_hover_and_on_keyboard_focus`; snapshots `guide-3-explanation-tooltip.png`, `guide-3-explanation-focus.png`)
- [x] 3.4 Preset tooltips say what each window is for (`preset_menu_says_what_each_window_is_for`; snapshot `guide-4-preset-tooltip.png`)
- [x] 3.5 Position figure: body outline for a chest CT, scanned block for an unknown body part, no figure without position information; it follows the active view (`figure_follows_the_active_view`, `unknown_body_part_shows_the_block_and_pictures_show_no_figure`, `front_view_places_the_image_truthfully`, `block_projection_keeps_the_radiological_view`, `every_region_lies_inside_the_outline`, `figures_render_in_both_themes`; snapshots `guide-2-coronal-reformat.png`, `guide-2-sagittal-reformat.png`, `guide-5-block-figure.png`, `guide-figures-dark.png`, `guide-figures-light.png`)
- [x] 3.6 Accessible names for the marks and the figures (the kittest accessibility check runs in the guide tests)

## 4. Setting

- [x] 4.1 `Settings::show_explanations`, on by default, also for settings stored without the field (`explanations_are_on_by_default_also_for_settings_stored_before`)
- [x] 4.2 Checkbox in the settings window, by mouse and by keyboard (`setting_is_switched_in_the_settings_window`; snapshots `guide-9-settings-window.png`, `guide-9-settings-off.png`)
- [x] 4.3 Switched off, the browser line, card, figure, marks and preset explanations are hidden (`switched_off_nothing_of_the_guide_is_shown`; snapshot `guide-8-switched-off.png`)

## 5. Privacy and verification

- [x] 5.1 Privacy mode does not change the guide, and no guide text of the synthetic study contains the patient's name, ID or birth date (`privacy_mode_does_not_change_the_guide`, `synthetic_series_are_described_sensibly`; snapshot `guide-10-privacy-mode.png`)
- [x] 5.2 `cargo xtask ci` passes
- [x] 5.3 `openspec validate --all --strict` passes
- 5.4 Moved to `field-validation` 5.6 when this change was archived: Review of the texts by patients and by a radiographer or radiologist (needs people; cannot be done on this machine)
- 5.5 Moved to `field-validation` 5.7 when this change was archived: Check of the classification rules against studies from several vendors (needs real, anonymised discs, which the tests must not use; tracked with `field-validation`)
