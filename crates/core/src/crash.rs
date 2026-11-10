//! Crash reports (OpenSpec change `crash-reports`).
//!
//! When the program stops because of a fault (a panic that nothing catches),
//! a short plain-text report is written to the user's log folder, so that the
//! user can be told what happened and can pass the report on. The report
//! holds nothing about the patient, by construction: only what is fixed in the
//! program itself (version, the place in the source code, the functions that
//! were running, a message only if it is literal text of the program) and
//! about the computer (system, processor, time). A message composed while the
//! program ran — which could quote a name, a file path or a value read from a
//! disc — is withheld, never filtered. Expected panics, on damaged files, leave
//! no report ([`summrise_dicom::panics`]).
//!
//! The binary installs the hook with [`install`]; the interface asks for
//! reports written while it runs with [`take_new_report`], and the binary asks
//! for the report of a crash that ended the previous run with
//! [`report_of_last_run`].

use std::backtrace::Backtrace;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use summrise_dicom::panics;

/// Reports kept in the folder; older ones are deleted.
pub const KEPT: usize = 5;
/// The beginning of the name of every report.
const PREFIX: &str = "SumMRIse-crash-";
/// The file naming the report of a crash that ended the program and that the
/// user has not been told about yet.
const UNSEEN: &str = "unseen.txt";

/// The folder of the reports, once the hook is installed.
static FOLDER: OnceLock<Option<PathBuf>> = OnceLock::new();
/// A report written while the program runs that the interface has not shown.
static NEW: Mutex<Option<PathBuf>> = Mutex::new(None);
/// The report of the fault that ended the program.
static FATAL: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The folder for crash reports of this user: `~/Library/Logs/SumMRIse` on
/// macOS, `%LOCALAPPDATA%\SumMRIse\Crash reports` on Windows and
/// `$XDG_STATE_HOME/summrise/crash-reports` (`~/.local/state/…`) elsewhere.
pub fn default_folder() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute());
    if cfg!(target_os = "macos") {
        Some(var("HOME")?.join("Library/Logs/SumMRIse"))
    } else if cfg!(windows) {
        Some(var("LOCALAPPDATA")?.join("SumMRIse").join("Crash reports"))
    } else {
        let state = var("XDG_STATE_HOME").or_else(|| Some(var("HOME")?.join(".local/state")))?;
        Some(state.join("summrise/crash-reports"))
    }
}

/// Installs the hook that writes crash reports to `folder` (no reports
/// without one). In debug builds the standard hook prints the panic as well,
/// with its message, for developers; release builds print only where the
/// report is.
pub fn install(folder: Option<PathBuf>) {
    let _ = FOLDER.set(folder);
    let standard = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        hook(info);
        if cfg!(debug_assertions) {
            standard(info);
        }
    }));
}

fn hook(info: &PanicHookInfo<'_>) {
    if panics::is_expected() {
        log::debug!("an expected panic was caught at {}", location(info));
        return;
    }
    let text = compose(&Facts::of(info, SystemTime::now()));
    if rayon::current_thread_index().is_some() {
        // rayon passes it on to the thread that started the work: decided there.
        panics::hold(text);
        return;
    }
    let fatal = std::thread::current().name() == Some("main");
    record(text, fatal);
}

/// Writes the held report of a worker thread's panic, if there is one: called
/// where a panic passed on by rayon ends a thread or the program.
pub fn flush_held(fatal: bool) -> Option<PathBuf> {
    record(panics::take_held()?, fatal)
}

/// Runs `work`, the whole of a thread's life: if a panic passed on by rayon
/// ends it, the held report is written before the thread ends.
pub fn watch<R>(work: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(result) => result,
        Err(panic) => {
            flush_held(false);
            std::panic::resume_unwind(panic)
        }
    }
}

/// Writes `text` (with a held report of a worker appended) to the folder of
/// the hook. A `fatal` report is the one to tell about at the next start, if
/// the user is not told at once.
fn record(mut text: String, fatal: bool) -> Option<PathBuf> {
    if let Some(earlier) = panics::take_held() {
        text.push_str("\n\nEarlier, on a worker thread:\n\n");
        text.push_str(&earlier);
    }
    let folder = FOLDER.get()?.as_deref()?;
    match write_report(folder, &text, SystemTime::now(), fatal) {
        Ok(path) => {
            // Only what the report holds; never the message of the panic.
            log::error!("SumMRIse stopped because of a fault; crash report: {}", path.display());
            let slot = if fatal { &FATAL } else { &NEW };
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(path.clone());
            Some(path)
        }
        Err(e) => {
            log::error!("the crash report could not be written: {e}");
            None
        }
    }
}

