//! The licences of the third-party software in the application, opened from the
//! About dialog. The text is `THIRD-PARTY-NOTICES.txt`, written by
//! `cargo xtask notices` and shipped next to the application in every package;
//! it is compiled in so that it can be read wherever the application runs.

use std::sync::OnceLock;

use egui::{Id, RichText, TextStyle};
use summrise_i18n::tr;

/// The notices, as generated.
pub(crate) const NOTICES: &str = include_str!("../../../THIRD-PARTY-NOTICES.txt");

/// The lines of the notices, split once.
fn lines() -> &'static [&'static str] {
    static LINES: OnceLock<Vec<&'static str>> = OnceLock::new();
    LINES.get_or_init(|| NOTICES.lines().collect())
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
        ui.add_space(6.0);
        let row_height = ui.text_style_height(&TextStyle::Monospace);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            // Only the rows on screen are laid out: the text has about ten thousand lines.
            egui::ScrollArea::both().max_height(height).auto_shrink([false, true]).show_rows(
                ui,
                row_height,
                lines().len(),
                |ui, rows| {
                    for line in &lines()[rows] {
                        ui.add(egui::Label::new(RichText::new(*line).monospace()).extend());
                    }
                },
            );
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
        assert!(lines().len() > 1000);
    }
}
