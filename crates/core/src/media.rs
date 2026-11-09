//! Detection of inserted CDs/DVDs/USB sticks that carry a DICOMDIR.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::jobs::Waker;

/// Mount points where removable media appear on this platform.
fn candidate_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut children = |dir: &Path| {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                // Skip symlinks such as /Volumes/Macintosh HD -> /.
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    out.push(e.path());
                }
            }
        }
    };
    if cfg!(target_os = "macos") {
        children(Path::new("/Volumes"));
    } else if cfg!(target_os = "linux") {
        let user = std::env::var("USER").unwrap_or_default();
        children(Path::new("/media"));
        children(&Path::new("/media").join(&user));
        children(&Path::new("/run/media").join(&user));
        children(Path::new("/mnt"));
    } else if cfg!(windows) {
        for letter in b'D'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", letter as char));
            if root.exists() {
                out.push(root);
            }
        }
    }
    out
}

fn has_dicomdir(root: &Path) -> bool {
    std::fs::read_dir(root).is_ok_and(|rd| {
        rd.flatten().any(|e| {
            let name = e.file_name().to_string_lossy().to_ascii_uppercase();
            (name == "DICOMDIR" || name == "DICOMDIR;1") && e.file_type().is_ok_and(|t| t.is_file())
        })
    })
}

/// Mounted volumes whose root contains a DICOMDIR.
pub fn find_dicom_media() -> Vec<PathBuf> {
    candidate_roots().into_iter().filter(|r| has_dicomdir(r)).collect()
}

/// Polls for DICOM media on a background thread (optical drives can block
/// for seconds while spinning up, so this never runs on the UI thread).
pub struct MediaWatcher {
    rx: Receiver<Vec<PathBuf>>,
    stop: Arc<AtomicBool>,
    current: Vec<PathBuf>,
}

impl MediaWatcher {
    pub fn spawn(waker: Option<Waker>) -> Self {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let spawned = std::thread::Builder::new().name("summrise-media".into()).spawn(move || {
            crate::crash::watch(|| {
                let mut last: Option<Vec<PathBuf>> = None;
                while !stop_flag.load(Ordering::Relaxed) {
                    let found = find_dicom_media();
                    if last.as_ref() != Some(&found) {
                        if tx.send(found.clone()).is_err() {
                            break;
                        }
                        if let Some(w) = &waker {
                            w();
                        }
                        last = Some(found);
                    }
                    for _ in 0..20 {
                        if stop_flag.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            })
        });
        if let Err(e) = spawned {
            log::warn!("media watcher unavailable: {e}");
        }
        Self { rx, stop, current: Vec::new() }
    }

    /// Currently mounted DICOM media (updated without blocking).
    pub fn media(&mut self) -> &[PathBuf] {
        if let Some(latest) = self.rx.try_iter().last() {
            self.current = latest;
        }
        &self.current
    }
}

impl Drop for MediaWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_dicomdir_at_volume_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!has_dicomdir(dir.path()));
        std::fs::write(dir.path().join("DICOMDIR"), b"x").expect("write");
        assert!(has_dicomdir(dir.path()));
    }
}
