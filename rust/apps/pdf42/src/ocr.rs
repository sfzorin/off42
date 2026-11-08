//! Sequential native OCR, retaining only recognized geometry/text between pages.
use pdf42_core::ocr::{Capture, Page, Text};
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
fn dpi(bounds: [f64; 4], requested: u32) -> Result<u32, String> {
    if !(36..=300).contains(&requested) {
        return Err("OCR resolution must be 36–300 DPI".into());
    }
    let (w, h) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err("Invalid OCR page geometry".into());
    }
    let fit = (72.0 * (3_900_000.0 / (w * h)).sqrt())
        .min(72.0 * 9900.0 / w.max(h))
        .floor();
    if fit < 36.0 {
        return Err("Page exceeds OCR image limits at the minimum resolution".into());
    }
    Ok(requested.min(fit as u32))
}
fn run<R, O>(
    pages: &[(usize, [f64; 4])],
    language: &str,
    resolution: u32,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    mut render: R,
    mut recognize: O,
) -> Result<Vec<Page>, String>
where
    R: FnMut(usize, u32) -> Result<(u32, u32, Vec<u8>), String>,
    O: FnMut(
        &win42::ocr::Image<'_>,
        &str,
        &AtomicBool,
    ) -> Result<(Vec<suite42_core::ocr::Word>, String), String>,
{
    let mut output = Vec::new();
    let (mut words_total, mut bytes_total) = (0usize, 0usize);
    for &(index, bounds) in pages {
        if cancel.load(Ordering::Relaxed) {
            return Err("Recognition cancelled; no pages changed".into());
        }
        let resolution = dpi(bounds, resolution)?;
        let (width, height, pixels) = render(index, resolution)
            .map_err(|e| format!("Page {}: {e}; no pages changed", index + 1))?;
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > 4_000_000
            || pixels.len() as u64 != u64::from(width) * u64::from(height) * 4
        {
            return Err("Invalid OCR page image".into());
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("Recognition cancelled; no pages changed".into());
        }
        let (words, actual) = recognize(
            &win42::ocr::Image {
                width,
                height,
                pixels: &pixels,
            },
            language,
            cancel,
        )
        .map_err(|e| format!("Page {}: {e}; no pages changed", index + 1))?;
        let mut texts = Vec::new();
        for word in words {
            words_total = words_total
                .checked_add(1)
                .filter(|n| *n <= 100_000)
                .ok_or("OCR exceeds its word limit; recognize fewer pages")?;
            bytes_total = bytes_total
                .checked_add(word.text.len())
                .filter(|n| *n <= 16 * 1024 * 1024)
                .ok_or("OCR exceeds its text limit; recognize fewer pages")?;
            if word.width == 0
                || word.height == 0
                || word.x.checked_add(word.width).is_none_or(|x| x > width)
                || word.y.checked_add(word.height).is_none_or(|y| y > height)
            {
                return Err("Invalid OCR word geometry".into());
            }
            let sx = (bounds[2] - bounds[0]) / f64::from(width);
            let sy = (bounds[3] - bounds[1]) / f64::from(height);
            texts.push(Text {
                text: word.text,
                rect: [
                    bounds[0] + f64::from(word.x) * sx,
                    bounds[3] - f64::from(word.y + word.height) * sy,
                    bounds[0] + f64::from(word.x + word.width) * sx,
                    bounds[3] - f64::from(word.y) * sy,
                ],
            });
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("Recognition cancelled; no pages changed".into());
        }
        output.push(Page {
            index,
            language: actual,
            words: texts,
        });
        progress.fetch_add(1, Ordering::Relaxed);
    }
    Ok(output)
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrotated_media_coordinates_scaling_cancel_and_errors_are_atomic() {
        let cancel = AtomicBool::new(false);
        let progress = AtomicUsize::new(0);
        let pages = [(2, [10., 20., 210., 320.])];
        let result = run(
            &pages,
            "auto",
            180,
            &cancel,
            &progress,
            |_, dpi| {
                assert!(dpi <= 180);
                Ok((100, 150, vec![255; 100 * 150 * 4]))
            },
            |_, _, _| {
                Ok((
                    vec![suite42_core::ocr::Word {
                        text: "Hello".into(),
                        x: 10,
                        y: 15,
                        width: 20,
                        height: 10,
                    }],
                    "en-US".into(),
                ))
            },
        )
        .unwrap();
        assert_eq!(result[0].index, 2);
        assert_eq!(result[0].words[0].rect, [30., 270., 70., 290.]);
        assert_eq!(progress.load(Ordering::Relaxed), 1);
        let mut calls = 0;
        let result = run(
            &[(0, [0., 0., 100., 100.]), (1, [0., 0., 100., 100.])],
            "auto",
            180,
            &cancel,
            &AtomicUsize::new(0),
            |_, _| Ok((1, 1, vec![255; 4])),
            |_, _, _| {
                calls += 1;
                if calls == 2 {
                    Err("engine error".into())
                } else {
                    Ok((Vec::new(), "en-US".into()))
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(calls, 2);
        cancel.store(true, Ordering::Relaxed);
        assert!(run(
            &pages,
            "auto",
            180,
            &cancel,
            &AtomicUsize::new(0),
            |_, _| panic!("render after cancellation"),
            |_, _, _| panic!()
        )
        .is_err());
        assert_eq!(dpi([0., 0., 600., 800.], 300).unwrap(), 205);
        assert!(dpi([0., 0., 1e8, 1e8], 180).is_err());
    }
}

impl super::Pdf {
    pub(super) fn ocr_request(&self) -> Option<win42::InputRequest> {
        let doc = self.doc.as_ref()?;
        Some(win42::InputRequest {
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
            use win42::App as _;
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
                    self.ocr_report = Some(win42::ReportRequest {
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
