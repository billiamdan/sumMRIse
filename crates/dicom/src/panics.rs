//! Panics that are expected, and the crash report hook of the application.
//!
//! A damaged file can make the DICOM library (or a document parser) panic.
//! Such panics are caught and become errors ("the image could not be
//! decoded"): they are an expected outcome, not a crash. [`catch`] runs code
//! whose panics are expected, so that the hook that writes crash reports
//! (`summrise_core::crash`) can tell them from faults that stop the program.
//!
//! Panics on the worker threads of rayon are a special case: rayon passes them
//! on to the thread that started the parallel work, without calling the hook
//! again, and only there is it known whether they are caught. The hook
//! therefore [holds](hold) the report of a worker's panic; [`catch`] drops it
//! when it catches the panic, and the places where a panic ends a thread
//! write it.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, PoisonError};

thread_local! {
    /// How many calls of [`catch`] are running on this thread.
    static CATCHING: Cell<u32> = const { Cell::new(0) };
}

/// The report of a panic on a worker thread, until it is known whether the
/// panic is caught.
static HELD: Mutex<Option<String>> = Mutex::new(None);

/// Runs `work` and catches a panic inside it; while it runs, panics on this
/// thread are expected and leave no crash report.
pub fn catch<R>(work: impl FnOnce() -> R) -> std::thread::Result<R> {
    CATCHING.with(|c| c.set(c.get() + 1));
    let result = catch_unwind(AssertUnwindSafe(work));
    CATCHING.with(|c| c.set(c.get().saturating_sub(1)));
    if result.is_err() {
        // A worker's panic passed on to this thread was expected too.
        drop(take_held());
    }
    result
}

/// Whether a panic on this thread now would be caught by [`catch`].
pub fn is_expected() -> bool {
    CATCHING.with(|c| c.get() > 0)
}

/// Keeps the report of a panic on a worker thread; the first one is kept when
/// several workers panic.
pub fn hold(report: String) {
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    if held.is_none() {
        *held = Some(report);
    }
}

/// The held report, which is no longer held.
pub fn take_held() -> Option<String> {
    HELD.lock().unwrap_or_else(PoisonError::into_inner).take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caught_panic_is_expected_while_it_happens_and_drops_a_held_report() {
        assert!(!is_expected());
        let seen = catch(is_expected).expect("no panic");
        assert!(seen, "expected inside");
        assert!(!is_expected(), "not after");
        let nested = catch(|| catch(is_expected).expect("inner") && is_expected()).expect("outer");
        assert!(nested);

        // Silence the default hook for the deliberate panic.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        hold("a worker panicked".to_owned());
        let result = catch(|| panic!("expected"));
        std::panic::set_hook(previous);
        assert!(result.is_err());
        assert!(!is_expected(), "the count is restored after a panic");
        assert_eq!(take_held(), None, "the report of the worker is dropped");
    }
}
