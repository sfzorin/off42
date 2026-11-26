//! Sequential native OCR, retaining only recognized geometry/text between pages.
use pdf42_core::ocr::{Capture, Page};
#[cfg(windows)]
use suite42_app::pdf::ocr::run;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc,
};
pub struct Worker {
    pub source: pdf42_core::SaveState,
    pub epoch: u64,
    pub total: usize,
    pub progress: Arc<AtomicUsize>,
    pub cancel: Arc<AtomicBool>,
    pub shown: usize,
    pub rx: mpsc::Receiver<Result<Vec<Page>, String>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Worker {
    pub fn start(
        capture: Capture,
        epoch: u64,
        language: String,
        resolution: u32,
    ) -> Result<Self, String> {
        if language.is_empty()
            || language.len() > 128
            || !language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+'))
        {
            return Err("Use auto or an built-in language tag such as en-US or eng+rus".into());
        }
        if !(36..=300).contains(&resolution) {
            return Err("OCR resolution must be 36–300 DPI".into());
        }
        let total = capture.pages.len();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel();
        let (token, counter) = (cancel.clone(), progress.clone());
        std::thread::spawn(move || {
            #[cfg(windows)]
            let result = (|| {
                let renderer = suite42_pdf_raster::Session::open(&capture.bytes)?;
                run(
                    &capture.pages,
                    &language,
                    resolution,
                    &token,
                    &counter,
                    |index, dpi| renderer.render_page(index as u32, dpi),
                    win42::ocr::recognize,
                )
            })();
            #[cfg(not(windows))]
            let result = {
                let _ = (&capture.bytes, &capture.pages, &language, &token, &counter);
                Err("Built-in OCR is available only on Windows".into())
            };
            let _ = tx.send(result);
        });
        Ok(Self {
            source: capture.source,
            epoch,
            total,
            progress,
            cancel,
            shown: 0,
            rx,
        })
    }
}
impl super::Pdf {
    pub(super) fn ocr_request(&self) -> Option<suite42_app::InputRequest> {
        let doc = self.doc.as_ref()?;
        Some(suite42_app::InputRequest {
            title: "Recognize PDF Text",
            fields: vec![
                (
                    "Pages (for example 1-3, 5)",
                    format!("1-{}", doc.page_count()),
                ),
                (
                    "Language (auto, en-US, eng+rus; up to 3)",
                    if self.ocr_language.is_empty() {
                        "auto".into()
                    } else {
                        self.ocr_language.clone()
                    },
                ),
                (
                    "Resolution DPI (large pages fit automatically)",
                    if self.ocr_dpi == 0 {
                        "180".into()
                    } else {
                        self.ocr_dpi.to_string()
                    },
                ),
            ],
        })
    }
    pub(super) fn start_ocr(&mut self, values: &[String]) -> Result<(), String> {
        #[cfg(windows)]
        {
            use suite42_app::App as _;
            if !self.enabled(114) {
                return Err("Text recognition is unavailable".into());
            }
            let [pages, language, resolution] = values else {
                return Err("Invalid OCR settings".into());
            };
            let doc = self.doc.as_ref().ok_or("Open a PDF first")?;
            let pages = super::page_export::parse_pages(pages, doc.page_count())?;
            let resolution = resolution
                .trim()
                .parse::<u32>()
                .map_err(|_| "Invalid OCR resolution")?;
            let language = language.trim().to_string();
            let capture = doc.capture_ocr(&pages)?;
            self.ocr_worker = Some(Worker::start(
                capture,
                self.current_epoch,
                language.clone(),
                resolution,
            )?);
            self.ocr_language = language;
            self.ocr_dpi = resolution;
            self.operation_error = None;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = values;
            Err("Built-in OCR is available only on Windows".into())
        }
    }
    #[cfg(windows)]
    pub(super) fn poll_ocr(&mut self) -> bool {
        let mut changed = false;
        if let Some(worker) = &mut self.ocr_worker {
            let progress = worker.progress.load(Ordering::Relaxed);
            if progress != worker.shown {
                worker.shown = progress;
                changed = true;
            }
        }
        let result = self
            .ocr_worker
            .as_ref()
            .and_then(|worker| match worker.rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Text recognition worker stopped; no pages changed".into(),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = result {
            let worker = self.ocr_worker.take().unwrap();
            if worker.epoch != self.current_epoch
                || !self
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.matches_snapshot(&worker.source))
            {
                self.operation_error = Some("Recognition discarded because the PDF changed".into());
            } else {
                let result = result.and_then(|pages| {
                    let count = self
                        .doc
                        .as_mut()
                        .unwrap()
                        .apply_ocr(&worker.source, pages)?;
                    if count > 0 {
                        self.branch();
                        self.request_render()?;
                    }
                    Ok(count)
                });
                self.operation_error = Some(match result {
                    Ok(count) => format!("Text recognized on {count} of {} pages", worker.total),
                    Err(error) => error,
                });
            }
            changed = true;
        }
        let languages = self
            .ocr_languages
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("OCR language discovery stopped".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = languages {
            self.ocr_languages = None;
            match result {
                Ok(languages) => {
                    self.ocr_report = Some(suite42_app::report::ReportRequest {
                        title: "Built-in OCR Languages".into(),
                        summary: format!(
                            "{} recognition languages are bundled with OFF42.",
                            languages.len()
                        ),
                        columns: vec!["Language tag", "Language"],
                        column_colors: vec![],
                        rows: languages
                            .into_iter()
                            .map(|(tag, name)| vec![tag, name])
                            .collect(),
                        highlight_controls: false,
                        compact_first_column: true,
                        allow_keep: false,
                        keeping: false,
                    });
                    self.operation_error = None;
                }
                Err(error) => self.operation_error = Some(error),
            }
            changed = true;
        }
        changed
    }
}
