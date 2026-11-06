//! The display check in the real app (display-fidelity, task 5.7): opened
//! from the settings window with the keyboard alone and from the About
//! dialog (F1); Escape closes it.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use summrise_ui::{AppOptions, SumMRIseApp};

fn steps(harness: &mut Harness<'_, SumMRIseApp>, frames: usize) {
    for _ in 0..frames {
        harness.step();
    }
}

fn shows(harness: &Harness<'_, SumMRIseApp>, text: &str) -> bool {
    harness.query_all_by_label_contains(text).next().is_some()
}

#[test]
fn the_display_check_opens_from_the_settings_and_the_about_dialog() {
    let mut harness =
        Harness::builder().with_size([1280.0, 800.0]).build_eframe(|cc| SumMRIseApp::new(cc, AppOptions::default()));
    steps(&mut harness, 5);
    assert!(!shows(&harness, "It is not a calibration"));

    // Ctrl/Cmd+comma opens the settings; the button takes the keyboard focus and Space presses it.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    steps(&mut harness, 3);
    assert!(shows(&harness, "Restore defaults"), "the settings window is open");
    harness.get_by_label("Check the display…").focus();
    steps(&mut harness, 2);
    assert!(harness.get_by_label("Check the display…").is_focused());
    harness.key_press(Key::Space);
    steps(&mut harness, 3);
    assert!(shows(&harness, "It is not a calibration"), "the display check is open");
    assert!(!shows(&harness, "Restore defaults"), "in place of the settings window");
    harness.key_press(Key::Escape);
    steps(&mut harness, 3);
    assert!(!shows(&harness, "It is not a calibration"), "Escape closes it");

    // From the About dialog, the app's help (F1).
    harness.key_press(Key::F1);
    steps(&mut harness, 3);
    assert!(shows(&harness, "Not a medical device"));
    harness.get_by_label("Check the display…").click();
    steps(&mut harness, 3);
    assert!(shows(&harness, "It is not a calibration"));
    assert!(!shows(&harness, "Not a medical device"), "in place of the About dialog");
    harness.get_by_label("Close").click();
    steps(&mut harness, 3);
    assert!(!shows(&harness, "It is not a calibration"), "Close closes it");
}
