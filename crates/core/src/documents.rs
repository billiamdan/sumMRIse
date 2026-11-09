//! The document on screen instead of the images: reading it off the UI thread,
//! its PDF pages, whether privacy mode has been lifted for it, and the way from
//! a link in a report to the image it refers to.
//!
//! Everything a document does runs on a thread of its own with panic isolation
//! and a large stack ([`Task`]): documents are untrusted input, and whatever
//! they do to a reader or to the PDF rasteriser ends in a message in the view.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use summrise_dicom::documents::{DoseReport, PlainText, ReportStatus, StructuredReport};
use summrise_dicom::{DocumentContent, DocumentKind, DocumentRef, Media, StudyIndex};
use summrise_i18n::tr;

use crate::jobs::Waker;
use crate::series::SeriesData;
use crate::session::{SeriesKey, Session};
use crate::view::{ViewKind, ViewState};

pub mod pdf;

pub use pdf::{MAX_PAGES, PageImage, PageSize, PdfPages, PdfProblem, bounded_scale};

/// Stack of the threads that read and render documents. Readers and the
/// rasteriser bound their recursion; this leaves them room to do so.
const TASK_STACK_BYTES: usize = 32 << 20;

/// One piece of work on its own thread. A panic inside it is a failed task.
pub(crate) struct Task<T> {
    rx: Receiver<Option<T>>,
}

impl<T: Send + 'static> Task<T> {
    pub(crate) fn spawn(waker: Option<Waker>, work: impl FnOnce() -> T + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("summrise-document".to_owned())
            .stack_size(TASK_STACK_BYTES)
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(work)).ok();
                let _ = tx.send(result);
                if let Some(wake) = waker {
                    wake();
                }
            });
        if let Err(e) = spawned {
            log::error!("failed to spawn a document thread: {e}");
        }
        Self { rx }
    }

    /// The result once there is one: `Some(None)` if the work panicked or
    /// could not be started.
    pub(crate) fn poll(&self) -> Option<Option<T>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(None),
        }
    }
}

/// What a document holds, ready to be shown.
#[derive(Debug)]
pub enum DocumentState {
    Loading,
    Report(StructuredReport),
    Dose(DoseReport),
    Pdf(PdfPages),
    Text(PlainText),
    /// It cannot be displayed; the text says why.
    Failed(String),
}

/// What the reading task hands over.
enum Loaded {
    Content(DocumentContent),
    Pdf { bytes: Arc<Vec<u8>>, sizes: Vec<PageSize>, total: usize, status: Option<ReportStatus> },
    Failed(String),
}

fn load(media: &Media, reference: &DocumentRef) -> Loaded {
    match summrise_dicom::read_document(media, reference) {
        Ok(DocumentContent::Pdf { bytes, status }) => {
            let bytes = Arc::new(bytes);
            match pdf::inspect(&bytes) {
                Ok((sizes, total)) => Loaded::Pdf { bytes, sizes, total, status },
                Err(problem) => Loaded::Failed(problem.to_string()),
            }
        }
        Ok(content) => Loaded::Content(content),
        Err(e) => {
            log::debug!("document could not be read: {e}");
            Loaded::Failed(e.to_string())
        }
    }
}

/// The document on screen.
pub struct OpenDocument {
    pub reference: DocumentRef,
    /// The study that was active when it was opened.
    pub study: usize,
    state: DocumentState,
    task: Option<Task<Loaded>>,
    /// The user chose to see it although privacy mode is on. Per document and
    /// not remembered: it ends when the document is closed.
    pub revealed: bool,
    waker: Option<Waker>,
}

impl std::fmt::Debug for OpenDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The title and the path can identify the patient: they are not part of the output.
        f.debug_struct("OpenDocument").field("kind", &self.reference.kind).finish_non_exhaustive()
    }
}

impl OpenDocument {
    /// Starts reading `reference` from `media`, unless it is too large to read.
    pub fn open(media: &Media, reference: DocumentRef, study: usize, waker: Option<Waker>) -> Self {
        let (state, task) = if reference.too_large {
            (DocumentState::Failed(summrise_dicom::documents::too_large_message()), None)
        } else {
            let (media, wanted) = (media.clone(), reference.clone());
            let task = Task::spawn(waker.clone(), move || load(&media, &wanted));
            (DocumentState::Loading, Some(task))
        };
        Self { reference, study, state, task, revealed: false, waker }
    }

    pub fn state(&self) -> &DocumentState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut DocumentState {
        &mut self.state
    }

    pub fn kind(&self) -> DocumentKind {
        self.reference.kind
    }