/// A report written while the program runs, once: the interface tells the user.
pub fn take_new_report() -> Option<PathBuf> {
    NEW.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// The report of the fault that ended the program, once the hook has written
/// it: the binary tells the user where it is.
pub fn fatal_report() -> Option<PathBuf> {
    FATAL.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The report of a crash that ended the previous run, if the user has not been
/// told about it; it is not returned again.
pub fn report_of_last_run(folder: &Path) -> Option<PathBuf> {
    let unseen = folder.join(UNSEEN);
    let name = fs::read_to_string(&unseen).ok()?;
    let _ = fs::remove_file(&unseen);
    let name = name.trim();
    // Only a report of ours in the folder, whatever the file says.
    let valid = name.starts_with(PREFIX) && name.ends_with(".txt") && !name.contains(['/', '\\']);
    let path = folder.join(name);
    (valid && path.is_file()).then_some(path)
}

/// The user has been told about the crash that ended this run.
pub fn mark_told(folder: &Path) {
    let _ = fs::remove_file(folder.join(UNSEEN));
}

/// Writes a report named after its time, deletes all but the newest
/// [`KEPT`], and for a `fatal` one leaves its name for the next start.
fn write_report(folder: &Path, text: &str, now: SystemTime, fatal: bool) -> std::io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let stamp = utc(now).file_stamp();
    let mut path = folder.join(format!("{PREFIX}{stamp}.txt"));
    let mut file = None;
    for n in 1..=20 {
        if n > 1 {
            path = folder.join(format!("{PREFIX}{stamp}-{n}.txt"));
        }
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(created) => {
                file = Some(created);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    let mut file = file.ok_or_else(|| std::io::Error::other("too many reports in one second"))?;
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")?;
    drop(file);
    let mut reports: Vec<PathBuf> = fs::read_dir(folder)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(".txt"))
        })
        .collect();
    reports.sort_by_key(|p| report_order(p));
    let excess = reports.len().saturating_sub(KEPT);
    // Never the one just written, even if the clock went back.
    for old in reports.into_iter().filter(|p| *p != path).take(excess) {
        let _ = fs::remove_file(old);
    }
    if fatal && let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        fs::write(folder.join(UNSEEN), name)?;
    }
    Ok(path)
}

/// What a report says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    pub(crate) time: SystemTime,
    pub(crate) thread: String,
    pub(crate) location: String,
    /// The message, if it is literal text of the program.
    pub(crate) message: Option<&'static str>,
    pub(crate) backtrace: String,
}

impl Facts {
    fn of(info: &PanicHookInfo<'_>, time: SystemTime) -> Self {
        Self {
            time,
            thread: thread_label(std::thread::current().name()),
            location: location(info),
            // `panic!("text")` and `.unwrap()` on `None` carry a `&'static str`; a
            // message composed at run time (`format!`, `expect`, the `Debug` of an
            // error) is a `String` and is withheld.
            message: info.payload().downcast_ref::<&'static str>().copied(),
            backtrace: shorten_paths(&Backtrace::force_capture().to_string()),
        }
    }
}

/// The text of a report.
pub(crate) fn compose(facts: &Facts) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "SumMRIse crash report\n=====================\n");
    text.push_str(
        "SumMRIse stopped because of a fault in the program. This report holds no patient data: no\n\
         names, IDs, dates of birth, names of files or folders, and no values read from the opened\n\
         files. It says where in the program the fault happened, so that it can be found and fixed.\n\
         You may attach it to a bug report.\n\n",
    );
    let build = if cfg!(debug_assertions) { "debug build" } else { "release build" };
    let _ = writeln!(text, "Version:  {} ({build})", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(text, "System:   {} {}", std::env::consts::OS, std::env::consts::ARCH);
    let _ = writeln!(text, "Time:     {}", utc(facts.time).text());
    let _ = writeln!(text, "Thread:   {}", facts.thread);
    let _ = writeln!(text, "Location: {}", facts.location);
    match facts.message {
        Some(message) => {
            let _ = writeln!(text, "Message:  {message}");
        }
        None => text.push_str(
            "Message:  withheld (it was composed while the program ran and could contain data from the\n          \
             opened files)\n",
        ),
    }
    let _ = write!(text, "\nBacktrace:\n{}", facts.backtrace.trim_end());
    text
}

/// The thread, by the names the program gives its threads; anything else is
/// not repeated.
fn thread_label(name: Option<&str>) -> String {
    match name {
        Some("main") => "main (the window)".to_owned(),
        Some(name)
            if name.len() <= 40
                && name.strip_prefix("summrise-").is_some_and(|rest| {
                    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                }) =>
        {
            name.to_owned()
        }
        _ if rayon::current_thread_index().is_some() => "a worker thread".to_owned(),
        _ => "another thread".to_owned(),
    }
}

