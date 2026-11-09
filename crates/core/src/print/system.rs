//! Handing a PDF to the print system (design D10): `lp` and `lpstat` of CUPS on
//! macOS and Linux, through absolute paths so that nothing on a disc or in the
//! working directory can be run by accident. The PDF goes to standard input, so
//! no file of ours exists at any moment, and the job title is neutral because
//! print queues are logged. Where there is no `lp`, the PDF goes to a place the
//! user chooses and opens in the default application for PDFs.
//!
//! Windows has no CUPS, and a path such as `/usr/bin/lp` would mean that folder
//! on the current drive there, which may be the disc: on Windows there is no
//! `lp` at all, and saved PDFs open through the Explorer of the system folder.
//!
//! [`PrintSystem::at`] is the seam for tests: a fake script stands in.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use summrise_i18n::tr;

use super::Paper;

/// The title of every job: print queues are logged.
pub const JOB_TITLE: &str = "SumMRIse";

/// The programs printing goes through.
#[derive(Debug, Clone)]
pub struct PrintSystem {
    /// `lp` and `lpstat`; `None` where printing does not go through CUPS (Windows).
    cups: Option<Cups>,
    /// What opens a file in its default application.
    opener: PathBuf,
}

/// The CUPS programs, by absolute path.
#[derive(Debug, Clone)]
struct Cups {
    lp: PathBuf,
    lpstat: PathBuf,
}

impl Default for PrintSystem {
    fn default() -> Self {
        Self::system()
    }
}

/// The printers the system lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Printers {
    pub names: Vec<String>,
    pub default: Option<String>,
}

/// One job: nothing here is about the patient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintJob {
    /// `None`: the system's default printer.
    pub printer: Option<String>,
    pub copies: u32,
    pub paper: Paper,
}

impl PrintJob {
    /// The arguments of `lp`; the PDF goes to its standard input.
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = Vec::new();
        if let Some(printer) = &self.printer {
            arguments.extend(["-d".to_owned(), printer.clone()]);
        }
        arguments.extend([
            "-n".to_owned(),
            self.copies.max(1).to_string(),
            "-t".to_owned(),
            JOB_TITLE.to_owned(),
            "-o".to_owned(),
            format!("media={}", self.paper.media()),
        ]);
        arguments
    }
}

/// What can go wrong; the text is the print system's where it has one. Shown
/// to the user as it is.
#[derive(Debug, thiserror::Error)]
pub enum PrintError {
    /// The program could not be started.
    Start(#[source] std::io::Error),
    /// The print system said no.
    Refused(String),
}

impl std::fmt::Display for PrintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Start(reason) => tr!("The print system could not be started: {reason}", reason = reason),
            Self::Refused(reason) => reason.clone(),
        };
        f.write_str(&text)
    }
}

impl PrintSystem {
    /// The programs of this computer.
    pub fn system() -> Self {
        if cfg!(windows) {
            return Self { cups: None, opener: windows_explorer(std::env::var_os("SystemRoot")) };
        }
        let opener = if cfg!(target_os = "macos") { "/usr/bin/open" } else { "/usr/bin/xdg-open" };
        let cups = Cups { lp: PathBuf::from("/usr/bin/lp"), lpstat: PathBuf::from("/usr/bin/lpstat") };
        Self { cups: Some(cups), opener: PathBuf::from(opener) }
    }

    /// Other programs in their place, for tests.
    pub fn at(lp: PathBuf, lpstat: PathBuf, opener: PathBuf) -> Self {
        Self { cups: Some(Cups { lp, lpstat }), opener }
    }

    /// Whether this computer has a print system to hand pages to.
    pub fn is_available(&self) -> bool {
        self.cups.as_ref().is_some_and(|cups| cups.lp.is_file())
    }

    /// The CUPS programs, or why there are none.
    fn cups(&self) -> Result<&Cups, PrintError> {
        self.cups.as_ref().ok_or_else(|| {
            PrintError::Start(std::io::Error::new(std::io::ErrorKind::NotFound, "this system has no CUPS"))
        })
    }