    /// Takes over what the reading task has found. Call once per UI frame.
    pub fn poll(&mut self) {
        let Some(result) = self.task.as_ref().and_then(Task::poll) else { return };
        self.task = None;
        self.state = match result {
            Some(Loaded::Content(DocumentContent::Report(report))) => DocumentState::Report(report),
            Some(Loaded::Content(DocumentContent::Dose(dose))) => DocumentState::Dose(dose),
            Some(Loaded::Content(DocumentContent::Text(text))) => DocumentState::Text(text),
            // A reader hands over the bytes of a PDF as `Loaded::Pdf`, after a look at its pages.
            Some(Loaded::Content(DocumentContent::Pdf { .. })) => {
                DocumentState::Failed(PdfProblem::Damaged.to_string())
            }
            Some(Loaded::Pdf { bytes, sizes, total, status }) => {
                DocumentState::Pdf(PdfPages::new(bytes, sizes, total, status, self.waker.clone()))
            }
            Some(Loaded::Failed(reason)) => DocumentState::Failed(reason),
            None => DocumentState::Failed(tr!("Reading this document failed unexpectedly.").to_owned()),
        };
    }
}

/// An image a link in a report refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageTarget {
    pub series: SeriesKey,
    /// Index into `Series::instances`.
    pub instance: usize,
    /// Frame within that file, counted from 0.
    pub frame: u32,
}

/// Where the images of the medium are, by their SOP Instance UID.
#[derive(Debug, Default)]
pub struct ImageLinks {
    images: HashMap<String, (SeriesKey, usize, u32)>,
}

impl ImageLinks {
    pub fn new(index: &StudyIndex) -> Self {
        let mut images = HashMap::new();
        for (s, study) in index.studies.iter().enumerate() {
            for (r, series) in study.series.iter().enumerate() {
                for (i, instance) in series.instances.iter().enumerate() {
                    let key = SeriesKey { study: s, series: r };
                    images.entry(instance.sop_instance_uid.clone()).or_insert((key, i, instance.frames));
                }
            }
        }
        Self { images }
    }

    /// The image with this UID and frame number (counted from 1 in DICOM; a
    /// reference to no frame in particular means the first). `None` when it
    /// is not on the medium, which includes a frame the file does not have.
    pub fn find(&self, sop_instance_uid: &str, frame: Option<u32>) -> Option<ImageTarget> {
        let &(series, instance, frames) = self.images.get(sop_instance_uid)?;
        let frame = frame.unwrap_or(1);
        (frame >= 1 && frame <= frames).then(|| ImageTarget { series, instance, frame: frame - 1 })
    }
}

/// The display index of the image `target` among the images of its series.
fn slot_of(series: &SeriesData, target: ImageTarget) -> Option<usize> {
    series.frames().iter().position(|info| info.instance == target.instance && info.frame == target.frame)
}

/// Selects the series of the image `target` and loads that image first. The
/// image comes on screen with [`settle_image`], once the series can be shown.
pub fn go_to_image(session: &mut Session, target: ImageTarget) {
    session.select_series(target.series);
    if let Some(series) = session.data(target.series)
        && let Some(slot) = slot_of(&series, target)
    {
        session.set_displayed(target.series, slot);
    }
}

