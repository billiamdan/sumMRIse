use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Thread-safe progress counters and cancellation flag for long-running
/// operations. Cheap to clone; the UI polls [`Progress::snapshot`].
#[derive(Debug, Clone, Default)]
pub struct Progress(Arc<Inner>);

#[derive(Debug, Default)]
struct Inner {
    done: AtomicUsize,
    total: AtomicUsize,
    cancelled: AtomicBool,
    stage: Mutex<String>,
}

/// A point-in-time copy of [`Progress`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgressSnapshot {
    pub stage: String,
    pub done: usize,
    pub total: usize,
}

impl ProgressSnapshot {
    /// Completed fraction in `[0, 1]`, or `None` while the total is unknown.
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f32 / self.total as f32).clamp(0.0, 1.0))
    }
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a new stage, resetting the counters.
    pub fn begin(&self, stage: impl Into<String>, total: usize) {
        if let Ok(mut s) = self.0.stage.lock() {
            *s = stage.into();
        }
        self.0.done.store(0, Ordering::Relaxed);
        self.0.total.store(total, Ordering::Relaxed);
    }

    pub fn advance(&self) {
        self.0.done.fetch_add(1, Ordering::Relaxed);
    }

    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            stage: self.0.stage.lock().map(|s| s.clone()).unwrap_or_default(),
            done: self.0.done.load(Ordering::Relaxed),
            total: self.0.total.load(Ordering::Relaxed),
        }
    }
}
