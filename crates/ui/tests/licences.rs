//! The licences of the third-party software, from the About dialog (F1): the
//! button opens them in place of the dialog, the text can be scrolled, and Close
//! and Escape close them.

use egui::Key;
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
fn the_licences_open_from_the_about_dialog() {
    let mut harness =
        Harness::builder().with_size([960.0, 600.0]).build_eframe(|cc| SumMRIseApp::new(cc, AppOptions::default()));
    steps(&mut harness, 5);
    harness.key_press(Key::F1);
    steps(&mut harness, 3);
    assert!(shows(&harness, "Not a medical device"));
    assert!(shows(&harness, "Licensed under the MIT licence"));
    harness.get_by_label("Third-party licences…").click();
    steps(&mut harness, 3);
    assert!(!shows(&harness, "Not a medical device"), "in place of the About dialog");
    assert!(shows(&harness, "THIRD-PARTY-NOTICES.txt"), "the explanation names the file");
    assert!(shows(&harness, "SumMRIse third-party notices"), "the text begins");
    assert!(!shows(&harness, "END OF TERMS AND CONDITIONS"), "only the rows on screen are laid out");
    if let Some(dir) = std::env::var_os("SUMMRISE_SNAPSHOT_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("snapshot dir");
        if let Ok(image) = harness.render() {
            image.save(dir.join("about-third-party-licences.png")).expect("save");
        }
    }
    // From the keyboard: End goes to the last line, Page Up back from it, Home to the first.
    harness.key_press(Key::End);
    steps(&mut harness, 12);
    assert!(
        shows(&harness, "OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE"),
        "the end"
    );
    assert!(!shows(&harness, "SumMRIse third-party notices"));
    harness.key_press(Key::PageUp);
    steps(&mut harness, 12);
    assert!(!shows(&harness, "OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE"));
    harness.key_press(Key::Home);
    steps(&mut harness, 12);
    assert!(shows(&harness, "SumMRIse third-party notices"), "the beginning again");
    harness.key_press(Key::PageDown);
    steps(&mut harness, 12);
    assert!(!shows(&harness, "SumMRIse third-party notices"), "a page further");

    harness.get_by_label("Close").click();
    steps(&mut harness, 3);
    assert!(!shows(&harness, "Scroll with the mouse"), "Close closes them");

    harness.key_press(Key::F1);
    steps(&mut harness, 3);
    harness.get_by_label("Third-party licences…").click();
    steps(&mut harness, 3);
    harness.key_press(Key::Escape);
    steps(&mut harness, 3);
    assert!(!shows(&harness, "Scroll with the mouse"), "Escape closes them");
}