/// The place in the source code: a path of the program's own source tree.
fn location(info: &PanicHookInfo<'_>) -> String {
    info.location()
        .map_or_else(|| "unknown".to_owned(), |l| format!("{}:{}:{}", shorten_path(l.file()), l.line(), l.column()))
}

/// Source paths without the folders of the computer that built the program:
/// from the crate's folder in Cargo's registry, or from the standard library.
fn shorten_path(path: &str) -> String {
    let unified = path.replace('\\', "/");
    if let Some(at) = unified.find("/registry/src/") {
        // …/registry/src/index.crates.io-<hash>/<crate>-<version>/src/file.rs
        let rest = &unified[at + "/registry/src/".len()..];
        return rest.split_once('/').map_or(rest, |(_, crate_path)| crate_path).to_owned();
    }
    if let Some(at) = unified.find("/library/") {
        return unified[at + 1..].to_owned();
    }
    for root in ["crates/", "apps/", "xtask/"] {
        if let Some(at) = unified.find(root) {
            return unified[at..].to_owned();
        }
    }
    // Something else: its file name only.
    unified.rsplit('/').next().unwrap_or(&unified).to_owned()
}

/// A backtrace with the paths of its "at <file>:<line>" lines shortened.
fn shorten_paths(backtrace: &str) -> String {
    backtrace
        .lines()
        .map(|line| match line.trim_start().strip_prefix("at ") {
            Some(location) => {
                let indent = &line[..line.len() - line.trim_start().len()];
                let (path, position) = split_position(location);
                format!("{indent}at {}{position}", shorten_path(path))
            }
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// "<path>:<line>:<column>" or "<path>:<line>" (Windows has no columns) split
/// into the path and the position; the path may hold a drive letter's colon.
fn split_position(location: &str) -> (&str, &str) {
    let mut path = location;
    for _ in 0..2 {
        match path.rsplit_once(':') {
            Some((head, number)) if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) => path = head,
            _ => break,
        }
    }
    (path, &location[path.len()..])
}

/// The order of reports by time: the stamp in the name, then the number added
/// for a second report in the same second.
fn report_order(path: &Path) -> (String, u32) {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let stem = name.trim_start_matches(PREFIX).trim_end_matches(".txt");
    match stem.split_once("Z-") {
        Some((stamp, n)) => (stamp.to_owned(), n.parse().unwrap_or(1)),
        None => (stem.trim_end_matches('Z').to_owned(), 1),
    }
}

/// A point in time in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Utc {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

impl Utc {
    fn text(self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    fn file_stamp(self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}-{:02}-{:02}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// The calendar date and time of `time` (proleptic Gregorian, UTC).
fn utc(time: SystemTime) -> Utc {
    let seconds = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    Utc {
        year,
        month,
        day,
        hour: (of_day / 3600) as u32,
        minute: (of_day % 3600 / 60) as u32,
        second: (of_day % 60) as u32,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn times_are_calendar_dates_in_utc() {
        assert_eq!(utc(at(0)).text(), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(at(951_782_400)).text(), "2000-02-29 00:00:00 UTC");
        assert_eq!(utc(at(1_794_258_192)).text(), "2026-11-09 21:03:12 UTC");
        assert_eq!(utc(at(1_794_258_192)).file_stamp(), "2026-11-09T21-03-12Z");
        assert_eq!(utc(at(4_107_542_399)).text(), "2100-02-28 23:59:59 UTC");
    }

    #[test]
    fn source_paths_lose_the_folders_of_the_build_machine() {
        assert_eq!(shorten_path("crates/core/src/session.rs"), "crates/core/src/session.rs");
        assert_eq!(
            shorten_path("/Users/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-0.37.0/src/ui.rs"),
            "egui-0.37.0/src/ui.rs"
        );
        assert_eq!(
            shorten_path("C:\\Users\\runner\\.cargo\\registry\\src\\index.crates.io-1\\zip-9.4.0\\src\\read.rs"),
            "zip-9.4.0/src/read.rs"
        );
        assert_eq!(shorten_path("/rustc/abc123/library/core/src/option.rs"), "library/core/src/option.rs");
        assert_eq!(shorten_path("/home/someone/sumMRIse/crates/ui/src/app.rs"), "crates/ui/src/app.rs");
        assert_eq!(shorten_path("/somewhere/else/build.rs"), "build.rs");
        let backtrace = "   0: summrise_core::session::open\n             at /home/x/sumMRIse/crates/core/src/session.rs:12:5\n   1: main";
        assert_eq!(
            shorten_paths(backtrace),
            "   0: summrise_core::session::open\n             at crates/core/src/session.rs:12:5\n   1: main"
        );
        assert_eq!(shorten_paths("  at C:\\b\\crates\\ui\\src\\a.rs:3:4"), "  at crates/ui/src/a.rs:3:4");
        // Windows backtraces have no column.
        assert_eq!(shorten_paths("  at C:\\b\\crates\\ui\\src\\a.rs:42"), "  at crates/ui/src/a.rs:42");
        assert_eq!(shorten_paths("  at C:\\Users\\x\\elsewhere.rs"), "  at elsewhere.rs");
    }

    #[test]
    fn only_the_programs_own_thread_names_are_repeated() {
        assert_eq!(thread_label(Some("main")), "main (the window)");
        assert_eq!(thread_label(Some("summrise-scan")), "summrise-scan");
        assert_eq!(thread_label(Some("summrise-decode-3")), "summrise-decode-3");
        assert_eq!(thread_label(Some("summrise-PHANTOM^SYNTHETIC")), "another thread");
        assert_eq!(thread_label(Some("Doe, Jane")), "another thread");
        assert_eq!(thread_label(None), "another thread");
    }

    #[test]
    fn a_report_says_what_it_holds_and_withholds_a_composed_message() {
        let facts = Facts {
            time: at(1_794_258_192),
            thread: "summrise-scan".to_owned(),
            location: "crates/core/src/session.rs:12:5".to_owned(),
            message: None,
            backtrace: "   0: summrise_core::session::open".to_owned(),
        };
        let text = compose(&facts);
        assert!(text.starts_with("SumMRIse crash report\n"));
        assert!(text.contains("holds no patient data"));
        assert!(text.contains(&format!("Version:  {}", env!("CARGO_PKG_VERSION"))));
        assert!(text.contains("Time:     2026-11-09 21:03:12 UTC"));
        assert!(text.contains("Thread:   summrise-scan"));
        assert!(text.contains("Location: crates/core/src/session.rs:12:5"));
        assert!(text.contains("Message:  withheld"));
        assert!(text.ends_with("   0: summrise_core::session::open"));
        let literal = compose(&Facts { message: Some("called `Option::unwrap()` on a `None` value"), ..facts });
        assert!(literal.contains("Message:  called `Option::unwrap()` on a `None` value"));
    }

    #[test]
    fn reports_are_kept_five_at_a_time_and_a_fatal_one_is_told_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let folder = dir.path().join("Crash reports");
        let mut written = Vec::new();
        for i in 0..7 {
            written.push(write_report(&folder, "report", at(1_794_258_192 + i), false).expect("written"));
        }
        let again = write_report(&folder, "report", at(1_794_258_198), false).expect("same second");
        assert!(again.ends_with("SumMRIse-crash-2026-11-09T21-03-18Z-2.txt"), "{}", again.display());
        assert!(again.exists(), "the newer of two in the same second is kept");
        let mut names: Vec<String> = fs::read_dir(&folder)
            .expect("folder")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into())
            .collect();
        names.sort();
        assert_eq!(names.len(), KEPT, "{names:?}");
        assert_eq!(names[0], "SumMRIse-crash-2026-11-09T21-03-15Z.txt", "the oldest are gone");
        assert_eq!(fs::read_to_string(&written[6]).expect("text"), "report\n");
        assert_eq!(report_of_last_run(&folder), None, "nothing fatal yet");

        let fatal = write_report(&folder, "report", at(1_794_258_200), true).expect("written");
        assert_eq!(report_of_last_run(&folder), Some(fatal.clone()));
        assert_eq!(report_of_last_run(&folder), None, "told once");

        write_report(&folder, "report", at(1_794_258_201), true).expect("written");
        mark_told(&folder);
        assert_eq!(report_of_last_run(&folder), None, "told at once");

        // A clock that went back: the report just written stays.
        let early = write_report(&folder, "report", at(1_794_000_000), false).expect("written");
        assert!(early.exists());

        // The file of the next start names only a report of ours in the folder.
        fs::write(folder.join(UNSEEN), "../../etc/passwd").expect("write");
        assert_eq!(report_of_last_run(&folder), None);
    }

    #[test]
    fn the_folder_is_the_usual_one_of_the_system() {
        let Some(folder) = default_folder() else { return };
        assert!(folder.is_absolute());
        let text = folder.to_string_lossy();
        if cfg!(target_os = "macos") {
            assert!(text.ends_with("Library/Logs/SumMRIse"), "{text}");
        } else if cfg!(windows) {
            assert!(text.ends_with("SumMRIse\\Crash reports"), "{text}");
        } else {
            assert!(text.ends_with("summrise/crash-reports"), "{text}");
        }
    }
}
