//! Telling the user about a crash (OpenSpec change `crash-reports`): what
//! happened, that the report holds no patient data, and where it is.
//!
//! In the window: the crash that ended the previous run (when the message after
//! it could not be shown), and a fault that stopped part of the program while
//! the window lives on. After a fault that closed the window, the binary shows
//! [`fatal_message`] in a message box of the system.

use std::path::{Path, PathBuf};

use egui::{Id, RichText};
use summrise_i18n::tr;

use crate::theme;

/// Which crash the notice is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum When {
    /// It ended the previous run.
    LastRun,
    /// It stopped part of the program, which runs on.
    ThisRun,
}

/// A notice about a crash report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CrashNotice {
    pub(crate) when: When,
    pub(crate) report: PathBuf,
}

impl CrashNotice {
    /// Shows the notice; `false` once the user has closed it.
    pub(crate) fn show(&self, ctx: &egui::Context) -> bool {
        let response = egui::Modal::new(Id::unique("crash notice")).show(ctx, |ui| {
            ui.set_width(480.0);
            let (title, text) = match self.when {
                When::LastRun => (
                    tr!("SumMRIse closed unexpectedly"),
                    tr!("The last time SumMRIse was used, it stopped because of a fault in the program."),
                ),
                When::ThisRun => (
                    tr!("Something went wrong"),
                    tr!("Part of SumMRIse stopped because of a fault in the program, and what it was doing may be \
                         incomplete. If something looks wrong, close SumMRIse and open it again."),
                ),
            };
            ui.heading(title);
            ui.add_space(6.0);
            ui.label(text);
            ui.add_space(6.0);
            ui.label(report_text());
            ui.add_space(4.0);
            let path = self.report.display().to_string();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(&path).monospace().color(theme::palette(ui).text_weak)).wrap());
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(tr!("Copy path")).clicked() {
                    ui.ctx().copy_text(path.clone());
                }
                ui.button(tr!("Close")).clicked()
            })
            .inner
        });
        !(response.inner || response.should_close())
    }
}

/// What the report is and what to do with it.
fn report_text() -> &'static str {
    tr!("A report was saved. It contains no patient data: no names, IDs, dates of birth, file names or values \
         from the images. You can send it with a bug report to help fix the fault.")
}

/// The title and text of the message box after a fault closed the window;
/// `report` is where the report is, if it could be written.
pub fn fatal_message(report: Option<&Path>) -> (String, String) {
    let title = tr!("SumMRIse closed unexpectedly").to_owned();
    let mut text = tr!("SumMRIse stopped because of a fault in the program.").to_owned();
    match report {
        Some(path) => {
            text.push_str("\n\n");
            text.push_str(report_text());
            text.push_str("\n\n");
            text.push_str(&path.display().to_string());
        }
        None => {
            text.push(' ');
            text.push_str(tr!("No report could be saved."));
        }
    }
    (title, text)
}

/// The title and text of the message box when the window cannot be opened;
/// `reason` is what the graphics system said.
pub fn start_failure_message(reason: &str) -> (String, String) {
    (tr!("SumMRIse could not start").to_owned(), tr!("The window could not be opened: {reason}", reason = reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_after_a_crash_says_where_the_report_is() {
        let path = Path::new("/home/someone/.local/state/summrise/crash-reports/SumMRIse-crash-x.txt");
        let (title, text) = fatal_message(Some(path));
        assert_eq!(title, "SumMRIse closed unexpectedly");
        assert!(text.starts_with("SumMRIse stopped because of a fault in the program."));
        assert!(text.contains("no patient data"));
        assert!(text.ends_with(&path.display().to_string()));
        let (_, text) = fatal_message(None);
        assert!(text.ends_with("No report could be saved."));
        let (title, text) = start_failure_message("no suitable graphics adapter");
        assert_eq!(title, "SumMRIse could not start");
        assert!(text.contains("no suitable graphics adapter"));
    }
}
