//! The licences of the third-party software in the application, opened from the
//! About dialog. The text is `THIRD-PARTY-NOTICES.txt`, written by
//! `cargo xtask notices` and shipped next to the application in every package;
//! it is compiled in so that it can be read wherever the application runs.

use std::sync::OnceLock;

use egui::{Id, Key, Modifiers, RichText, TextStyle};
use summrise_i18n::tr;

/// The notices, as generated.
pub(crate) const NOTICES: &str = include_str!("../../../THIRD-PARTY-NOTICES.txt");

/// The longest line shown: a few licence texts come as one line per paragraph,
/// up to several hundred characters long.
const COLUMNS: usize = 100;

/// The lines of the notices as they are shown, split once: long lines are
/// broken between words.
fn lines() -> &'static [&'static str] {
    static LINES: OnceLock<Vec<&'static str>> = OnceLock::new();
    LINES.get_or_init(|| NOTICES.lines().flat_map(broken).collect())
}

/// `line` in pieces of at most [`COLUMNS`] characters, broken at spaces where
/// there are any.
fn broken(mut line: &'static str) -> Vec<&'static str> {
    let mut pieces = Vec::new();
    while line.chars().count() > COLUMNS {
        // The byte just past the last character that fits.
        let limit = line.char_indices().nth(COLUMNS).map_or(line.len(), |(at, _)| at);
        let (head, rest) = match line[..limit].rfind(' ').filter(|at| *at > 0) {
            Some(space) => (&line[..space], line[space..].trim_start_matches(' ')),
            None => line.split_at(limit),
        };
        pieces.push(head);
        line = rest;
    }
    pieces.push(line);
    pieces
}

/// Shows the licences; `false` once the user has closed them.
pub(crate) fn show(ctx: &egui::Context) -> bool {
    let screen = ctx.content_rect();
    let width = (screen.width() - 48.0).clamp(320.0, 820.0);
    let height = (screen.height() - 220.0).clamp(160.0, 560.0);
    let response = egui::Modal::new(Id::unique("third-party licences")).show(ctx, |ui| {
        ui.set_width(width);
        ui.heading(tr!("Third-party licences"));
        ui.add_space(4.0);
        ui.label(tr!(
            "SumMRIse is built with software by others. Their licences follow in their original wording, in \
             English. The same text is in the file THIRD-PARTY-NOTICES.txt that comes with the application."
        ));
        ui.label(RichText::new(tr!("Scroll with the mouse or with Page Up, Page Down, Home and End.")).small().weak());
        ui.add_space(6.0);
        let row_height = ui.text_style_height(&TextStyle::Monospace);
        // From the keyboard too. The position is set, not animated: the text is long,
        // and Home and End go to its very ends. It is kept between frames together
        // with the furthest position there is.
        let state = Id::unique("third-party licences position");
        let (position, furthest): (f32, f32) = ui.data(|data| data.get_temp(state)).unwrap_or_default();
        let (page, line) = (height * 0.9, row_height * 3.0);
        let wanted = ui.input_mut(|input| {
            let mut pressed = |key| input.consume_key(Modifiers::NONE, key);
            if pressed(Key::Home) {
                Some(0.0)
            } else if pressed(Key::End) {
                Some(furthest)
            } else if pressed(Key::PageDown) {
                Some(position + page)
            } else if pressed(Key::PageUp) {
                Some(position - page)
            } else if pressed(Key::ArrowDown) {
                Some(position + line)
            } else if pressed(Key::ArrowUp) {
                Some(position - line)
            } else {
                None
            }
        });
        egui::Frame::group(ui.style()).show(ui, |ui| {
            let mut area = egui::ScrollArea::both().max_height(height).auto_shrink([false, true]);
            if let Some(wanted) = wanted {
                area = area.vertical_scroll_offset(wanted.clamp(0.0, furthest));
            }
            // Only the rows on screen are laid out: the text has about ten thousand lines.
            let shown = area.show_rows(ui, row_height, lines().len(), |ui, rows| {
                for line in &lines()[rows] {
                    ui.add(egui::Label::new(RichText::new(*line).monospace()).extend());
                }
            });
            let furthest = (shown.content_size.y - shown.inner_rect.height()).max(0.0);
            ui.data_mut(|data| data.insert_temp(state, (shown.state.clamped_offset().y, furthest)));
        });
        ui.add_space(6.0);
        ui.vertical_centered(|ui| ui.button(tr!("Close")).clicked()).inner
    });
    !(response.inner || response.should_close())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The notices name the libraries the application is built on, the C++ library
    /// of the JPEG-LS decoder, and the acknowledgement the IJG licence asks for.
    #[test]
    fn the_notices_name_what_the_application_is_built_with() {
        assert!(NOTICES.starts_with("SumMRIse third-party notices"));
        for component in ["egui", "eframe", "wgpu", "dicom-object", "dicom-pixeldata", "charls-sys", "hayro", "zip"] {
            assert!(NOTICES.lines().any(|line| line.starts_with(&format!("{component} — "))), "{component} is listed");
        }
        assert!(NOTICES.contains("Jan de Vaan and Victor Derks"), "the copyright of CharLS");
        assert!(NOTICES.contains("based in part on the work of the Independent JPEG Group"));
        assert!(NOTICES.contains("SIL OPEN FONT LICENSE Version 1.1"));
        // What no Cargo.lock lists: the standard library, and a font inside a crate.
        assert!(NOTICES.lines().any(|line| line.starts_with("Rust standard library")));
        assert!(NOTICES.contains("Cantarell"));
        // Copyright lines of files that only point to their licences are kept.
        assert!(NOTICES.contains("Frank Denis"));
        assert!(lines().len() > 1000);
    }

    /// Long lines are broken between words for the window; nothing is lost.
    #[test]
    fn long_lines_are_broken_between_words() {
        assert!(NOTICES.lines().any(|line| line.chars().count() > 300), "there are long lines to break");
        assert!(lines().iter().all(|line| line.chars().count() <= COLUMNS));
        // A word longer than a line (one address is) is cut; nothing else changes.
        let characters = |text: &str| text.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        assert!(characters(&lines().concat()) == characters(NOTICES), "the same characters in order");
        assert_eq!(broken("short"), ["short"]);
        let unbroken: &'static str = Box::leak("x".repeat(250).into_boxed_str());
        assert_eq!(broken(unbroken).iter().map(|piece| piece.len()).collect::<Vec<_>>(), [100, 100, 50]);
    }
}