/// Puts the image `target` on screen as soon as its series can be shown.
/// Returns whether that is over and done with: the image is on screen, the
/// series failed or the user has moved on.
pub fn settle_image(session: &Session, views: &mut HashMap<SeriesKey, ViewState>, target: ImageTarget) -> bool {
    if session.active_series != Some(target.series) {
        return true;
    }
    let Some(series) = session.data(target.series) else { return true };
    if !series.is_viewable() {
        return false;
    }
    if let Some(slot) = slot_of(&series, target) {
        let view = views.entry(target.series).or_insert_with(|| ViewState::new(&series));
        let per_stack = series.stack_len().max(1);
        view.set_stack(&series, slot / per_stack);
        view.set_slice(ViewKind::Stack, &series, slot % per_stack);
    }
    true
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use summrise_dicom::{Progress, scan};
    use summrise_testkit::PhantomOptions;
    use summrise_testkit::reports::{self, MISSING_IMAGE_UID, REFERENCED_MR_INSTANCE, write_reports_cd};

    use super::*;

    fn cd() -> (tempfile::TempDir, reports::ReportsCd, StudyIndex) {
        let dir = tempfile::tempdir().expect("tempdir");
        let written = write_reports_cd(dir.path(), &PhantomOptions::small()).expect("fixture");
        let index = scan(dir.path(), &Progress::new()).expect("scan");
        (dir, written, index)
    }

    fn opened(media: &Media, reference: &DocumentRef) -> OpenDocument {
        let mut document = OpenDocument::open(media, reference.clone(), 0, None);
        let start = Instant::now();
        while matches!(document.state(), DocumentState::Loading) {
            assert!(start.elapsed() < Duration::from_secs(30), "timed out reading the document");
            document.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
        document
    }

    fn by_title<'a>(index: &'a StudyIndex, title: &str) -> &'a DocumentRef {
        index.studies[0]
            .documents
            .iter()
            .chain(&index.loose_documents)
            .find(|d| d.title.as_deref() == Some(title))
            .unwrap_or_else(|| panic!("no document {title}"))
    }

    #[test]
    fn each_kind_of_document_is_read_off_the_ui_thread() {
        let (_dir, _, index) = cd();
        let media = &index.media;
        let report = opened(media, by_title(&index, reports::MR_REPORT_TITLE));
        assert!(matches!(report.state(), DocumentState::Report(r) if r.status.is_final() && !r.items.is_empty()));
        let dose = opened(media, by_title(&index, reports::DOSE_REPORT_TITLE));
        assert!(matches!(dose.state(), DocumentState::Dose(d) if d.events.len() == 2));
        let pdf = opened(media, by_title(&index, reports::STORED_PDF_TITLE));
        assert!(matches!(pdf.state(), DocumentState::Pdf(p) if p.page_count() == 2));
        let file = opened(media, by_title(&index, "REPORT.PDF"));
        assert!(matches!(file.state(), DocumentState::Pdf(p) if p.page_count() == 1));
        let rtf = opened(media, by_title(&index, "report_001.rtf"));
        assert!(matches!(rtf.state(), DocumentState::Text(t) if t.text.contains(reports::RTF_CYRILLIC_TEXT)));
        let html = opened(media, by_title(&index, "report_002.htm"));
        assert!(matches!(html.state(), DocumentState::Text(t) if !t.text.contains("SCRIPT RAN")));
    }

    #[test]
    fn a_panic_while_reading_becomes_a_message() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let woken = Arc::new(AtomicBool::new(false));
        let wake = woken.clone();
        let waker: Waker = Arc::new(move || wake.store(true, Ordering::SeqCst));
        let task: Task<u32> =
            Task::spawn(Some(waker), || panic!("deliberate panic in a test: the task must survive it"));
        let start = Instant::now();
        // The UI hears of it like of any other result, so that the view shows the message.
        while !woken.load(Ordering::SeqCst) {
            assert!(start.elapsed() < Duration::from_secs(30), "the panic woke nobody");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(task.poll(), Some(None));
    }

    #[test]
    fn a_file_too_large_is_not_read_at_all() {
        let (dir, _, index) = cd();
        // Not on the disc any more: reading would fail differently.
        let reference = DocumentRef {
            kind: DocumentKind::PdfFile,
            title: Some("HUGE.PDF".to_owned()),
            date: None,
            path: dir.path().join("does-not-exist"),
            sop_instance_uid: None,
            too_large: true,
        };
        let document = OpenDocument::open(&index.media, reference, 0, None);
        match document.state() {
            DocumentState::Failed(reason) => assert!(reason.contains("larger than 50 MB"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn links_find_images_by_uid_and_frame() {
        let (_dir, written, index) = cd();
        let links = ImageLinks::new(&index);
        let target = links.find(&written.referenced_mr_uid, None).expect("on the disc");
        let series = &index.studies[target.series.study].series[target.series.series];
        assert_eq!(series.modality, "MR");
        assert_eq!(series.instances[target.instance].instance_number, Some(REFERENCED_MR_INSTANCE as i32));
        assert_eq!(target.frame, 0);
        // A frame the file does not have, and an image that is not on the disc.
        assert_eq!(links.find(&written.referenced_mr_uid, Some(2)), None);
        assert_eq!(links.find(&written.referenced_mr_uid, Some(0)), None);
        assert_eq!(links.find(MISSING_IMAGE_UID, None), None);
    }

    #[test]
    fn a_link_opens_the_series_on_the_referenced_image() {
        let (_dir, written, index) = cd();
        let links = ImageLinks::new(&index);
        let target = links.find(&written.referenced_mr_uid, None).expect("on the disc");
        let instance = index.studies[0].series[target.series.series].instances[target.instance].clone();
        let mut session = Session::new(index, 1 << 30, None);
        let mut views = HashMap::new();
        assert_ne!(session.active_series, Some(target.series), "the CT is selected at first");
        go_to_image(&mut session, target);
        assert_eq!(session.active_series, Some(target.series));
        let start = Instant::now();
        while !settle_image(&session, &mut views, target) {
            assert!(start.elapsed() < Duration::from_secs(30), "timed out loading the series");
            session.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
        let series = session.data(target.series).expect("loading");
        let view = views.get(&target.series).expect("the view");
        let shown = &series.frames()[view.slot_index(&series)];
        assert_eq!(shown.instance, target.instance);
        assert_eq!(shown.instance_number, instance.instance_number);
        // Already settled: once the user has moved on, it asks for nothing any more.
        session.select_series(SeriesKey { study: 0, series: 0 });
        assert!(settle_image(&session, &mut views, target));
    }
}
