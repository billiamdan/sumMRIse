//! Background work. Each job runs on its own thread (with rayon parallelism
//! inside) and wakes the UI when it finishes, so the UI thread never blocks
//! on disc I/O or decoding.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use summrise_dicom::{Progress, ProgressSnapshot};

/// Called from worker threads to request a UI repaint.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// A single-result background job with progress and cancellation.
#[derive(Debug)]
pub struct Job<T> {
    progress: Progress,
    rx: Receiver<T>,
    finished: bool,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(name: &str, waker: Option<Waker>, work: impl FnOnce(&Progress) -> T + Send + 'static) -> Self {
        let progress = Progress::new();
        let (tx, rx) = mpsc::channel();
        let worker_progress = progress.clone();
        let spawned = std::thread::Builder::new().name(format!("summrise-{name}")).spawn(move || {
            let result = work(&worker_progress);
            let _ = tx.send(result);
            if let Some(wake) = waker {
                wake();
            }
        });
        if let Err(e) = spawned {
            log::error!("failed to spawn worker thread: {e}");
        }
        Self { progress, rx, finished: false }
    }

    /// Takes the result once the job has finished.
    pub fn poll(&mut self) -> Option<T> {
        if self.finished {
            return None;
        }
        match self.rx.try_recv() {
            Ok(v) => {
                self.finished = true;
                Some(v)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.finished = true;
                None
            }
        }
    }

    pub fn progress(&self) -> ProgressSnapshot {
        self.progress.snapshot()
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

/// A job that streams many results (e.g. thumbnails) as they become ready.
#[derive(Debug)]
pub struct Stream<T> {
    progress: Progress,
    rx: Receiver<T>,
}

impl<T: Send + 'static> Stream<T> {
    pub fn spawn(
        name: &str,
        waker: Option<Waker>,
        work: impl FnOnce(&Progress, &(dyn Fn(T) + Sync)) + Send + 'static,
    ) -> Self {
        let progress = Progress::new();
        let (tx, rx) = mpsc::channel();
        let worker_progress = progress.clone();
        let spawned = std::thread::Builder::new().name(format!("summrise-{name}")).spawn(move || {
            let emit = |item: T| {
                let _ = tx.send(item);
                if let Some(wake) = &waker {
                    wake();
                }
            };
            work(&worker_progress, &emit);
        });
        if let Err(e) = spawned {
            log::error!("failed to spawn worker thread: {e}");
        }
        Self { progress, rx }
    }

    /// Drains all results produced so far.
    pub fn drain(&self) -> Vec<T> {
        self.rx.try_iter().collect()
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }
}

impl<T> Drop for Stream<T> {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait<T: Send + 'static>(job: &mut Job<T>) -> T {
        let start = Instant::now();
        loop {
            if let Some(v) = job.poll() {
                return v;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "job timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn job_returns_result_and_wakes() {
        let woke = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let w = woke.clone();
        let mut job =
            Job::spawn("test", Some(Arc::new(move || w.store(true, std::sync::atomic::Ordering::SeqCst))), |p| {
                p.begin("work", 1);
                p.advance();
                42
            });
        assert_eq!(wait(&mut job), 42);
        assert!(job.is_finished());
        assert!(job.poll().is_none());
        assert!(woke.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn stream_emits_items() {
        let stream = Stream::spawn("test", None, |_, emit| (0..3).for_each(emit));
        let start = Instant::now();
        let mut items = Vec::new();
        while items.len() < 3 && start.elapsed() < Duration::from_secs(10) {
            items.extend(stream.drain());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(items, vec![0, 1, 2]);
    }
}