    /// The printers the system lists and its default one.
    pub fn printers(&self) -> Result<Printers, PrintError> {
        // In English whatever the system's language is: this output is parsed.
        let lpstat = &self.cups()?.lpstat;
        let list = |flag: &str| {
            Command::new(lpstat).arg(flag).env("LC_ALL", "C").stdin(Stdio::null()).output().map_err(PrintError::Start)
        };
        let names = list("-e")?;
        if !names.status.success() {
            return Err(PrintError::Refused(reason_of(&names)));
        }
        let names: Vec<String> = String::from_utf8_lossy(&names.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        let default = list("-d")
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| parse_default(&String::from_utf8_lossy(&output.stdout)))
            .filter(|default| names.contains(default));
        Ok(Printers { names, default })
    }

    /// Hands `pdf` to the print system on standard input. Nothing is retried.
    pub fn send(&self, pdf: &[u8], job: &PrintJob) -> Result<(), PrintError> {
        let mut child = Command::new(&self.cups()?.lp)
            .args(job.arguments())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(PrintError::Start)?;
        let stdin = child.stdin.take();
        // On a thread of its own: `lp` may answer before it has read everything, and
        // either side blocks when a pipe is full.
        let output = std::thread::scope(|scope| {
            scope.spawn(move || {
                if let Some(mut stdin) = stdin {
                    // A broken pipe means that `lp` has stopped reading: its exit status says why.
                    let _ = stdin.write_all(pdf);
                }
            });
            child.wait_with_output()
        })
        .map_err(PrintError::Start)?;
        if output.status.success() { Ok(()) } else { Err(PrintError::Refused(reason_of(&output))) }
    }

    /// Opens the saved file `path` in the default application for PDFs.
    pub fn open(&self, path: &Path) -> Result<(), PrintError> {
        let output = Command::new(&self.opener).arg(path).stdin(Stdio::null()).output().map_err(PrintError::Start)?;
        // The Windows Explorer gives no useful exit code.
        if output.status.success() || cfg!(windows) { Ok(()) } else { Err(PrintError::Refused(reason_of(&output))) }
    }
}

/// The Explorer in the Windows folder `system_root` (`%SystemRoot%`); the
/// usual folder when that is not an absolute path.
fn windows_explorer(system_root: Option<std::ffi::OsString>) -> PathBuf {
    match system_root.map(PathBuf::from) {
        Some(root) if root.is_absolute() => root.join("explorer.exe"),
        _ => PathBuf::from("C:\\Windows\\explorer.exe"),
    }
}

/// What a program said about its failure.
fn reason_of(output: &std::process::Output) -> String {
    let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_owned();
    let (error, normal) = (said(&output.stderr), said(&output.stdout));
    if !error.is_empty() {
        error
    } else if !normal.is_empty() {
        normal
    } else {
        output.status.to_string()
    }
}

