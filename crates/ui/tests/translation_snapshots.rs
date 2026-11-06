//! Snapshot helper for translators. With `SUMMRISE_LANGUAGE=<code>` (`de`,
//! `es`, `fr`, `ru` or `en`) it shows the app in that language — the welcome
//! screen, the settings window, the display check, the About dialog, the
//! viewer with the inspector and the guide (CT, axial and MPR), the toolbar
//! at 1440, 1280 and 960 points with reformats and a thick slab and with the
//! 3D view, the PET menu, an archive that cannot be opened and the playback
//! bar of a series with several time points — and with
//! `SUMMRISE_SNAPSHOT_DIR=<dir>` saves each as `<dir>/<code>-<screen>.png`.
//! It fails if a control of the toolbar is cut off or covers another (after
//! saving everything) or if a character has no glyph. Without
//! `SUMMRISE_LANGUAGE` it does nothing:
//!
//! ```text
//! SUMMRISE_LANGUAGE=de SUMMRISE_SNAPSHOT_DIR=/tmp/de-shots \
//!     cargo test -p summrise-ui --test translation_snapshots -- --nocapture
//! ```

use std::path::PathBuf;

mod tour;

#[test]
fn renders_the_screens_in_the_language_of_summrise_language() {
    let Ok(requested) = std::env::var("SUMMRISE_LANGUAGE") else { return };
    let language = summrise_i18n::supported(&requested)
        .unwrap_or_else(|| panic!("SUMMRISE_LANGUAGE={requested}: the interface has en, de, es, fr and ru"))
        .code;
    let snapshots = std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(PathBuf::from);
    summrise_i18n::with_catalogue(None, || {
        let mut tour = tour::Tour::new(language, snapshots.clone());
        tour.main_screens();
        assert_eq!(summrise_i18n::active_language(), language, "the app shows {language}");
        tour.other_screens();
        if let Some(dir) = &snapshots {
            println!("saved the screens in {language} to {}", dir.display());
        }
        assert!(
            tour.problems.is_empty(),
            "the toolbar or the status bar does not fit in {language}:\n{}",
            tour.problems.join("\n")
        );
    });
}