/// The printer in "system default destination: Office".
fn parse_default(text: &str) -> Option<String> {
    let line = text.lines().find(|line| line.contains("system default destination:"))?;
    let name = line.rsplit(':').next()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_printer_output_is_understood() {
        assert_eq!(parse_default("system default destination: Office\n").as_deref(), Some("Office"));
        assert_eq!(parse_default("no system default destination\n"), None);
        assert_eq!(parse_default(""), None);
    }

    #[test]
    fn nothing_but_the_job_is_passed_to_lp() {
        let job = PrintJob { printer: None, copies: 0, paper: Paper::Letter };
        assert_eq!(job.arguments(), ["-n", "1", "-t", "SumMRIse", "-o", "media=Letter"]);
    }

    #[test]
    fn a_missing_print_system_is_noticed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = |name: &str| dir.path().join(name);
        let system = PrintSystem::at(path("lp"), path("lpstat"), path("open"));
        assert!(!system.is_available());
        assert!(matches!(system.printers(), Err(PrintError::Start(_))));
        let job = PrintJob { printer: None, copies: 1, paper: Paper::A4 };
        assert!(matches!(system.send(b"%PDF", &job), Err(PrintError::Start(_))));
        assert!(matches!(system.open(&path("a.pdf")), Err(PrintError::Start(_))));
    }

    /// Absolute paths only: nothing on a disc or in the working directory is run.
    #[test]
    fn the_real_programs_are_given_by_absolute_paths() {
        let system = PrintSystem::system();
        assert!(system.opener.is_absolute(), "{}", system.opener.display());
        if let Some(cups) = &system.cups {
            assert!(cups.lp.is_absolute() && cups.lpstat.is_absolute());
        }
    }

    /// On Windows there is no CUPS: nothing is looked for in `\usr\bin` of the
    /// current drive, which may be the disc, and saved PDFs open through the
    /// Explorer of the system folder.
    #[cfg(windows)]
    #[test]
    fn windows_has_no_lp_and_opens_pdfs_with_the_explorer_of_the_system() {
        let system = PrintSystem::system();
        assert!(system.cups.is_none());
        assert!(!system.is_available());
        assert!(matches!(system.printers(), Err(PrintError::Start(_))));
        let job = PrintJob { printer: None, copies: 1, paper: Paper::A4 };
        assert!(matches!(system.send(b"%PDF", &job), Err(PrintError::Start(_))));
        assert!(system.opener.ends_with("explorer.exe"));
        assert!(system.opener.is_file(), "{} exists", system.opener.display());
    }

    #[test]
    fn the_explorer_comes_from_an_absolute_system_folder_only() {
        let root = if cfg!(windows) { "D:\\Win" } else { "/win" };
        assert_eq!(windows_explorer(Some(root.into())), Path::new(root).join("explorer.exe"));
        let usual = PathBuf::from("C:\\Windows\\explorer.exe");
        assert_eq!(windows_explorer(Some("Windows".into())), usual, "a relative folder is not used");
        assert_eq!(windows_explorer(None), usual);
    }
}

// Fake print systems are shell scripts.
#[cfg(all(test, unix))]
pub(crate) mod fake {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;
    use crate::print::tests::{Blocks, picture};
    use crate::print::{PRINT_DPI, PageHeader, PageOrientation, PageSetup, PdfWriter, compose_page};

    /// A folder with fake `lp`, `lpstat` and `open` scripts. The fake `lp` keeps
    /// its arguments and what arrives on its standard input in `record/`.
    pub(crate) struct Fake {
        pub(crate) dir: tempfile::TempDir,
    }

    impl Fake {
        pub(crate) fn new(lp_body: &str) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = dir.path().display();
            std::fs::create_dir(dir.path().join("record")).expect("record folder");
            let script = |name: &str, body: String| {
                let path = dir.path().join(name);
                std::fs::write(&path, format!("#!/bin/sh\n[ \"$1\" = --probe ] && exit 0\n{body}\n")).expect("script");
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("permissions");
                // A thread forking at the moment the file was written can keep it busy for a moment.
                let start = std::time::Instant::now();
                while Command::new(&path).arg("--probe").status().is_err() {
                    assert!(start.elapsed().as_secs() < 5, "the script does not start");
                }
            };
            script("lp", lp_body.replace("{record}", &format!("{root}/record")));
            script(
                "lpstat",
                format!(
                    "echo \"$LC_ALL\" > {root}/record/lpstat-locale\n\
                     case \"$1\" in\n\
                     -e) printf 'Home\\nOffice\\n' ;;\n\
                     -d) echo 'system default destination: Office' ;;\n\
                     esac"
                ),
            );
            script("open", format!("echo \"$1\" > {root}/record/opened"));
            Self { dir }
        }

        pub(crate) fn system(&self) -> PrintSystem {
            let path = |name: &str| self.dir.path().join(name);
            PrintSystem::at(path("lp"), path("lpstat"), path("open"))
        }

        pub(crate) fn record(&self, name: &str) -> PathBuf {
            self.dir.path().join("record").join(name)
        }
    }

    /// Reads all of standard input, like the real `lp`.
    pub(crate) const ACCEPTING: &str =
        "printf '%s\\n' \"$@\" > {record}/args\ncat > {record}/stdin\necho 'request id is Home-1 (1 file(s))'";

    fn pdf() -> Vec<u8> {
        let setup =
            PageSetup { paper: Paper::A4, orientation: PageOrientation::Portrait, per_page: 1, dpi: PRINT_DPI / 4 };
        let page = compose_page(
            &setup,
            &PageHeader::default(),
            0,
            1,
            &[picture(16, 16, 80, Some(1.0))],
            true,
            &mut Blocks::default(),
        );
        let mut writer = PdfWriter::new();
        writer.add_page(&page.image, setup.dpi).expect("page");
        writer.finish()
    }

    #[test]
    fn the_printers_and_the_default_come_from_lpstat_in_english() {
        let fake = Fake::new(ACCEPTING);
        let system = fake.system();
        assert!(system.is_available());
        let printers = system.printers().expect("printers");
        assert_eq!(printers.names, ["Home", "Office"]);
        assert_eq!(printers.default.as_deref(), Some("Office"));
        assert_eq!(std::fs::read_to_string(fake.record("lpstat-locale")).expect("record").trim(), "C");
    }

    /// Spec "Printing on macOS or Linux with CUPS" and task 4.5: `lp` gets the
    /// PDF on standard input with the printer, the copies and the title, and
    /// nothing else: no file of the application appears anywhere.
    #[test]
    fn lp_receives_the_pdf_on_standard_input_with_printer_copies_and_title() {
        let fake = Fake::new(ACCEPTING);
        let pdf = pdf();
        let temp = std::env::temp_dir();
        let pdfs_in_temp = || {
            let mut names: Vec<_> = std::fs::read_dir(&temp)
                .expect("temp")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".pdf") || name.contains("SumMRIse"))
                .collect();
            names.sort();
            names
        };
        let before = pdfs_in_temp();
        let job = PrintJob { printer: Some("Home".to_owned()), copies: 2, paper: Paper::A4 };
        fake.system().send(&pdf, &job).expect("printed");

        let received = std::fs::read(fake.record("stdin")).expect("what lp read");
        assert_eq!(received, pdf, "all of the PDF, unchanged");
        assert!(received.starts_with(b"%PDF"));
        let arguments = std::fs::read_to_string(fake.record("args")).expect("arguments");
        assert_eq!(
            arguments.lines().collect::<Vec<_>>(),
            ["-d", "Home", "-n", "2", "-t", "SumMRIse", "-o", "media=A4"]
        );
        // Only what the fake itself wrote is there.
        let mut recorded: Vec<_> = std::fs::read_dir(fake.dir.path().join("record"))
            .expect("record")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        recorded.sort();
        assert_eq!(recorded, ["args", "stdin"]);
        assert_eq!(pdfs_in_temp(), before, "no PDF of the application in the temporary folder");
    }

    /// Spec "Printer refuses the job": its error text is reported.
    #[test]
    fn an_error_of_lp_is_reported_with_its_text() {
        // It does not even read the PDF, which is far larger than a pipe holds.
        let fake = Fake::new("echo 'lp: The printer or class does not exist.' >&2\nexit 1");
        let job = PrintJob { printer: Some("Gone".to_owned()), copies: 1, paper: Paper::A4 };
        let error = fake.system().send(&vec![0u8; 4 << 20], &job).expect_err("refused");
        assert_eq!(error.to_string(), "lp: The printer or class does not exist.");
    }

    #[test]
    fn a_saved_pdf_is_opened_with_the_default_application() {
        let fake = Fake::new(ACCEPTING);
        let file = fake.dir.path().join("SumMRIse-CT-S1.pdf");
        fake.system().open(&file).expect("opened");
        assert_eq!(std::fs::read_to_string(fake.record("opened")).expect("record").trim(), file.display().to_string());
    }
}
